#![forbid(unsafe_code)]

//! Modul arsitektur pipeline eksekusi 3-tahap (3-Stage Execution Pipeline).
//!
//! - Stage 1: Pool verifikator tanda tangan Ed25519, saldo transfer+fee, dan invarian paralel multi-threaded.
//! - Stage 2: Sequencer deterministik, pelacakan fee burn/tips, emisi record sintetis, dan transisi status in-memory (Keydir).
//! - Stage 3: Ingest penyimpanan sekuensial disk berbasis batch (SegmentWriter) single-threaded.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{available_parallelism, JoinHandle};
use std::time::Duration;

use ratu_aurion_index::entry::AccountLocation;
use ratu_aurion_index::keydir::Keydir;
use ratu_aurion_primitives::{
    crypto::{AccountId, Hash, Signature},
    record::{
        MutationRecord, BASE_TX_FEE, BPS_DENOMINATOR, DEFAULT_BURN_RATIO_BPS,
        RECORD_KIND_BLOCK_REWARD, RECORD_KIND_FEE_BURN, RECORD_KIND_SYSTEM_NOTIF,
        RECORD_KIND_TRANSFER, RECORD_SIZE,
    },
    value::AurValue,
};
use ratu_aurion_storage::writer::SegmentWriter;

use crate::error::EngineError;

/// Kapasitas antrean channel Stage 1 (bounded backpressure).
pub const STAGE1_QUEUE_CAPACITY: usize = 16_384;

/// Kapasitas antrean channel Stage 2.
pub const STAGE2_QUEUE_CAPACITY: usize = 16_384;

/// Ukuran maksimal pengelompokan batch penulisan disk Stage 3 (menyesuaikan batas buffer 128 KB).
pub const STAGE3_BATCH_MAX_SIZE: usize = 400;

/// Tanda terima komitmen mutasi yang berhasil ditulis ke disk dan diperbarui di memori.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitReceipt {
    /// Nomor epoch transaksi.
    pub epoch: u64,
    /// Nomor urut transaksi yang dikomit.
    pub sequence_number: u64,
    /// Posisi byte fisik record pada berkas segmen disk.
    pub disk_offset: u64,
}

/// Tanda terima komitmen slot yang berhasil difinalisasi bersama emisi reward dan burn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotReceipt {
    /// Nomor epoch saat slot dikomit.
    pub epoch: u64,
    /// Reward blok inflasi yang dialokasikan.
    pub block_reward: AurValue,
    /// Akumulasi fee tip yang diberikan ke leader.
    pub fee_tip: AurValue,
    /// Akumulasi fee yang dimusnahkan permanen dari total supply.
    pub fee_burned: AurValue,
    /// Jumlah rekaman sintetis yang diemisikan ke segmen log (burn + reward).
    pub records_emitted: usize,
}

/// Amplop mutasi mentah yang masuk ke Stage 1.
pub struct RawTxEnvelope {
    pub record: MutationRecord,
    pub response_tx: SyncSender<Result<CommitReceipt, EngineError>>,
}

/// Amplop mutasi yang telah lolos verifikasi kriptografis Stage 1 untuk diteruskan ke Stage 2.
pub struct VerifiedTxEnvelope {
    pub record: MutationRecord,
    pub response_tx: SyncSender<Result<CommitReceipt, EngineError>>,
}

/// Batch mutasi yang telah lolos validasi urutan dan saldo pada Stage 2 untuk ditulis oleh Stage 3.
pub struct SequencedBatch {
    pub records: Vec<MutationRecord>,
    pub response_senders: Vec<Option<SyncSender<Result<CommitReceipt, EngineError>>>>,
}

/// Pesan internal untuk antrean Stage 2 sequencer.
enum Stage2Message {
    Tx(VerifiedTxEnvelope),
    CumulativeFees(SyncSender<(AurValue, AurValue)>),
    CommitSlot(AccountId, SyncSender<Result<SlotReceipt, EngineError>>),
}

/// Koordinator eksekusi pipeline 3-tahap.
pub struct PipelineCoordinator {
    stage1_tx: Option<SyncSender<RawTxEnvelope>>,
    stage2_tx: Option<SyncSender<Stage2Message>>,
    stage1_handles: Vec<JoinHandle<()>>,
    stage2_handle: Option<JoinHandle<()>>,
    stage3_handle: Option<JoinHandle<()>>,
    shutdown_signal: Arc<AtomicBool>,
    index_shared: Arc<RwLock<Keydir>>,
}

impl Drop for PipelineCoordinator {
    fn drop(&mut self) {
        self.shutdown_signal.store(true, Ordering::SeqCst);
        drop(self.stage1_tx.take());
        drop(self.stage2_tx.take());
    }
}

impl PipelineCoordinator {
    /// Menginisialisasi dan menyalakan seluruh thread worker pipeline 3-tahap (tanpa deduksi biaya transaksi).
    pub fn spawn(
        writer: SegmentWriter,
        index: Keydir,
        num_verifiers: usize,
    ) -> Result<Self, EngineError> {
        Self::spawn_with_fee(writer, index, num_verifiers, AurValue::ZERO)
    }

    /// Menginisialisasi pipeline moneter dengan biaya transaksi baku `BASE_TX_FEE` (0.001 AUR).
    pub fn spawn_monetary(
        writer: SegmentWriter,
        index: Keydir,
        num_verifiers: usize,
    ) -> Result<Self, EngineError> {
        Self::spawn_with_fee(writer, index, num_verifiers, BASE_TX_FEE)
    }

    /// Menginisialisasi dan menyalakan seluruh thread worker pipeline 3-tahap dengan kebijakan biaya transaksi eksplisit.
    pub fn spawn_with_fee(
        writer: SegmentWriter,
        index: Keydir,
        num_verifiers: usize,
        base_fee: AurValue,
    ) -> Result<Self, EngineError> {
        let verifier_count = if num_verifiers == 0 {
            available_parallelism().map_or(4, |n| n.get())
        } else {
            num_verifiers
        };

        let (stage1_tx, stage1_rx) = sync_channel::<RawTxEnvelope>(STAGE1_QUEUE_CAPACITY);
        let (stage2_tx, stage2_rx) = sync_channel::<Stage2Message>(STAGE2_QUEUE_CAPACITY);
        let (stage3_tx, stage3_rx) =
            sync_channel::<SequencedBatch>(STAGE2_QUEUE_CAPACITY / STAGE3_BATCH_MAX_SIZE + 16);

        let shutdown_signal = Arc::new(AtomicBool::new(false));
        let stage1_rx = Arc::new(Mutex::new(stage1_rx));
        let index_shared = Arc::new(RwLock::new(index));

        // ---------------------------------------------------------------------
        // STAGE 1: Parallel Stateless Cryptographic Verifiers (N threads)
        // ---------------------------------------------------------------------
        let mut stage1_handles = Vec::with_capacity(verifier_count);
        for _ in 0..verifier_count {
            let rx = Arc::clone(&stage1_rx);
            let s2_tx = stage2_tx.clone();
            let index_ref = Arc::clone(&index_shared);

            let handle = std::thread::spawn(move || {
                loop {
                    let envelope = {
                        let lock = match rx.lock() {
                            Ok(guard) => guard,
                            Err(_) => break,
                        };
                        match lock.recv() {
                            Ok(env) => env,
                            Err(_) => break,
                        }
                    };

                    // Invarian 1: Nominal harus lebih besar dari 0
                    if envelope.record.amount == AurValue::ZERO {
                        let _ = envelope.response_tx.send(Err(EngineError::InvalidTransaction(
                            "Transaction amount must be greater than zero".to_string(),
                        )));
                        continue;
                    }

                    // Invarian 2: Pengirim dan penerima tidak boleh identik
                    if envelope.record.sender == envelope.record.recipient {
                        let _ = envelope.response_tx.send(Err(EngineError::InvalidTransaction(
                            "Sender and recipient cannot be identical".to_string(),
                        )));
                        continue;
                    }

                    // Invarian 3: Jenis record harus valid
                    if envelope.record.record_kind != RECORD_KIND_TRANSFER
                        && envelope.record.record_kind != RECORD_KIND_SYSTEM_NOTIF
                        && envelope.record.record_kind != RECORD_KIND_FEE_BURN
                        && envelope.record.record_kind != RECORD_KIND_BLOCK_REWARD
                    {
                        let _ = envelope.response_tx.send(Err(EngineError::InvalidTransaction(
                            format!("Invalid record kind: {}", envelope.record.record_kind),
                        )));
                        continue;
                    }

                    // Invarian 4: Verifikasi pra-kondisi saldo pengirim terhadap amount + base_fee
                    if base_fee > AurValue::ZERO {
                        let required = match envelope.record.amount.checked_add(base_fee) {
                            Some(v) => v,
                            None => {
                                let _ = envelope.response_tx.send(Err(EngineError::InvalidTransaction(
                                    "Arithmetic overflow calculating amount + fee".to_string(),
                                )));
                                continue;
                            }
                        };
                        if let Ok(guard) = index_ref.read() {
                            let available = guard.get_balance(&envelope.record.sender);
                            if available < required {
                                let _ = envelope.response_tx.send(Err(EngineError::InsufficientBalance {
                                    requested: required,
                                    available,
                                }));
                                continue;
                            }
                        }
                    }

                    // Invarian 5: Verifikasi tanda tangan kriptografis Ed25519
                    if let Err(err) = crate::validator::verify_record_signature(&envelope.record) {
                        let _ = envelope.response_tx.send(Err(err));
                        continue;
                    }

                    let verified = VerifiedTxEnvelope {
                        record: envelope.record,
                        response_tx: envelope.response_tx,
                    };

                    if s2_tx.send(Stage2Message::Tx(verified)).is_err() {
                        break;
                    }
                }
            });
            stage1_handles.push(handle);
        }

        // ---------------------------------------------------------------------
        // STAGE 2: Deterministic Sequencer & RAM State Transition (1 thread)
        // ---------------------------------------------------------------------
        let initial_offset = writer.current_offset();
        let epoch = writer.header().epoch;
        let segment_idx = writer.header().segment_index;
        let index_s2 = Arc::clone(&index_shared);

        let stage2_handle = std::thread::spawn(move || {
            let mut current_offset = initial_offset;
            let mut current_batch = SequencedBatch {
                records: Vec::with_capacity(STAGE3_BATCH_MAX_SIZE),
                response_senders: Vec::with_capacity(STAGE3_BATCH_MAX_SIZE),
            };
            let mut cumulative_fee_burned = AurValue::ZERO;
            let mut cumulative_fee_tips = AurValue::ZERO;
            let mut last_seq = 0u64;

            let flush_batch = |batch: &mut SequencedBatch,
                               s3_tx: &SyncSender<SequencedBatch>|
             -> bool {
                if batch.records.is_empty() {
                    return true;
                }
                let to_send = std::mem::replace(
                    batch,
                    SequencedBatch {
                        records: Vec::with_capacity(STAGE3_BATCH_MAX_SIZE),
                        response_senders: Vec::with_capacity(STAGE3_BATCH_MAX_SIZE),
                    },
                );
                s3_tx.send(to_send).is_ok()
            };

            loop {
                match stage2_rx.recv() {
                    Ok(Stage2Message::Tx(verified)) => {
                        let mut keydir = match index_s2.write() {
                            Ok(g) => g,
                            Err(_) => break,
                        };

                        Self::process_stage2_tx(
                            verified,
                            &mut keydir,
                            base_fee,
                            &mut cumulative_fee_burned,
                            &mut cumulative_fee_tips,
                            &mut last_seq,
                            &mut current_batch,
                            &mut current_offset,
                            epoch,
                            segment_idx,
                        );

                        // Kuras mutasi lain yang siap dalam antrean hingga batas batch
                        while current_batch.records.len() < STAGE3_BATCH_MAX_SIZE {
                            match stage2_rx.try_recv() {
                                Ok(Stage2Message::Tx(next_tx)) => {
                                    Self::process_stage2_tx(
                                        next_tx,
                                        &mut keydir,
                                        base_fee,
                                        &mut cumulative_fee_burned,
                                        &mut cumulative_fee_tips,
                                        &mut last_seq,
                                        &mut current_batch,
                                        &mut current_offset,
                                        epoch,
                                        segment_idx,
                                    );
                                }
                                Ok(Stage2Message::CumulativeFees(resp)) => {
                                    let _ = resp.send((cumulative_fee_burned, cumulative_fee_tips));
                                }
                                Ok(Stage2Message::CommitSlot(leader, resp)) => {
                                    Self::process_commit_slot(
                                        leader,
                                        resp,
                                        &mut keydir,
                                        &mut cumulative_fee_burned,
                                        &mut cumulative_fee_tips,
                                        &mut last_seq,
                                        &mut current_batch,
                                        &mut current_offset,
                                        epoch,
                                        segment_idx,
                                        &stage3_tx,
                                    );
                                }
                                Err(_) => break,
                            }
                        }

                        drop(keydir);

                        // Kirim batch ke Stage 3
                        if !flush_batch(&mut current_batch, &stage3_tx) {
                            break;
                        }
                    }
                    Ok(Stage2Message::CumulativeFees(resp)) => {
                        let _ = resp.send((cumulative_fee_burned, cumulative_fee_tips));
                    }
                    Ok(Stage2Message::CommitSlot(leader, resp)) => {
                        let mut keydir = match index_s2.write() {
                            Ok(g) => g,
                            Err(_) => {
                                let _ = resp.send(Err(EngineError::WorkerThreadPanicked));
                                break;
                            }
                        };
                        Self::process_commit_slot(
                            leader,
                            resp,
                            &mut keydir,
                            &mut cumulative_fee_burned,
                            &mut cumulative_fee_tips,
                            &mut last_seq,
                            &mut current_batch,
                            &mut current_offset,
                            epoch,
                            segment_idx,
                            &stage3_tx,
                        );
                    }
                    Err(_) => {
                        // Seluruh pengirim Stage 2 telah selesai
                        let _ = flush_batch(&mut current_batch, &stage3_tx);
                        break;
                    }
                }
            }
        });

        // ---------------------------------------------------------------------
        // STAGE 3: Sequential Batch Storage Writer (1 thread)
        // ---------------------------------------------------------------------
        let stage3_handle = std::thread::spawn(move || {
            let mut writer = writer;

            while let Ok(batch) = stage3_rx.recv() {
                for (record, resp_opt) in batch.records.into_iter().zip(batch.response_senders) {
                    match writer.append_record(&record) {
                        Ok(disk_offset) => {
                            if let Some(resp_tx) = resp_opt {
                                let receipt = CommitReceipt {
                                    epoch: record.epoch,
                                    sequence_number: record.sequence_number,
                                    disk_offset,
                                };
                                let _ = resp_tx.send(Ok(receipt));
                            }
                        }
                        Err(err) => {
                            if let Some(resp_tx) = resp_opt {
                                let _ = resp_tx.send(Err(EngineError::StorageError(err)));
                            }
                        }
                    }
                }
                let _ = writer.flush_buffer();
            }

            // Flush fisik dan segel segmen saat shutdown
            let _ = writer.flush_and_sync();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let dummy_digest = Hash::new([0xAA; 32]);
            let _ = writer.seal_segment(dummy_digest, now);
        });

        Ok(Self {
            stage1_tx: Some(stage1_tx),
            stage2_tx: Some(stage2_tx),
            stage1_handles,
            stage2_handle: Some(stage2_handle),
            stage3_handle: Some(stage3_handle),
            shutdown_signal,
            index_shared,
        })
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn process_stage2_tx(
        verified: VerifiedTxEnvelope,
        keydir: &mut Keydir,
        base_fee: AurValue,
        cumulative_fee_burned: &mut AurValue,
        cumulative_fee_tips: &mut AurValue,
        last_seq: &mut u64,
        current_batch: &mut SequencedBatch,
        current_offset: &mut u64,
        epoch: u64,
        segment_idx: u32,
    ) {
        let total_required = match verified.record.amount.checked_add(base_fee) {
            Some(v) => v,
            None => {
                let _ = verified.response_tx.send(Err(EngineError::InvalidTransaction(
                    "Arithmetic overflow calculating amount + fee".to_string(),
                )));
                return;
            }
        };

        let sender_balance = keydir.get_balance(&verified.record.sender);
        if sender_balance < total_required {
            let _ = verified.response_tx.send(Err(EngineError::InsufficientBalance {
                requested: total_required,
                available: sender_balance,
            }));
            return;
        }

        if let Some(state) = keydir.get_account_state(&verified.record.sender) {
            if verified.record.sequence_number <= state.location.sequence_number {
                let _ = verified.response_tx.send(Err(EngineError::StaleSequenceNumber {
                    expected: state.location.sequence_number + 1,
                    found: verified.record.sequence_number,
                }));
                return;
            }
        }

        match keydir.apply_mutation_with_fee(
            &verified.record,
            base_fee,
            epoch,
            segment_idx,
            *current_offset,
        ) {
            Ok(()) => {
                if base_fee > AurValue::ZERO {
                    let burn = base_fee
                        .checked_mul_ratio(DEFAULT_BURN_RATIO_BPS, BPS_DENOMINATOR)
                        .unwrap_or(AurValue::ZERO);
                    let tip = base_fee.checked_sub(burn).unwrap_or(AurValue::ZERO);
                    *cumulative_fee_burned = cumulative_fee_burned
                        .checked_add(burn)
                        .unwrap_or(AurValue::MAX);
                    *cumulative_fee_tips = cumulative_fee_tips
                        .checked_add(tip)
                        .unwrap_or(AurValue::MAX);
                }
                *last_seq = verified.record.sequence_number;
                *current_offset += RECORD_SIZE as u64;
                current_batch.records.push(verified.record);
                current_batch.response_senders.push(Some(verified.response_tx));
            }
            Err(err) => {
                let _ = verified.response_tx.send(Err(EngineError::IndexError(err)));
            }
        }
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn process_commit_slot(
        leader: AccountId,
        resp: SyncSender<Result<SlotReceipt, EngineError>>,
        keydir: &mut Keydir,
        cumulative_fee_burned: &mut AurValue,
        cumulative_fee_tips: &mut AurValue,
        last_seq: &mut u64,
        current_batch: &mut SequencedBatch,
        current_offset: &mut u64,
        epoch: u64,
        segment_idx: u32,
        stage3_tx: &SyncSender<SequencedBatch>,
    ) {
        let block_reward = keydir.compute_block_reward();
        let tip = *cumulative_fee_tips;
        let burned = *cumulative_fee_burned;
        *cumulative_fee_tips = AurValue::ZERO;
        *cumulative_fee_burned = AurValue::ZERO;

        let mut emitted = 0;

        // 1. Emisi synthetic fee burn record jika burned > ZERO
        if burned > AurValue::ZERO {
            *last_seq = last_seq.checked_add(1).unwrap_or(*last_seq);
            let burn_rec = MutationRecord {
                epoch,
                sequence_number: *last_seq,
                record_kind: RECORD_KIND_FEE_BURN,
                sender: AccountId::ZERO,
                recipient: AccountId::ZERO,
                amount: burned,
                signature: Signature::new([0x02; 64]),
            };
            current_batch.records.push(burn_rec);
            current_batch.response_senders.push(None);
            *current_offset += RECORD_SIZE as u64;
            emitted += 1;
        }

        // 2. Emisi synthetic block reward record
        let leader_total = match block_reward.checked_add(tip) {
            Some(v) => v,
            None => {
                let _ = resp.send(Err(EngineError::FeeCalculationOverflow));
                return;
            }
        };

        *last_seq = last_seq.checked_add(1).unwrap_or(*last_seq);
        let reward_rec = MutationRecord {
            epoch,
            sequence_number: *last_seq,
            record_kind: RECORD_KIND_BLOCK_REWARD,
            sender: AccountId::ZERO,
            recipient: leader,
            amount: leader_total,
            signature: Signature::new([0x03; 64]),
        };

        let reward_offset = *current_offset;
        *current_offset += RECORD_SIZE as u64;
        emitted += 1;

        let location = AccountLocation::new(epoch, segment_idx, reward_offset, *last_seq);
        if let Err(err) = keydir.finalize_block_rewards(&leader, block_reward, tip, burned, location) {
            let _ = resp.send(Err(EngineError::IndexError(err)));
            return;
        }

        let (confirm_tx, confirm_rx) = sync_channel(1);
        current_batch.records.push(reward_rec);
        current_batch.response_senders.push(Some(confirm_tx));

        let to_send = std::mem::replace(
            current_batch,
            SequencedBatch {
                records: Vec::with_capacity(STAGE3_BATCH_MAX_SIZE),
                response_senders: Vec::with_capacity(STAGE3_BATCH_MAX_SIZE),
            },
        );

        if stage3_tx.send(to_send).is_err() {
            let _ = resp.send(Err(EngineError::PipelineChannelClosed));
            return;
        }

        match confirm_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(_receipt)) => {
                let slot_receipt = SlotReceipt {
                    epoch,
                    block_reward,
                    fee_tip: tip,
                    fee_burned: burned,
                    records_emitted: emitted,
                };
                let _ = resp.send(Ok(slot_receipt));
            }
            Ok(Err(err)) => {
                let _ = resp.send(Err(err));
            }
            Err(_) => {
                let _ = resp.send(Err(EngineError::PipelineChannelClosed));
            }
        }
    }

    /// Mengirimkan transaksi mutasi ke dalam pipeline dan menunggu tanda terima komitmen.
    pub fn submit(&self, record: MutationRecord) -> Result<CommitReceipt, EngineError> {
        if self.shutdown_signal.load(Ordering::SeqCst) {
            return Err(EngineError::PipelineChannelClosed);
        }

        let (response_tx, response_rx) = sync_channel(1);
        let envelope = RawTxEnvelope {
            record,
            response_tx,
        };

        let tx = self
            .stage1_tx
            .as_ref()
            .ok_or(EngineError::PipelineChannelClosed)?;

        tx.send(envelope)
            .map_err(|_| EngineError::PipelineChannelClosed)?;

        response_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|err| match err {
                std::sync::mpsc::RecvTimeoutError::Timeout => EngineError::PipelineChannelClosed,
                std::sync::mpsc::RecvTimeoutError::Disconnected => {
                    EngineError::PipelineChannelClosed
                }
            })?
    }

    /// Mengirimkan mutasi secara non-blocking; jika antrean Stage 1 penuh, langsung mengembalikan `PipelineQueueFull`.
    pub fn try_submit(&self, record: MutationRecord) -> Result<CommitReceipt, EngineError> {
        if self.shutdown_signal.load(Ordering::SeqCst) {
            return Err(EngineError::PipelineChannelClosed);
        }

        let (response_tx, response_rx) = sync_channel(1);
        let envelope = RawTxEnvelope {
            record,
            response_tx,
        };

        let tx = self
            .stage1_tx
            .as_ref()
            .ok_or(EngineError::PipelineChannelClosed)?;

        match tx.try_send(envelope) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(EngineError::PipelineQueueFull),
            Err(TrySendError::Disconnected(_)) => return Err(EngineError::PipelineChannelClosed),
        }

        response_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|err| match err {
                std::sync::mpsc::RecvTimeoutError::Timeout => EngineError::PipelineChannelClosed,
                std::sync::mpsc::RecvTimeoutError::Disconnected => {
                    EngineError::PipelineChannelClosed
                }
            })?
    }

    /// Menanyakan saldo akun terkini ke Keydir secara deterministik.
    pub fn query_balance(&self, account: &AccountId) -> Result<AurValue, EngineError> {
        if self.shutdown_signal.load(Ordering::SeqCst) {
            return Err(EngineError::PipelineChannelClosed);
        }
        let guard = self
            .index_shared
            .read()
            .map_err(|_| EngineError::WorkerThreadPanicked)?;
        Ok(guard.get_balance(account))
    }

    /// Mengambil total pasokan koin global terkini dari Keydir.
    pub fn query_total_supply(&self) -> Result<AurValue, EngineError> {
        if self.shutdown_signal.load(Ordering::SeqCst) {
            return Err(EngineError::PipelineChannelClosed);
        }
        let guard = self
            .index_shared
            .read()
            .map_err(|_| EngineError::WorkerThreadPanicked)?;
        Ok(guard.total_supply())
    }

    /// Mengambil akumulasi jumlah saldo pada seluruh bucket Keydir untuk verifikasi invarian moneter.
    pub fn query_sum_balances(&self) -> Result<AurValue, EngineError> {
        if self.shutdown_signal.load(Ordering::SeqCst) {
            return Err(EngineError::PipelineChannelClosed);
        }
        let guard = self
            .index_shared
            .read()
            .map_err(|_| EngineError::WorkerThreadPanicked)?;
        Ok(guard.sum_bucket_balances())
    }

    /// Menghitung estimasi block reward berdasarkan pasokan total terkini.
    pub fn compute_block_reward(&self) -> Result<AurValue, EngineError> {
        if self.shutdown_signal.load(Ordering::SeqCst) {
            return Err(EngineError::PipelineChannelClosed);
        }
        let guard = self
            .index_shared
            .read()
            .map_err(|_| EngineError::WorkerThreadPanicked)?;
        Ok(guard.compute_block_reward())
    }

    /// Mengambil nilai akumulasi fee yang dibakar dan fee tip saat ini.
    pub fn cumulative_fees(&self) -> Result<(AurValue, AurValue), EngineError> {
        if self.shutdown_signal.load(Ordering::SeqCst) {
            return Err(EngineError::PipelineChannelClosed);
        }
        let (resp_tx, resp_rx) = sync_channel(1);
        let s2_tx = self
            .stage2_tx
            .as_ref()
            .ok_or(EngineError::PipelineChannelClosed)?;
        s2_tx
            .send(Stage2Message::CumulativeFees(resp_tx))
            .map_err(|_| EngineError::PipelineChannelClosed)?;
        resp_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| EngineError::PipelineChannelClosed)
    }

    /// Mengomitsikan slot / blok secara final ke akun validator leader:
    /// - Mengalokasikan block reward inflasi dinamis + akumulasi tips ke leader.
    /// - Memusnahkan akumulasi fee burn dari pasokan total.
    /// - Mengemisikan record biner sintetis ke berkas segmen disk secara atomik.
    pub fn commit_slot(&self, leader: &AccountId) -> Result<SlotReceipt, EngineError> {
        if self.shutdown_signal.load(Ordering::SeqCst) {
            return Err(EngineError::PipelineChannelClosed);
        }
        let (resp_tx, resp_rx) = sync_channel(1);
        let s2_tx = self
            .stage2_tx
            .as_ref()
            .ok_or(EngineError::PipelineChannelClosed)?;
        s2_tx
            .send(Stage2Message::CommitSlot(*leader, resp_tx))
            .map_err(|_| EngineError::PipelineChannelClosed)?;
        resp_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| EngineError::PipelineChannelClosed)?
    }

    /// Menghentikan operasi pipeline secara graceful:
    /// - Menguras seluruh antrean yang sedang berjalan.
    /// - Menulis batch aktif ke disk.
    /// - Menyegel segmen log aktif.
    /// - Menggabungkan (join) seluruh thread worker.
    pub fn shutdown(mut self) -> Result<(), EngineError> {
        self.shutdown_signal.store(true, Ordering::SeqCst);
        drop(self.stage1_tx.take());

        let stage1_handles = std::mem::take(&mut self.stage1_handles);
        for handle in stage1_handles {
            handle
                .join()
                .map_err(|_| EngineError::WorkerThreadPanicked)?;
        }

        drop(self.stage2_tx.take());

        if let Some(handle) = self.stage2_handle.take() {
            handle
                .join()
                .map_err(|_| EngineError::WorkerThreadPanicked)?;
        }

        if let Some(handle) = self.stage3_handle.take() {
            handle
                .join()
                .map_err(|_| EngineError::WorkerThreadPanicked)?;
        }

        Ok(())
    }
}
