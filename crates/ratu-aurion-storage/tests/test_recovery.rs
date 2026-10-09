#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use ratu_aurion_primitives::crypto::{AccountId, Signature};
use ratu_aurion_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER, RECORD_SIZE};
use ratu_aurion_primitives::value::AurValue;
use ratu_aurion_storage::{
    RecordStream, SegmentReader, SegmentWriter, StorageError, SEGMENT_HEADER_SIZE,
};

fn unique_test_path(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ratu_aurion_recovery_test_{label}_{nanos}.log"))
}

fn sample_record(seq: u64, amount_raw: u128) -> MutationRecord {
    MutationRecord {
        epoch: 1,
        sequence_number: seq,
        record_kind: RECORD_KIND_TRANSFER,
        sender: AccountId::new([0x01; 32]),
        recipient: AccountId::new([0x02; 32]),
        amount: AurValue::from_atomic(amount_raw),
        signature: Signature::new([0x03; 64]),
    }
}

#[test]
fn test_torn_write_recovery_at_tail() {
    let path = unique_test_path("torn_write_tail");

    // 1. Inisialisasi segmen baru
    let mut writer = SegmentWriter::create(&path, 1, 1, 0).expect("Gagal membuat segmen");
    assert_eq!(writer.current_offset(), 42);

    // 2. Tambahkan 3 record valid (Ukuran: 42 + 3 * 161 = 525 byte)
    let rec1 = sample_record(1, 10_000);
    let rec2 = sample_record(2, 20_000);
    let rec3 = sample_record(3, 30_000);

    writer.append_record(&rec1).expect("Append record 1");
    writer.append_record(&rec2).expect("Append record 2");
    writer.append_record(&rec3).expect("Append record 3");

    let valid_size = 42 + (3 * RECORD_SIZE as u64);
    assert_eq!(valid_size, 525);
    assert_eq!(writer.current_offset(), valid_size);
    drop(writer);

    // Simulasikan berkas dinamis tak berprapra-alokasi berukuran 525 byte sebelum injeksi sampah
    OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("Buka berkas untuk set_len")
        .set_len(525)
        .expect("Set len 525");

    let initial_meta_len = fs::metadata(&path).expect("Metadata segmen").len();
    assert_eq!(initial_meta_len, 525);

    // 3. Simulasi ungraceful crash / power loss: injeksi 73 byte sampah acak di ekor berkas (total 598 byte)
    let garbage_len = 73usize;
    {
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("Buka berkas untuk injeksi sampah");
        let garbage = vec![0xde; garbage_len];
        file.write_all(&garbage).expect("Injeksi torn-write bytes");
        file.flush().expect("Flush torn-write bytes");
    }

    let corrupted_len = fs::metadata(&path).expect("Metadata segmen korup").len();
    assert_eq!(corrupted_len, 598);

    // Invarian batas: terdapat sisa torn-write
    let payload_bytes = corrupted_len - SEGMENT_HEADER_SIZE as u64;
    let remainder = payload_bytes % (RECORD_SIZE as u64);
    assert_eq!(remainder, 73);

    // 4. Pembacaan stream sebelum recovery mendeteksi anomali (offset 525 tidak dapat membaca record 161B penuh)
    {
        let mut reader = SegmentReader::open(&path).expect("Buka segmen korup untuk inspeksi");
        let err = reader.read_record_at(525);
        assert!(err.is_err(), "Membaca record parsial harus menghasilkan galat OutOfBounds");

        let stream: RecordStream = reader.stream_records().expect("Buka stream reader");
        let parsed_records: Vec<_> = stream.collect();
        // Stream berhenti setelah 3 record karena sisa 73 byte tidak mencukupi 161 byte
        assert_eq!(parsed_records.len(), 3);
        for item in parsed_records {
            assert!(item.is_ok());
        }
    }

    // 5. Eksekusi mekanisme pemulihan SegmentWriter::recover_or_open
    let mut recovered_writer =
        SegmentWriter::recover_or_open(&path).expect("Pemulihan segmen gagal");

    // 6. Pastikan ukuran berkas di disk telah dipotong kembali ke tepat 525 byte
    let disk_len_after_recovery = fs::metadata(&path).expect("Metadata paska pemulihan").len();
    assert_eq!(disk_len_after_recovery, 525);
    assert_eq!(recovered_writer.current_offset(), 525);
    assert_eq!(recovered_writer.total_records(), 3);

    // 7. Pastikan seluruh 3 record asli tetap utuh dan terbaca melalui RecordStream
    {
        let reader = SegmentReader::open(&path).expect("Buka segmen pulih");
        let stream = reader.stream_records().expect("Buka stream record pulih");
        let items: Vec<_> = stream.collect::<Result<Vec<_>, _>>().expect("Stream harus valid");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].1.sequence_number, 1);
        assert_eq!(items[1].1.sequence_number, 2);
        assert_eq!(items[2].1.sequence_number, 3);
    }

    // 8. Tulis record ke-4 ke writer yang telah dipulihkan
    let rec4 = sample_record(4, 40_000);
    let offset4 = recovered_writer
        .append_record(&rec4)
        .expect("Append record 4 ke writer pulih");
    assert_eq!(offset4, 525);
    recovered_writer
        .flush_and_sync()
        .expect("Flush record 4 ke disk");

    // 9. Pastikan ukuran berkas bertambah bersih ke 525 + 161 = 686 byte tanpa korupsi
    let final_disk_len = fs::metadata(&path).expect("Metadata berkas akhir").len();
    assert_eq!(final_disk_len, 686);
    assert_eq!(recovered_writer.current_offset(), 686);
    assert_eq!(recovered_writer.total_records(), 4);

    {
        let mut reader = SegmentReader::open(&path).expect("Buka segmen akhir");
        let read_rec4 = reader.read_record_at(525).expect("Baca record ke-4");
        assert_eq!(read_rec4.sequence_number, 4);
        assert_eq!(read_rec4.amount.to_atomic(), 40_000);
    }

    let _ = fs::remove_file(&path);
}

#[test]
fn test_sub_header_truncation_recovery() {
    let test_sizes = [1usize, 10, 20, 41];

    for &sub_size in &test_sizes {
        let path = unique_test_path(&format!("sub_header_{sub_size}"));

        // Buat berkas terpotong dengan ukuran sub-header
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)
                .expect("Buat berkas sub-header");
            let truncated_bytes = vec![0xaa; sub_size];
            file.write_all(&truncated_bytes).expect("Tulis byte terpotong");
            file.flush().expect("Flush berkas terpotong");
        }

        // Jalankan recover_or_open dan pastikan menghasilkan CorruptedHeader tanpa panik
        let res = SegmentWriter::recover_or_open(&path);
        match res {
            Err(StorageError::CorruptedHeader { size }) => {
                assert_eq!(size, sub_size);
            }
            Err(err) => panic!("Harus mengembalikan CorruptedHeader, didapat: {:?}", err),
            Ok(_) => panic!("Harus mengembalikan CorruptedHeader, tetapi menghasilkan Ok"),
        }

        let _ = fs::remove_file(&path);
    }
}
