#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_primitives::{
    crypto::{AccountId, Hash, Signature},
    record::{MutationRecord, RECORD_KIND_TRANSFER, RECORD_SIZE},
    value::AxmValue,
};
use axiom_storage::{
    error::StorageError,
    scanner::SegmentScanner,
    writer::{DurabilityPolicy, SegmentWriter},
};
use ed25519_dalek::{Signer, SigningKey};

fn unique_test_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("axiom_scanner_test_{label}_{nanos}.log"))
}

fn create_keypair(seed_byte: u8) -> (SigningKey, AccountId) {
    let mut seed = [0u8; 32];
    seed[0] = seed_byte;
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();
    let account = AccountId::new(verifying_key.to_bytes());
    (signing_key, account)
}

fn sign_record(
    signing_key: &SigningKey,
    epoch: u64,
    seq: u64,
    sender: AccountId,
    recipient: AccountId,
    amount: u128,
) -> MutationRecord {
    let mut record = MutationRecord {
        epoch,
        sequence_number: seq,
        record_kind: RECORD_KIND_TRANSFER,
        sender,
        recipient,
        amount: AxmValue::from_atomic(amount),
        signature: Signature::ZERO,
    };
    let payload = record.signing_payload();
    let dalek_sig = signing_key.sign(&payload);
    record.signature = Signature::new(dalek_sig.to_bytes());
    record
}

#[test]
fn test_clean_sealed_segment_scan() {
    let path = unique_test_path("clean_sealed");
    let (signing_key, sender) = create_keypair(1);
    let (_, recipient) = create_keypair(2);

    let mut writer = SegmentWriter::create_with_policy(
        &path,
        1,
        1,
        0,
        DurabilityPolicy::Strict,
    )
    .expect("Gagal membuat writer");

    let mut records = Vec::with_capacity(10);
    let mut hasher = blake3::Hasher::new();

    for seq in 1..=10 {
        let rec = sign_record(&signing_key, 1, seq, sender, recipient, seq as u128 * 1_000);
        let rec_bytes = rec.to_bytes();
        hasher.update(&rec_bytes);
        writer.append_record(&rec).expect("Append record");
        records.push(rec);
    }

    let expected_digest = Hash::new(*hasher.finalize().as_bytes());
    let sealed_timestamp = 1_728_200_000u64;

    writer
        .seal_segment(expected_digest, sealed_timestamp)
        .expect("Gagal menyegel segmen");
    drop(writer);

    let report = SegmentScanner::scan_segment(&path).expect("Scan segmen bersegel gagal");

    assert!(report.is_valid);
    assert_eq!(report.total_records_scanned, 10);
    assert!(report.is_sealed);
    assert_eq!(report.first_sequence, 1);
    assert_eq!(report.last_sequence, 10);
    assert_eq!(report.calculated_digest, expected_digest);
    assert_eq!(report.footer_digest, Some(expected_digest));

    let _ = fs::remove_file(&path);
}

#[test]
fn test_clean_unsealed_preallocated_segment_scan() {
    let path = unique_test_path("clean_unsealed");
    let (signing_key, sender) = create_keypair(3);
    let (_, recipient) = create_keypair(4);

    let mut writer = SegmentWriter::create_with_policy(
        &path,
        1,
        1,
        0,
        DurabilityPolicy::BufferedRelaxed,
    )
    .expect("Gagal membuat writer");

    for seq in 1..=5 {
        let rec = sign_record(&signing_key, 1, seq, sender, recipient, seq as u128 * 500);
        writer.append_record(&rec).expect("Append record");
    }

    writer.flush_and_sync().expect("Flush and sync buffer");
    drop(writer);

    let report = SegmentScanner::scan_segment(&path).expect("Scan segmen unsealed gagal");

    assert!(report.is_valid);
    assert_eq!(report.total_records_scanned, 5);
    assert!(!report.is_sealed);
    assert_eq!(report.first_sequence, 1);
    assert_eq!(report.last_sequence, 5);
    assert_eq!(report.footer_digest, None);

    let _ = fs::remove_file(&path);
}

#[test]
fn test_structural_fail_fast_invalid_record_kind() {
    let path = unique_test_path("invalid_record_kind");
    let (signing_key, sender) = create_keypair(5);
    let (_, recipient) = create_keypair(6);

    let mut writer = SegmentWriter::create_with_policy(
        &path,
        1,
        1,
        0,
        DurabilityPolicy::Strict,
    )
    .expect("Gagal membuat writer");

    for seq in 1..=3 {
        let rec = sign_record(&signing_key, 1, seq, sender, recipient, seq as u128 * 1_000);
        writer.append_record(&rec).expect("Append record");
    }
    drop(writer);

    // Tamper record ke-2 pada offset 42 + 161 = 203, byte record_kind berada di offset 203 + 16 = 219
    let tamper_offset = 42 + RECORD_SIZE as u64 + 16;
    let mut file = OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("Buka file untuk tamper");
    file.seek(SeekFrom::Start(tamper_offset))
        .expect("Seek ke record_kind");
    file.write_all(&[0xFF]).expect("Tulis record_kind korup");
    file.flush().expect("Flush tamper");
    drop(file);

    let result = SegmentScanner::scan_segment(&path);
    match result {
        Err(StorageError::StructuralInvariantViolation { offset, reason }) => {
            assert_eq!(offset, 203);
            assert!(reason.contains("Invalid record kind"));
        }
        other => panic!("Harus mengembalikan StructuralInvariantViolation pada offset 203, didapat: {:?}", other),
    }

    let _ = fs::remove_file(&path);
}

#[test]
fn test_structural_fail_fast_broken_sequence() {
    let path = unique_test_path("broken_sequence");
    let (signing_key, sender) = create_keypair(7);
    let (_, recipient) = create_keypair(8);

    let mut writer = SegmentWriter::create_with_policy(
        &path,
        1,
        1,
        0,
        DurabilityPolicy::Strict,
    )
    .expect("Gagal membuat writer");

    for seq in 1..=3 {
        let rec = sign_record(&signing_key, 1, seq, sender, recipient, seq as u128 * 1_000);
        writer.append_record(&rec).expect("Append record");
    }
    drop(writer);

    // Tamper sequence number record ke-3 pada offset 42 + 2 * 161 = 364
    // Sequence number disimpan pada bytes 8..16 dari record (offset 364 + 8 = 372)
    let seq3_offset = 42 + 2 * RECORD_SIZE as u64 + 8;
    let mut file = OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("Buka file untuk tamper");
    file.seek(SeekFrom::Start(seq3_offset))
        .expect("Seek ke sequence number");
    file.write_all(&5u64.to_le_bytes())
        .expect("Tulis sequence number loncat ke 5");
    file.flush().expect("Flush tamper");
    drop(file);

    let result = SegmentScanner::scan_segment(&path);
    match result {
        Err(StorageError::SequenceMismatch { expected, found, offset }) => {
            assert_eq!(expected, 3);
            assert_eq!(found, 5);
            assert_eq!(offset, 364);
        }
        other => panic!("Harus mengembalikan SequenceMismatch pada offset 364, didapat: {:?}", other),
    }

    let _ = fs::remove_file(&path);
}

#[test]
fn test_bit_rot_fail_fast_sealed_digest_mismatch() {
    let path = unique_test_path("bit_rot_mismatch");
    let (signing_key, sender) = create_keypair(9);
    let (_, recipient) = create_keypair(10);

    let mut writer = SegmentWriter::create_with_policy(
        &path,
        1,
        1,
        0,
        DurabilityPolicy::Strict,
    )
    .expect("Gagal membuat writer");

    let mut hasher = blake3::Hasher::new();
    for seq in 1..=5 {
        let rec = sign_record(&signing_key, 1, seq, sender, recipient, seq as u128 * 2_000);
        hasher.update(&rec.to_bytes());
        writer.append_record(&rec).expect("Append record");
    }

    let original_digest = Hash::new(*hasher.finalize().as_bytes());
    writer
        .seal_segment(original_digest, 1_728_300_000)
        .expect("Segel segmen");
    drop(writer);

    // Tamper 1 bit pada payload record ke-2 di offset 250 (XOR dengan 0x01)
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("Buka file untuk bit-rot");
    file.seek(SeekFrom::Start(250)).expect("Seek ke offset 250");
    let mut b = [0u8; 1];
    file.read_exact(&mut b).expect("Baca byte di offset 250");
    b[0] ^= 0x01;
    file.seek(SeekFrom::Start(250)).expect("Seek ulang ke 250");
    file.write_all(&b).expect("Tulis byte bit-rot");
    file.flush().expect("Flush bit-rot");
    drop(file);

    let result = SegmentScanner::scan_segment(&path);
    match result {
        Err(StorageError::BitRotDetected { expected, actual, .. }) => {
            assert_eq!(expected, *original_digest.as_bytes());
            assert_ne!(expected, actual);
        }
        other => panic!("Harus mengembalikan BitRotDetected, didapat: {:?}", other),
    }

    let _ = fs::remove_file(&path);
}

#[test]
fn test_deep_verify_signatures() {
    let path = unique_test_path("deep_verify");
    let (signing_key, sender) = create_keypair(11);
    let (_, recipient) = create_keypair(12);

    let mut writer = SegmentWriter::create_with_policy(
        &path,
        1,
        1,
        0,
        DurabilityPolicy::Strict,
    )
    .expect("Gagal membuat writer");

    for seq in 1..=2 {
        let rec = sign_record(&signing_key, 1, seq, sender, recipient, seq as u128 * 3_000);
        writer.append_record(&rec).expect("Append record");
    }
    drop(writer);

    // Verifikasi awal: deep_verify_signatures harus sukses sebelum tamper
    let report = SegmentScanner::deep_verify_signatures(&path)
        .expect("Deep verify awal harus sukses");
    assert_eq!(report.total_records_scanned, 2);

    // Tamper 1 byte pada tanda tangan record ke-1 (tanda tangan berada pada bytes 97..161 dari record)
    // Offset tanda tangan record ke-1: 42 + 97 = 139
    let sig_byte_offset = 42 + 97;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("Buka file untuk tamper signature");
    file.seek(SeekFrom::Start(sig_byte_offset))
        .expect("Seek ke byte signature");
    let mut sig_byte = [0u8; 1];
    file.read_exact(&mut sig_byte).expect("Baca byte signature");
    sig_byte[0] ^= 0x01;
    // Pastikan tidak menjadi nol seluruhnya agar Invariant 4 tetap lolos
    if sig_byte[0] == 0 {
        sig_byte[0] = 0xAA;
    }
    file.seek(SeekFrom::Start(sig_byte_offset))
        .expect("Seek ulang ke signature");
    file.write_all(&sig_byte).expect("Tulis signature korup");
    file.flush().expect("Flush tamper signature");
    drop(file);

    // Pemindaian struktural tetap lolos karena signature tidak seluruhnya nol dan field lainnya valid
    let scan_res = SegmentScanner::scan_segment(&path);
    assert!(scan_res.is_ok(), "scan_segment struktural harus tetap lolos");

    // Tetapi deep_verify_signatures harus gagal dengan SignatureVerificationFailed pada offset 42
    let deep_res = SegmentScanner::deep_verify_signatures(&path);
    match deep_res {
        Err(StorageError::SignatureVerificationFailed { offset }) => {
            assert_eq!(offset, 42);
        }
        other => panic!("Harus mengembalikan SignatureVerificationFailed pada offset 42, didapat: {:?}", other),
    }

    let _ = fs::remove_file(&path);
}
