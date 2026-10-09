//! Integration tests for Checkpoint Snapshotting and Fast Recovery (OPT-INDEX-02).

use std::fs;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ratu_aurion_index::entry::AccountLocation;
use ratu_aurion_index::error::IndexError;
use ratu_aurion_index::keydir::Keydir;
use ratu_aurion_index::snapshot::SnapshotHeader;
use ratu_aurion_primitives::crypto::{AccountId, Hash, Signature};
use ratu_aurion_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER};
use ratu_aurion_primitives::value::AurValue;
use ratu_aurion_storage::writer::SegmentWriter;

fn unique_test_path(label: &str, ext: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "axiom_snap_test_{label}_{}_{nanos}.{ext}",
        std::process::id()
    ))
}

#[test]
fn test_snapshot_header_binary_layout() {
    assert_eq!(
        std::mem::size_of::<SnapshotHeader>(),
        96,
        "SnapshotHeader must be exactly 96 bytes"
    );
    assert_eq!(
        std::mem::align_of::<SnapshotHeader>(),
        16,
        "SnapshotHeader must have 16-byte alignment"
    );
}

#[test]
fn test_snapshot_roundtrip_save_and_load() {
    let mut keydir = Keydir::new();
    let mut seeded_accounts = Vec::with_capacity(1280);

    // Seed 1,280 accounts uniformly distributed across 256 buckets (5 accounts per bucket)
    for b in 0..256u16 {
        for i in 0..5u64 {
            let mut key = [0u8; 32];
            key[0] = b as u8;
            key[1..9].copy_from_slice(&i.to_le_bytes());
            key[9..17].copy_from_slice(&(b as u64).to_le_bytes());
            let account = AccountId::new(key);
            let balance = AurValue::from_atomic((b as u128 + 1) * 10_000 + i as u128);
            let location = AccountLocation::new(1, 0, (b as u64) * 10 + i, 1);
            keydir.seed_account(account, balance, location);
            seeded_accounts.push(account);
        }
    }

    assert_eq!(keydir.account_count(), 1280);

    let snap_path = unique_test_path("roundtrip", "snap");
    let epoch = 3u64;
    let seg_idx = 7u32;

    keydir
        .save_checkpoint(&snap_path, epoch, seg_idx)
        .expect("save_checkpoint should succeed");

    // Physical file size on disk must equal exactly 96 + (1,280 * 80) = 102,496 bytes
    let file_len = fs::metadata(&snap_path)
        .expect("metadata should be readable")
        .len();
    assert_eq!(
        file_len,
        96 + (1280 * 80),
        "Snapshot file length must match 96B header + 1280 * 80B entries"
    );
    assert_eq!(file_len, 102_496);

    // Load checkpoint into fresh Keydir
    let (restored_keydir, header) =
        Keydir::load_checkpoint(&snap_path).expect("load_checkpoint should succeed");

    assert_eq!(header.epoch, epoch);
    assert_eq!(header.segment_index, seg_idx);
    assert_eq!(header.total_accounts, 1280);
    assert_eq!(restored_keydir.account_count(), 1280);
    assert_eq!(restored_keydir.total_supply(), keydir.total_supply());

    // Assert every individual account balance and location strictly matches
    for account in &seeded_accounts {
        assert_eq!(
            restored_keydir.get_balance(account),
            keydir.get_balance(account)
        );
        assert_eq!(
            restored_keydir.get_location(account),
            keydir.get_location(account)
        );
    }

    let _ = fs::remove_file(&snap_path);
}

#[test]
fn test_snapshot_fail_fast_checksum_tampering() {
    let mut keydir = Keydir::new();
    for i in 0..10u8 {
        let account = AccountId::new([i; 32]);
        keydir.seed_account(
            account,
            AurValue::from_atomic((i as u128 + 1) * 1_000_000),
            AccountLocation::new(1, 0, i as u64 * 100, 1),
        );
    }

    let snap_path = unique_test_path("tampered", "snap");
    keydir
        .save_checkpoint(&snap_path, 1, 0)
        .expect("save_checkpoint should succeed");

    // Corrupt 1 byte in the payload area at byte offset 200
    let mut file_bytes = fs::read(&snap_path).expect("read raw snapshot");
    assert!(
        file_bytes.len() > 200,
        "Snapshot file must be larger than 200 bytes"
    );
    file_bytes[200] ^= 0xff;
    fs::write(&snap_path, file_bytes).expect("write corrupted bytes");

    let result = Keydir::load_checkpoint(&snap_path);
    assert!(
        matches!(result, Err(IndexError::SnapshotChecksumMismatch { .. })),
        "Expected SnapshotChecksumMismatch upon payload byte corruption"
    );

    let _ = fs::remove_file(&snap_path);
}

#[test]
fn test_snapshot_fail_fast_invalid_magic_or_truncated() {
    // 1. Test truncated file (size < 96 bytes)
    let trunc_path = unique_test_path("truncated", "snap");
    fs::write(&trunc_path, [0xaa; 50]).expect("write truncated file");

    let res_trunc = Keydir::load_checkpoint(&trunc_path);
    assert!(
        matches!(res_trunc, Err(IndexError::CorruptedSnapshotHeader { size: 50 })),
        "Expected CorruptedSnapshotHeader for sub-96B file"
    );
    let _ = fs::remove_file(&trunc_path);

    // 2. Test invalid magic bytes in 96-byte header
    let bad_magic_path = unique_test_path("bad_magic", "snap");
    let mut bad_header = [0u8; 96];
    bad_header[0..8].copy_from_slice(b"BADMAGIC");
    fs::write(&bad_magic_path, bad_header).expect("write bad magic file");

    let res_magic = Keydir::load_checkpoint(&bad_magic_path);
    assert!(
        matches!(res_magic, Err(IndexError::InvalidSnapshotMagic)),
        "Expected InvalidSnapshotMagic for corrupted magic"
    );
    let _ = fs::remove_file(&bad_magic_path);
}

#[test]
fn test_cold_boot_fast_recovery_with_delta_replay() {
    let seg0_path = unique_test_path("seg0", "log");
    let snap_path = unique_test_path("checkpoint", "snap");
    let seg1_path = unique_test_path("seg1", "log");

    let epoch = 1u64;
    let initial_supply = AurValue::from_whole_aur(100_000).expect("100_000 AXM");
    let genesis = AccountId::new([0xaa; 32]);

    // Stage 1: Produce Segment 0 with 20 sequential transactions
    let mut writer0 =
        SegmentWriter::create(&seg0_path, 1, epoch, 0).expect("create writer for seg 0");
    let mut baseline_keydir = Keydir::new();
    baseline_keydir.seed_account(genesis, initial_supply, AccountLocation::new(0, 0, 0, 0));

    let transfer_amount = AurValue::from_whole_aur(10).expect("10 AXM");

    for seq in 1..=20u64 {
        let recipient = AccountId::new([(seq as u8); 32]);
        let tx = MutationRecord {
            epoch,
            sequence_number: seq,
            record_kind: RECORD_KIND_TRANSFER,
            sender: genesis,
            recipient,
            amount: transfer_amount,
            signature: Signature::new([(seq as u8); 64]),
        };
        let off = writer0.append_record(&tx).expect("append tx to seg 0");
        baseline_keydir
            .apply_mutation(&tx, epoch, 0, off)
            .expect("apply tx to baseline");
    }
    writer0
        .seal_segment(Hash::new([0x11; 32]), 1_728_300_000)
        .expect("seal seg 0");

    // Save snapshot checkpoint at epoch 1, segment 0
    baseline_keydir
        .save_checkpoint(&snap_path, epoch, 0)
        .expect("save checkpoint at seg 0");

    // Stage 2: Produce Segment 1 with 15 subsequent transactions (seq 21..=35)
    let mut writer1 =
        SegmentWriter::create(&seg1_path, 1, epoch, 1).expect("create writer for seg 1");
    for seq in 21..=35u64 {
        let recipient = AccountId::new([(seq as u8); 32]);
        let tx = MutationRecord {
            epoch,
            sequence_number: seq,
            record_kind: RECORD_KIND_TRANSFER,
            sender: genesis,
            recipient,
            amount: transfer_amount,
            signature: Signature::new([(seq as u8); 64]),
        };
        writer1.append_record(&tx).expect("append tx to seg 1");
        baseline_keydir
            .apply_mutation(&tx, epoch, 1, 42)
            .expect("apply tx to baseline");
    }
    writer1
        .seal_segment(Hash::new([0x22; 32]), 1_728_300_100)
        .expect("seal seg 1");

    // Stage 3: Cold Boot Fast Recovery
    // Load snapshot checkpoint
    let start_load = Instant::now();
    let (mut restored_keydir, header) =
        Keydir::load_checkpoint(&snap_path).expect("load_checkpoint should succeed");
    let load_latency = start_load.elapsed();

    // Verify fast snapshot loading latency (< 50ms)
    assert!(
        load_latency.as_millis() < 50,
        "Snapshot restore must complete in < 50ms, took {load_latency:?}"
    );

    assert_eq!(header.epoch, epoch);
    assert_eq!(header.segment_index, 0);
    assert_eq!(restored_keydir.account_count(), 21); // Genesis + 20 recipients

    // Replay delta mutations from Segment 1
    let replayed_delta = restored_keydir
        .replay_segment(&seg1_path)
        .expect("replay_segment should succeed for delta segment");
    assert_eq!(replayed_delta, 15, "Replayed count must equal exactly 15");

    // Stage 4: Verify complete state conservation across all 35 transactions
    assert_eq!(
        restored_keydir.account_count(),
        36,
        "Restored accounts count must equal Genesis + 35 recipients = 36"
    );
    assert_eq!(
        restored_keydir.total_supply(),
        initial_supply,
        "Total supply must remain invariant"
    );

    // Genesis balance: 100,000 - (35 * 10) = 99,650 AXM
    let expected_genesis_balance = AurValue::from_whole_aur(99_650).expect("99,650 AXM");
    assert_eq!(
        restored_keydir.get_balance(&genesis),
        expected_genesis_balance
    );

    // Each recipient (1..=35) has exactly 10 AXM
    for seq in 1..=35u64 {
        let recipient = AccountId::new([(seq as u8); 32]);
        assert_eq!(
            restored_keydir.get_balance(&recipient),
            transfer_amount,
            "Recipient {seq} balance must match transfer amount"
        );
    }

    let _ = fs::remove_file(&seg0_path);
    let _ = fs::remove_file(&snap_path);
    let _ = fs::remove_file(&seg1_path);
}
