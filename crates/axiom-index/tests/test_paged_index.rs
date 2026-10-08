//! Integration tests for LRU Sparse Paging & Cold State Eviction (OPT-INDEX-03).

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_index::cold::{ColdStore, COLD_HEADER_SIZE};
use axiom_index::entry::{AccountLocation, CompactAccountEntry, COMPACT_ACCOUNT_ENTRY_SIZE};
use axiom_index::error::IndexError;
use axiom_index::keydir::Keydir;
use axiom_primitives::crypto::{AccountId, Signature};
use axiom_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER};
use axiom_primitives::value::AxmValue;

fn unique_cold_path(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "axiom_cold_test_{label}_{}_{nanos}.idx",
        std::process::id()
    ))
}

#[test]
fn test_cold_store_header_and_binary_search_seek() {
    let path = unique_cold_path("header_layout");
    let mut cold_store = ColdStore::create_or_open(&path).expect("create_or_open");

    // Generate 512 accounts: exactly 2 accounts per bucket (256 * 2 = 512)
    let mut accounts = Vec::with_capacity(512);
    for b in 0..256u16 {
        for i in 0..2u64 {
            let mut key = [0u8; 32];
            key[0] = b as u8;
            key[1..9].copy_from_slice(&i.to_le_bytes());
            key[9..17].copy_from_slice(&(b as u64).to_le_bytes());
            let account = AccountId::new(key);
            let balance = AxmValue::from_atomic((b as u128 + 1) * 100_000 + i as u128);
            let entry = CompactAccountEntry::new(
                account,
                balance,
                1,
                0,
                (b as u64) * 10 + i,
                0,
                0,
            );
            accounts.push(entry);
        }
    }

    cold_store
        .flush_evicted_entries(&accounts)
        .expect("flush_evicted_entries");

    // Assert physical file layout
    // COLD_HEADER_SIZE (4,112 bytes) + 512 * 80 bytes = 4,112 + 40,960 = 45,072 bytes
    let file_len = fs::metadata(&path).expect("metadata").len();
    let expected_len = (COLD_HEADER_SIZE as u64)
        + 512 * (COMPACT_ACCOUNT_ENTRY_SIZE as u64);
    assert_eq!(file_len, expected_len);
    assert_eq!(file_len, 45_072);
    assert_eq!(cold_store.total_cold_accounts(), 512);

    // Assert on-disk binary search correctly resolves every cold account
    for entry in &accounts {
        let found = cold_store
            .get_account(&entry.account)
            .expect("get_account")
            .expect("account must exist in cold store");
        assert_eq!(found.account, entry.account);
        assert_eq!(found.balance, entry.balance);
        assert_eq!(found.sequence_number, entry.sequence_number);
    }

    let _ = fs::remove_file(&path);
}

#[test]
fn test_keydir_eviction_under_memory_budget() {
    let path = unique_cold_path("eviction_budget");
    let max_hot_capacity = 256;
    let mut keydir =
        Keydir::with_capacity(&path, max_hot_capacity).expect("with_capacity");

    // Seed 512 accounts (2 per bucket)
    let mut total_expected_supply = AxmValue::ZERO;
    let mut seeded_accounts = Vec::with_capacity(512);

    for b in 0..256u16 {
        for i in 0..2u64 {
            let mut key = [0u8; 32];
            key[0] = b as u8;
            key[1..9].copy_from_slice(&i.to_le_bytes());
            let account = AccountId::new(key);
            let balance = AxmValue::from_atomic((b as u128 + 1) * 1_000 + i as u128);
            total_expected_supply = total_expected_supply
                .checked_add(balance)
                .expect("supply overflow");
            let loc = AccountLocation::new(0, 0, (b as u64) * 10 + i, 1);
            keydir.seed_account(account, balance, loc);
            seeded_accounts.push(account);
        }
    }

    // Assert in-memory hot accounts are capped at <= max_hot_capacity (256)
    assert!(
        keydir.hot_account_count() <= max_hot_capacity,
        "Hot accounts count ({}) must not exceed max_hot_capacity ({})",
        keydir.hot_account_count(),
        max_hot_capacity
    );
    assert_eq!(keydir.hot_account_count(), 256);

    // Assert remaining 256 accounts are cleanly evicted into disk ColdStore
    assert_eq!(keydir.cold_account_count(), 256);
    assert_eq!(keydir.total_account_count(), 512);

    // Assert global total_supply strictly equals the sum of all 512 account balances
    assert_eq!(keydir.total_supply(), total_expected_supply);

    let _ = fs::remove_file(&path);
}

#[test]
fn test_cold_account_transparent_paging_and_mutation() {
    let path = unique_cold_path("paging_mutation");
    let mut keydir = Keydir::with_budget(&path, 2).expect("with_budget");

    let account_a = AccountId::new([0x01; 32]);
    let account_b = AccountId::new([0x02; 32]);

    let initial_a = AxmValue::from_whole_axm(50).expect("50 AXM");
    let initial_b = AxmValue::from_whole_axm(10).expect("10 AXM");

    keydir.seed_account(account_a, initial_a, AccountLocation::new(0, 0, 0, 0));
    keydir.seed_account(account_b, initial_b, AccountLocation::new(0, 0, 100, 0));

    let expected_total_supply = AxmValue::from_whole_axm(60).expect("60 AXM");
    assert_eq!(keydir.total_supply(), expected_total_supply);

    // Evict Account A to disk cold store
    let evicted = keydir.evict_account(&account_a).expect("evict_account");
    assert!(evicted, "Account A should be successfully evicted");

    // Assert Account A is NOT present in RAM directly
    assert!(!keydir.is_hot(&account_a));
    assert!(keydir.get_hot_entry(&account_a).is_none());
    assert_eq!(keydir.cold_account_count(), 1);
    assert_eq!(keydir.hot_account_count(), 1);

    // Execute transfer mutation: Account A (cold, 50 AXM) transfers 20 AXM to Account B (hot, 10 AXM)
    let transfer_amount = AxmValue::from_whole_axm(20).expect("20 AXM");
    let record = MutationRecord {
        epoch: 1,
        sequence_number: 1,
        record_kind: RECORD_KIND_TRANSFER,
        sender: account_a,
        recipient: account_b,
        amount: transfer_amount,
        signature: Signature::new([0xaa; 64]),
    };

    keydir
        .apply_mutation(&record, 1, 0, 203)
        .expect("apply_mutation must transparently page Account A and succeed");

    // Assert Account A is now paged back into RAM as hot
    assert!(
        keydir.is_hot(&account_a),
        "Account A must be hot in RAM after mutation"
    );
    assert!(keydir.is_hot(&account_b));

    // Assert Account A balance is 30 AXM (50 - 20)
    let expected_a = AxmValue::from_whole_axm(30).expect("30 AXM");
    assert_eq!(keydir.get_balance(&account_a), expected_a);

    // Assert Account B balance is 30 AXM (10 + 20)
    let expected_b = AxmValue::from_whole_axm(30).expect("30 AXM");
    assert_eq!(keydir.get_balance(&account_b), expected_b);

    // Assert total_supply remains strictly preserved
    assert_eq!(keydir.total_supply(), expected_total_supply);

    let _ = fs::remove_file(&path);
}

#[test]
fn test_prefetch_interface_for_stage1() {
    let path = unique_cold_path("prefetch");
    let mut keydir = Keydir::with_budget(&path, 1).expect("with_budget");

    let cold_account = AccountId::new([0x55; 32]);
    let initial_balance = AxmValue::from_whole_axm(100).expect("100 AXM");
    keydir.seed_account(
        cold_account,
        initial_balance,
        AccountLocation::new(0, 0, 0, 0),
    );

    // Force eviction to cold storage
    keydir.evict_account(&cold_account).expect("evict");
    assert!(!keydir.is_hot(&cold_account));
    assert_eq!(keydir.cold_account_count(), 1);

    // Stage 1 prefetch: call prefetch before executing transaction
    let prefetched = keydir.prefetch(&cold_account).expect("prefetch");
    assert!(
        prefetched,
        "Prefetch should return true when loading cold account"
    );

    // Assert account is now hot in RAM
    assert!(keydir.is_hot(&cold_account));
    assert_eq!(keydir.get_balance(&cold_account), initial_balance);

    // Repeated prefetch on already hot account must return false
    let prefetched_again = keydir.prefetch(&cold_account).expect("prefetch again");
    assert!(
        !prefetched_again,
        "Prefetch should return false when account is already in RAM"
    );

    let _ = fs::remove_file(&path);
}

#[test]
fn test_tampered_cold_index_fail_fast() {
    let path = unique_cold_path("tampered");

    // 1. Truncated header (< 4,112 bytes)
    fs::write(&path, [0x00; 100]).expect("write truncated");
    let res_trunc = ColdStore::open(&path);
    assert!(
        matches!(res_trunc, Err(IndexError::CorruptedColdIndex(_))),
        "Expected CorruptedColdIndex on truncated header"
    );

    // 2. Corrupted magic bytes in 4,112-byte header
    let mut bad_header = vec![0u8; COLD_HEADER_SIZE];
    bad_header[0..8].copy_from_slice(b"BADMAGIC");
    fs::write(&path, bad_header).expect("write bad magic");
    let res_magic = ColdStore::open(&path);
    assert!(
        matches!(res_magic, Err(IndexError::CorruptedColdIndex(_))),
        "Expected CorruptedColdIndex on invalid magic"
    );

    let _ = fs::remove_file(&path);
}
