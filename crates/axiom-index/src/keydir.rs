//! Modul tabel indeks in-memory (Keydir) terkompresi dengan topologi partisi datar 256-bucket.

use std::path::Path;

use axiom_primitives::crypto::AccountId;
use axiom_primitives::record::MutationRecord;
use axiom_primitives::value::AxmValue;

use crate::entry::{AccountLocation, AccountState, CompactAccountEntry};
use crate::error::IndexError;
use crate::replay::rebuild_index_from_segment;

/// Jumlah partisi (bucket) berbasis prefix byte pertama dari AccountId (256 partisi).
pub const BUCKET_COUNT: usize = 256;

/// Tabel indeks in-memory Bitcask terpadatkan dengan topologi partisi datar 256-bucket.
///
/// Setiap akun menempati tepat 80 byte pada array datar tersortir di dalam bucket yang sesuai
/// dengan byte pertama `AccountId`. Kompleksitas pencarian adalah O(log2(N / 256)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keydir {
    pub(crate) buckets: Box<[Vec<CompactAccountEntry>; BUCKET_COUNT]>,
    pub(crate) total_accounts: usize,
    pub(crate) total_supply: AxmValue,
}

impl Default for Keydir {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Keydir {
    /// Membuat instance Keydir baru yang kosong di memori RAM.
    #[inline]
    pub fn new() -> Self {
        Self {
            buckets: Box::new(core::array::from_fn(|_| Vec::new())),
            total_accounts: 0,
            total_supply: AxmValue::ZERO,
        }
    }

    /// Menghitung indeks bucket berdasarkan byte pertama AccountId.
    #[inline]
    pub fn bucket_index_for(account: &AccountId) -> usize {
        account.as_bytes()[0] as usize
    }

    /// Total akun terdaftar dalam seluruh partisi indeks RAM dalam O(1).
    #[inline]
    pub fn account_count(&self) -> usize {
        self.total_accounts
    }

    /// Total pasokan koin global teragregasi secara O(1).
    #[inline]
    pub fn total_supply(&self) -> AxmValue {
        self.total_supply
    }

    /// Jumlah entri yang terdaftar dalam indeks RAM (sinonim `account_count`).
    #[inline]
    pub fn len(&self) -> usize {
        self.total_accounts
    }

    /// Apakah indeks RAM kosong.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.total_accounts == 0
    }

    /// Jumlah entri pada bucket partisi tertentu.
    #[inline]
    pub fn bucket_len(&self, bucket_idx: usize) -> usize {
        if bucket_idx < BUCKET_COUNT {
            self.buckets[bucket_idx].len()
        } else {
            0
        }
    }

    /// Mengambil saldo moneter akun terkini. Mengembalikan `AxmValue::ZERO` jika akun belum terdaftar.
    #[inline]
    pub fn get_balance(&self, account: &AccountId) -> AxmValue {
        let b_idx = Self::bucket_index_for(account);
        let bucket = &self.buckets[b_idx];
        match bucket.binary_search_by(|entry| entry.account.cmp(account)) {
            Ok(idx) => bucket[idx].balance,
            Err(_) => AxmValue::ZERO,
        }
    }

    /// Mengambil penunjuk lokasi disk rekaman mutasi terakhir akun.
    #[inline]
    pub fn get_location(&self, account: &AccountId) -> Option<AccountLocation> {
        let b_idx = Self::bucket_index_for(account);
        let bucket = &self.buckets[b_idx];
        match bucket.binary_search_by(|entry| entry.account.cmp(account)) {
            Ok(idx) => Some(bucket[idx].location()),
            Err(_) => None,
        }
    }

    /// Mengambil status lengkap akun (saldo dan lokasi) yang dikonversi dari entri padat.
    #[inline]
    pub fn get_account_state(&self, account: &AccountId) -> Option<AccountState> {
        let b_idx = Self::bucket_index_for(account);
        let bucket = &self.buckets[b_idx];
        match bucket.binary_search_by(|entry| entry.account.cmp(account)) {
            Ok(idx) => Some(bucket[idx].to_account_state()),
            Err(_) => None,
        }
    }

    /// Mengambil referensi langsung ke entri padat 80-byte akun jika ada.
    #[inline]
    pub fn get_entry(&self, account: &AccountId) -> Option<&CompactAccountEntry> {
        let b_idx = Self::bucket_index_for(account);
        let bucket = &self.buckets[b_idx];
        match bucket.binary_search_by(|entry| entry.account.cmp(account)) {
            Ok(idx) => Some(&bucket[idx]),
            Err(_) => None,
        }
    }

    /// Menambahkan akun awal (misal genesis atau injeksi saldo pengujian).
    pub fn seed_account(
        &mut self,
        account: AccountId,
        balance: AxmValue,
        location: AccountLocation,
    ) {
        let b_idx = Self::bucket_index_for(&account);
        let bucket = &mut self.buckets[b_idx];
        match bucket.binary_search_by(|entry| entry.account.cmp(&account)) {
            Ok(idx) => {
                let entry = &mut bucket[idx];
                let old_balance = entry.balance;
                entry.balance = balance;
                entry.sequence_number = location.sequence_number;
                entry.epoch = location.epoch;
                entry.segment_index = location.segment_idx;
                entry.offset = location.offset;

                if balance >= old_balance {
                    if let Some(diff) = balance.checked_sub(old_balance) {
                        if let Some(new_supply) = self.total_supply.checked_add(diff) {
                            self.total_supply = new_supply;
                        }
                    }
                } else if let Some(diff) = old_balance.checked_sub(balance) {
                    if let Some(new_supply) = self.total_supply.checked_sub(diff) {
                        self.total_supply = new_supply;
                    }
                }
            }
            Err(insert_idx) => {
                bucket.insert(
                    insert_idx,
                    CompactAccountEntry {
                        account,
                        balance,
                        sequence_number: location.sequence_number,
                        epoch: location.epoch,
                        offset: location.offset,
                        segment_index: location.segment_idx,
                        flags: 0,
                    },
                );
                self.total_accounts += 1;
                if let Some(new_supply) = self.total_supply.checked_add(balance) {
                    self.total_supply = new_supply;
                }
            }
        }
    }

    /// Menerapkan satu rekaman mutasi ke dalam indeks in-memory.
    ///
    /// Validasi:
    /// - Sequence number mutasi wajib lebih tinggi dari sequence number akun pengirim sebelumnya.
    /// - Saldo akun pengirim wajib mencukupi nominal mutasi.
    /// - Saldo akun penerima ditambahkan secara terproteksi terhadap overflow.
    /// - Pointer lokasi disk dan sequence number diperbarui in-place tanpa duplikasi entri.
    /// - Pasokan total koin global (`total_supply`) terkonservasi murni secara deterministik.
    pub fn apply_mutation(
        &mut self,
        record: &MutationRecord,
        epoch: u64,
        segment_index: u32,
        offset: u64,
    ) -> Result<(), IndexError> {
        let sender_bucket_idx = Self::bucket_index_for(&record.sender);
        let recipient_bucket_idx = Self::bucket_index_for(&record.recipient);

        // 1. Validasi akun pengirim (sequence number dan saldo)
        let sender_entry_opt = {
            let bucket = &self.buckets[sender_bucket_idx];
            bucket
                .binary_search_by(|e| e.account.cmp(&record.sender))
                .ok()
                .map(|idx| bucket[idx])
        };

        if let Some(sender_entry) = sender_entry_opt {
            if record.sequence_number <= sender_entry.sequence_number {
                return Err(IndexError::StaleSequenceNumber);
            }
        }

        let sender_balance = sender_entry_opt.map_or(AxmValue::ZERO, |e| e.balance);
        let new_sender_balance = sender_balance
            .checked_sub(record.amount)
            .ok_or(IndexError::InsufficientBalance)?;

        // 2. Validasi akun penerima (overflow saldo)
        let recipient_entry_opt = {
            let bucket = &self.buckets[recipient_bucket_idx];
            bucket
                .binary_search_by(|e| e.account.cmp(&record.recipient))
                .ok()
                .map(|idx| bucket[idx])
        };

        let recipient_balance = recipient_entry_opt.map_or(AxmValue::ZERO, |e| e.balance);
        let new_recipient_balance = recipient_balance
            .checked_add(record.amount)
            .ok_or(IndexError::ArithmeticOverflow)?;

        // Kasus transfer ke diri sendiri: saldo akhir tetap sama dengan saldo awal
        if record.sender == record.recipient {
            let bucket = &mut self.buckets[sender_bucket_idx];
            match bucket.binary_search_by(|e| e.account.cmp(&record.sender)) {
                Ok(idx) => {
                    let entry = &mut bucket[idx];
                    entry.sequence_number = record.sequence_number;
                    entry.epoch = epoch;
                    entry.segment_index = segment_index;
                    entry.offset = offset;
                }
                Err(insert_idx) => {
                    bucket.insert(
                        insert_idx,
                        CompactAccountEntry {
                            account: record.sender,
                            balance: sender_balance,
                            sequence_number: record.sequence_number,
                            epoch,
                            offset,
                            segment_index,
                            flags: 0,
                        },
                    );
                    self.total_accounts += 1;
                }
            }
            return Ok(());
        }

        // Tentukan sequence number penerima: adopsi yang lebih tinggi jika sudah pernah ada
        let recipient_seq = recipient_entry_opt
            .map_or(record.sequence_number, |e| {
                e.sequence_number.max(record.sequence_number)
            });

        // 3. Perbarui entri pengirim
        {
            let sender_bucket = &mut self.buckets[sender_bucket_idx];
            match sender_bucket.binary_search_by(|e| e.account.cmp(&record.sender)) {
                Ok(idx) => {
                    let entry = &mut sender_bucket[idx];
                    entry.balance = new_sender_balance;
                    entry.sequence_number = record.sequence_number;
                    entry.epoch = epoch;
                    entry.segment_index = segment_index;
                    entry.offset = offset;
                }
                Err(insert_idx) => {
                    sender_bucket.insert(
                        insert_idx,
                        CompactAccountEntry {
                            account: record.sender,
                            balance: new_sender_balance,
                            sequence_number: record.sequence_number,
                            epoch,
                            offset,
                            segment_index,
                            flags: 0,
                        },
                    );
                    self.total_accounts += 1;
                }
            }
        }

        // 4. Perbarui entri penerima
        {
            let recipient_bucket = &mut self.buckets[recipient_bucket_idx];
            match recipient_bucket.binary_search_by(|e| e.account.cmp(&record.recipient)) {
                Ok(idx) => {
                    let entry = &mut recipient_bucket[idx];
                    entry.balance = new_recipient_balance;
                    entry.sequence_number = recipient_seq;
                    entry.epoch = epoch;
                    entry.segment_index = segment_index;
                    entry.offset = offset;
                }
                Err(insert_idx) => {
                    recipient_bucket.insert(
                        insert_idx,
                        CompactAccountEntry {
                            account: record.recipient,
                            balance: new_recipient_balance,
                            sequence_number: recipient_seq,
                            epoch,
                            offset,
                            segment_index,
                            flags: 0,
                        },
                    );
                    self.total_accounts += 1;
                }
            }
        }

        // Catatan: total_supply tidak berubah karena mutasi transfer mengonservasi saldo total.
        Ok(())
    }

    /// Membaca dan merekonstruksi indeks dari berkas segmen log di disk.
    pub fn replay_segment<P: AsRef<Path>>(&mut self, path: P) -> Result<u64, IndexError> {
        let mut reader = axiom_storage::reader::SegmentReader::open(path)?;
        let epoch = reader.header().epoch;
        let segment_idx = reader.header().segment_index;
        rebuild_index_from_segment(&mut reader, self, epoch, segment_idx)
    }

    /// Iterator deterministik melintasi seluruh akun di 256 bucket (terurut per bucket).
    pub fn iter(&self) -> impl Iterator<Item = (&AccountId, AccountState)> {
        self.buckets.iter().flat_map(|bucket| {
            bucket
                .iter()
                .map(|entry| (&entry.account, entry.to_account_state()))
        })
    }

    /// Iterator referensi entri padat 80-byte.
    pub fn entries(&self) -> impl Iterator<Item = &CompactAccountEntry> {
        self.buckets.iter().flat_map(|bucket| bucket.iter())
    }

    /// Menyimpan checkpoint snapshot status in-memory Keydir secara atomik ke disk.
    pub fn save_checkpoint<P: AsRef<Path>>(
        &self,
        path: P,
        epoch: u64,
        segment_index: u32,
    ) -> Result<(), IndexError> {
        crate::snapshot::write_snapshot(self, path, epoch, segment_index)
    }

    /// Memuat checkpoint snapshot dari disk dan merekonstruksi Keydir dalam O(N).
    pub fn load_checkpoint<P: AsRef<Path>>(
        path: P,
    ) -> Result<(Self, crate::snapshot::SnapshotHeader), IndexError> {
        crate::snapshot::read_snapshot(path)
    }
}
