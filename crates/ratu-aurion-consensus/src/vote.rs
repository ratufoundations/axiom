//! Modul suara validator atas proposal segmen.

use ratu_aurion_primitives::crypto::{AccountId, Hash, Signature};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};

use crate::error::ConsensusError;
use crate::proposal::SegmentProposal;

/// Suara tanda tangan individual dari validator untuk proposal segmen tertentu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vote {
    /// 32 byte intisari BLAKE3 dari signing_bytes proposal.
    pub proposal_digest: Hash,
    /// Identitas kunci publik validator yang memberikan suara (32 byte).
    pub validator: AccountId,
    /// Tanda tangan digital kriptografis Ed25519 (64 byte).
    pub signature: Signature,
}

impl Vote {
    /// Mengonstruksi Vote secara langsung dari bagian-bagiannya.
    #[inline]
    pub const fn new(proposal_digest: Hash, validator: AccountId, signature: Signature) -> Self {
        Self {
            proposal_digest,
            validator,
            signature,
        }
    }

    /// Menghasilkan dan menandatangani suara baru untuk proposal menggunakan SigningKey Ed25519.
    pub fn sign(
        proposal: &SegmentProposal,
        validator: AccountId,
        signing_key: &SigningKey,
    ) -> Self {
        let proposal_digest = proposal.digest();
        // Validator menandatangani 32 byte intisari proposal
        let dalek_sig = signing_key.sign(proposal_digest.as_bytes());
        let signature = Signature::new(dalek_sig.to_bytes());

        Self {
            proposal_digest,
            validator,
            signature,
        }
    }

    /// Memverifikasi kecocokan `proposal_digest` dan keabsahan tanda tangan Ed25519 terhadap `validator`.
    pub fn verify(&self, proposal: &SegmentProposal) -> Result<(), ConsensusError> {
        // 1. Verifikasi kecocokan intisari proposal
        if self.proposal_digest != proposal.digest() {
            return Err(ConsensusError::ProposalMismatch);
        }

        // 2. Verifikasi tanda tangan kriptografis Ed25519
        let verifying_key = VerifyingKey::from_bytes(&self.validator.to_bytes())
            .map_err(|_| ConsensusError::InvalidSignature)?;

        let dalek_sig = ed25519_dalek::Signature::from_bytes(&self.signature.to_bytes());

        // Menerima tanda tangan atas proposal_digest (32 byte) atau langsung signing_bytes (84 byte)
        let is_valid = verifying_key
            .verify(self.proposal_digest.as_bytes(), &dalek_sig)
            .is_ok()
            || verifying_key
                .verify(&proposal.signing_bytes(), &dalek_sig)
                .is_ok();

        if !is_valid {
            return Err(ConsensusError::InvalidSignature);
        }

        Ok(())
    }
}
