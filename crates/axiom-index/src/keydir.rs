//! Modul tabel indeks in-memory (Keydir) berbasis BTreeMap untuk determinisme replikasi.

use std::collections::BTreeMap;

use axiom_primitives::crypto::AccountId;
use axiom_primitives::record::MutationRecord;
use axiom_primitives::value::AxmValue;

use crate::entry::{AccountLocation, AccountState};
use crate::error::IndexError;

/// Tabel indeks in-memory Bitcask yang memetakan AccountId ke status saldo dan lokasi disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keydir {
    accounts: BTreeMap<AccountId, AccountState>,
}

impl Keydir {
    /// Membuat instance Keydir baru yang kosong di memori RAM.
    #[inline]
    pub fn new() -> Self {
        Self {
            accounts: BTreeMap::new(),
        }
    }

    /// Menerapkan satu rekaman mutasi ke dalam indeks in-memory.
    ///
    /// Validasi:
    /// - Sequence number mutasi wajib lebih tinggi dari sequence number akun pengirim sebelumnya.
    /// - Saldo akun pengirim wajib mencukupi nominal mutasi.
    /// - Saldo akun penerima ditambahkan secara terproteksi terhadap overflow.
    /// - Pointer lokasi disk diperbarui ke posisi byte record terkini.
    pub fn apply_mutation(
        &mut self,
        record: &MutationRecord,
        epoch: u64,
        segment_idx: u32,
        offset: u64,
    ) -> Result<(), IndexError> {
        // 1. Validasi sequence number pengirim (mencegah replay atau transaksi out-of-order)
        if let Some(sender_state) = self.accounts.get(&record.sender) {
            if record.sequence_number <= sender_state.location.sequence_number {
                return Err(IndexError::StaleSequenceNumber);
            }
        }

        // 2. Periksa kecukupan saldo pengirim
        let sender_balance = self.get_balance(&record.sender);
        let new_sender_balance = sender_balance
            .checked_sub(record.amount)
            .ok_or(IndexError::InsufficientBalance)?;

        // 3. Hitung saldo penerima terproteksi terhadap overflow
        let recipient_balance = self.get_balance(&record.recipient);
        let new_recipient_balance = recipient_balance
            .checked_add(record.amount)
            .ok_or(IndexError::ArithmeticOverflow)?;

        let sender_loc = AccountLocation::new(epoch, segment_idx, offset, record.sequence_number);

        // Kasus transfer ke diri sendiri: saldo akhir tetap sama dengan saldo awal
        if record.sender == record.recipient {
            self.accounts.insert(
                record.sender,
                AccountState::new(sender_balance, sender_loc),
            );
            return Ok(());
        }

        // Tentukan sequence penerima: pertahankan sequence penerima jika lebih tinggi, atau adopsi
        let recipient_seq = self
            .accounts
            .get(&record.recipient)
            .map_or(record.sequence_number, |s| {
                s.location.sequence_number.max(record.sequence_number)
            });
        let recipient_loc =
            AccountLocation::new(epoch, segment_idx, offset, recipient_seq);

        // 4. Perbarui status kedua akun di RAM
        self.accounts.insert(
            record.sender,
            AccountState::new(new_sender_balance, sender_loc),
        );
        self.accounts.insert(
            record.recipient,
            AccountState::new(new_recipient_balance, recipient_loc),
        );

        Ok(())
    }

    /// Mengambil saldo moneter akun terkini. Mengembalikan `AxmValue::ZERO` jika akun belum pernah bermutasi.
    #[inline]
    pub fn get_balance(&self, account: &AccountId) -> AxmValue {
        self.accounts
            .get(account)
            .map_or(AxmValue::ZERO, |state| state.balance)
    }

    /// Mengambil penunjuk lokasi disk rekaman mutasi terakhir akun.
    #[inline]
    pub fn get_location(&self, account: &AccountId) -> Option<AccountLocation> {
        self.accounts.get(account).map(|state| state.location)
    }

    /// Mengambil referensi status lengkap akun.
    #[inline]
    pub fn get_account_state(&self, account: &AccountId) -> Option<&AccountState> {
        self.accounts.get(account)
    }

    /// Menambahkan akun awal (misal untuk alokasi genesis atau injeksi saldo pengujian).
    #[inline]
    pub fn seed_account(
        &mut self,
        account: AccountId,
        balance: AxmValue,
        location: AccountLocation,
    ) {
        self.accounts
            .insert(account, AccountState::new(balance, location));
    }

    /// Total akun yang terdaftar dalam indeks RAM.
    #[inline]
    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    /// Apakah indeks RAM kosong.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    /// Iterator deterministik (terurut berdasarkan kunci AccountId).
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&AccountId, &AccountState)> {
        self.accounts.iter()
    }
}
