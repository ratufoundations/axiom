//! Modul penanganan galat indeks memori RAM (Keydir).

use core::fmt;

/// Ragam galat operasional pada manipulasi dan rekonstruksi indeks memori.
#[derive(Debug)]
pub enum IndexError {
    /// Saldo akun pengirim tidak mencukupi untuk nominal transfer yang diminta.
    InsufficientBalance,
    /// Nomor urut (nonce / sequence number) mutasi usang atau out-of-order.
    StaleSequenceNumber,
    /// Galat saat membaca log dari subsistem penyimpanan disk.
    StorageError(ratu_aurion_storage::error::StorageError),
    /// Overflow aritmatika saat penambahan saldo akun.
    ArithmeticOverflow,
    /// Header snapshot korup atau ukuran berkas tidak valid.
    CorruptedSnapshotHeader { size: usize },
    /// Intisari BLAKE3 payload snapshot tidak cocok dengan header.
    SnapshotChecksumMismatch { expected: [u8; 32], actual: [u8; 32] },
    /// Magic biner berkas snapshot tidak valid.
    InvalidSnapshotMagic,
    /// Galat I/O saat membaca atau menulis berkas snapshot.
    SnapshotIoError(String),
    /// Indeks dingin korup atau tidak valid.
    CorruptedColdIndex(String),
    /// Galat I/O saat manipulasi media simpan dingin di disk.
    ColdStoreIoError(String),
    /// Batas kapasitas penyimpanan dingin terlampaui.
    ColdStorageCapacityExceeded,
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsufficientBalance => write!(f, "Insufficient account balance for mutation"),
            Self::StaleSequenceNumber => {
                write!(f, "Stale or out-of-order transaction sequence number")
            }
            Self::StorageError(e) => write!(f, "Underlying storage engine error: {e}"),
            Self::ArithmeticOverflow => {
                write!(f, "Integer arithmetic overflow during state mutation")
            }
            Self::CorruptedSnapshotHeader { size } => {
                write!(f, "Corrupted snapshot header: invalid size ({size} bytes)")
            }
            Self::SnapshotChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "Snapshot checksum mismatch: expected {expected:?}, got {actual:?}"
                )
            }
            Self::InvalidSnapshotMagic => write!(f, "Invalid snapshot magic bytes"),
            Self::SnapshotIoError(e) => write!(f, "Snapshot I/O error: {e}"),
            Self::CorruptedColdIndex(e) => write!(f, "Corrupted cold index: {e}"),
            Self::ColdStoreIoError(e) => write!(f, "Cold store I/O error: {e}"),
            Self::ColdStorageCapacityExceeded => {
                write!(f, "Cold storage capacity limit exceeded")
            }
        }
    }
}

impl std::error::Error for IndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StorageError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ratu_aurion_storage::error::StorageError> for IndexError {
    #[inline]
    fn from(err: ratu_aurion_storage::error::StorageError) -> Self {
        Self::StorageError(err)
    }
}

impl From<std::io::Error> for IndexError {
    #[inline]
    fn from(err: std::io::Error) -> Self {
        Self::SnapshotIoError(err.to_string())
    }
}
