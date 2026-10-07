#![forbid(unsafe_code)]

//! # Axiom Index
//!
//! Tabel indeks in-memory deterministik berbasis pola Bitcask (Keydir)
//! untuk pencarian instan status saldo dan lokasi disk mutasi akun.

pub mod entry;
pub mod error;
pub mod keydir;
pub mod replay;

pub use entry::*;
pub use error::*;
pub use keydir::*;
pub use replay::*;

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_primitives::crypto::{AccountId, Hash, Signature};
    use axiom_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER};
    use axiom_primitives::value::AxmValue;
    use axiom_storage::reader::SegmentReader;
    use axiom_storage::writer::SegmentWriter;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_path(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("axiom_index_test_{label}_{nanos}.log"))
    }

    #[test]
    fn test_transfer_mutation_balance_and_location_update() {
        let mut keydir = Keydir::new();
        let alice = AccountId::new([0x01; 32]);
        let bob = AccountId::new([0x02; 32]);

        // Berikan saldo awal 100 AXM kepada Alice
        let initial_alice_balance = AxmValue::from_atomic(100_000_000_000);
        keydir.seed_account(
            alice,
            initial_alice_balance,
            AccountLocation::new(0, 0, 0, 0),
        );

        // Mutasi 1: Alice transfer 30 AXM ke Bob (seq 1)
        let record1 = MutationRecord {
            epoch: 1,
            sequence_number: 1,
            record_kind: RECORD_KIND_TRANSFER,
            sender: alice,
            recipient: bob,
            amount: AxmValue::from_atomic(30_000_000_000),
            signature: Signature::new([0x99; 64]),
        };

        keydir
            .apply_mutation(&record1, 1, 0, 42)
            .expect("Mutation 1 should succeed");

        assert_eq!(
            keydir.get_balance(&alice).to_atomic(),
            70_000_000_000
        );
        assert_eq!(
            keydir.get_balance(&bob).to_atomic(),
            30_000_000_000
        );
        assert_eq!(
            keydir.get_location(&alice),
            Some(AccountLocation::new(1, 0, 42, 1))
        );
        assert_eq!(
            keydir.get_location(&bob),
            Some(AccountLocation::new(1, 0, 42, 1))
        );

        // Mutasi 2: Alice transfer 20 AXM lagi ke Bob (seq 2)
        let record2 = MutationRecord {
            epoch: 1,
            sequence_number: 2,
            record_kind: RECORD_KIND_TRANSFER,
            sender: alice,
            recipient: bob,
            amount: AxmValue::from_atomic(20_000_000_000),
            signature: Signature::new([0x99; 64]),
        };

        keydir
            .apply_mutation(&record2, 1, 0, 203)
            .expect("Mutation 2 should succeed");

        assert_eq!(
            keydir.get_balance(&alice).to_atomic(),
            50_000_000_000
        );
        assert_eq!(
            keydir.get_balance(&bob).to_atomic(),
            50_000_000_000
        );
        assert_eq!(
            keydir.get_location(&alice),
            Some(AccountLocation::new(1, 0, 203, 2))
        );
    }

    #[test]
    fn test_insufficient_balance_rejection() {
        let mut keydir = Keydir::new();
        let charlie = AccountId::new([0x03; 32]);
        let dave = AccountId::new([0x04; 32]);

        // Charlie hanya memiliki 10 AXM
        keydir.seed_account(
            charlie,
            AxmValue::from_atomic(10_000_000_000),
            AccountLocation::new(1, 0, 0, 0),
        );

        // Charlie mencoba transfer 15 AXM (melebihi saldo)
        let record = MutationRecord {
            epoch: 1,
            sequence_number: 1,
            record_kind: RECORD_KIND_TRANSFER,
            sender: charlie,
            recipient: dave,
            amount: AxmValue::from_atomic(15_000_000_000),
            signature: Signature::new([0x00; 64]),
        };

        let result = keydir.apply_mutation(&record, 1, 0, 42);
        assert!(matches!(result, Err(IndexError::InsufficientBalance)));

        // Saldo Charlie dan Dave harus tetap utuh
        assert_eq!(
            keydir.get_balance(&charlie).to_atomic(),
            10_000_000_000
        );
        assert_eq!(keydir.get_balance(&dave), AxmValue::ZERO);
    }

    #[test]
    fn test_stale_sequence_number_rejection() {
        let mut keydir = Keydir::new();
        let sender = AccountId::new([0x05; 32]);
        let recipient = AccountId::new([0x06; 32]);

        keydir.seed_account(
            sender,
            AxmValue::from_atomic(100_000_000_000),
            AccountLocation::new(1, 0, 42, 10), // Terakhir sequence 10
        );

        // Transaksi dengan sequence yang sama (10) harus ditolak
        let record_stale = MutationRecord {
            epoch: 1,
            sequence_number: 10,
            record_kind: RECORD_KIND_TRANSFER,
            sender,
            recipient,
            amount: AxmValue::from_atomic(5_000_000_000),
            signature: Signature::new([0x00; 64]),
        };

        let err_stale = keydir.apply_mutation(&record_stale, 1, 0, 203);
        assert!(matches!(err_stale, Err(IndexError::StaleSequenceNumber)));

        // Transaksi dengan sequence lebih kecil (9) juga harus ditolak
        let mut record_older = record_stale;
        record_older.sequence_number = 9;
        let err_older = keydir.apply_mutation(&record_older, 1, 0, 203);
        assert!(matches!(err_older, Err(IndexError::StaleSequenceNumber)));
    }

    #[test]
    fn test_full_lifecycle_replay_from_disk_segment() {
        let path = unique_test_path("full_replay");
        let epoch = 5;
        let segment_idx = 1;

        let mut writer = SegmentWriter::create(&path, 1, epoch, segment_idx)
            .expect("Writer creation should succeed");

        let genesis = AccountId::new([0xaa; 32]);
        let user_a = AccountId::new([0xbb; 32]);
        let user_b = AccountId::new([0xcc; 32]);

        // Siapkan Keydir pembanding
        let mut expected_keydir = Keydir::new();
        let initial_funding = AxmValue::from_atomic(1_000_000_000_000);
        expected_keydir.seed_account(
            genesis,
            initial_funding,
            AccountLocation::new(0, 0, 0, 0),
        );

        // Buat 3 transaksi beruntun
        let tx1 = MutationRecord {
            epoch,
            sequence_number: 1,
            record_kind: RECORD_KIND_TRANSFER,
            sender: genesis,
            recipient: user_a,
            amount: AxmValue::from_atomic(400_000_000_000),
            signature: Signature::new([0x11; 64]),
        };
        let off1 = writer.append_record(&tx1).expect("Append tx1");
        expected_keydir
            .apply_mutation(&tx1, epoch, segment_idx, off1)
            .unwrap();

        let tx2 = MutationRecord {
            epoch,
            sequence_number: 2,
            record_kind: RECORD_KIND_TRANSFER,
            sender: user_a,
            recipient: user_b,
            amount: AxmValue::from_atomic(150_000_000_000),
            signature: Signature::new([0x22; 64]),
        };
        let off2 = writer.append_record(&tx2).expect("Append tx2");
        expected_keydir
            .apply_mutation(&tx2, epoch, segment_idx, off2)
            .unwrap();

        let tx3 = MutationRecord {
            epoch,
            sequence_number: 3,
            record_kind: RECORD_KIND_TRANSFER,
            sender: genesis,
            recipient: user_b,
            amount: AxmValue::from_atomic(200_000_000_000),
            signature: Signature::new([0x33; 64]),
        };
        let off3 = writer.append_record(&tx3).expect("Append tx3");
        expected_keydir
            .apply_mutation(&tx3, epoch, segment_idx, off3)
            .unwrap();

        // Segel segmen pada disk
        let digest = Hash::new([0xfe; 32]);
        writer.seal_segment(digest, 1_728_100_000).expect("Seal segment");

        // Simulasi node booting: Buka SegmentReader dan jalankan Replay Engine
        let mut reader = SegmentReader::open(&path).expect("Reader should open");
        let mut replayed_keydir = Keydir::new();
        replayed_keydir.seed_account(
            genesis,
            initial_funding,
            AccountLocation::new(0, 0, 0, 0),
        );

        let replayed_count = rebuild_index_from_segment(
            &mut reader,
            &mut replayed_keydir,
            epoch,
            segment_idx,
        )
        .expect("Replay should succeed");

        assert_eq!(replayed_count, 3);
        assert_eq!(
            replayed_keydir.get_balance(&genesis),
            expected_keydir.get_balance(&genesis)
        );
        assert_eq!(
            replayed_keydir.get_balance(&user_a),
            expected_keydir.get_balance(&user_a)
        );
        assert_eq!(
            replayed_keydir.get_balance(&user_b),
            expected_keydir.get_balance(&user_b)
        );
        assert_eq!(
            replayed_keydir.get_location(&genesis),
            expected_keydir.get_location(&genesis)
        );
        assert_eq!(
            replayed_keydir.get_location(&user_a),
            expected_keydir.get_location(&user_a)
        );
        assert_eq!(
            replayed_keydir.get_location(&user_b),
            expected_keydir.get_location(&user_b)
        );

        let _ = fs::remove_file(&path);
    }
}
