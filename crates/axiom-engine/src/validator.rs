//! Modul validasi kriptografi tanda tangan transaksi Ed25519.

use axiom_primitives::record::MutationRecord;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

use crate::error::EngineError;

/// Panjang payload data mutasi yang ditandatangani (97 byte).
///
/// Komposisi:
/// - epoch (8 byte)
/// - sequence_number (8 byte)
/// - record_kind (1 byte)
/// - sender (32 byte)
/// - recipient (32 byte)
/// - amount (16 byte)
pub const SIGNING_PAYLOAD_SIZE: usize = MutationRecord::SIGNING_PAYLOAD_SIZE;

/// Menghasilkan 97 byte payload biner yang wajib ditandatangani oleh akun pengirim (RFC-0001 §3.3).
#[inline]
pub fn compute_signing_payload(record: &MutationRecord) -> [u8; SIGNING_PAYLOAD_SIZE] {
    record.signing_payload()
}

/// Memverifikasi keabsahan tanda tangan Ed25519 pada MutationRecord terhadap kunci publik pengirim.
pub fn verify_record_signature(record: &MutationRecord) -> Result<(), EngineError> {
    let payload = compute_signing_payload(record);
    let pubkey_bytes = record.sender.to_bytes();
    let sig_bytes = record.signature.to_bytes();

    let verifying_key =
        VerifyingKey::from_bytes(&pubkey_bytes).map_err(|_| EngineError::InvalidSignature)?;
    let dalek_signature = Signature::from_bytes(&sig_bytes);

    verifying_key
        .verify(&payload, &dalek_signature)
        .map_err(|_| EngineError::InvalidSignature)?;

    Ok(())
}
