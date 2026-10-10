//! Modul format berkas keystore terenkripsi, KDF PBKDF-BLAKE3, dan cipher ChaCha20 in-tree.
//!
//! Mengimplementasikan spesifikasi tiket SEC-KEYSTORE-01:
//! - Berkas biner tetap 128 byte (`KEYSTORE_FILE_SIZE = 128`).
//! - Header 64 byte dengan magic `*b"RAURKEY\x01"`.
//! - Derivasi kunci KDF PBKDF-BLAKE3 100.000 iterasi.
//! - Enkripsi seed ChaCha20 (RFC 8439) berbasis aritmatika integer murni 32-bit.
//! - Autentikasi integritas BLAKE3 Keyed MAC 32-byte atas byte 0..96.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use ed25519_dalek::SigningKey;

use crate::crypto::AccountId;

/// Ukuran berkas keystore terenkripsi dalam byte (tepat 128 byte).
pub const KEYSTORE_FILE_SIZE: usize = 128;

/// Ukuran header metadata keystore dalam byte (64 byte).
pub const KEYSTORE_HEADER_SIZE: usize = 64;

/// Magic bytes identifikasi berkas keystore Ratu Aurion.
pub const KEYSTORE_MAGIC: [u8; 8] = *b"RAURKEY\x01";

/// Versi format keystore aktif (versi 1).
pub const KEYSTORE_VERSION: u16 = 1;

/// Pengidentifikasi algoritma KDF: PBKDF-BLAKE3.
pub const KEYSTORE_KDF_PBKDF_BLAKE3: u16 = 1;

/// Jumlah iterasi default untuk PBKDF-BLAKE3 (100.000 iterasi).
pub const DEFAULT_KDF_ITERATIONS: u32 = 100_000;

/// Enumerasi galat terstruktur untuk operasi keystore.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum KeystoreError {
    /// Magic byte tidak cocok dengan spesifikasi protokol.
    InvalidMagic,
    /// Versi berkas atau tipe KDF tidak didukung.
    UnsupportedVersion(u16),
    /// Verifikasi MAC gagal (kata sandi salah atau data dimanipulasi).
    MacMismatch,
    /// Ukuran berkas tidak tepat 128 byte.
    CorruptedFile { size: usize },
    /// Kesalahan I/O sistem berkas.
    IoError(String),
    /// Panjang kunci tidak valid.
    InvalidKeyLength,
}

impl core::fmt::Display for KeystoreError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "Invalid keystore magic bytes"),
            Self::UnsupportedVersion(v) => write!(f, "Unsupported keystore version or KDF: {v}"),
            Self::MacMismatch => write!(f, "Keystore MAC verification failed or invalid passphrase"),
            Self::CorruptedFile { size } => {
                write!(f, "Corrupted keystore file, size: {size} bytes (expected 128)")
            }
            Self::IoError(e) => write!(f, "Keystore I/O error: {e}"),
            Self::InvalidKeyLength => write!(f, "Invalid key length in keystore"),
        }
    }
}

impl std::error::Error for KeystoreError {}

/// Operasi Quarter Round ChaCha20 sesuai spesifikasi RFC 8439 Bagian 2.1.
#[inline]
pub fn quarter_round(a: &mut u32, b: &mut u32, c: &mut u32, d: &mut u32) {
    *a = a.wrapping_add(*b);
    *d ^= *a;
    *d = d.rotate_left(16);

    *c = c.wrapping_add(*d);
    *b ^= *c;
    *b = b.rotate_left(12);

    *a = a.wrapping_add(*b);
    *d ^= *a;
    *d = d.rotate_left(8);

    *c = c.wrapping_add(*d);
    *b ^= *c;
    *b = b.rotate_left(7);
}

#[inline]
fn apply_qr(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    let mut va = state[a];
    let mut vb = state[b];
    let mut vc = state[c];
    let mut vd = state[d];
    quarter_round(&mut va, &mut vb, &mut vc, &mut vd);
    state[a] = va;
    state[b] = vb;
    state[c] = vc;
    state[d] = vd;
}

/// Menghasilkan 64 byte blok keystream ChaCha20 untuk counter dan nonce yang diberikan (RFC 8439).
pub fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    state[0] = 0x6170_7865;
    state[1] = 0x3320_646e;
    state[2] = 0x7962_2d32;
    state[3] = 0x6b20_6574;

    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes([
            key[i * 4],
            key[i * 4 + 1],
            key[i * 4 + 2],
            key[i * 4 + 3],
        ]);
    }

    state[12] = counter;

    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([
            nonce[i * 4],
            nonce[i * 4 + 1],
            nonce[i * 4 + 2],
            nonce[i * 4 + 3],
        ]);
    }

    let initial = state;

    for _ in 0..10 {
        // Column rounds
        apply_qr(&mut state, 0, 4, 8, 12);
        apply_qr(&mut state, 1, 5, 9, 13);
        apply_qr(&mut state, 2, 6, 10, 14);
        apply_qr(&mut state, 3, 7, 11, 15);

        // Diagonal rounds
        apply_qr(&mut state, 0, 5, 10, 15);
        apply_qr(&mut state, 1, 6, 11, 12);
        apply_qr(&mut state, 2, 7, 8, 13);
        apply_qr(&mut state, 3, 4, 9, 14);
    }

    let mut out = [0u8; 64];
    for i in 0..16 {
        let sum = state[i].wrapping_add(initial[i]);
        out[i * 4..i * 4 + 4].copy_from_slice(&sum.to_le_bytes());
    }
    out
}

/// Melakukan enkripsi atau dekripsi data biner menggunakan ChaCha20 in-tree (RFC 8439).
pub fn chacha20_xor(data: &mut [u8], key: &[u8; 32], counter: u32, nonce: &[u8; 12]) {
    let mut block_counter = counter;
    for chunk in data.chunks_mut(64) {
        let block = chacha20_block(key, block_counter, nonce);
        for (b, k) in chunk.iter_mut().zip(block.iter()) {
            *b ^= *k;
        }
        block_counter = block_counter.wrapping_add(1);
    }
}

/// Menurunkan pasangan kunci enkripsi dan MAC 64 byte dari kata sandi dan salt menggunakan PBKDF-BLAKE3.
pub fn derive_keystore_keys(
    passphrase: &str,
    salt: &[u8; 16],
    iterations: u32,
) -> ([u8; 32], [u8; 32]) {
    let mut hasher = blake3::Hasher::new();
    hasher.update(passphrase.as_bytes());
    hasher.update(salt);
    let mut digest = *hasher.finalize().as_bytes();

    let count = if iterations == 0 { 1 } else { iterations };
    for _ in 1..count {
        digest = *blake3::hash(&digest).as_bytes();
    }

    let enc_key = blake3::derive_key("ratu-aurion-keystore-enc-key-v1", &digest);
    let mac_key = blake3::derive_key("ratu-aurion-keystore-mac-key-v1", &digest);

    (enc_key, mac_key)
}

/// Perbandingan konstan-waktu (constant-time) untuk array 32-byte untuk mencegah timing side-channel attack.
#[inline]
pub fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

static KEYSTORE_ENTROPY_COUNTER: AtomicU64 = AtomicU64::new(1);

fn generate_salt_and_nonce() -> ([u8; 16], [u8; 12]) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let count = KEYSTORE_ENTROPY_COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut hasher_salt = blake3::Hasher::new();
    hasher_salt.update(b"ratu-aurion-salt-v1");
    hasher_salt.update(&nanos.to_le_bytes());
    hasher_salt.update(&pid.to_le_bytes());
    hasher_salt.update(&count.to_le_bytes());
    let salt_hash = hasher_salt.finalize();

    let mut hasher_nonce = blake3::Hasher::new();
    hasher_nonce.update(b"ratu-aurion-nonce-v1");
    hasher_nonce.update(&nanos.to_le_bytes());
    hasher_nonce.update(&pid.to_le_bytes());
    hasher_nonce.update(&count.to_le_bytes());
    let nonce_hash = hasher_nonce.finalize();

    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    salt.copy_from_slice(&salt_hash.as_bytes()[0..16]);
    nonce.copy_from_slice(&nonce_hash.as_bytes()[0..12]);
    (salt, nonce)
}

/// Mengenkripsi kunci privat `SigningKey` menjadi representasi biner berkas 128-byte.
pub fn encrypt_signing_key(
    signing_key: &SigningKey,
    passphrase: &str,
    salt: [u8; 16],
    nonce: [u8; 12],
) -> [u8; KEYSTORE_FILE_SIZE] {
    let mut out = [0u8; KEYSTORE_FILE_SIZE];

    // 0..8: Magic bytes
    out[0..8].copy_from_slice(&KEYSTORE_MAGIC);
    // 8..10: Versi format (1)
    out[8..10].copy_from_slice(&KEYSTORE_VERSION.to_le_bytes());
    // 10..12: ID KDF (1 = PBKDF-BLAKE3)
    out[10..12].copy_from_slice(&KEYSTORE_KDF_PBKDF_BLAKE3.to_le_bytes());
    // 12..16: Jumlah iterasi KDF (100.000)
    out[12..16].copy_from_slice(&DEFAULT_KDF_ITERATIONS.to_le_bytes());
    // 16..32: Salt
    out[16..32].copy_from_slice(&salt);
    // 32..44: Nonce
    out[32..44].copy_from_slice(&nonce);
    // 44..64: Reserved bytes (seluruhnya 0)

    // Derivasi kunci enkripsi dan MAC
    let (enc_key, mac_key) = derive_keystore_keys(passphrase, &salt, DEFAULT_KDF_ITERATIONS);

    // 64..96: Ciphertext kunci privat Ed25519 (seed 32-byte)
    let mut secret_seed = signing_key.to_bytes();
    chacha20_xor(&mut secret_seed, &enc_key, 1, &nonce);
    out[64..96].copy_from_slice(&secret_seed);

    // 96..128: Keyed MAC BLAKE3 atas header dan ciphertext (0..96)
    let mac = blake3::keyed_hash(&mac_key, &out[0..KEYSTORE_HEADER_SIZE + 32]);
    out[96..128].copy_from_slice(mac.as_bytes());

    out
}

/// Mendekripsi berkas keystore biner 128-byte menjadi `SigningKey`.
pub fn decrypt_signing_key(
    bytes: &[u8; KEYSTORE_FILE_SIZE],
    passphrase: &str,
) -> Result<SigningKey, KeystoreError> {
    if bytes[0..8] != KEYSTORE_MAGIC {
        return Err(KeystoreError::InvalidMagic);
    }

    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != KEYSTORE_VERSION {
        return Err(KeystoreError::UnsupportedVersion(version));
    }

    let kdf_type = u16::from_le_bytes([bytes[10], bytes[11]]);
    if kdf_type != KEYSTORE_KDF_PBKDF_BLAKE3 {
        return Err(KeystoreError::UnsupportedVersion(kdf_type));
    }

    let iterations = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let mut salt = [0u8; 16];
    salt.copy_from_slice(&bytes[16..32]);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&bytes[32..44]);

    let (enc_key, mac_key) = derive_keystore_keys(passphrase, &salt, iterations);

    let expected_mac = blake3::keyed_hash(&mac_key, &bytes[0..96]);
    let mut actual_mac = [0u8; 32];
    actual_mac.copy_from_slice(&bytes[96..128]);

    if !constant_time_eq(expected_mac.as_bytes(), &actual_mac) {
        return Err(KeystoreError::MacMismatch);
    }

    let mut secret_seed = [0u8; 32];
    secret_seed.copy_from_slice(&bytes[64..96]);
    chacha20_xor(&mut secret_seed, &enc_key, 1, &nonce);

    Ok(SigningKey::from_bytes(&secret_seed))
}

/// Menyimpan `SigningKey` terenkripsi ke berkas fisik biner 128-byte.
pub fn save_keystore_file<P: AsRef<Path>>(
    path: P,
    signing_key: &SigningKey,
    passphrase: &str,
) -> Result<AccountId, KeystoreError> {
    let (salt, nonce) = generate_salt_and_nonce();
    let bytes = encrypt_signing_key(signing_key, passphrase, salt, nonce);
    let path_ref = path.as_ref();
    if let Some(parent) = path_ref.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| KeystoreError::IoError(e.to_string()))?;
        }
    }
    std::fs::write(path_ref, bytes).map_err(|e| KeystoreError::IoError(e.to_string()))?;
    let verifying_key = signing_key.verifying_key();
    Ok(AccountId::new(verifying_key.to_bytes()))
}

/// Membaca dan mendekripsi berkas keystore fisik biner 128-byte.
pub fn load_keystore_file<P: AsRef<Path>>(
    path: P,
    passphrase: &str,
) -> Result<(SigningKey, AccountId), KeystoreError> {
    let path_ref = path.as_ref();
    let data = std::fs::read(path_ref).map_err(|e| KeystoreError::IoError(e.to_string()))?;
    if data.len() != KEYSTORE_FILE_SIZE {
        return Err(KeystoreError::CorruptedFile { size: data.len() });
    }
    let mut bytes = [0u8; KEYSTORE_FILE_SIZE];
    bytes.copy_from_slice(&data);
    let signing_key = decrypt_signing_key(&bytes, passphrase)?;
    let verifying_key = signing_key.verifying_key();
    let account_id = AccountId::new(verifying_key.to_bytes());
    Ok((signing_key, account_id))
}
