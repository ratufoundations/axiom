#![forbid(unsafe_code)]

//! # Axiom Storage
//!
//! Append-only log engine berkapasitas tetap (128 MB) dengan catatan kaki
//! segmen deterministik (sealed footer).

pub mod error;
pub mod reader;
pub mod segment;
pub mod writer;

pub use error::*;
pub use reader::*;
pub use segment::*;
pub use writer::*;

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_primitives::crypto::{AccountId, Hash, Signature};
    use axiom_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER, RECORD_SIZE};
    use axiom_primitives::value::AxmValue;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_path(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("axiom_storage_test_{label}_{nanos}.log"))
    }

    fn sample_record(seq: u64, amount_raw: u128) -> MutationRecord {
        MutationRecord {
            epoch: 1,
            sequence_number: seq,
            record_kind: RECORD_KIND_TRANSFER,
            sender: AccountId::new([0x01; 32]),
            recipient: AccountId::new([0x02; 32]),
            amount: AxmValue::from_atomic(amount_raw),
            signature: Signature::new([0x03; 64]),
        }
    }

    #[test]
    fn test_segment_header_and_footer_sizes() {
        let header = SegmentHeader::new(1, 10, 0);
        let header_bytes = header.to_bytes();
        assert_eq!(header_bytes.len(), SEGMENT_HEADER_SIZE);
        assert_eq!(header_bytes.len(), 42);

        let restored_header =
            SegmentHeader::from_bytes(&header_bytes).expect("Valid header must deserialize");
        assert_eq!(header, restored_header);

        let footer = SegmentFooter::new(100, 10, 1, 100, Hash::new([0xaa; 32]), 1_700_000_000);
        let footer_bytes = footer.to_bytes();
        assert_eq!(footer_bytes.len(), SEGMENT_FOOTER_SIZE);
        assert_eq!(footer_bytes.len(), 88);

        let restored_footer =
            SegmentFooter::from_bytes(&footer_bytes).expect("Valid footer must deserialize");
        assert_eq!(footer, restored_footer);
    }

    #[test]
    fn test_initial_file_size_is_header_only() {
        let path = unique_test_path("initial_size");
        let writer = SegmentWriter::create(&path, 1, 1, 0).expect("Writer creation should succeed");

        let metadata = fs::metadata(&path).expect("File metadata should exist");
        assert_eq!(metadata.len(), SEGMENT_HEADER_SIZE as u64);
        assert_eq!(writer.current_offset(), SEGMENT_HEADER_SIZE as u64);
        assert_eq!(writer.total_records(), 0);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_linear_append_and_random_read_records() {
        let path = unique_test_path("linear_append");
        let mut writer =
            SegmentWriter::create(&path, 1, 1, 0).expect("Writer creation should succeed");

        let rec1 = sample_record(101, 10_000_000_000);
        let rec2 = sample_record(102, 20_000_000_000);
        let rec3 = sample_record(103, 30_000_000_000);

        let offset1 = writer.append_record(&rec1).expect("Append rec1");
        let offset2 = writer.append_record(&rec2).expect("Append rec2");
        let offset3 = writer.append_record(&rec3).expect("Append rec3");

        assert_eq!(offset1, SEGMENT_HEADER_SIZE as u64);
        assert_eq!(offset2, offset1 + RECORD_SIZE as u64);
        assert_eq!(offset3, offset2 + RECORD_SIZE as u64);

        let expected_size = (SEGMENT_HEADER_SIZE + (3 * RECORD_SIZE)) as u64;
        let metadata = fs::metadata(&path).expect("File metadata should exist");
        assert_eq!(metadata.len(), expected_size);
        assert_eq!(writer.total_records(), 3);

        // Verifikasi pembacaan melalui SegmentReader
        let mut reader = SegmentReader::open(&path).expect("Reader should open");
        assert_eq!(reader.header().epoch, 1);
        assert_eq!(reader.header().segment_index, 0);

        let read_rec1 = reader.read_record_at(offset1).expect("Read rec1");
        let read_rec2 = reader.read_record_at(offset2).expect("Read rec2");
        let read_rec3 = reader.read_record_at(offset3).expect("Read rec3");

        assert_eq!(rec1, read_rec1);
        assert_eq!(rec2, read_rec2);
        assert_eq!(rec3, read_rec3);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_seal_segment_and_rejection_of_further_appends() {
        let path = unique_test_path("seal_segment");
        let mut writer =
            SegmentWriter::create(&path, 1, 5, 2).expect("Writer creation should succeed");

        let rec1 = sample_record(5001, 5_000_000_000);
        let rec2 = sample_record(5002, 7_500_000_000);

        writer.append_record(&rec1).expect("Append rec1");
        writer.append_record(&rec2).expect("Append rec2");

        let digest = Hash::new([0x77; 32]);
        let sealed_timestamp = 1_728_000_000;

        let footer = writer
            .seal_segment(digest, sealed_timestamp)
            .expect("Seal segment should succeed");

        assert_eq!(footer.total_records, 2);
        assert_eq!(footer.first_sequence, 5001);
        assert_eq!(footer.last_sequence, 5002);
        assert_eq!(footer.state_digest, digest);
        assert_eq!(footer.sealed_at, sealed_timestamp);
        assert!(writer.is_sealed());

        // Verifikasi bahwa penulisan baru ditolak setelah disegel
        let rec3 = sample_record(5003, 1_000_000_000);
        let err_append = writer.append_record(&rec3);
        assert!(matches!(err_append, Err(StorageError::SegmentAlreadySealed)));

        // Verifikasi bahwa penyegelan ulang ditolak
        let err_reseal = writer.seal_segment(digest, sealed_timestamp);
        assert!(matches!(err_reseal, Err(StorageError::SegmentAlreadySealed)));

        // Total ukuran berkas harus tepat: Header (42) + 2 Records (322) + Footer (88) = 452
        let expected_total =
            (SEGMENT_HEADER_SIZE + (2 * RECORD_SIZE) + SEGMENT_FOOTER_SIZE) as u64;
        let metadata = fs::metadata(&path).expect("Metadata");
        assert_eq!(metadata.len(), expected_total);

        // Verifikasi pembacaan footer melalui reader
        let mut reader = SegmentReader::open(&path).expect("Reader should open");
        let read_footer = reader.read_footer().expect("Read footer should succeed");
        assert_eq!(footer, read_footer);

        let _ = fs::remove_file(&path);
    }
}
