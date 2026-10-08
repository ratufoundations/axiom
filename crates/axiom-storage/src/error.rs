//! Modul penanganan galat mesin penyimpanan Axiom.

use core::fmt;
use std::io;

/// Representasi galat operasional pada modul penyimpanan Axiom.
#[derive(Debug)]
pub enum StorageError {
    /// Galat I/O sistem berkas mendasar.
    IoError(io::Error),
    /// Nilai magic bytes pada header atau footer tidak valid.
    InvalidMagic,
    /// Kapasitas segmen telah mencapai batas maksimal (128 MB).
    SegmentFull,
    /// Upaya penulisan pada segmen yang telah disegel (read-only).
    SegmentAlreadySealed,
    /// Data rekaman biner rusak atau tidak dapat didekode.
    CorruptedRecord,
    /// Posisi offset berada di luar batas berkas segmen yang valid.
    OutOfBounds,
    /// Ukuran header berkas segmen terpotong atau korup (< 42 byte).
    CorruptedHeader { size: usize },
    /// Kegagalan pemulihan segmen setelah crash atau torn write.
    RecoveryFailed(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "Storage I/O Error: {e}"),
            Self::InvalidMagic => write!(f, "Invalid magic bytes on segment header/footer"),
            Self::SegmentFull => write!(f, "Segment file reached maximum capacity (128 MB)"),
            Self::SegmentAlreadySealed => write!(f, "Cannot append to an already sealed segment"),
            Self::CorruptedRecord => write!(f, "Corrupted mutation record bytes detected"),
            Self::OutOfBounds => write!(f, "Offset out of bounds in segment file"),
            Self::CorruptedHeader { size } => {
                write!(f, "Segment header is corrupted or incomplete (size: {size} < 42 bytes)")
            }
            Self::RecoveryFailed(reason) => write!(f, "Segment recovery failed: {reason}"),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for StorageError {
    #[inline]
    fn from(err: io::Error) -> Self {
        Self::IoError(err)
    }
}
