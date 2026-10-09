#![forbid(unsafe_code)]

//! Modul penanganan galat mesin eksekusi dan orkestrasi Axiom.

use core::fmt;
use std::io;

use ratu_aurion_primitives::value::AurValue;

/// Ragam galat pada proses validasi, eksekusi, persistensi, dan rotasi mesin.
#[derive(Debug)]
pub enum EngineError {
    /// Galat subsistem penyimpanan append-only disk.
    StorageError(ratu_aurion_storage::error::StorageError),
    /// Galat indeks tabel memori RAM (Keydir).
    IndexError(ratu_aurion_index::error::IndexError),
    /// Galat proses kompresi atau pembersihan arsip.
    ArchiveError(ratu_aurion_archive::error::ArchiveError),
    /// Tanda tangan kriptografis Ed25519 tidak valid terhadap kunci publik pengirim.
    InvalidSignature,
    /// Nomor epoch baru lebih kecil atau sama dengan epoch aktif saat ini.
    EpochRegression,
    /// Galat I/O sistem berkas lokal.
    IoError(io::Error),
    /// Nomor urut (sequence number) mutasi usang atau duplikat.
    StaleSequenceNumber { expected: u64, found: u64 },
    /// Saldo akun pengirim tidak mencukupi untuk nominal transfer yang diminta.
    InsufficientBalance { requested: AurValue, available: AurValue },
    /// Mutasi transaksi tidak valid (misal: nominal 0, pengirim dan penerima sama).
    InvalidTransaction(String),
    /// Saluran antrean channel pipeline telah ditutup.
    PipelineChannelClosed,
    /// Antrean buffer channel pipeline telah penuh (bounded backpressure).
    PipelineQueueFull,
    /// Thread worker pipeline mengalami panik yang tidak diharapkan.
    WorkerThreadPanicked,
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
            Self::StaleSequenceNumber { expected, found } => {
                write!(f, "Stale sequence number: expected {expected}, found {found}")
            }
            Self::InsufficientBalance { requested, available } => {
                write!(
                    f,
                    "Insufficient balance: requested {}, available {}",
                    requested.to_atomic(),
                    available.to_atomic()
                )
            }
            Self::InvalidTransaction(reason) => write!(f, "Invalid transaction: {reason}"),
            Self::PipelineChannelClosed => write!(f, "Pipeline channel closed"),
            Self::PipelineQueueFull => write!(f, "Pipeline bounded queue is full"),
            Self::WorkerThreadPanicked => write!(f, "Pipeline worker thread panicked"),
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

impl From<ratu_aurion_storage::error::StorageError> for EngineError {
    #[inline]
    fn from(err: ratu_aurion_storage::error::StorageError) -> Self {
        Self::StorageError(err)
    }
}

impl From<ratu_aurion_index::error::IndexError> for EngineError {
    #[inline]
    fn from(err: ratu_aurion_index::error::IndexError) -> Self {
        Self::IndexError(err)
    }
}

impl From<ratu_aurion_archive::error::ArchiveError> for EngineError {
    #[inline]
    fn from(err: ratu_aurion_archive::error::ArchiveError) -> Self {
        Self::ArchiveError(err)
    }
}

impl From<io::Error> for EngineError {
    #[inline]
    fn from(err: io::Error) -> Self {
        Self::IoError(err)
    }
}
