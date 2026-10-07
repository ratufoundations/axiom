//! Modul struktur proposal segmen berukuran tetap untuk konsensus finalitas linear.

use axiom_primitives::crypto::{AccountId, Hash};

/// Panjang total payload biner proposal yang ditandatangani (84 byte).
///
/// Komposisi:
/// - epoch: 8 byte
/// - segment_index: 4 byte
/// - state_digest: 32 byte
/// - proposer: 32 byte
/// - round: 8 byte
pub const PROPOSAL_SIGNING_SIZE: usize = 84;

/// Proposal komit segmen log disk aktif yang diajukan oleh validator proposer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentProposal {
    /// Nomor epoch bulan/siklus berjalan.
    pub epoch: u64,
    /// Indeks segmen log fisik di disk (`epoch_{epoch}_seg_{index}.log`).
    pub segment_index: u32,
    /// Intisari status kriptografis segmen yang disegel (state digest dari SegmentFooter).
    pub state_digest: Hash,
    /// Identitas kunci publik validator yang mengajukan proposal.
    pub proposer: AccountId,
    /// Putaran konsensus pada segmen ini.
    pub round: u64,
}

impl SegmentProposal {
    /// Mengonstruksi SegmentProposal baru.
    #[inline]
    pub const fn new(
        epoch: u64,
        segment_index: u32,
        state_digest: Hash,
        proposer: AccountId,
        round: u64,
    ) -> Self {
        Self {
            epoch,
            segment_index,
            state_digest,
            proposer,
            round,
        }
    }

    /// Menghasilkan 84 byte serialisasi little-endian deterministik dari seluruh field proposal.
    pub fn signing_bytes(&self) -> [u8; PROPOSAL_SIGNING_SIZE] {
        let mut buf = [0u8; PROPOSAL_SIGNING_SIZE];

        buf[0..8].copy_from_slice(&self.epoch.to_le_bytes());
        buf[8..12].copy_from_slice(&self.segment_index.to_le_bytes());
        buf[12..44].copy_from_slice(self.state_digest.as_bytes());
        buf[44..76].copy_from_slice(self.proposer.as_bytes());
        buf[76..84].copy_from_slice(&self.round.to_le_bytes());

        buf
    }

    /// Rekonstruksi SegmentProposal dari array 84 byte serialisasi little-endian.
    pub fn from_signing_bytes(bytes: &[u8; PROPOSAL_SIGNING_SIZE]) -> Self {
        let mut epoch_bytes = [0u8; 8];
        epoch_bytes.copy_from_slice(&bytes[0..8]);
        let epoch = u64::from_le_bytes(epoch_bytes);

        let mut seg_bytes = [0u8; 4];
        seg_bytes.copy_from_slice(&bytes[8..12]);
        let segment_index = u32::from_le_bytes(seg_bytes);

        let mut digest_bytes = [0u8; 32];
        digest_bytes.copy_from_slice(&bytes[12..44]);
        let state_digest = Hash::new(digest_bytes);

        let mut proposer_bytes = [0u8; 32];
        proposer_bytes.copy_from_slice(&bytes[44..76]);
        let proposer = AccountId::new(proposer_bytes);

        let mut round_bytes = [0u8; 8];
        round_bytes.copy_from_slice(&bytes[76..84]);
        let round = u64::from_le_bytes(round_bytes);

        Self {
            epoch,
            segment_index,
            state_digest,
            proposer,
            round,
        }
    }

    /// Menghasilkan 32 byte intisari BLAKE3 dari signing_bytes proposal.
    #[inline]
    pub fn digest(&self) -> Hash {
        let blake = blake3::hash(&self.signing_bytes());
        Hash::new(*blake.as_bytes())
    }
}
