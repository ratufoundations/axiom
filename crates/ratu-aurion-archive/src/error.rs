//! Modul penanganan galat pengarsipan bulanan Axiom.

use core::fmt;
use std::io;

/// Ragam galat operasional pada pengemasan, verifikasi, dan pembersihan berkas arsip.
#[derive(Debug)]
pub enum ArchiveError {
    /// Galat I/O sistem berkas lokal.
    IoError(io::Error),
    /// Galat dari subsistem penyimpanan log.
    StorageError(ratu_aurion_storage::error::StorageError),
    /// Galat internal pustaka kompresi ZIP.
    ZipError(String),
    /// Nilai pengenal magic bytes manifes tidak valid.
    InvalidMagic,
    /// Digest status antara manifes arsip dan berkas segmen tidak cocok (gagal verifikasi).
    DigestMismatch,
    /// Berkas segmen rusak atau tidak dapat dipilah.
    CorruptedSegment,
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "Archive I/O error: {e}"),
            Self::StorageError(e) => write!(f, "Storage engine error: {e}"),
            Self::ZipError(e) => write!(f, "Zip archive error: {e}"),
            Self::InvalidMagic => write!(f, "Invalid manifest magic bytes"),
            Self::DigestMismatch => write!(f, "State digest mismatch between manifest and segment"),
            Self::CorruptedSegment => write!(f, "Corrupted segment file during archiving"),
        }
    }
}

impl std::error::Error for ArchiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IoError(e) => Some(e),
            Self::StorageError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ArchiveError {
    #[inline]
    fn from(err: io::Error) -> Self {
        Self::IoError(err)
    }
}

impl From<ratu_aurion_storage::error::StorageError> for ArchiveError {
    #[inline]
    fn from(err: ratu_aurion_storage::error::StorageError) -> Self {
        Self::StorageError(err)
    }
}

impl From<zip::result::ZipError> for ArchiveError {
    #[inline]
    fn from(err: zip::result::ZipError) -> Self {
        Self::ZipError(err.to_string())
    }
}
