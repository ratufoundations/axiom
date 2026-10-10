//! Modul orkestrator mesin eksekusi utama (EngineCoordinator).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ratu_aurion_archive::archiver::create_monthly_archive;
use ratu_aurion_archive::pruner::verify_and_prune_segment;
use ratu_aurion_index::entry::AccountLocation;
use ratu_aurion_index::error::IndexError;
use ratu_aurion_index::keydir::Keydir;
use ratu_aurion_primitives::crypto::{AccountId, Hash};
use ratu_aurion_primitives::record::MutationRecord;
use ratu_aurion_primitives::value::AurValue;
use ratu_aurion_storage::error::StorageError;
use ratu_aurion_storage::writer::SegmentWriter;

use crate::error::EngineError;
use crate::validator::verify_record_signature;

/// Koordinator transaksi utama yang menghubungkan kriptografi, storage, RAM index, dan pengarsipan.
pub struct EngineCoordinator {
    storage_writer: SegmentWriter,
    keydir: Keydir,
    current_epoch: u64,
    current_segment_idx: u32,
    data_dir: PathBuf,
    archive_dir: PathBuf,
}

impl EngineCoordinator {
    /// Menginisialisasi koordinator mesin baru dengan direktori data aktif dan direktori arsip.
    pub fn new<P: AsRef<Path>, Q: AsRef<Path>>(
        data_dir: P,
        archive_dir: Q,
        initial_epoch: u64,
    ) -> Result<Self, EngineError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        let archive_dir = archive_dir.as_ref().to_path_buf();

        fs::create_dir_all(&data_dir)?;
        fs::create_dir_all(&archive_dir)?;

        let current_segment_idx = 0;
        let seg_path = Self::format_segment_path(&data_dir, initial_epoch, current_segment_idx);
        let storage_writer = SegmentWriter::create(&seg_path, 1, initial_epoch, current_segment_idx)?;
        let keydir = Keydir::new();

        Ok(Self {
            storage_writer,
            keydir,
            current_epoch: initial_epoch,
            current_segment_idx,
            data_dir,
            archive_dir,
        })
    }

    /// Format jalur berkas segmen deterministik: `epoch_{epoch}_seg_{index}.log`.
    #[inline]
    fn format_segment_path(dir: &Path, epoch: u64, segment_idx: u32) -> PathBuf {
        dir.join(format!("epoch_{epoch}_seg_{segment_idx}.log"))
    }

    /// Memproses penyerahan mutasi transaksi:
    /// 1. Verifikasi tanda tangan kriptografis Ed25519.
    /// 2. Validasi pra-kondisi saldo pengirim dan urutan sequence pada Keydir.
    /// 3. Penulisan sekuensial linear ke SegmentWriter disk (merotasi segmen jika penuh).
    /// 4. Penerapan pembaruan status ke Keydir RAM menggunakan offset fisik disk.
    pub fn submit_transaction(&mut self, record: &MutationRecord) -> Result<u64, EngineError> {
        // 1. Validasi tanda tangan kriptografis
        verify_record_signature(record)?;

        // 2. Cek pra-kondisi saldo pengirim pada Keydir (tanpa mutasi RAM dahulu)
        let sender_balance = self.keydir.get_balance(&record.sender);
        if sender_balance < record.amount {
            return Err(EngineError::IndexError(IndexError::InsufficientBalance));
        }

        if let Some(state) = self.keydir.get_account_state(&record.sender) {
            if record.sequence_number <= state.location.sequence_number {
                return Err(EngineError::IndexError(IndexError::StaleSequenceNumber));
            }
        }

        // 3. Tulis record ke media disk aktif
        let offset = match self.storage_writer.append_record(record) {
            Ok(off) => off,
            Err(StorageError::SegmentFull) => {
                // Rotasi segmen internal jika kapasitas segmen aktif telah tercapai
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let digest = Hash::new([0xee; 32]);
                self.storage_writer.seal_segment(digest, now)?;

                self.current_segment_idx = self
                    .current_segment_idx
                    .checked_add(1)
                    .ok_or(EngineError::StorageError(StorageError::OutOfBounds))?;

                let new_path = Self::format_segment_path(
                    &self.data_dir,
                    self.current_epoch,
                    self.current_segment_idx,
                );
                self.storage_writer = SegmentWriter::create(
                    &new_path,
                    1,
                    self.current_epoch,
                    self.current_segment_idx,
                )?;
                self.storage_writer.append_record(record)?
            }
            Err(e) => return Err(EngineError::StorageError(e)),
        };

        // 4. Komit mutasi ke RAM Keydir dengan pointer offset fisik
        self.keydir.apply_mutation(
            record,
            self.current_epoch,
            self.current_segment_idx,
            offset,
        )?;

        Ok(offset)
    }

    /// Melakukan rotasi epoch (pergantian bulan/periode):
    /// 1. Menyegel segmen aktif bulan berjalan.
    /// 2. Mengemas berkas segmen ke dalam format arsip bulanan ZIP.
    /// 3. Memverifikasi integritas arsip dan memangkas/menghapus segmen lama dari disk aktif.
    /// 4. Membuka segmen baru untuk epoch baru dengan indeks segmen 0.
    pub fn rotate_epoch(&mut self, new_epoch: u64) -> Result<PathBuf, EngineError> {
        if new_epoch <= self.current_epoch {
            return Err(EngineError::EpochRegression);
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let digest = Hash::new([0xaa; 32]);

        // 1. Segel segmen aktif
        self.storage_writer.seal_segment(digest, now)?;

        let old_seg_path = Self::format_segment_path(
            &self.data_dir,
            self.current_epoch,
            self.current_segment_idx,
        );

        // 2. Buat arsip bulanan (.zip)
        let archive_zip_path = self
            .archive_dir
            .join(format!("archive_epoch_{}.zip", self.current_epoch));
        create_monthly_archive(self.current_epoch, &old_seg_path, &archive_zip_path)?;

        // 3. Verifikasi dan pangkas (prune) segmen lama dari disk aktif
        verify_and_prune_segment(&old_seg_path, &archive_zip_path)?;

        // 4. Buka segmen baru untuk epoch baru
        self.current_epoch = new_epoch;
        self.current_segment_idx = 0;
        let new_seg_path = Self::format_segment_path(
            &self.data_dir,
            self.current_epoch,
            self.current_segment_idx,
        );
        self.storage_writer = SegmentWriter::create(
            &new_seg_path,
            1,
            self.current_epoch,
            self.current_segment_idx,
        )?;

        Ok(archive_zip_path)
    }

    /// Mengambil saldo akun terkini dari indeks RAM.
    #[inline]
    pub fn query_balance(&self, account: &AccountId) -> AurValue {
        self.keydir.get_balance(account)
    }

    /// Mengambil lokasi mutasi disk terakhir akun dari indeks RAM.
    #[inline]
    pub fn query_last_location(&self, account: &AccountId) -> Option<AccountLocation> {
        self.keydir.get_location(account)
    }

    /// Menginjeksi saldo awal / genesis ke dalam Keydir.
    #[inline]
    pub fn seed_account(&mut self, account: AccountId, balance: AurValue) {
        self.keydir.seed_account(
            account,
            balance,
            AccountLocation::new(self.current_epoch, self.current_segment_idx, 0, 0),
        );
    }

    /// Mengambil epoch aktif saat ini.
    #[inline]
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Mengambil indeks segmen aktif saat ini.
    #[inline]
    pub fn current_segment_idx(&self) -> u32 {
        self.current_segment_idx
    }
}
