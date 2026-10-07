//! Modul penanganan galat mesin eksekusi dan orkestrasi Axiom.

use core::fmt;
use std::io;

/// Ragam galat pada proses validasi, eksekusi, persistensi, dan rotasi mesin.
#[derive(Debug)]
pub enum EngineError {
    /// Galat subsistem penyimpanan append-only disk.
    StorageError(axiom_storage::error::StorageError),
    /// Galat indeks tabel memori RAM (Keydir).
    IndexError(axiom_index::error::IndexError),
    /// Galat proses kompresi atau pembersihan arsip.
    ArchiveError(axiom_archive::error::ArchiveError),
    /// Tanda tangan kriptografis Ed25519 tidak valid terhadap kunci publik pengirim.
    InvalidSignature,
    /// Nomor epoch baru lebih kecil atau sama dengan epoch aktif saat ini.
    EpochRegression,
    /// Galat I/O sistem berkas lokal.
    IoError(io::Error),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StorageError(e) => write!(f, "Engine Storage Error: {e}"),
            Self::IndexError(e) => write!(f, "Engine Index Error: {e}"),
            Self::ArchiveError(e) => write!(f, "Engine Archive Error: {e}"),
            Self::InvalidSignature => write!(f, "Cryptographic signature verification failed"),
            Self::EpochRegression => write!(f, "Cannot rotate to an older or identical epoch number"),
            Self::IoError(e) => write!(f, "Engine I/O Error: {e}"),
        }
    }
}

impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StorageError(e) => Some(e),
            Self::IndexError(e) => Some(e),
            Self::ArchiveError(e) => Some(e),
            Self::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<axiom_storage::error::StorageError> for EngineError {
    #[inline]
    fn from(err: axiom_storage::error::StorageError) -> Self {
        Self::StorageError(err)
    }
}

impl From<axiom_index::error::IndexError> for EngineError {
    #[inline]
    fn from(err: axiom_index::error::IndexError) -> Self {
        Self::IndexError(err)
    }
}

impl From<axiom_archive::error::ArchiveError> for EngineError {
    #[inline]
    fn from(err: axiom_archive::error::ArchiveError) -> Self {
        Self::ArchiveError(err)
    }
}

impl From<io::Error> for EngineError {
    #[inline]
    fn from(err: io::Error) -> Self {
        Self::IoError(err)
    }
}
