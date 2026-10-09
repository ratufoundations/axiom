//! Modul penanganan galat antarmuka baris perintah (ratu-aurion-cli).

use core::fmt;
use std::io;

/// Ragam galat pada operasi CLI, manajemen dompet, dan transmisi transaksi.
#[derive(Debug)]
pub enum CliError {
    /// Galat operasi I/O berkas atau soket sistem operasi.
    IoError(io::Error),
    /// Galat framing atau transmisi paket jaringan Axiom.
    NetworkError(ratu_aurion_network::error::NetworkError),
    /// Format string nilai nominal AXM tidak valid.
    InvalidAmountFormat(&'static str),
    /// Presisi nilai pecahan melebihi batas 10 digit desimal.
    PrecisionExceeded,
    /// Format string heksadesimal tidak valid.
    InvalidHexFormat,
    /// Panjang kunci rahasia atau identitas publik tidak tepat 32 byte.
    InvalidKeyLength,
    /// Transaksi ditolak oleh simpul node jaringan.
    NodeRejectedTransaction(String),
    /// Argumen baris perintah wajib tidak ditemukan.
    MissingArgument(&'static str),
    /// Sub-perintah CLI tidak dikenali.
    UnknownCommand(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "CLI I/O Error: {e}"),
            Self::NetworkError(e) => write!(f, "CLI Network Error: {e}"),
            Self::InvalidAmountFormat(msg) => write!(f, "Invalid amount format: {msg}"),
            Self::PrecisionExceeded => {
                write!(f, "Amount precision exceeded: maximum allowed is 10 decimal places")
            }
            Self::InvalidHexFormat => write!(f, "Invalid hex string format"),
            Self::InvalidKeyLength => write!(f, "Invalid key length: expected 32 bytes (64 hex characters)"),
            Self::NodeRejectedTransaction(reason) => {
                write!(f, "Node rejected transaction: {reason}")
            }
            Self::MissingArgument(arg) => write!(f, "Missing required argument: {arg}"),
            Self::UnknownCommand(cmd) => write!(f, "Unknown CLI command: '{cmd}'"),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IoError(e) => Some(e),
            Self::NetworkError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for CliError {
    #[inline]
    fn from(err: io::Error) -> Self {
        Self::IoError(err)
    }
}

impl From<ratu_aurion_network::error::NetworkError> for CliError {
    #[inline]
    fn from(err: ratu_aurion_network::error::NetworkError) -> Self {
        Self::NetworkError(err)
    }
}
