//! Modul manajemen dompet kunci Ed25519 dan penandatanganan mutasi.

use std::fs;
use std::path::Path;

use axiom_primitives::crypto::{AccountId, Signature};
use axiom_primitives::value::AxmValue;
use ed25519_dalek::{Signer, SigningKey};

use crate::error::CliError;
use crate::parser::{bytes_to_hex, hex_to_bytes_32};

/// Ukuran payload biner mutasi yang ditandatangani oleh akun pengirim (97 byte).
pub const MUTATION_SIGNING_PAYLOAD_LEN: usize = 97;

/// Dompet identitas kriptografis berbasis kunci privat Ed25519.
pub struct Wallet {
    /// Kunci privat penandatanganan Ed25519.
    pub signing_key: SigningKey,
}

impl Wallet {
    /// Mengonstruksi Wallet dari 32 byte entropi biner.
    #[inline]
    pub fn generate_from_entropy(seed: [u8; 32]) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(&seed),
        }
    }

    /// Mengonstruksi Wallet dari string heksadesimal 64 karakter kunci privat.
    pub fn from_secret_hex(hex_str: &str) -> Result<Self, CliError> {
        let bytes = hex_to_bytes_32(hex_str)?;
        Ok(Self::generate_from_entropy(bytes))
    }

    /// Mengekspor kunci privat dalam format string heksadesimal 64 karakter.
    #[inline]
    pub fn to_secret_hex(&self) -> String {
        bytes_to_hex(&self.signing_key.to_bytes())
    }

    /// Mengambil identitas publik akun (`AccountId`, 32 byte).
    #[inline]
    pub fn account_id(&self) -> AccountId {
        let verifying_key = self.signing_key.verifying_key();
        AccountId::new(verifying_key.to_bytes())
    }

    /// Menyimpan kunci privat dalam format teks heksadesimal ke berkas fisik.
    pub fn save_to_file(&self, path: &Path) -> Result<(), CliError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.to_secret_hex())?;
        Ok(())
    }

    /// Memuat dompet dari berkas teks kunci privat heksadesimal.
    pub fn load_from_file(path: &Path) -> Result<Self, CliError> {
        let content = fs::read_to_string(path)?;
        Self::from_secret_hex(content.trim())
    }

    /// Menyusun dan menandatangani 97-byte payload mutasi transaksi.
    ///
    /// Komposisi:
    /// - epoch (8 byte, little-endian)
    /// - sequence_number (8 byte, little-endian)
    /// - record_kind (1 byte)
    /// - sender (32 byte)
    /// - recipient (32 byte)
    /// - amount (16 byte, little-endian)
    pub fn sign_mutation_payload(
        &self,
        epoch: u64,
        sequence_number: u64,
        record_kind: u8,
        recipient: &AccountId,
        amount: AxmValue,
    ) -> Signature {
        let mut payload = [0u8; MUTATION_SIGNING_PAYLOAD_LEN];

        payload[0..8].copy_from_slice(&epoch.to_le_bytes());
        payload[8..16].copy_from_slice(&sequence_number.to_le_bytes());
        payload[16] = record_kind;
        payload[17..49].copy_from_slice(self.account_id().as_bytes());
        payload[49..81].copy_from_slice(recipient.as_bytes());
        payload[81..97].copy_from_slice(&amount.to_le_bytes());

        let dalek_sig = self.signing_key.sign(&payload);
        Signature::new(dalek_sig.to_bytes())
    }
}
