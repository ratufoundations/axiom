//! Modul penanganan galat subsistem jaringan dan protokol transmisi biner Axiom.

use core::fmt;
use std::io;

/// Ragam galat pada proses framing, decoding paket, dan pengelolaan peer node.
#[derive(Debug)]
pub enum NetworkError {
    /// Galat header pembungkus frame paket (magic bytes atau struktur tidak valid).
    FramingError(&'static str),
    /// Ketidakcocokan intisari BLAKE3 checksum payload dengan yang tertera pada FrameHeader.
    ChecksumMismatch,
    /// Pengenal jenis pesan (Type ID) tidak dikenal dalam protokol.
    UnknownMessageType(u8),
    /// Struktur biner isi payload rusak, tidak lengkap, atau melebihi batas yang diizinkan.
    MalformedPayload,
    /// Identitas peer sudah terdaftar dalam PeerTable.
    PeerAlreadyExists,
    /// Identitas peer tidak ditemukan dalam PeerTable.
    PeerNotFound,
    /// Galat operasi I/O jaringan atau sistem operasi.
    IoError(io::Error),
    /// Galat yang berasal dari subsistem konsensus.
    ConsensusError(axiom_consensus::error::ConsensusError),
    /// Laju transmisi melebihi kuota token bucket yang ditentukan untuk peer.
    RateLimitExceeded { peer: String },
    /// Ukuran muatan frame melebihi batas fisik maksimum yang diizinkan (64 KB).
    FrameTooLarge { size: usize, max: usize },
    /// Batas waktu (timeout) operasi I/O soket tercapai.
    IoTimeout,
    /// Koneksi ditahan atau ditolak karena antrean saluran masuk penuh (backpressure).
    ConnectionThrottled,
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FramingError(msg) => write!(f, "Network Framing Error: {msg}"),
            Self::ChecksumMismatch => write!(f, "Network Checksum Mismatch: payload corrupted"),
            Self::UnknownMessageType(t) => write!(f, "Network Error: Unknown message type 0x{t:02x}"),
            Self::MalformedPayload => write!(f, "Network Error: Malformed message payload"),
            Self::PeerAlreadyExists => write!(f, "Network Error: Peer already exists in peer table"),
            Self::PeerNotFound => write!(f, "Network Error: Peer not found in peer table"),
            Self::IoError(e) => write!(f, "Network I/O Error: {e}"),
            Self::ConsensusError(e) => write!(f, "Network Consensus Error: {e}"),
            Self::RateLimitExceeded { peer } => {
                write!(f, "Network Error: Rate limit exceeded for peer {peer}")
            }
            Self::FrameTooLarge { size, max } => {
                write!(f, "Network Error: Frame size {size} exceeds maximum limit of {max} bytes")
            }
            Self::IoTimeout => write!(f, "Network Error: Socket I/O operation timed out"),
            Self::ConnectionThrottled => {
                write!(f, "Network Error: Connection throttled due to excessive backpressure")
            }
        }
    }
}

impl PartialEq for NetworkError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::FramingError(a), Self::FramingError(b)) => a == b,
            (Self::ChecksumMismatch, Self::ChecksumMismatch) => true,
            (Self::UnknownMessageType(a), Self::UnknownMessageType(b)) => a == b,
            (Self::MalformedPayload, Self::MalformedPayload) => true,
            (Self::PeerAlreadyExists, Self::PeerAlreadyExists) => true,
            (Self::PeerNotFound, Self::PeerNotFound) => true,
            (Self::IoTimeout, Self::IoTimeout) => true,
            (Self::ConnectionThrottled, Self::ConnectionThrottled) => true,
            (Self::RateLimitExceeded { peer: p1 }, Self::RateLimitExceeded { peer: p2 }) => p1 == p2,
            (Self::FrameTooLarge { size: s1, max: m1 }, Self::FrameTooLarge { size: s2, max: m2 }) => {
                s1 == s2 && m1 == m2
            }
            (Self::ConsensusError(c1), Self::ConsensusError(c2)) => c1 == c2,
            (Self::IoError(e1), Self::IoError(e2)) => e1.kind() == e2.kind(),
            _ => false,
        }
    }
}

impl std::error::Error for NetworkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IoError(e) => Some(e),
            Self::ConsensusError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for NetworkError {
    #[inline]
    fn from(err: io::Error) -> Self {
        Self::IoError(err)
    }
}

impl From<axiom_consensus::error::ConsensusError> for NetworkError {
    #[inline]
    fn from(err: axiom_consensus::error::ConsensusError) -> Self {
        Self::ConsensusError(err)
    }
}
