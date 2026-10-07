//! Modul definisi varian pesan protokol biner Axiom Network.

use axiom_consensus::certificate::QuorumCertificate;
use axiom_consensus::proposal::SegmentProposal;
use axiom_consensus::vote::Vote;

/// Pengenal tipe pesan untuk SegmentProposal (0x01).
pub const MSG_PROPOSAL: u8 = 0x01;
/// Pengenal tipe pesan untuk Vote (0x02).
pub const MSG_VOTE: u8 = 0x02;
/// Pengenal tipe pesan untuk QuorumCertificate (0x03).
pub const MSG_CERTIFICATE: u8 = 0x03;
/// Pengenal tipe pesan untuk SyncRequest (0x04).
pub const MSG_SYNC_REQ: u8 = 0x04;
/// Pengenal tipe pesan untuk SyncChunk (0x05).
pub const MSG_SYNC_CHUNK: u8 = 0x05;

/// Seluruh varian pesan yang dapat ditransmisikan melintasi protokol jaringan P2P Axiom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkMessage {
    /// Proposal segmen disk dari validator proposer.
    Proposal(SegmentProposal),
    /// Suara tanda tangan persetujuan validator terhadap proposal tertentu.
    Vote(Vote),
    /// Bukti sertifikat kuorum finalitas segmen yang telah melampaui supermayoritas validator.
    Certificate(QuorumCertificate),
    /// Permintaan sinkronisasi potongan data segmen log dari peer lain.
    SyncRequest {
        /// Epoch dari segmen yang diminta.
        epoch: u64,
        /// Indeks berkas segmen log yang diminta.
        segment_index: u32,
        /// Offset byte awal pembacaan data.
        from_offset: u64,
    },
    /// Potongan biner data segmen log yang dikirimkan sebagai respons sinkronisasi.
    SyncChunk {
        /// Epoch dari segmen data.
        epoch: u64,
        /// Indeks berkas segmen log.
        segment_index: u32,
        /// Offset byte data pada segmen berkas log.
        offset: u64,
        /// Isi potongan data mentah segmen log.
        data: Vec<u8>,
    },
}

impl NetworkMessage {
    /// Mengembalikan pengenal tipe biner (Type ID) untuk varian pesan aktif.
    #[inline]
    pub const fn type_id(&self) -> u8 {
        match self {
            Self::Proposal(_) => MSG_PROPOSAL,
            Self::Vote(_) => MSG_VOTE,
            Self::Certificate(_) => MSG_CERTIFICATE,
            Self::SyncRequest { .. } => MSG_SYNC_REQ,
            Self::SyncChunk { .. } => MSG_SYNC_CHUNK,
        }
    }
}
