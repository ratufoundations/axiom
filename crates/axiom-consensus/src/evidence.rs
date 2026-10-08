//! Modul rekaman suara konsensus dan bukti kriptografis kecurangan ganda (EquivocationEvidence).

use axiom_primitives::crypto::{AccountId, Hash, Signature};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};

use crate::error::ConsensusError;

/// Ukuran fisik muatan penandatanganan suara konsensus (80 byte).
///
/// Komposisi:
/// - epoch: 8 byte (Little-Endian)
/// - round: 8 byte (Little-Endian)
/// - block_hash: 32 byte
/// - validator: 32 byte
pub const VOTE_SIGNING_PAYLOAD_SIZE: usize = 80;

/// Rekaman suara individu dari validator pada slot tertentu (epoch, round).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteRecord {
    /// Kunci publik validator yang memberikan suara (32 byte).
    pub validator: AccountId,
    /// Nomor epoch saat suara diberikan (8 byte).
    pub epoch: u64,
    /// Nomor ronde konsensus saat suara diberikan (8 byte).
    pub round: u64,
    /// Intisari kriptografis blok proposal yang disetujui (32 byte).
    pub block_hash: Hash,
    /// Tanda tangan digital kriptografis Ed25519 atas signing_payload (64 byte).
    pub signature: Signature,
}

impl VoteRecord {
    /// Mengonstruksi VoteRecord baru secara langsung.
    #[inline]
    pub const fn new(
        validator: AccountId,
        epoch: u64,
        round: u64,
        block_hash: Hash,
        signature: Signature,
    ) -> Self {
        Self {
            validator,
            epoch,
            round,
            block_hash,
            signature,
        }
    }

    /// Menghasilkan 80 byte muatan terdeterminasi untuk penandatanganan suara.
    #[inline]
    pub fn signing_payload(&self) -> [u8; VOTE_SIGNING_PAYLOAD_SIZE] {
        let mut payload = [0u8; VOTE_SIGNING_PAYLOAD_SIZE];
        payload[0..8].copy_from_slice(&self.epoch.to_le_bytes());
        payload[8..16].copy_from_slice(&self.round.to_le_bytes());
        payload[16..48].copy_from_slice(self.block_hash.as_bytes());
        payload[48..80].copy_from_slice(self.validator.as_bytes());
        payload
    }

    /// Menghasilkan dan menandatangani suara baru menggunakan kunci privat Ed25519.
    pub fn sign(
        validator: AccountId,
        epoch: u64,
        round: u64,
        block_hash: Hash,
        signing_key: &SigningKey,
    ) -> Self {
        let mut vote = Self {
            validator,
            epoch,
            round,
            block_hash,
            signature: Signature::new([0u8; 64]),
        };
        let payload = vote.signing_payload();
        let dalek_sig = signing_key.sign(&payload);
        vote.signature = Signature::new(dalek_sig.to_bytes());
        vote
    }

    /// Memverifikasi keabsahan tanda tangan kriptografis Ed25519 terhadap validator.
    pub fn verify_signature(&self) -> Result<(), ConsensusError> {
        let verifying_key = VerifyingKey::from_bytes(self.validator.as_bytes())
            .map_err(|_| ConsensusError::InvalidSignature)?;

        let dalek_sig = ed25519_dalek::Signature::from_bytes(&self.signature.to_bytes());
        verifying_key
            .verify(&self.signing_payload(), &dalek_sig)
            .map_err(|_| ConsensusError::InvalidSignature)
    }
}

/// Bukti kriptografis mandiri (self-contained fraud proof) atas pelanggaran
/// penandatanganan ganda (equivocation / double-signing) oleh validator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquivocationEvidence {
    /// Identitas kunci publik validator pelaku pelanggaran.
    pub validator: AccountId,
    /// Nomor epoch tempat pelanggaran terjadi.
    pub epoch: u64,
    /// Nomor ronde konsensus tempat pelanggaran terjadi.
    pub round: u64,
    /// Suara pertama yang ditandatangani oleh validator.
    pub vote_a: VoteRecord,
    /// Suara kedua yang ditandatangani oleh validator pada slot yang sama untuk blok berbeda.
    pub vote_b: VoteRecord,
}

impl EquivocationEvidence {
    /// Mengonstruksi bukti kecurangan ganda baru.
    #[inline]
    pub const fn new(
        validator: AccountId,
        epoch: u64,
        round: u64,
        vote_a: VoteRecord,
        vote_b: VoteRecord,
    ) -> Self {
        Self {
            validator,
            epoch,
            round,
            vote_a,
            vote_b,
        }
    }

    /// Memverifikasi validitas bukti kecurangan ganda secara mandiri dan nir-status.
    pub fn verify(&self) -> Result<(), ConsensusError> {
        // 1. Verifikasi kecocokan identitas validator antara bukti dan kedua suara
        if self.vote_a.validator != self.validator || self.vote_b.validator != self.validator {
            return Err(ConsensusError::ValidatorMismatch);
        }

        // 2. Verifikasi keselarasan slot (epoch dan round)
        if self.vote_a.epoch != self.epoch
            || self.vote_b.epoch != self.epoch
            || self.vote_a.round != self.round
            || self.vote_b.round != self.round
        {
            return Err(ConsensusError::InvalidEvidenceSlotMismatch);
        }

        // 3. Verifikasi sifat kontradiktif (blok yang disetujui harus berbeda)
        if self.vote_a.block_hash == self.vote_b.block_hash {
            return Err(ConsensusError::NonEquivocatingVotes);
        }

        // 4. Verifikasi keabsahan tanda tangan Ed25519 untuk kedua suara
        self.vote_a.verify_signature()?;
        self.vote_b.verify_signature()?;

        Ok(())
    }
}
