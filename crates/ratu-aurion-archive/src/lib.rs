#![forbid(unsafe_code)]

//! # Axiom Archive
//!
//! Modul kompresi bulanan (.zip), segmentasi data per akun (partitioning),
//! dan pemotongan log aktif (*pruning*) berbasis verifikasi digest.

pub mod archiver;
pub mod error;
pub mod manifest;
pub mod partitioner;
pub mod pruner;

pub use archiver::*;
pub use error::*;
pub use manifest::*;
pub use partitioner::*;
pub use pruner::*;

#[cfg(test)]
mod tests {
    use super::*;
    use ratu_aurion_primitives::crypto::{AccountId, Hash, Signature};
    use ratu_aurion_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER, RECORD_SIZE};
    use ratu_aurion_primitives::value::AurValue;
    use ratu_aurion_storage::reader::SegmentReader;
    use ratu_aurion_storage::writer::SegmentWriter;
    use std::fs::{self, File};
    use std::io::Read;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::ZipArchive;

    fn unique_test_path(label: &str, ext: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ratu_aurion_archive_test_{label}_{nanos}.{ext}"))
    }

    fn sample_tx(epoch: u64, seq: u64, sender: AccountId, recipient: AccountId, amount: u128) -> MutationRecord {
        MutationRecord {
            epoch,
            sequence_number: seq,
            record_kind: RECORD_KIND_TRANSFER,
            sender,
            recipient,
            amount: AurValue::from_atomic(amount),
            signature: Signature::new([0x77; 64]),
        }
    }

    #[test]
    fn test_partition_segment_by_account() {
        let seg_path = unique_test_path("partition", "log");
        let epoch = 3;
        let mut writer = SegmentWriter::create(&seg_path, 1, epoch, 0).expect("Writer create");

        let alice = AccountId::new([0x01; 32]);
        let bob = AccountId::new([0x02; 32]);
        let charlie = AccountId::new([0x03; 32]);

        let tx1 = sample_tx(epoch, 1, alice, bob, 10_000_000_000);
        let tx2 = sample_tx(epoch, 2, bob, charlie, 5_000_000_000);
        let tx3 = sample_tx(epoch, 3, alice, charlie, 2_000_000_000);

        writer.append_record(&tx1).unwrap();
        writer.append_record(&tx2).unwrap();
        writer.append_record(&tx3).unwrap();
        writer.seal_segment(Hash::new([0xaa; 32]), 1_728_200_000).unwrap();

        let mut reader = SegmentReader::open(&seg_path).expect("Reader open");
        let partitioner = AccountPartitioner::partition_segment(&mut reader).expect("Partitioning");

        assert_eq!(partitioner.total_accounts(), 3);
        assert_eq!(partitioner.total_records(), 3);

        // Alice terlibat di tx1 dan tx3
        let alice_records = partitioner.accounts.get(&alice).expect("Alice records");
        assert_eq!(alice_records.len(), 2);
        assert_eq!(alice_records[0], tx1);
        assert_eq!(alice_records[1], tx3);

        // Bob terlibat di tx1 dan tx2
        let bob_records = partitioner.accounts.get(&bob).expect("Bob records");
        assert_eq!(bob_records.len(), 2);
        assert_eq!(bob_records[0], tx1);
        assert_eq!(bob_records[1], tx2);

        // Charlie terlibat di tx2 dan tx3
        let charlie_records = partitioner.accounts.get(&charlie).expect("Charlie records");
        assert_eq!(charlie_records.len(), 2);
        assert_eq!(charlie_records[0], tx2);
        assert_eq!(charlie_records[1], tx3);

        let _ = fs::remove_file(&seg_path);
    }

    #[test]
    fn test_create_monthly_archive_zip_integrity() {
        let seg_path = unique_test_path("archive_create", "log");
        let zip_path = unique_test_path("archive_output", "zip");
        let epoch = 4;

        let mut writer = SegmentWriter::create(&seg_path, 1, epoch, 0).expect("Writer create");
        let user1 = AccountId::new([0x10; 32]);
        let user2 = AccountId::new([0x20; 32]);

        let tx = sample_tx(epoch, 1, user1, user2, 100_000_000_000);
        writer.append_record(&tx).unwrap();

        let digest = Hash::new([0x42; 32]);
        writer.seal_segment(digest, 1_728_300_000).unwrap();

        let manifest = create_monthly_archive(epoch, &seg_path, &zip_path).expect("Archive create");
        assert_eq!(manifest.epoch, epoch);
        assert_eq!(manifest.total_records, 1);
        assert_eq!(manifest.total_accounts, 2);
        assert_eq!(manifest.state_digest, digest);

        // Buka berkas ZIP dan verifikasi komponen internal
        let zip_file = File::open(&zip_path).expect("Open zip");
        let mut zip_archive = ZipArchive::new(zip_file).expect("Parse zip");

        // 1. Manifes terbaca
        let restored_manifest = {
            let mut mf = zip_archive.by_name("manifest.bin").expect("manifest.bin exists");
            let mut m_buf = [0u8; MANIFEST_HEADER_SIZE];
            mf.read_exact(&mut m_buf).unwrap();
            ArchiveManifestHeader::from_bytes(&m_buf).unwrap()
        };
        assert_eq!(manifest, restored_manifest);

        // 2. Berkas akun ada dan ukurannya kelipatan RECORD_SIZE (161 byte)
        let mut user1_hex = String::with_capacity(64);
        for b in user1.as_bytes() {
            use core::fmt::Write;
            let _ = write!(&mut user1_hex, "{b:02x}");
        }
        let user1_entry = format!("accounts/{user1_hex}.bin");
        let af = zip_archive.by_name(&user1_entry).expect("Account entry exists");
        assert_eq!(af.size(), RECORD_SIZE as u64);


        let _ = fs::remove_file(&seg_path);
        let _ = fs::remove_file(&zip_path);
    }

    #[test]
    fn test_verify_and_prune_success_and_digest_mismatch() {
        let seg_path = unique_test_path("prune_target", "log");
        let zip_path = unique_test_path("prune_archive", "zip");
        let epoch = 5;

        let mut writer = SegmentWriter::create(&seg_path, 1, epoch, 0).expect("Writer create");
        let u1 = AccountId::new([0x31; 32]);
        let u2 = AccountId::new([0x32; 32]);
        let tx = sample_tx(epoch, 1, u1, u2, 50_000_000_000);
        writer.append_record(&tx).unwrap();

        let real_digest = Hash::new([0x99; 32]);
        writer.seal_segment(real_digest, 1_728_400_000).unwrap();

        // Buat arsip yang valid
        create_monthly_archive(epoch, &seg_path, &zip_path).expect("Archive create");
        assert!(seg_path.exists());

        // Uji coba penolakan jika file segmen berbeda digest
        let corrupted_seg_path = unique_test_path("corrupted_seg", "log");
        let mut corrupt_writer = SegmentWriter::create(&corrupted_seg_path, 1, epoch, 1).unwrap();
        corrupt_writer.seal_segment(Hash::new([0x00; 32]), 1_728_400_000).unwrap();

        let mismatch_result = verify_and_prune_segment(&corrupted_seg_path, &zip_path);
        assert!(matches!(mismatch_result, Err(ArchiveError::DigestMismatch)));
        assert!(corrupted_seg_path.exists(), "Corrupted segment must NOT be pruned");
        let _ = fs::remove_file(&corrupted_seg_path);

        // Uji coba sukses: segmen asli berhasil dipangkas (pruned)
        assert!(seg_path.exists());
        let prune_result = verify_and_prune_segment(&seg_path, &zip_path);
        assert!(prune_result.is_ok());
        assert!(!seg_path.exists(), "Verified segment must be pruned from disk");
        assert!(zip_path.exists(), "Archive zip must remain intact");

        let _ = fs::remove_file(&zip_path);
    }
}
