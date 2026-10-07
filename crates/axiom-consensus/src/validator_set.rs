//! Modul himpunan validator aktif dan kalkulasi ambang kuorum supermayoritas.

use std::collections::BTreeMap;

use axiom_primitives::crypto::AccountId;

use crate::error::ConsensusError;

/// Informasi konfigurasi validator individual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorInfo {
    /// Identitas kunci publik validator (32 byte).
    pub account_id: AccountId,
    /// Bobot suara hak pilih berbasis integer integer murni.
    pub voting_weight: u64,
}

impl ValidatorInfo {
    /// Mengonstruksi ValidatorInfo baru.
    #[inline]
    pub const fn new(account_id: AccountId, voting_weight: u64) -> Self {
        Self {
            account_id,
            voting_weight,
        }
    }
}

/// Himpunan validator aktif terdaftar yang berhak memberikan suara konsensus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorSet {
    /// Pemetaan identitas validator ke bobot suaranya (menggunakan BTreeMap untuk determinisme mutlak).
    validators: BTreeMap<AccountId, u64>,
    /// Total agregat bobot suara seluruh validator terdaftar.
    total_weight: u64,
}

impl ValidatorSet {
    /// Mengonstruksi ValidatorSet baru dari daftar validator.
    ///
    /// Menolak jika ada validator berbobot nol, akun duplikat, atau terjadi luapan penjumlahan bobot.
    pub fn new(validators: Vec<ValidatorInfo>) -> Result<Self, ConsensusError> {
        if validators.is_empty() {
            return Err(ConsensusError::InsufficientQuorum);
        }

        let mut map = BTreeMap::new();
        let mut total_weight: u64 = 0;

        for v in validators {
            if v.voting_weight == 0 {
                return Err(ConsensusError::ZeroWeightValidator);
            }

            if map.insert(v.account_id, v.voting_weight).is_some() {
                return Err(ConsensusError::DuplicateVote);
            }

            total_weight = total_weight
                .checked_add(v.voting_weight)
                .ok_or(ConsensusError::ArithmeticOverflow)?;
        }

        Ok(Self {
            validators: map,
            total_weight,
        })
    }

    /// Menghitung ambang batas kuorum supermayoritas: `(total_weight * 2) / 3 + 1`.
    ///
    /// Seluruh operasi dilakukan dengan aritmatika integer terproteksi untuk menjamin ketiadaan float.
    pub fn quorum_threshold(&self) -> u64 {
        self.total_weight
            .checked_mul(2)
            .and_then(|w| w.checked_div(3))
            .and_then(|w| w.checked_add(1))
            .unwrap_or(u64::MAX)
    }

    /// Memeriksa apakah suatu akun terdaftar sebagai validator aktif.
    #[inline]
    pub fn contains(&self, account: &AccountId) -> bool {
        self.validators.contains_key(account)
    }

    /// Mengambil bobot hak pilih suatu validator jika terdaftar.
    #[inline]
    pub fn get_weight(&self, account: &AccountId) -> Option<u64> {
        self.validators.get(account).copied()
    }

    /// Mengambil total bobot keseluruhan validator.
    #[inline]
    pub fn total_weight(&self) -> u64 {
        self.total_weight
    }

    /// Referensi pemetaan validator internal.
    #[inline]
    pub fn validators(&self) -> &BTreeMap<AccountId, u64> {
        &self.validators
    }

    /// Jumlah validator yang terdaftar.
    #[inline]
    pub fn len(&self) -> usize {
        self.validators.len()
    }

    /// Memeriksa apakah himpunan validator kosong.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.validators.is_empty()
    }
}
