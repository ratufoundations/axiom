//! Modul definisi varian pesan protokol biner Axiom Network.

use ratu_aurion_consensus::certificate::QuorumCertificate;
use ratu_aurion_consensus::proposal::SegmentProposal;
use ratu_aurion_consensus::vote::Vote;
use ratu_aurion_primitives::record::MutationRecord;

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
/// Pengenal tipe pesan untuk TxSubmit (0x06).
pub const MSG_TX_SUBMIT: u8 = 0x06;
/// Pengenal tipe pesan untuk TxResult (0x07).
pub const MSG_TX_RESULT: u8 = 0x07;

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
    /// Penyerahan transaksi mutasi untuk dieksekusi dan dicatat ke segmen disk.
    TxSubmit(MutationRecord),
    /// Hasil/konfirmasi eksekusi mutasi dari simpul.
    TxResult {
        /// Indikator keberhasilan mutasi.
        success: bool,
        /// Offset byte mutasi di disk jika berhasil.
        offset: u64,
        /// Pesan status atau alasan penolakan jika gagal.
        message: String,
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
            Self::TxSubmit(_) => MSG_TX_SUBMIT,
            Self::TxResult { .. } => MSG_TX_RESULT,
        }
    }
}
