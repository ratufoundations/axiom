//! Modul pesan timeout dan perakitan sertifikat timeout (TimeoutCertificate).

use std::collections::BTreeSet;

use ratu_aurion_primitives::crypto::{AccountId, Signature};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};

use crate::error::ConsensusError;
use crate::validator_set::ValidatorSet;

/// Ukuran fisik muatan penandatanganan pesan timeout (56 byte).
///
/// Komposisi:
/// - epoch: 8 byte (Little-Endian)
/// - round: 8 byte (Little-Endian)
/// - high_qc_round: 8 byte (Little-Endian)
/// - validator: 32 byte
pub const TIMEOUT_PAYLOAD_SIZE: usize = 56;

/// Pesan pemberitahuan timeout yang disiarkan validator saat timer ronde habis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeoutMsg {
    /// Identitas kunci publik validator yang mengalami timeout (32 byte).
    pub validator: AccountId,
    /// Nomor epoch saat timeout terjadi (8 byte).
    pub epoch: u64,
    /// Nomor ronde konsensus saat timeout terjadi (8 byte).
    pub round: u64,
    /// Ronde tertinggi dari QuorumCertificate yang dimiliki oleh validator (8 byte).
    pub high_qc_round: u64,
    /// Tanda tangan digital kriptografis Ed25519 atas muatan timeout (64 byte).
    pub signature: Signature,
}

impl TimeoutMsg {
    /// Mengonstruksi TimeoutMsg baru secara langsung.
    #[inline]
    pub const fn new(
        validator: AccountId,
        epoch: u64,
        round: u64,
        high_qc_round: u64,
        signature: Signature,
    ) -> Self {
        Self {
            validator,
            epoch,
            round,
            high_qc_round,
            signature,
        }
    }

    /// Menghasilkan 56 byte muatan terdeterminasi untuk penandatanganan pesan timeout.
    #[inline]
    pub fn signing_payload(&self) -> [u8; TIMEOUT_PAYLOAD_SIZE] {
        let mut payload = [0u8; TIMEOUT_PAYLOAD_SIZE];
        payload[0..8].copy_from_slice(&self.epoch.to_le_bytes());
        payload[8..16].copy_from_slice(&self.round.to_le_bytes());
        payload[16..24].copy_from_slice(&self.high_qc_round.to_le_bytes());
        payload[24..56].copy_from_slice(self.validator.as_bytes());
        payload
    }

    /// Menghasilkan dan menandatangani pesan timeout menggunakan kunci privat Ed25519.
    pub fn sign(
        validator: AccountId,
        epoch: u64,
        round: u64,
        high_qc_round: u64,
        signing_key: &SigningKey,
    ) -> Self {
        let mut msg = Self {
            validator,
            epoch,
            round,
            high_qc_round,
            signature: Signature::new([0u8; 64]),
        };
        let payload = msg.signing_payload();
        let dalek_sig = signing_key.sign(&payload);
        msg.signature = Signature::new(dalek_sig.to_bytes());
        msg
    }

    /// Memverifikasi keabsahan tanda tangan kriptografis Ed25519 terhadap validator.
    pub fn verify_signature(&self) -> Result<(), ConsensusError> {
        let verifying_key = VerifyingKey::from_bytes(self.validator.as_bytes())
            .map_err(|_| ConsensusError::InvalidTimeoutSignature)?;

        let dalek_sig = ed25519_dalek::Signature::from_bytes(&self.signature.to_bytes());
        verifying_key
            .verify(&self.signing_payload(), &dalek_sig)
            .map_err(|_| ConsensusError::InvalidTimeoutSignature)
    }
}

/// Sertifikat timeout yang membuktikan bahwa supermayoritas validator (>= 2/3 + 1)
/// telah sepakat untuk melompati ronde tertentu dan berpindah ke ronde berikutnya.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeoutCertificate {
    /// Nomor epoch terjadinya pergantian ronde.
    pub epoch: u64,
    /// Nomor ronde yang disepakati untuk dilompati.
    pub round: u64,
    /// Nilai high_qc_round tertinggi di antara seluruh pesan timeout yang terkumpul.
    pub high_qc_round: u64,
    /// Daftar pasangan (AccountId, Signature) dari validator yang menyetujui timeout.
    pub signatures: Vec<(AccountId, Signature)>,
}

impl TimeoutCertificate {
    /// Mengonstruksi TimeoutCertificate baru secara langsung.
    #[inline]
    pub const fn new(
        epoch: u64,
        round: u64,
        high_qc_round: u64,
        signatures: Vec<(AccountId, Signature)>,
    ) -> Self {
        Self {
            epoch,
            round,
            high_qc_round,
            signatures,
        }
    }

    /// Mengagregasikan kumpulan TimeoutMsg menjadi TimeoutCertificate yang valid.
    ///
    /// Menegakkan invarian:
    /// 1. Kesesuaian epoch dan round untuk setiap pesan.
    /// 2. Setiap validator terdaftar dalam himpunan validator aktif.
    /// 3. Ketiadaan tanda tangan duplikat dari validator yang sama.
    /// 4. Validitas tanda tangan digital Ed25519 untuk setiap pesan.
    /// 5. Pemenuhan ambang batas kuorum supermayoritas (>= 2/3 bobot + 1).
    pub fn assemble(
        epoch: u64,
        round: u64,
        timeout_messages: &[TimeoutMsg],
        validator_set: &ValidatorSet,
    ) -> Result<Self, ConsensusError> {
        let mut seen = BTreeSet::new();
        let mut accumulated_weight: u64 = 0;
        let mut max_high_qc_round: u64 = 0;
        let mut signatures = Vec::with_capacity(timeout_messages.len());

        for msg in timeout_messages {
            // 1. Verifikasi slot (epoch dan round)
            if msg.epoch != epoch || msg.round != round {
                return Err(ConsensusError::InvalidTimeoutSlotMismatch);
            }

            // 2. Verifikasi status pendaftaran validator
            let weight = validator_set
                .get_weight(&msg.validator)
                .ok_or(ConsensusError::ValidatorNotRegistered)?;

            // 3. Cegah duplikasi suara timeout
            if !seen.insert(msg.validator) {
                return Err(ConsensusError::DuplicateTimeoutVote(msg.validator));
            }

            // 4. Verifikasi tanda tangan kriptografis Ed25519
            msg.verify_signature()?;

            // 5. Akumulasi bobot dan lacak high_qc_round maksimum
            accumulated_weight = accumulated_weight
                .checked_add(weight)
                .ok_or(ConsensusError::ArithmeticOverflow)?;

            if msg.high_qc_round > max_high_qc_round {
                max_high_qc_round = msg.high_qc_round;
            }

            signatures.push((msg.validator, msg.signature));
        }

        // 6. Validasi kuorum supermayoritas (2*W / 3 + 1)
        let required_quorum = validator_set.quorum_threshold();
        if accumulated_weight < required_quorum {
            return Err(ConsensusError::InsufficientTimeoutQuorum {
                required: required_quorum as usize,
                actual: accumulated_weight as usize,
            });
        }

        Ok(Self {
            epoch,
            round,
            high_qc_round: max_high_qc_round,
            signatures,
        })
    }

    /// Memverifikasi keabsahan kuorum dan integritas tanda tangan sertifikat terhadap ValidatorSet.
    pub fn verify(&self, validator_set: &ValidatorSet) -> Result<(), ConsensusError> {
        let mut seen = BTreeSet::new();
        let mut accumulated_weight: u64 = 0;

        for (validator, _) in &self.signatures {
            let weight = validator_set
                .get_weight(validator)
                .ok_or(ConsensusError::ValidatorNotRegistered)?;

            if !seen.insert(*validator) {
                return Err(ConsensusError::DuplicateTimeoutVote(*validator));
            }

            accumulated_weight = accumulated_weight
                .checked_add(weight)
                .ok_or(ConsensusError::ArithmeticOverflow)?;
        }

        let required_quorum = validator_set.quorum_threshold();
        if accumulated_weight < required_quorum {
            return Err(ConsensusError::InsufficientTimeoutQuorum {
                required: required_quorum as usize,
                actual: accumulated_weight as usize,
            });
        }

        Ok(())
    }
}
