//! Integration tests for Compact Flat In-Memory Index (OPT-INDEX-01).

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_index::entry::{AccountLocation, CompactAccountEntry};
use axiom_index::error::IndexError;
use axiom_index::keydir::{Keydir, BUCKET_COUNT};
use axiom_primitives::crypto::{AccountId, Hash, Signature};
use axiom_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER};
use axiom_primitives::value::AxmValue;
use axiom_storage::writer::SegmentWriter;

#[test]
fn test_compact_account_entry_size_and_alignment() {
    assert_eq!(
        std::mem::size_of::<CompactAccountEntry>(),
        80,
        "CompactAccountEntry must be exactly 80 bytes"
    );
    assert_eq!(
        std::mem::align_of::<CompactAccountEntry>(),
        16,
        "CompactAccountEntry must have 16-byte alignment"
    );
}

#[test]
fn test_prefix_bucket_distribution_and_binary_search() {
    let mut keydir = Keydir::new();
    let mut accounts = Vec::with_capacity(2560);

    // Generate 2,560 accounts with uniformly distributed prefix bytes across 256 buckets
    for b in 0..256u16 {
        for i in 0..10u64 {
            let mut key = [0u8; 32];
            key[0] = b as u8;
            key[1..9].copy_from_slice(&i.to_le_bytes());
            key[9..17].copy_from_slice(&(b as u64).to_le_bytes());
            let account = AccountId::new(key);
            accounts.push((account, b as usize, i));
        }
    }

    assert_eq!(accounts.len(), 2560);

    // Seed each account into Keydir
    for (account, b, i) in &accounts {
        let balance = AxmValue::from_atomic((*b as u128 + 1) * 1_000_000 + *i as u128);
        let location = AccountLocation::new(1, 0, (*b as u64) * 100 + *i, 1);
        keydir.seed_account(*account, balance, location);
    }

    assert_eq!(keydir.account_count(), 2560);
    assert_eq!(keydir.len(), 2560);

    // Assert every bucket has exactly 10 accounts
    for b in 0..BUCKET_COUNT {
        assert_eq!(
            keydir.bucket_len(b),
            10,
            "Bucket {b} should have exactly 10 entries"
        );
    }

    // Assert every account is located correctly via binary search
    for (account, b, i) in &accounts {
        let expected_balance = AxmValue::from_atomic((*b as u128 + 1) * 1_000_000 + *i as u128);
        let expected_location = AccountLocation::new(1, 0, (*b as u64) * 100 + *i, 1);

        assert_eq!(keydir.get_balance(account), expected_balance);

        let state = keydir
            .get_account_state(account)
            .expect("Account must exist in Keydir");
        assert_eq!(state.balance, expected_balance);
        assert_eq!(state.location, expected_location);
        assert_eq!(keydir.get_location(account), Some(expected_location));
    }
}

#[test]
fn test_in_place_balance_mutation_and_supply_invariants() {
    let mut keydir = Keydir::new();
    let account_a = AccountId::new([0x01; 32]);
    let account_b = AccountId::new([0x02; 32]);

    let initial_a = AxmValue::from_whole_axm(100).expect("100 AXM");
    let initial_b = AxmValue::from_whole_axm(50).expect("50 AXM");
    let expected_supply = AxmValue::from_whole_axm(150).expect("150 AXM");

    keydir.seed_account(account_a, initial_a, AccountLocation::new(0, 0, 0, 0));
    keydir.seed_account(account_b, initial_b, AccountLocation::new(0, 0, 0, 0));

    assert_eq!(keydir.total_supply(), expected_supply);
    assert_eq!(keydir.account_count(), 2);

    // Execute transfer mutation of 30 AXM from A to B
    let transfer_amount = AxmValue::from_whole_axm(30).expect("30 AXM");
    let record = MutationRecord {
        epoch: 1,
        sequence_number: 1,
        record_kind: RECORD_KIND_TRANSFER,
        sender: account_a,
        recipient: account_b,
        amount: transfer_amount,
        signature: Signature::new([0xaa; 64]),
    };

    let mutation_offset = 1024u64;
    keydir
        .apply_mutation(&record, 1, 0, mutation_offset)
        .expect("Transfer mutation must succeed");

    let expected_a = AxmValue::from_whole_axm(70).expect("70 AXM");
    let expected_b = AxmValue::from_whole_axm(80).expect("80 AXM");

    assert_eq!(keydir.get_balance(&account_a), expected_a);
    assert_eq!(keydir.get_balance(&account_b), expected_b);
    assert_eq!(keydir.total_supply(), expected_supply);
    assert_eq!(keydir.account_count(), 2);

    let state_a = keydir
        .get_account_state(&account_a)
        .expect("account_a state must exist");
    assert_eq!(state_a.location.sequence_number, 1);
    assert_eq!(state_a.location.epoch, 1);
    assert_eq!(state_a.location.segment_idx, 0);
    assert_eq!(state_a.location.offset, mutation_offset);

    let state_b = keydir
        .get_account_state(&account_b)
        .expect("account_b state must exist");
    assert_eq!(state_b.location.sequence_number, 1);
    assert_eq!(state_b.location.epoch, 1);
    assert_eq!(state_b.location.segment_idx, 0);
    assert_eq!(state_b.location.offset, mutation_offset);
}

#[test]
fn test_stale_sequence_and_overdraft_rejections() {
    let mut keydir = Keydir::new();
    let sender = AccountId::new([0x05; 32]);
    let recipient = AccountId::new([0x06; 32]);

    let initial_balance = AxmValue::from_whole_axm(50).expect("50 AXM");
    keydir.seed_account(sender, initial_balance, AccountLocation::new(1, 0, 100, 5));

    // 1. Replay with stale sequence number (seq 5 <= 5)
    let stale_record = MutationRecord {
        epoch: 1,
        sequence_number: 5,
        record_kind: RECORD_KIND_TRANSFER,
        sender,
        recipient,
        amount: AxmValue::from_whole_axm(10).expect("10 AXM"),
        signature: Signature::new([0x00; 64]),
    };
    let err_stale = keydir.apply_mutation(&stale_record, 1, 0, 200);
    assert!(matches!(err_stale, Err(IndexError::StaleSequenceNumber)));

    // Replay with older sequence number (seq 4 < 5)
    let older_record = MutationRecord {
        epoch: 1,
        sequence_number: 4,
        record_kind: RECORD_KIND_TRANSFER,
        sender,
        recipient,
        amount: AxmValue::from_whole_axm(10).expect("10 AXM"),
        signature: Signature::new([0x00; 64]),
    };
    let err_older = keydir.apply_mutation(&older_record, 1, 0, 200);
    assert!(matches!(err_older, Err(IndexError::StaleSequenceNumber)));

    // 2. Overdraft transfer (60 AXM > 50 AXM) with valid sequence number (seq 6)
    let overdraft_record = MutationRecord {
        epoch: 1,
        sequence_number: 6,
        record_kind: RECORD_KIND_TRANSFER,
        sender,
        recipient,
        amount: AxmValue::from_whole_axm(60).expect("60 AXM"),
        signature: Signature::new([0x00; 64]),
    };
    let err_overdraft = keydir.apply_mutation(&overdraft_record, 1, 0, 300);
    assert!(matches!(err_overdraft, Err(IndexError::InsufficientBalance)));

    // Sender balance and sequence number must remain untouched
    assert_eq!(keydir.get_balance(&sender), initial_balance);
    assert_eq!(
        keydir.get_location(&sender),
        Some(AccountLocation::new(1, 0, 100, 5))
    );
    assert_eq!(keydir.get_balance(&recipient), AxmValue::ZERO);
}

#[test]
fn test_replay_segment_into_compact_keydir() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock must be monotonic")
        .as_nanos();
    let segment_path =
        std::env::temp_dir().join(format!("test_compact_replay_{}_{}.log", std::process::id(), nanos));

    let epoch = 2;
    let segment_index = 0;
    let mut writer = SegmentWriter::create(&segment_path, 1, epoch, segment_index)
        .expect("create SegmentWriter");

    let genesis = AccountId::new([0xaa; 32]);
    let initial_genesis_balance = AxmValue::from_whole_axm(10_000).expect("10,000 AXM");

    // Write 50 sequential transfer mutations from genesis to distinct accounts
    let transfer_per_tx = AxmValue::from_whole_axm(10).expect("10 AXM");
    let mut expected_recipients = Vec::with_capacity(50);

    for seq in 1..=50u64 {
        let mut recipient_key = [0xbb; 32];
        recipient_key[0] = (seq as u8).wrapping_add(1);
        recipient_key[1..9].copy_from_slice(&seq.to_le_bytes());
        let recipient = AccountId::new(recipient_key);
        expected_recipients.push(recipient);

        let tx = MutationRecord {
            epoch,
            sequence_number: seq,
            record_kind: RECORD_KIND_TRANSFER,
            sender: genesis,
            recipient,
            amount: transfer_per_tx,
            signature: Signature::new([(seq as u8); 64]),
        };

        writer.append_record(&tx).expect("append tx");
    }

    let digest = Hash::new([0xdd; 32]);
    writer
        .seal_segment(digest, 1_728_200_000)
        .expect("seal segment");

    // Initialize compact keydir, seed genesis, and replay
    let mut keydir = Keydir::new();
    keydir.seed_account(
        genesis,
        initial_genesis_balance,
        AccountLocation::new(0, 0, 0, 0),
    );

    let replayed = keydir
        .replay_segment(&segment_path)
        .expect("replay_segment should succeed");
    assert_eq!(replayed, 50);

    // Verify balances
    // Genesis: 10_000 - 50 * 10 = 9_500 AXM
    let expected_genesis = AxmValue::from_whole_axm(9_500).expect("9,500 AXM");
    assert_eq!(keydir.get_balance(&genesis), expected_genesis);
    let genesis_state = keydir
        .get_account_state(&genesis)
        .expect("genesis state must exist");
    assert_eq!(genesis_state.location.sequence_number, 50);
    assert_eq!(genesis_state.location.epoch, epoch);
    assert_eq!(genesis_state.location.segment_idx, segment_index);

    // Each recipient: 10 AXM, sequence number matching tx seq
    for (i, recipient) in expected_recipients.iter().enumerate() {
        assert_eq!(keydir.get_balance(recipient), transfer_per_tx);
        let r_state = keydir
            .get_account_state(recipient)
            .expect("recipient state must exist");
        assert_eq!(r_state.location.sequence_number, (i as u64) + 1);
    }

    assert_eq!(keydir.total_supply(), initial_genesis_balance);
    assert_eq!(keydir.account_count(), 51);

    let _ = fs::remove_file(&segment_path);
}
