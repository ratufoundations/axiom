//! Modul eksekusi pemotongan pasak (slashing) dan pencabutan hak suara permanen (tombstoning).

use std::collections::BTreeSet;

use axiom_primitives::crypto::{AccountId, Signature};
use axiom_primitives::record::{MutationRecord, RECORD_KIND_SYSTEM_NOTIF};
use axiom_primitives::value::AxmValue;

use crate::error::ConsensusError;
use crate::evidence::{EquivocationEvidence, VoteRecord};

/// Penyebut basis poin perhitungan denda slashing (10.000 BPS = 100%).
pub const BPS_DENOMINATOR: u128 = 10_000;

/// Denda pemotongan pasak keras untuk pelanggaran fatal double-signing (100% = 10.000 BPS).
pub const HARD_SLASH_PENALTY_BPS: u128 = 10_000;

/// Buku besar pencatatan pemotongan pasak dan status tombstone validator.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlashingLedger {
    tombstones: BTreeSet<AccountId>,
    total_burned: AxmValue,
}

impl SlashingLedger {
    /// Membuat instance SlashingLedger baru yang kosong.
    #[inline]
    pub fn new() -> Self {
        Self {
            tombstones: BTreeSet::new(),
            total_burned: AxmValue::ZERO,
        }
    }

    /// Memeriksa apakah validator telah dikenai sanksi pencabutan hak permanen (tombstone).
    #[inline]
    pub fn is_tombstoned(&self, validator: &AccountId) -> bool {
        self.tombstones.contains(validator)
    }

    /// Total nilai koin AXM yang telah dimusnahkan (burned) akibat pemotongan pasak.
    #[inline]
    pub fn total_burned(&self) -> AxmValue {
        self.total_burned
    }

    /// Kumpulan identitas validator yang telah ditandai tombstone.
    #[inline]
    pub fn tombstones(&self) -> &BTreeSet<AccountId> {
        &self.tombstones
    }

    /// Memverifikasi bahwa validator belum ditandai tombstone.
    pub fn check_validator(&self, validator: &AccountId) -> Result<(), ConsensusError> {
        if self.is_tombstoned(validator) {
            return Err(ConsensusError::ValidatorTombstoned);
        }
        Ok(())
    }

    /// Mencatat suara validator dengan verifikasi status tombstone fail-fast.
    pub fn record_vote(&mut self, vote: &VoteRecord) -> Result<(), ConsensusError> {
        if self.is_tombstoned(&vote.validator) {
            return Err(ConsensusError::ValidatorTombstoned);
        }
        vote.verify_signature()?;
        Ok(())
    }

    /// Memproses bukti kecurangan ganda, memotong pasak secara deterministik,
    /// memasukkan validator ke tombstone set, dan menghasilkan MutationRecord notifikasi sistem.
    pub fn process_evidence(
        &mut self,
        evidence: &EquivocationEvidence,
        bonded_stake: AxmValue,
        penalty_bps: u128,
    ) -> Result<(AxmValue, MutationRecord), ConsensusError> {
        // 1. Validator tidak boleh sudah berada dalam status tombstone
        if self.is_tombstoned(&evidence.validator) {
            return Err(ConsensusError::ValidatorTombstoned);
        }

        // 2. Batas persentase penalti tidak boleh melebihi 100% (10.000 BPS)
        if penalty_bps > BPS_DENOMINATOR {
            return Err(ConsensusError::SlashingCalculationOverflow);
        }

        // 3. Verifikasi keabsahan bukti kriptografis secara menyeluruh
        evidence.verify()?;

        // 4. Hitung jumlah pasak yang dipotong menggunakan rasio integer deterministik
        let slashed_amount = bonded_stake
            .checked_mul_ratio(penalty_bps, BPS_DENOMINATOR)
            .ok_or(ConsensusError::SlashingCalculationOverflow)?;

        // 5. Akumulasi total koin yang dimusnahkan
        self.total_burned = self
            .total_burned
            .checked_add(slashed_amount)
            .ok_or(ConsensusError::ArithmeticOverflow)?;

        // 6. Masukkan validator ke himpunan tombstone
        self.tombstones.insert(evidence.validator);

        // 7. Konstruksi tanda tangan bukti sistem non-zero dari bukti penandatanganan suara
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(evidence.vote_a.signature.as_bytes());

        // 8. Hasilkan mutasi sistem resmi RECORD_KIND_SYSTEM_NOTIF
        let mutation = MutationRecord {
            epoch: evidence.epoch,
            sequence_number: 0,
            record_kind: RECORD_KIND_SYSTEM_NOTIF,
            sender: evidence.validator,
            recipient: AccountId::new([0u8; 32]), // Alamat pemusnahan (burn address)
            amount: slashed_amount,
            signature: Signature::new(sig_bytes),
        };

        Ok((slashed_amount, mutation))
    }
}
