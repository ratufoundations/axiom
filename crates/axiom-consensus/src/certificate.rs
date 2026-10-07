//! Modul sertifikat kuorum finalitas segmen (QuorumCertificate).

use std::collections::BTreeMap;

use axiom_primitives::crypto::{AccountId, Signature};
use ed25519_dalek::{Verifier, VerifyingKey};

use crate::error::ConsensusError;
use crate::proposal::SegmentProposal;
use crate::validator_set::ValidatorSet;
use crate::vote::Vote;

/// Sertifikat kuorum yang membuktikan bahwa proposal segmen telah disetujui
/// oleh supermayoritas validator (> 2/3 bobot) secara kriptografis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuorumCertificate {
    /// Proposal segmen yang mencapai konsensus final.
    pub proposal: SegmentProposal,
    /// Kumpulan tanda tangan digital Ed25519 dari validator terdaftar (diurutkan via BTreeMap).
    pub signatures: BTreeMap<AccountId, Signature>,
    /// Akumulasi total bobot suara dari seluruh tanda tangan valid yang terkumpul.
    pub accumulated_weight: u64,
}

impl QuorumCertificate {
    /// Mengonstruksi QuorumCertificate baru untuk suatu proposal dengan suara awal kosong.
    #[inline]
    pub fn new(proposal: SegmentProposal) -> Self {
        Self {
            proposal,
            signatures: BTreeMap::new(),
            accumulated_weight: 0,
        }
    }

    /// Menambahkan suara validator ke dalam sertifikat kuorum.
    ///
    /// Memverifikasi keabsahan suara terhadap proposal, memastikan validator terdaftar,
    /// menolak suara duplikat, dan mengumpulkan bobot suara secara aman.
    pub fn add_vote(
        &mut self,
        vote: &Vote,
        validator_set: &ValidatorSet,
    ) -> Result<u64, ConsensusError> {
        // 1. Verifikasi suara terhadap proposal
        vote.verify(&self.proposal)?;

        // 2. Periksa apakah validator terdaftar dalam ValidatorSet
        let weight = validator_set
            .get_weight(&vote.validator)
            .ok_or(ConsensusError::ValidatorNotRegistered)?;

        // 3. Cegah duplikasi suara dari validator yang sama
        if self.signatures.contains_key(&vote.validator) {
            return Err(ConsensusError::DuplicateVote);
        }

        // 4. Catat tanda tangan dan akumulasikan bobot
        self.signatures.insert(vote.validator, vote.signature);
        self.accumulated_weight = self
            .accumulated_weight
            .checked_add(weight)
            .ok_or(ConsensusError::ArithmeticOverflow)?;

        Ok(self.accumulated_weight)
    }

    /// Memeriksa apakah sertifikat telah mencapai ambang batas kuorum supermayoritas.
    #[inline]
    pub fn is_final(&self, validator_set: &ValidatorSet) -> bool {
        self.accumulated_weight >= validator_set.quorum_threshold()
    }

    /// Memverifikasi keabsahan menyeluruh QuorumCertificate:
    /// 1. Setiap validator penandatangan harus terdaftar dalam `validator_set`.
    /// 2. Setiap tanda tangan Ed25519 harus sah terhadap intisari proposal.
    /// 3. Akumulasi bobot sah harus mencapai atau melebihi `quorum_threshold()`.
    pub fn verify(&self, validator_set: &ValidatorSet) -> Result<(), ConsensusError> {
        let proposal_digest = self.proposal.digest();
        let signing_bytes = self.proposal.signing_bytes();
        let mut computed_weight: u64 = 0;

        for (account_id, sig) in &self.signatures {
            // 1. Pastikan validator terdaftar
            let weight = validator_set
                .get_weight(account_id)
                .ok_or(ConsensusError::ValidatorNotRegistered)?;

            // 2. Verifikasi tanda tangan kriptografis
            let verifying_key = VerifyingKey::from_bytes(&account_id.to_bytes())
                .map_err(|_| ConsensusError::InvalidSignature)?;

            let dalek_sig = ed25519_dalek::Signature::from_bytes(&sig.to_bytes());

            let is_valid = verifying_key
                .verify(proposal_digest.as_bytes(), &dalek_sig)
                .is_ok()
                || verifying_key
                    .verify(&signing_bytes, &dalek_sig)
                    .is_ok();

            if !is_valid {
                return Err(ConsensusError::InvalidSignature);
            }

            // 3. Akumulasi bobot terhitung
            computed_weight = computed_weight
                .checked_add(weight)
                .ok_or(ConsensusError::ArithmeticOverflow)?;
        }

        // 4. Verifikasi bahwa bobot memenuhi ambang batas supermayoritas
        let threshold = validator_set.quorum_threshold();
        if computed_weight < threshold {
            return Err(ConsensusError::InsufficientQuorum);
        }

        Ok(())
    }
}
