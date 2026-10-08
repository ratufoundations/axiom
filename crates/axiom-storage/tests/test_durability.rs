#![forbid(unsafe_code)]

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_primitives::crypto::{AccountId, Hash, Signature};
use axiom_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER, RECORD_SIZE};
use axiom_primitives::value::AxmValue;
use axiom_storage::{
    DurabilityPolicy, SegmentReader, SegmentWriter, WRITE_BUFFER_CAPACITY,
};

fn unique_test_path(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("axiom_durability_test_{label}_{nanos}.log"))
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
fn test_strict_durability_mode() {
    let path = unique_test_path("strict_mode");

    // 1. Inisialisasi writer dengan DurabilityPolicy::Strict
    let mut writer = SegmentWriter::create_with_policy(
        &path,
        1,
        1,
        0,
        DurabilityPolicy::Strict,
    )
    .expect("Gagal membuat writer Strict");

    assert_eq!(writer.policy(), DurabilityPolicy::Strict);

    // 2. Tulis 3 record mutasi
    for seq in 1..=3 {
        let rec = sample_record(seq, seq as u128 * 1_000);
        let offset = writer.append_record(&rec).expect("Append record");
        assert_eq!(offset, 42 + ((seq - 1) * RECORD_SIZE as u64));

        // Setiap record harus langsung diflush ke disk dan sync_data() dieksekusi
        assert_eq!(writer.uncommitted_records(), 0);
        assert!(writer.write_buffer().is_empty());
        assert_eq!(writer.flushed_offset(), writer.current_offset());
    }

    // 3. Buka SegmentReader terpisah pada berkas yang sama tanpa menutup/drop writer
    let mut reader = SegmentReader::open(&path).expect("Buka reader terpisah");
    for seq in 1..=3 {
        let offset = 42 + ((seq - 1) * RECORD_SIZE as u64);
        let rec = reader
            .read_record_at(offset)
            .expect("Record harus terbaca fisik di disk secara instan");
        assert_eq!(rec.sequence_number, seq);
        assert_eq!(rec.amount.to_atomic(), seq as u128 * 1_000);
    }

    let _ = fs::remove_file(&path);
}

#[test]
fn test_group_commit_batch_threshold() {
    let path = unique_test_path("group_commit_threshold");

    // 1. Inisialisasi writer dengan batch_size: 10
    let mut writer = SegmentWriter::create_with_policy(
        &path,
        1,
        1,
        0,
        DurabilityPolicy::GroupCommit { batch_size: 10 },
    )
    .expect("Gagal membuat writer GroupCommit");

    // 2. Tulis 9 record: data harus tertahan di memory buffer dan belum disinkronisasi ke disk
    for seq in 1..=9 {
        let rec = sample_record(seq, seq as u128 * 2_000);
        writer.append_record(&rec).expect("Append record");
    }

    assert_eq!(writer.uncommitted_records(), 9);
    assert_eq!(writer.flushed_offset(), 42);
    assert_eq!(writer.write_buffer().len(), 9 * RECORD_SIZE);
    assert_eq!(writer.write_buffer().len(), 1449);

    // Verifikasi pembacaan langsung di disk: record belum diflush (berisi 0x00 pra-alokasi)
    {
        let mut reader = SegmentReader::open(&path).expect("Buka reader sebelum batch threshold");
        let disk_rec = reader
            .read_record_at(42)
            .expect("Membaca slot pra-alokasi");
        // Di disk masih berupa nol biner sehingga sequence number bernilai 0
        assert_eq!(disk_rec.sequence_number, 0);
    }

    // 3. Tulis record ke-10: ambang batas batch terpenuhi!
    let rec10 = sample_record(10, 20_000);
    writer.append_record(&rec10).expect("Append record ke-10");

    // Buffer harus ditulis ke disk, sync_data() dieksekusi, dan uncommitted_records reset ke 0
    assert_eq!(writer.uncommitted_records(), 0);
    assert!(writer.write_buffer().is_empty());
    assert_eq!(writer.flushed_offset(), 42 + (10 * RECORD_SIZE as u64));
    assert_eq!(writer.flushed_offset(), 1652);

    // 4. Verifikasi seluruh 10 record kini terbaca fisik di disk via SegmentReader
    let mut reader = SegmentReader::open(&path).expect("Buka reader setelah batch threshold");
    for seq in 1..=10 {
        let offset = 42 + ((seq - 1) * RECORD_SIZE as u64);
        let rec = reader
            .read_record_at(offset)
            .expect("Record batch harus terbaca di disk");
        assert_eq!(rec.sequence_number, seq);
        assert_eq!(rec.amount.to_atomic(), seq as u128 * 2_000);
    }

    let _ = fs::remove_file(&path);
}

#[test]
fn test_buffered_relaxed_128kb_auto_flush() {
    let path = unique_test_path("buffered_relaxed_128kb");

    // 1. Inisialisasi writer dengan DurabilityPolicy::BufferedRelaxed
    let mut writer = SegmentWriter::create_with_policy(
        &path,
        1,
        1,
        0,
        DurabilityPolicy::BufferedRelaxed,
    )
    .expect("Gagal membuat writer BufferedRelaxed");

    assert_eq!(WRITE_BUFFER_CAPACITY, 131_072);

    // 2. Maksimum record yang muat pada buffer 128 KB tanpa overflow: 131.072 / 161 = 814 record (131.054 byte)
    let max_records_in_buffer = WRITE_BUFFER_CAPACITY / RECORD_SIZE;
    assert_eq!(max_records_in_buffer, 814);
    let expected_buffer_bytes = max_records_in_buffer * RECORD_SIZE;
    assert_eq!(expected_buffer_bytes, 131_054);

    for seq in 1..=814 {
        let rec = sample_record(seq, seq as u128 * 10);
        writer.append_record(&rec).expect("Append buffer record");
    }

    // Pastikan buffer memuat tepat 131.054 byte dan belum diflush ke disk
    assert_eq!(writer.write_buffer().len(), 131_054);
    assert_eq!(writer.flushed_offset(), 42);

    // 3. Tulis record ke-815: kapasitas buffer terlampaui, memicu auto-flush chunk 128 KB ke disk
    let rec815 = sample_record(815, 8150);
    writer.append_record(&rec815).expect("Append record ke-815");

    // flushed_offset maju sebanyak 131.054 byte ke 42 + 131.054 = 131.096 byte
    assert_eq!(writer.flushed_offset(), 42 + 131_054);
    assert_eq!(writer.flushed_offset(), 131_096);

    // Buffer kini hanya memuat record ke-815 (161 byte)
    assert_eq!(writer.write_buffer().len(), RECORD_SIZE);
    assert_eq!(writer.write_buffer().len(), 161);
    assert_eq!(writer.current_offset(), 42 + (815 * RECORD_SIZE as u64));
    assert_eq!(writer.current_offset(), 131_257);

    let _ = fs::remove_file(&path);
}

#[test]
fn test_explicit_flush_and_sync_and_seal() {
    let path = unique_test_path("explicit_flush_seal");

    // 1. Inisialisasi writer dengan DurabilityPolicy::BufferedRelaxed
    let mut writer = SegmentWriter::create_with_policy(
        &path,
        1,
        1,
        0,
        DurabilityPolicy::BufferedRelaxed,
    )
    .expect("Gagal membuat writer");

    // 2. Tulis 5 record
    for seq in 1..=5 {
        let rec = sample_record(seq, seq as u128 * 500);
        writer.append_record(&rec).expect("Append record");
    }

    assert_eq!(writer.write_buffer().len(), 5 * RECORD_SIZE);
    assert_eq!(writer.flushed_offset(), 42);

    // 3. Panggil flush_and_sync() secara eksplisit
    writer.flush_and_sync().expect("Eksekusi flush_and_sync gagal");

    assert!(writer.write_buffer().is_empty());
    assert_eq!(writer.flushed_offset(), 42 + (5 * RECORD_SIZE as u64));
    assert_eq!(writer.flushed_offset(), 847);
    assert_eq!(writer.uncommitted_records(), 0);

    // 4. Segel segmen: sisa buffer harus diflush, footer 88 byte ditulis, dan file dipotong
    let digest = Hash::new([0x99; 32]);
    let sealed_timestamp = 1_728_200_000u64;
    let footer = writer
        .seal_segment(digest, sealed_timestamp)
        .expect("Penyegelan segmen gagal");

    assert_eq!(footer.total_records, 5);
    assert!(writer.is_sealed());

    // Ukuran berkas terpotong ke 42 + 5 * 161 + 88 = 935 byte
    let meta_len = fs::metadata(&path).expect("Metadata").len();
    assert_eq!(meta_len, 935);

    // 5. Verifikasi bahwa reader membaca seluruh 5 record dan footer secara utuh
    let mut reader = SegmentReader::open(&path).expect("Buka reader segmen bersegel");
    let read_footer = reader.read_footer().expect("Baca footer");
    assert_eq!(read_footer.total_records, 5);

    for seq in 1..=5 {
        let offset = 42 + ((seq - 1) * RECORD_SIZE as u64);
        let rec = reader.read_record_at(offset).expect("Baca record");
        assert_eq!(rec.sequence_number, seq);
    }

    let _ = fs::remove_file(&path);
}
