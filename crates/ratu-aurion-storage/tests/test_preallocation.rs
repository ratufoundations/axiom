#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use ratu_aurion_primitives::crypto::{AccountId, Hash, Signature};
use ratu_aurion_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER, RECORD_SIZE};
use ratu_aurion_primitives::value::AurValue;
use ratu_aurion_storage::{
    SegmentReader, SegmentWriter, MAX_SEGMENT_SIZE, SEGMENT_FOOTER_SIZE, SEGMENT_HEADER_SIZE,
};

fn unique_test_path(label: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ratu_aurion_prealloc_test_{label}_{nanos}.log"))
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
fn test_preallocated_segment_creation_and_size() {
    let path = unique_test_path("creation_size");

    // 1. Buat segmen baru dengan SegmentWriter::create
    let mut writer = SegmentWriter::create(&path, 1, 1, 0).expect("Gagal membuat segmen");

    // 2. Pastikan ukuran fisik di disk langsung berukuran 128 MB (134.217.728 byte)
    let meta_len = fs::metadata(&path).expect("Metadata berkas").len();
    assert_eq!(meta_len, MAX_SEGMENT_SIZE);
    assert_eq!(meta_len, 134_217_728);

    // 3. Pastikan writer.current_offset() berada tepat di offset 42 (setelah SegmentHeader)
    assert_eq!(writer.current_offset(), SEGMENT_HEADER_SIZE as u64);
    assert_eq!(writer.current_offset(), 42);

    // 4. Tambahkan 5 record valid
    for seq in 1..=5 {
        let rec = sample_record(seq, seq as u128 * 1_000);
        let offset = writer.append_record(&rec).expect("Append record");
        let expected_offset = SEGMENT_HEADER_SIZE as u64 + ((seq - 1) * RECORD_SIZE as u64);
        assert_eq!(offset, expected_offset);
    }

    // current_offset harus tepat 42 + 5 * 161 = 847
    let expected_offset = 42 + (5 * RECORD_SIZE as u64);
    assert_eq!(expected_offset, 847);
    assert_eq!(writer.current_offset(), expected_offset);

    // 5. Ukuran fisik berkas di disk tetap persis 128 MB tanpa pertumbuhan metadata disk
    let meta_len_after_appends = fs::metadata(&path).expect("Metadata setelah append").len();
    assert_eq!(meta_len_after_appends, MAX_SEGMENT_SIZE);
    assert_eq!(meta_len_after_appends, 134_217_728);

    let _ = fs::remove_file(&path);
}

#[test]
fn test_preallocated_recovery_with_binary_zero_scan() {
    let path = unique_test_path("binary_zero_scan");

    // Inisialisasi segmen dan tulis 5 record
    {
        let mut writer = SegmentWriter::create(&path, 1, 1, 0).expect("Gagal membuat segmen");
        for seq in 1..=5 {
            let rec = sample_record(seq, seq as u128 * 10_000);
            writer.append_record(&rec).expect("Append record");
        }
        assert_eq!(writer.current_offset(), 847);
        assert_eq!(writer.total_records(), 5);
        // Writer di-drop tanpa disegel (unsealed)
    }

    // Buka kembali segmen unsealed dengan SegmentWriter::recover_or_open
    let mut recovered =
        SegmentWriter::recover_or_open(&path).expect("Pemulihan segmen pra-alokasi gagal");

    // Pastikan pemulihan menemukan batas data valid dengan tepat
    assert_eq!(recovered.current_offset(), 847);
    assert_eq!(recovered.total_records(), 5);

    // Tambahkan record ke-6 ke writer yang dipulihkan
    let rec6 = sample_record(6, 60_000);
    let offset6 = recovered
        .append_record(&rec6)
        .expect("Append record 6 pada writer pulih");
    assert_eq!(offset6, 847);
    assert_eq!(recovered.current_offset(), 847 + RECORD_SIZE as u64);
    assert_eq!(recovered.current_offset(), 1008);
    assert_eq!(recovered.total_records(), 6);

    let _ = fs::remove_file(&path);
}

#[test]
fn test_preallocated_torn_write_recovery() {
    let path = unique_test_path("torn_write_recovery");

    // Inisialisasi segmen dengan 5 record valid
    {
        let mut writer = SegmentWriter::create(&path, 1, 1, 0).expect("Gagal membuat segmen");
        for seq in 1..=5 {
            let rec = sample_record(seq, seq as u128 * 1_000);
            writer.append_record(&rec).expect("Append record");
        }
        assert_eq!(writer.current_offset(), 847);
    }

    // Simulasi crash pada slot ke-6 (offset 847): tulis 45 byte parsial, sisa 116 byte adalah 0x00
    let slot6_offset = 847u64;
    {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("Buka berkas untuk injeksi torn-write");
        file.seek(SeekFrom::Start(slot6_offset))
            .expect("Seek ke slot 6");
        let partial_bytes = [0xbb; 45];
        file.write_all(&partial_bytes)
            .expect("Tulis byte parsial slot 6");
        file.flush().expect("Flush byte parsial");
    }

    // recover_or_open harus mendeteksi korupsi pada slot 6, menolkan slot tersebut, dan mereset offset ke 847
    let mut recovered =
        SegmentWriter::recover_or_open(&path).expect("recover_or_open harus berhasil tanpa panik");

    assert_eq!(recovered.current_offset(), 847);
    assert_eq!(recovered.total_records(), 5);

    // Verifikasi bahwa 161 byte pada slot 6 (847..1008) telah dinolkan (zeroed out) di disk
    {
        let mut file = OpenOptions::new()
            .read(true)
            .open(&path)
            .expect("Buka berkas untuk verifikasi nol");
        file.seek(SeekFrom::Start(slot6_offset))
            .expect("Seek ke slot 6");
        let mut slot_buf = [0u8; RECORD_SIZE];
        file.read_exact(&mut slot_buf).expect("Baca slot 6");
        assert!(
            slot_buf.iter().all(|&b| b == 0),
            "Slot 6 harus bersih ternolkan setelah pemulihan"
        );
    }

    // Tulis record ke-6 baru yang valid
    let rec6 = sample_record(6, 60_000);
    let offset6 = recovered
        .append_record(&rec6)
        .expect("Append record 6 valid");
    assert_eq!(offset6, 847);
    assert_eq!(recovered.current_offset(), 1008);
    assert_eq!(recovered.total_records(), 6);

    let _ = fs::remove_file(&path);
}

#[test]
fn test_seal_segment_reclaims_unused_space() {
    let path = unique_test_path("seal_reclaims_space");

    // Inisialisasi dan tulis 5 record
    let mut writer = SegmentWriter::create(&path, 1, 1, 0).expect("Gagal membuat segmen");
    for seq in 1..=5 {
        let rec = sample_record(seq, seq as u128 * 5_000);
        writer.append_record(&rec).expect("Append record");
    }
    assert_eq!(writer.current_offset(), 847);

    // Sebelum disegel: ukuran berkas adalah 128 MB
    let unsealed_size = fs::metadata(&path).expect("Metadata unsealed").len();
    assert_eq!(unsealed_size, MAX_SEGMENT_SIZE);

    // Segel segmen
    let digest = Hash::new([0x88; 32]);
    let sealed_timestamp = 1_728_100_000u64;
    let footer = writer
        .seal_segment(digest, sealed_timestamp)
        .expect("Penyegelan segmen gagal");

    assert_eq!(footer.total_records, 5);
    assert!(writer.is_sealed());

    // Ukuran berkas akhir harus dipotong ke 42 + 5 * 161 + 88 = 935 byte
    let expected_sealed_size =
        SEGMENT_HEADER_SIZE as u64 + (5 * RECORD_SIZE as u64) + SEGMENT_FOOTER_SIZE as u64;
    assert_eq!(expected_sealed_size, 935);

    let final_size = fs::metadata(&path).expect("Metadata sealed").len();
    assert_eq!(final_size, expected_sealed_size);
    assert_eq!(final_size, 935);

    // Verifikasi keterbacaan data melalui reader
    let mut reader = SegmentReader::open(&path).expect("Buka segmen bersegel");
    let read_footer = reader.read_footer().expect("Baca footer bersegel");
    assert_eq!(read_footer.total_records, 5);
    assert_eq!(read_footer.first_sequence, 1);
    assert_eq!(read_footer.last_sequence, 5);

    let _ = fs::remove_file(&path);
}
