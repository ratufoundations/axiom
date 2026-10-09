#![forbid(unsafe_code)]

//! # Axiom Engine
//!
//! Mesin validasi dan orkestrasi transaksi utama yang menghubungkan verifikasi
//! kriptografis Ed25519, penyimpanan disk sekuensial linear, proyeksi status RAM,
//! dan rotasi arsip periodik.

pub mod coordinator;
pub mod error;
pub mod pipeline;
pub mod validator;

pub use coordinator::*;
pub use error::*;
pub use pipeline::*;
pub use validator::*;

#[cfg(test)]
mod tests {
    use super::*;
    use ratu_aurion_primitives::crypto::{AccountId, Signature};
    use ratu_aurion_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER};
    use ratu_aurion_primitives::value::AurValue;
    use ed25519_dalek::{Signer, SigningKey};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dirs(label: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("ratu_aurion_engine_{label}_{nanos}"));
        let data_dir = base.join("data");
        let archive_dir = base.join("archive");
        (data_dir, archive_dir)
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
        let payload = compute_signing_payload(&record);
        let dalek_sig = signing_key.sign(&payload);
        record.signature = Signature::new(dalek_sig.to_bytes());
        record
    }

    #[test]
    fn test_invalid_signature_rejection() {
        let (data_dir, archive_dir) = unique_test_dirs("invalid_sig");
        let mut engine = EngineCoordinator::new(&data_dir, &archive_dir, 1).expect("Engine init");

        let (alice_key, alice) = create_keypair(0x01);
        let (_, bob) = create_keypair(0x02);

        engine.seed_account(alice, AurValue::from_atomic(100_000_000_000));

        let mut record = sign_mutation(&alice_key, 1, 1, alice, bob, 20_000_000_000);

        // Manipulasi nominal transaksi tanpa memperbarui tanda tangan
        record.amount = AurValue::from_atomic(25_000_000_000);

        let result = engine.submit_transaction(&record);
        assert!(matches!(result, Err(EngineError::InvalidSignature)));

        let _ = fs::remove_dir_all(data_dir.parent().unwrap());
    }

    #[test]
    fn test_valid_transaction_pipeline_and_location_tracking() {
        let (data_dir, archive_dir) = unique_test_dirs("pipeline");
        let mut engine = EngineCoordinator::new(&data_dir, &archive_dir, 1).expect("Engine init");

        let (alice_key, alice) = create_keypair(0x10);
        let (_, bob) = create_keypair(0x20);

        // Beri saldo awal 500 AXM kepada Alice
        engine.seed_account(alice, AurValue::from_atomic(500_000_000_000));

        // Transaksi 1: Alice kirim 150 AXM ke Bob
        let tx1 = sign_mutation(&alice_key, 1, 1, alice, bob, 150_000_000_000);
        let offset1 = engine.submit_transaction(&tx1).expect("Submit tx1");
        assert_eq!(offset1, 42); // Offset pertama setelah header segmen

        assert_eq!(
            engine.query_balance(&alice).to_atomic(),
            350_000_000_000
        );
        assert_eq!(
            engine.query_balance(&bob).to_atomic(),
            150_000_000_000
        );

        let loc1 = engine.query_last_location(&alice).expect("Loc alice");
        assert_eq!(loc1.offset, 42);
        assert_eq!(loc1.sequence_number, 1);

        // Transaksi 2: Alice kirim 50 AXM lagi ke Bob
        let tx2 = sign_mutation(&alice_key, 1, 2, alice, bob, 50_000_000_000);
        let offset2 = engine.submit_transaction(&tx2).expect("Submit tx2");
        assert_eq!(offset2, 42 + 161);

        assert_eq!(
            engine.query_balance(&alice).to_atomic(),
            300_000_000_000
        );
        assert_eq!(
            engine.query_balance(&bob).to_atomic(),
            200_000_000_000
        );

        let loc2 = engine.query_last_location(&alice).expect("Loc alice 2");
        assert_eq!(loc2.offset, 203);
        assert_eq!(loc2.sequence_number, 2);

        let _ = fs::remove_dir_all(data_dir.parent().unwrap());
    }

    #[test]
    fn test_epoch_rotation_and_archival_lifecycle() {
        let (data_dir, archive_dir) = unique_test_dirs("rotation");
        let mut engine = EngineCoordinator::new(&data_dir, &archive_dir, 1).expect("Engine init");

        let (alice_key, alice) = create_keypair(0x30);
        let (_, bob) = create_keypair(0x40);

        engine.seed_account(alice, AurValue::from_atomic(100_000_000_000));

        let tx = sign_mutation(&alice_key, 1, 1, alice, bob, 40_000_000_000);
        engine.submit_transaction(&tx).expect("Submit tx");

        // Lakukan rotasi epoch dari epoch 1 ke epoch 2
        let archive_zip_path = engine.rotate_epoch(2).expect("Epoch rotation");

        // 1. Berkas zip arsip bulanan harus terbentuk
        assert!(archive_zip_path.exists());

        // 2. Berkas segmen disk aktif lama harus sudah dipangkas (pruned)
        let old_seg_path = data_dir.join("epoch_1_seg_0.log");
        assert!(!old_seg_path.exists());

        // 3. Epoch aktif harus berpindah ke 2
        assert_eq!(engine.current_epoch(), 2);
        assert_eq!(engine.current_segment_idx(), 0);

        // 4. Kirim transaksi baru di epoch 2
        let tx_new = sign_mutation(&alice_key, 2, 2, alice, bob, 10_000_000_000);
        let off_new = engine.submit_transaction(&tx_new).expect("Submit in epoch 2");
        assert_eq!(off_new, 42); // Di segmen baru epoch 2, offset mulai dari 42

        assert_eq!(
            engine.query_balance(&alice).to_atomic(),
            50_000_000_000
        );
        assert_eq!(
            engine.query_balance(&bob).to_atomic(),
            50_000_000_000
        );

        let _ = fs::remove_dir_all(data_dir.parent().unwrap());
    }
}
