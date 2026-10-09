#![forbid(unsafe_code)]

//! End-to-End Stress Test & Throughput Saturation Suite (E2E-BENCH-01)
//!
//! Validasi saturasi beban penuh pada pipeline eksekusi 3-tahap (Stage 1 Verifier,
//! Stage 2 Sequencer, Stage 3 Storage Writer) dengan media simpan dingin disk (ColdStore).

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ratu_aurion_engine::error::EngineError;
use ratu_aurion_engine::pipeline::PipelineCoordinator;
use ratu_aurion_index::entry::AccountLocation;
use ratu_aurion_index::keydir::Keydir;
use ratu_aurion_index::replay::rebuild_index_from_segment;
use ratu_aurion_primitives::{
    crypto::{AccountId, Signature},
    record::{MutationRecord, RECORD_KIND_TRANSFER},
    value::AurValue,
};
use ratu_aurion_storage::reader::SegmentReader;
use ratu_aurion_storage::writer::SegmentWriter;
use ed25519_dalek::{Signer, SigningKey};

fn unique_test_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ratu_aurion_e2e_sat_{label}_{nanos}"));
    fs::create_dir_all(&path).expect("Create test dir");
    path
}

fn create_keypair(seed_byte: u8) -> (SigningKey, AccountId) {
    let mut seed = [0u8; 32];
    seed[0] = seed_byte;
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();
    let account = AccountId::new(verifying_key.to_bytes());
    (signing_key, account)
}

fn sign_mutation(
    signing_key: &SigningKey,
    epoch: u64,
    seq: u64,
    sender: AccountId,
    recipient: AccountId,
    amount_atomic: u128,
) -> MutationRecord {
    let mut record = MutationRecord {
        epoch,
        sequence_number: seq,
        record_kind: RECORD_KIND_TRANSFER,
        sender,
        recipient,
        amount: AurValue::from_atomic(amount_atomic),
        signature: Signature::ZERO,
    };
    let payload = record.signing_payload();
    let dalek_sig = signing_key.sign(&payload);
    record.signature = Signature::new(dalek_sig.to_bytes());
    record
}

#[test]
fn test_engine_saturation_sustained_throughput() {
    let dir = unique_test_dir("sustained_throughput");
    let seg_path = dir.join("epoch_1_seg_0.log");
    let cold_path = dir.join("cold_state.idx");

    let writer = SegmentWriter::create(&seg_path, 1, 1, 0).expect("Create writer");
    let mut keydir = Keydir::with_budget(&cold_path, 100).expect("Create keydir with 100 hot budget");

    let initial_balance = AurValue::from_whole_aur(1_000).expect("1,000 AUR");
    let transfer_amount = AurValue::from_whole_aur(1).expect("1 AUR").to_atomic();

    // 1. Seed 20 client accounts dengan 1,000 AUR per akun
    let mut clients = Vec::with_capacity(20);
    for i in 0..20 {
        let (signing_key, account) = create_keypair(0x10 + i as u8);
        keydir.seed_account(
            account,
            initial_balance,
            AccountLocation::new(1, 0, 0, 0),
        );
        clients.push((signing_key, account));
    }

    let expected_total_supply = AurValue::from_whole_aur(20_000).expect("20,000 AUR");
    assert_eq!(keydir.total_supply(), expected_total_supply);

    // 2. Spawn PipelineCoordinator dengan 8 verifier threads
    let coordinator = Arc::new(
        PipelineCoordinator::spawn(writer, keydir, 8).expect("Spawn pipeline coordinator"),
    );

    let start_wall_clock = Instant::now();

    // 3. Spawn 10 concurrent client worker threads, masing-masing mengirimkan 200 transaksi berurutan (total 2,000 tx)
    let mut worker_handles = Vec::with_capacity(10);
    for thread_idx in 0..10 {
        let coord = Arc::clone(&coordinator);
        let sender_key = clients[thread_idx].0.clone();
        let sender = clients[thread_idx].1;
        let recipient = clients[(thread_idx + 10) % 20].1;

        let handle = std::thread::spawn(move || {
            let mut receipts = Vec::with_capacity(200);
            for seq in 1..=200 {
                let tx = sign_mutation(
                    &sender_key,
                    1,
                    seq,
                    sender,
                    recipient,
                    transfer_amount,
                );
                let receipt = coord.submit(tx).expect("Submit valid tx under saturation");
                assert_eq!(receipt.sequence_number, seq, "Sequence must match strictly");
                receipts.push(receipt);
            }
            receipts
        });
        worker_handles.push(handle);
    }

    let mut total_committed_tx: usize = 0;
    for handle in worker_handles {
        let receipts = handle.join().expect("Worker thread join");
        total_committed_tx += receipts.len();
    }

    let elapsed_duration = start_wall_clock.elapsed();
    let elapsed_ms = elapsed_duration.as_millis().max(1);
    let tps_integer = (total_committed_tx as u128 * 1_000) / elapsed_ms;

    // 4. Assert 2,000 transaksi berhasil dikomit dengan nomor urut yang tepat
    assert_eq!(total_committed_tx, 2_000, "All 2,000 transactions must be committed");

    // 5. Assert pasokan koin global tetap terkonservasi murni
    let mut query_sum_atomic: u128 = 0;
    for (_, account) in &clients {
        let bal = coordinator.query_balance(account).expect("Query account balance");
        query_sum_atomic = query_sum_atomic.checked_add(bal.to_atomic()).expect("Supply overflow");
    }
    assert_eq!(
        AurValue::from_atomic(query_sum_atomic),
        expected_total_supply,
        "Total monetary supply must be conserved exactly at 20,000 AUR"
    );

    // 6. Cetak metrik hasil uji saturasi
    println!("\n================================================================================");
    println!("=== RATU AURION PIPELINE SUSTAINED THROUGHPUT BENCHMARK (E2E-BENCH-01) ===");
    println!("================================================================================");
    println!("Total Mutasi Transaksi  : {} tx (161-byte per record)", total_committed_tx);
    println!("Konfigurasi Pipeline    : 8 verifier threads, 1 sequencer, 1 storage writer");
    println!("Kapasitas RAM Keydir    : 100 hot accounts (didukung ColdStore)");
    println!("Durasi Total Eksekusi   : {} ms", elapsed_ms);
    println!("Throughput Rata-rata    : {} TPS (integer transactions-per-second)", tps_integer);
    println!("Konservasi Pasokan Koin : {} AUR (Invarian Terverifikasi)", expected_total_supply);
    println!("================================================================================\n");

    // 7. Graceful Shutdown & Replay Verification
    let coordinator_instance = Arc::try_unwrap(coordinator)
        .map_err(|_| "Arc still held")
        .expect("Coordinator unwrapped successfully");
    coordinator_instance.shutdown().expect("Coordinator shutdown cleanly");

    let mut reader = SegmentReader::open(&seg_path).expect("Open sealed segment");
    let footer = reader.read_footer().expect("Read footer");
    assert_eq!(footer.total_records, 2_000);

    let mut replay_keydir = Keydir::new();
    for (_, account) in &clients {
        replay_keydir.seed_account(*account, initial_balance, AccountLocation::new(1, 0, 0, 0));
    }
    let replayed = rebuild_index_from_segment(&mut reader, &mut replay_keydir, 1, 0).expect("Replay segment");
    assert_eq!(replayed, 2_000);
    assert_eq!(replay_keydir.total_supply(), expected_total_supply);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_engine_saturation_with_fault_injection() {
    let dir = unique_test_dir("fault_injection");
    let seg_path = dir.join("epoch_1_seg_0.log");
    let cold_path = dir.join("cold_state.idx");

    let writer = SegmentWriter::create(&seg_path, 1, 1, 0).expect("Create writer");
    let mut keydir = Keydir::with_budget(&cold_path, 100).expect("Create keydir with 100 hot budget");

    let initial_balance = AurValue::from_whole_aur(1_000).expect("1,000 AUR");
    let valid_amount = AurValue::from_whole_aur(1).expect("1 AUR").to_atomic();
    let overdraft_amount = AurValue::from_whole_aur(5_000).expect("5,000 AUR").to_atomic();

    // Seed 10 sender accounts + 10 recipient accounts
    let mut senders = Vec::with_capacity(10);
    for i in 0..10 {
        let (signing_key, account) = create_keypair(0x30 + i as u8);
        keydir.seed_account(
            account,
            initial_balance,
            AccountLocation::new(1, 0, 0, 0),
        );
        senders.push((signing_key, account));
    }

    let mut recipients = Vec::with_capacity(10);
    for i in 0..10 {
        let (_, account) = create_keypair(0x70 + i as u8);
        recipients.push(account);
    }

    let coordinator = Arc::new(
        PipelineCoordinator::spawn(writer, keydir, 8).expect("Spawn pipeline coordinator"),
    );

    // Spawn 10 client threads: masing-masing melakukan 100 submission attempts
    // Pola per 10 transaksi (100 total attempts per worker):
    // - attempt % 10 == 1: 10% Corrupted signature (Stage 1 reject)
    // - attempt % 10 == 2: 10% Overdraft transfer (Stage 2 reject)
    // - attempt % 10 == 3: 10% Stale sequence number (Stage 2 reject)
    // - sisanya (70%): Valid transaction (Committed Ok)
    let mut handles = Vec::with_capacity(10);
    for thread_idx in 0..10 {
        let coord = Arc::clone(&coordinator);
        let sender_key = senders[thread_idx].0.clone();
        let sender = senders[thread_idx].1;
        let recipient = recipients[thread_idx];

        let handle = std::thread::spawn(move || {
            let mut valid_seq = 1u64;
            let mut valid_commits = 0usize;
            let mut sig_errors = 0usize;
            let mut balance_errors = 0usize;
            let mut stale_seq_errors = 0usize;

            for step in 0..100 {
                match step % 10 {
                    1 => {
                        // 10% Corrupted signature injection
                        let mut bad_sig_tx = sign_mutation(
                            &sender_key,
                            1,
                            valid_seq,
                            sender,
                            recipient,
                            valid_amount,
                        );
                        bad_sig_tx.signature = Signature::new([0xDE; 64]);
                        match coord.submit(bad_sig_tx) {
                            Err(EngineError::InvalidSignature) => sig_errors += 1,
                            other => panic!("Expected InvalidSignature, got: {:?}", other),
                        }
                    }
                    2 => {
                        // 10% Overdraft transfer injection
                        let overdraft_tx = sign_mutation(
                            &sender_key,
                            1,
                            valid_seq,
                            sender,
                            recipient,
                            overdraft_amount,
                        );
                        match coord.submit(overdraft_tx) {
                            Err(EngineError::InsufficientBalance { .. }) => balance_errors += 1,
                            other => panic!("Expected InsufficientBalance, got: {:?}", other),
                        }
                    }
                    3 => {
                        // 10% Stale sequence number injection
                        let stale_seq = valid_seq.saturating_sub(1);
                        let stale_tx = sign_mutation(
                            &sender_key,
                            1,
                            stale_seq,
                            sender,
                            recipient,
                            valid_amount,
                        );
                        match coord.submit(stale_tx) {
                            Err(EngineError::StaleSequenceNumber { .. }) => stale_seq_errors += 1,
                            other => panic!("Expected StaleSequenceNumber, got: {:?}", other),
                        }
                    }
                    _ => {
                        // 70% Valid transactions
                        let tx = sign_mutation(
                            &sender_key,
                            1,
                            valid_seq,
                            sender,
                            recipient,
                            valid_amount,
                        );
                        let receipt = coord.submit(tx).expect("Valid transaction must commit");
                        assert_eq!(receipt.sequence_number, valid_seq);
                        valid_seq += 1;
                        valid_commits += 1;
                    }
                }
            }

            (valid_commits, sig_errors, balance_errors, stale_seq_errors)
        });
        handles.push(handle);
    }

    let mut total_valid = 0usize;
    let mut total_sig_err = 0usize;
    let mut total_bal_err = 0usize;
    let mut total_seq_err = 0usize;

    for handle in handles {
        let (valid, sig_err, bal_err, seq_err) = handle.join().expect("Join thread");
        total_valid += valid;
        total_sig_err += sig_err;
        total_bal_err += bal_err;
        total_seq_err += seq_err;
    }

    assert_eq!(total_valid, 700, "Exactly 700 valid transactions committed");
    assert_eq!(total_sig_err, 100, "Exactly 100 invalid signatures rejected fail-fast");
    assert_eq!(total_bal_err, 100, "Exactly 100 overdrafts rejected fail-fast");
    assert_eq!(total_seq_err, 100, "Exactly 100 stale sequences rejected fail-fast");
    let total_rejected = total_sig_err + total_bal_err + total_seq_err;
    assert_eq!(total_rejected, 300, "Exactly 30% faulty transactions rejected");

    let coordinator_instance = Arc::try_unwrap(coordinator)
        .map_err(|_| "Arc still held")
        .expect("Coordinator unwrapped");
    coordinator_instance.shutdown().expect("Coordinator shutdown cleanly");

    let mut reader = SegmentReader::open(&seg_path).expect("Open segment");
    let footer = reader.read_footer().expect("Read footer");
    assert_eq!(footer.total_records, 700, "Disk segment must only contain valid 700 records");

    let _ = fs::remove_dir_all(&dir);
}
