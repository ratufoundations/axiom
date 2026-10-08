//! Modul penanganan galat subsistem konsensus dan pembuktian kuorum Axiom.

use core::fmt;

/// Ragam galat pada proses validasi suara, verifikasi kuorum, dan sertifikasi proposal.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ConsensusError {
    /// Tanda tangan kriptografis Ed25519 tidak valid terhadap kunci publik validator.
    InvalidSignature,
    /// Akun validator tidak terdaftar dalam himpunan validator aktif (ValidatorSet).
    ValidatorNotRegistered,
    /// Akumulasi bobot suara belum mencapai ambang batas supermayoritas (2/3 + 1).
    InsufficientQuorum,
    /// Validator telah memberikan suara sebelumnya pada proposal yang sama.
    DuplicateVote,
    /// Intisari proposal pada suara tidak cocok dengan intisari proposal yang diverifikasi.
    ProposalMismatch,
    /// Terjadi luapan aritmatika saat mengakumulasi bobot validator atau batas kuorum.
    ArithmeticOverflow,
    /// Validator memiliki bobot nol yang tidak valid.
    ZeroWeightValidator,
    /// Suara yang diajukan tidak membuktikan equivocation (misal intisari blok identik).
    NonEquivocatingVotes,
    /// Slot bukti tidak cocok (epoch atau round berbeda antara suara dan bukti).
    InvalidEvidenceSlotMismatch,
    /// Identitas validator pada suara tidak cocok dengan validator pada bukti.
    ValidatorMismatch,
    /// Validator telah dicabut hak suaranya secara permanen (tombstoned) akibat slashing.
    ValidatorTombstoned,
    /// Terjadi luapan aritmatika saat menghitung penalti pemotongan pasak (slashing).
    SlashingCalculationOverflow,
}

impl fmt::Display for ConsensusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "Consensus Error: Invalid cryptographic signature"),
            Self::ValidatorNotRegistered => write!(f, "Consensus Error: Validator not in active validator set"),
            Self::InsufficientQuorum => write!(f, "Consensus Error: Accumulated weight below quorum threshold"),
            Self::DuplicateVote => write!(f, "Consensus Error: Duplicate vote from the same validator"),
            Self::ProposalMismatch => write!(f, "Consensus Error: Proposal digest mismatch"),
            Self::ArithmeticOverflow => write!(f, "Consensus Error: Arithmetic overflow in weight calculation"),
            Self::ZeroWeightValidator => write!(f, "Consensus Error: Validator voting weight cannot be zero"),
            Self::NonEquivocatingVotes => write!(f, "Consensus Error: Votes do not equivocate (identical block hash)"),
            Self::InvalidEvidenceSlotMismatch => write!(f, "Consensus Error: Evidence slot mismatch (epoch or round differs)"),
            Self::ValidatorMismatch => write!(f, "Consensus Error: Vote validator does not match evidence validator"),
            Self::ValidatorTombstoned => write!(f, "Consensus Error: Validator is tombstoned due to slashing"),
            Self::SlashingCalculationOverflow => write!(f, "Consensus Error: Arithmetic overflow in slashing penalty calculation"),
        }
    }
}

impl std::error::Error for ConsensusError {}
