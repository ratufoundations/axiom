#![forbid(unsafe_code)]

//! Suite pengujian integrasi untuk tiket SEC-KEYSTORE-01: Encrypted Keystore & Validator Key Management.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::SigningKey;
use ratu_aurion_primitives::crypto::AccountId;
use ratu_aurion_primitives::keystore::{
    chacha20_xor, decrypt_signing_key, encrypt_signing_key, load_keystore_file,
    save_keystore_file, KeystoreError, KEYSTORE_FILE_SIZE, KEYSTORE_HEADER_SIZE,
};

fn unique_test_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ratu_aurion_keystore_{label}_{nanos}.keystore"))
}

#[test]
fn test_keystore_binary_layout_size() {
    assert_eq!(KEYSTORE_FILE_SIZE, 128);
    assert_eq!(KEYSTORE_HEADER_SIZE, 64);
}

#[test]
fn test_keystore_encryption_and_decryption_roundtrip() {
    let mut seed = [0u8; 32];
    seed[0] = 0x42;
    seed[31] = 0x99;
    let original_key = SigningKey::from_bytes(&seed);
    let original_account = AccountId::new(original_key.verifying_key().to_bytes());

    let passphrase = "RatuAurionSecurePassphrase2026!";
    let path = unique_test_path("roundtrip");

    // Simpan ke berkas fisik terenkripsi
    let saved_account =
        save_keystore_file(&path, &original_key, passphrase).expect("Save keystore");
    assert_eq!(original_account, saved_account);

    // Validasi ukuran berkas fisik tepat 128 byte
    let file_bytes = fs::read(&path).expect("Read keystore file");
    assert_eq!(file_bytes.len(), 128);

    // Muat kembali dan dekripsi
    let (loaded_key, loaded_account) =
        load_keystore_file(&path, passphrase).expect("Load keystore");
    assert_eq!(loaded_key.to_bytes(), original_key.to_bytes());
    assert_eq!(loaded_account, original_account);

    let _ = fs::remove_file(&path);
}

#[test]
fn test_keystore_fail_fast_wrong_passphrase() {
    let mut seed = [0u8; 32];
    seed[0] = 0x77;
    let key = SigningKey::from_bytes(&seed);

    let salt = [0x11u8; 16];
    let nonce = [0x22u8; 12];
    let keystore_bytes = encrypt_signing_key(&key, "correct_password", salt, nonce);

    // Percobaan dekripsi dengan kata sandi salah
    let res = decrypt_signing_key(&keystore_bytes, "wrong_password");
    assert_eq!(res, Err(KeystoreError::MacMismatch));
}

#[test]
fn test_keystore_fail_fast_bit_tampering() {
    let mut seed = [0u8; 32];
    seed[0] = 0x88;
    let key = SigningKey::from_bytes(&seed);

    let salt = [0x33u8; 16];
    let nonce = [0x44u8; 12];
    let original_bytes = encrypt_signing_key(&key, "tamper_pass", salt, nonce);

    // 1. Manipulasi 1 bit pada ciphertext (offset 70)
    let mut tampered_cipher = original_bytes;
    tampered_cipher[70] ^= 0x01;
    let err_cipher = decrypt_signing_key(&tampered_cipher, "tamper_pass");
    assert_eq!(err_cipher, Err(KeystoreError::MacMismatch));

    // 2. Manipulasi 1 bit pada header salt (offset 20)
    let mut tampered_header = original_bytes;
    tampered_header[20] ^= 0x01;
    let err_header = decrypt_signing_key(&tampered_header, "tamper_pass");
    assert_eq!(err_header, Err(KeystoreError::MacMismatch));

    // 3. Manipulasi 1 bit pada magic byte (offset 0)
    let mut tampered_magic = original_bytes;
    tampered_magic[0] ^= 0xFF;
    let err_magic = decrypt_signing_key(&tampered_magic, "tamper_pass");
    assert_eq!(err_magic, Err(KeystoreError::InvalidMagic));
}

#[test]
fn test_keystore_sub_128_bytes_truncation_rejection() {
    let path_50 = unique_test_path("trunc_50");
    let path_127 = unique_test_path("trunc_127");

    fs::write(&path_50, vec![0u8; 50]).expect("Write 50B");
    fs::write(&path_127, vec![0u8; 127]).expect("Write 127B");

    let err_50 = load_keystore_file(&path_50, "any_pass");
    assert_eq!(err_50, Err(KeystoreError::CorruptedFile { size: 50 }));

    let err_127 = load_keystore_file(&path_127, "any_pass");
    assert_eq!(err_127, Err(KeystoreError::CorruptedFile { size: 127 }));

    let _ = fs::remove_file(&path_50);
    let _ = fs::remove_file(&path_127);
}

#[test]
fn test_in_tree_chacha20_rfc8439_test_vector() {
    // Vektor resmi RFC 8439 §2.4.2
    let key: [u8; 32] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
        0x1e, 0x1f,
    ];
    let nonce: [u8; 12] = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4a, 0x00, 0x00, 0x00, 0x00,
    ];
    let initial_counter = 1u32;

    let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";

    let expected_ciphertext: [u8; 114] = [
        0x6e, 0x2e, 0x35, 0x9a, 0x25, 0x68, 0xf9, 0x80, 0x41, 0xba, 0x07, 0x28, 0xdd, 0x0d, 0x69,
        0x81, 0xe9, 0x7e, 0x7a, 0xec, 0x1d, 0x43, 0x60, 0xc2, 0x0a, 0x27, 0xaf, 0xcc, 0xfd, 0x9f,
        0xae, 0x0b, 0xf9, 0x1b, 0x65, 0xc5, 0x52, 0x47, 0x33, 0xab, 0x8f, 0x59, 0x3d, 0xab, 0xcd,
        0x62, 0xb3, 0x57, 0x16, 0x39, 0xd6, 0x24, 0xe6, 0x51, 0x52, 0xab, 0x8f, 0x53, 0x0c, 0x35,
        0x9f, 0x08, 0x61, 0xd8, 0x07, 0xca, 0x0d, 0xbf, 0x50, 0x0d, 0x6a, 0x61, 0x56, 0xa3, 0x8e,
        0x08, 0x8a, 0x22, 0xb6, 0x5e, 0x52, 0xbc, 0x51, 0x4d, 0x16, 0xcc, 0xf8, 0x06, 0x81, 0x8c,
        0xe9, 0x1a, 0xb7, 0x79, 0x37, 0x36, 0x5a, 0xf9, 0x0b, 0xbf, 0x74, 0xa3, 0x5b, 0xe6, 0xb4,
        0x0b, 0x8e, 0xed, 0xf2, 0x78, 0x5e, 0x42, 0x87, 0x4d,
    ];

    let mut buffer = plaintext.to_vec();
    chacha20_xor(&mut buffer, &key, initial_counter, &nonce);
    assert_eq!(buffer.as_slice(), &expected_ciphertext);

    // Dekripsi (XOR balik dengan keystream yang sama)
    chacha20_xor(&mut buffer, &key, initial_counter, &nonce);
    assert_eq!(buffer.as_slice(), plaintext);
}
