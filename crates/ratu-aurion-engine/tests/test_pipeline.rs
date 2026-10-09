#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ratu_aurion_engine::error::EngineError;
use ratu_aurion_engine::pipeline::PipelineCoordinator;
use ratu_aurion_index::entry::AccountLocation;
use ratu_aurion_index::keydir::Keydir;
use ratu_aurion_primitives::{
    crypto::{AccountId, Signature},
    record::{MutationRecord, RECORD_KIND_TRANSFER, RECORD_SIZE},
    value::AurValue,
};
use ratu_aurion_storage::reader::SegmentReader;
use ratu_aurion_storage::writer::SegmentWriter;
use ed25519_dalek::{Signer, SigningKey};

fn unique_test_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("axiom_pipeline_test_{label}_{nanos}"));
    fs::create_dir_all(&path).expect("Create test dir");
    path
}

fn create_keypair(seed_byte: u8) -> (SigningKey, AccountId) {
    let mut seed = [0u8; 32];
    seed[0] = seed_byte;
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();
    let account = AccountId::new(verifying_key.to_bytes());
    (signing_key, account)
}

fn sign_mutation(
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
        amount: AurValue::from_atomic(amount),
        signature: Signature::ZERO,
    };
    let payload = record.signing_payload();
    let dalek_sig = signing_key.sign(&payload);
    record.signature = Signature::new(dalek_sig.to_bytes());
    record
}

#[test]
fn test_pipeline_concurrent_valid_transactions() {
    let dir = unique_test_dir("concurrent_valid");
    let seg_path = dir.join("epoch_1_seg_0.log");
    let writer = SegmentWriter::create(&seg_path, 1, 1, 0).expect("Create writer");
    let mut keydir = Keydir::new();

    let amount_per_tx = 1_000_000_000u128; // 0.1 AXM
    let initial_balance = 100_000_000_000u128; // 10 AXM

    let mut client_keys = Vec::with_capacity(8);
    for i in 0..8 {
        let (alice_key, alice) = create_keypair(0x10 + i as u8);
        let (_, bob) = create_keypair(0x90 + i as u8);
        keydir.seed_account(
            alice,
            AurValue::from_atomic(initial_balance),
            AccountLocation::new(1, 0, 0, 0),
        );
        client_keys.push((alice_key, alice, bob));
    }

    let coordinator =
        Arc::new(PipelineCoordinator::spawn(writer, keydir, 4).expect("Spawn pipeline"));

    let mut handles = Vec::with_capacity(8);
    for (alice_key, alice, bob) in client_keys.clone() {
        let coord = Arc::clone(&coordinator);
        let handle = std::thread::spawn(move || {
            let mut receipts = Vec::with_capacity(50);
            for seq in 1..=50 {
                let tx = sign_mutation(&alice_key, 1, seq, alice, bob, amount_per_tx);
                let receipt = coord.submit(tx).expect("Submit valid tx");
                assert_eq!(receipt.sequence_number, seq);
                receipts.push(receipt);
            }
            receipts
        });
        handles.push(handle);
    }

    let mut all_receipts = Vec::with_capacity(400);
    for h in handles {
        let receipts = h.join().expect("Client thread panicked");
        assert_eq!(receipts.len(), 50);
        for (i, r) in receipts.iter().enumerate() {
            assert_eq!(r.sequence_number, (i + 1) as u64);
        }
        all_receipts.extend(receipts);
    }

    assert_eq!(all_receipts.len(), 400);

    // Verifikasi seluruh 400 mutasi memiliki disk_offset yang unik dan bertambah secara monotonik
    all_receipts.sort_by_key(|r| r.disk_offset);
    for (i, r) in all_receipts.iter().enumerate() {
        let expected_offset = 42 + (i as u64 * RECORD_SIZE as u64);
        assert_eq!(r.disk_offset, expected_offset);
    }

    // Verifikasi saldo akhir via query_balance
    for (_, alice, bob) in &client_keys {
        let alice_bal = coordinator.query_balance(alice).expect("Query alice balance");
        let bob_bal = coordinator.query_balance(bob).expect("Query bob balance");
        let expected_alice = initial_balance - (50 * amount_per_tx);
        let expected_bob = 50 * amount_per_tx;
        assert_eq!(alice_bal.to_atomic(), expected_alice);
        assert_eq!(bob_bal.to_atomic(), expected_bob);
    }

    let coord = Arc::try_unwrap(coordinator)
        .ok()
        .expect("Arc unwrap failed");
    coord.shutdown().expect("Shutdown pipeline");

    // Verifikasi total mutasi fisik di disk
    let mut reader = SegmentReader::open(&seg_path).expect("Open reader");
    let footer = reader.read_footer().expect("Read footer");
    assert_eq!(footer.total_records, 400);

    let stream = reader.stream_records().expect("Stream records");
    assert_eq!(stream.count(), 400);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_pipeline_fail_fast_invalid_signature_at_stage1() {
    let dir = unique_test_dir("invalid_sig_stage1");
    let seg_path = dir.join("epoch_1_seg_0.log");
    let writer = SegmentWriter::create(&seg_path, 1, 1, 0).expect("Create writer");
    let mut keydir = Keydir::new();

    let (alice_key, alice) = create_keypair(0x01);
    let (_, bob) = create_keypair(0x02);
    keydir.seed_account(
        alice,
        AurValue::from_atomic(100_000_000_000),
        AccountLocation::new(1, 0, 0, 0),
    );

    let coordinator = PipelineCoordinator::spawn(writer, keydir, 2).expect("Spawn pipeline");

    // Manipulasi transaksi tanpa menandatangani ulang (merusak signature)
    let mut bad_tx = sign_mutation(&alice_key, 1, 1, alice, bob, 10_000_000_000);
    bad_tx.amount = AurValue::from_atomic(20_000_000_000);

    let result = coordinator.submit(bad_tx);
    match result {
        Err(EngineError::InvalidSignature) => {}
        other => panic!("Harus mengembalikan InvalidSignature, didapat: {:?}", other),
    }

    // Kirim transaksi sah dengan seq = 1; harus sukses di offset 42 (Stage 2 & Stage 3 tidak tersentuh transaksi palsu)
    let good_tx = sign_mutation(&alice_key, 1, 1, alice, bob, 10_000_000_000);
    let receipt = coordinator.submit(good_tx).expect("Submit valid tx");
    assert_eq!(receipt.sequence_number, 1);
    assert_eq!(receipt.disk_offset, 42);

    coordinator.shutdown().expect("Shutdown");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_pipeline_fail_fast_stale_sequence_at_stage2() {
    let dir = unique_test_dir("stale_seq_stage2");
    let seg_path = dir.join("epoch_1_seg_0.log");
    let writer = SegmentWriter::create(&seg_path, 1, 1, 0).expect("Create writer");
    let mut keydir = Keydir::new();

    let (alice_key, alice) = create_keypair(0x03);
    let (_, bob) = create_keypair(0x04);
    keydir.seed_account(
        alice,
        AurValue::from_atomic(100_000_000_000),
        AccountLocation::new(1, 0, 0, 0),
    );

    let coordinator = PipelineCoordinator::spawn(writer, keydir, 2).expect("Spawn pipeline");

    let tx1 = sign_mutation(&alice_key, 1, 1, alice, bob, 10_000_000_000);
    let receipt1 = coordinator.submit(tx1).expect("Submit tx1");
    assert_eq!(receipt1.sequence_number, 1);
    assert_eq!(receipt1.disk_offset, 42);

    // Menggunakan kembali seq = 1
    let tx2_stale = sign_mutation(&alice_key, 1, 1, alice, bob, 15_000_000_000);
    let result = coordinator.submit(tx2_stale);
    match result {
        Err(EngineError::StaleSequenceNumber { expected, found }) => {
            assert_eq!(expected, 2);
            assert_eq!(found, 1);
        }
        other => panic!("Harus mengembalikan StaleSequenceNumber, didapat: {:?}", other),
    }

    // Kirim mutasi sah seq = 2 untuk memastikan storage disk tidak menulis record duplikat
    let tx3 = sign_mutation(&alice_key, 1, 2, alice, bob, 5_000_000_000);
    let receipt3 = coordinator.submit(tx3).expect("Submit tx3");
    assert_eq!(receipt3.sequence_number, 2);
    assert_eq!(receipt3.disk_offset, 203); // Tepat pada 42 + 161 (hanya 2 record yang tertulis)

    coordinator.shutdown().expect("Shutdown");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_pipeline_insufficient_balance_rejection_at_stage2() {
    let dir = unique_test_dir("insufficient_balance_stage2");
    let seg_path = dir.join("epoch_1_seg_0.log");
    let writer = SegmentWriter::create(&seg_path, 1, 1, 0).expect("Create writer");
    let mut keydir = Keydir::new();

    let (alice_key, alice) = create_keypair(0x05);
    let (_, bob) = create_keypair(0x06);
    // Saldo Alice hanya 10 AXM
    keydir.seed_account(
        alice,
        AurValue::from_atomic(10_000_000_000),
        AccountLocation::new(1, 0, 0, 0),
    );

    let coordinator = PipelineCoordinator::spawn(writer, keydir, 2).expect("Spawn pipeline");

    // Alice mencoba mengirim 50 AXM
    let tx_overdraft = sign_mutation(&alice_key, 1, 1, alice, bob, 50_000_000_000);
    let result = coordinator.submit(tx_overdraft);
    match result {
        Err(EngineError::InsufficientBalance { requested, available }) => {
            assert_eq!(requested.to_atomic(), 50_000_000_000);
            assert_eq!(available.to_atomic(), 10_000_000_000);
        }
        other => panic!("Harus mengembalikan InsufficientBalance, didapat: {:?}", other),
    }

    // Alice mengirim 5 AXM: harus berhasil di offset 42 (storage belum termodifikasi sebelumnya)
    let tx_valid = sign_mutation(&alice_key, 1, 1, alice, bob, 5_000_000_000);
    let receipt = coordinator.submit(tx_valid).expect("Submit valid tx");
    assert_eq!(receipt.sequence_number, 1);
    assert_eq!(receipt.disk_offset, 42);

    coordinator.shutdown().expect("Shutdown");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_pipeline_graceful_shutdown_and_flush() {
    let dir = unique_test_dir("graceful_shutdown");
    let seg_path = dir.join("epoch_1_seg_0.log");
    let writer = SegmentWriter::create(&seg_path, 1, 1, 0).expect("Create writer");
    let mut keydir = Keydir::new();

    let (alice_key, alice) = create_keypair(0x07);
    let (_, bob) = create_keypair(0x08);
    keydir.seed_account(
        alice,
        AurValue::from_atomic(100_000_000_000),
        AccountLocation::new(1, 0, 0, 0),
    );

    let coordinator = PipelineCoordinator::spawn(writer, keydir, 4).expect("Spawn pipeline");

    for seq in 1..=20 {
        let tx = sign_mutation(&alice_key, 1, seq, alice, bob, 1_000_000_000);
        let receipt = coordinator.submit(tx).expect("Submit tx");
        assert_eq!(receipt.sequence_number, seq);
    }

    coordinator
        .shutdown()
        .expect("Shutdown harus selesai tanpa error");

    let mut reader = SegmentReader::open(&seg_path).expect("Open reader");
    let footer = reader.read_footer().expect("Read footer");
    assert_eq!(footer.total_records, 20);
    assert_eq!(footer.first_sequence, 1);
    assert_eq!(footer.last_sequence, 20);

    let meta = fs::metadata(&seg_path).expect("Metadata");
    assert_eq!(meta.len(), 42 + (20 * RECORD_SIZE as u64) + 88);

    let _ = fs::remove_dir_all(&dir);
}
