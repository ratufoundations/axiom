#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::time::Instant;

use axiom_primitives::{
    crypto::{AccountId, Hash, Signature},
    record::MutationRecord,
    value::AxmValue,
};
use axiom_storage::{
    error::StorageError,
    reader::SegmentReader,
    segment::{MAX_SEGMENT_SIZE, SEGMENT_FOOTER_SIZE, SEGMENT_HEADER_SIZE},
    writer::SegmentWriter,
};

const BATCH_SIZE: usize = 1_000;
const TOTAL_RECORDS_TARGET: u64 = 833_649;

fn create_dummy_record(seq: u64) -> MutationRecord {
    let mut sender_bytes = [0u8; 32];
    sender_bytes[..8].copy_from_slice(&1u64.to_le_bytes());

    let mut recipient_bytes = [0u8; 32];
    recipient_bytes[..8].copy_from_slice(&2u64.to_le_bytes());

    MutationRecord {
        epoch: 1,
        sequence_number: seq,
        record_kind: 1,
        sender: AccountId(sender_bytes),
        recipient: AccountId(recipient_bytes),
        amount: AxmValue(10_000_000_000), // 1.0 AXM
        signature: Signature([0xAA; 64]),
    }
}

#[test]
fn bench_sequential_128mb_lifecycle() {
    let temp_dir = tempfile::tempdir().expect("Gagal membuat direktori benchmark sementara");
    let file_path: PathBuf = temp_dir.path().join("epoch_1_seg_0.log");

    println!("\n========================================================");
    println!("=== AXIOM BENCHMARK: 128 MB SEQUENTIAL LOG LIFECYCLE ===");
    println!("========================================================");

    // -------------------------------------------------------------------------
    // FASE 1: Penulisan Sekuensial Linear hingga Batas 128 MB
    // -------------------------------------------------------------------------
    let mut writer =
        SegmentWriter::create(&file_path, 1, 1, 0).expect("Gagal inisialisasi writer");
    let mut latencies_micros: Vec<u64> =
        Vec::with_capacity((TOTAL_RECORDS_TARGET / BATCH_SIZE as u64 + 1) as usize);

    let start_write = Instant::now();
    let mut batch_start = Instant::now();
    let mut written_records: u64 = 0;

    for seq in 1..=TOTAL_RECORDS_TARGET {
        let record = create_dummy_record(seq);
        match writer.append_record(&record) {
            Ok(_) => {
                written_records += 1;
            }
            Err(StorageError::SegmentFull) => {
                println!(
                    "[BENCH] Segmen penuh tercapai pada mutasi ke-{}",
                    written_records
                );
                break;
            }
            Err(err) => panic!("Kegagalan tidak terduga saat append: {:?}", err),
        }

        if written_records.is_multiple_of(BATCH_SIZE as u64) {
            let elapsed_batch = batch_start.elapsed().as_micros() as u64;
            latencies_micros.push(elapsed_batch);
            batch_start = Instant::now();
        }
    }

    let write_duration_micros = start_write.elapsed().as_micros();
    assert_eq!(written_records, TOTAL_RECORDS_TARGET);

    // -------------------------------------------------------------------------
    // FASE 2: Penyegelan Catatan Kaki (Sealed Footer) & Flush Fisik
    // -------------------------------------------------------------------------
    let start_seal = Instant::now();
    let dummy_digest = Hash([0x77; 32]);
    let footer = writer
        .seal_segment(dummy_digest, 1728345600)
        .expect("Gagal menyegel segmen");
    let seal_duration_micros = start_seal.elapsed().as_micros();

    let final_file_size = std::fs::metadata(&file_path)
        .expect("Gagal membaca metadata")
        .len();
    assert!(final_file_size <= MAX_SEGMENT_SIZE);

    // -------------------------------------------------------------------------
    // FASE 3: Pemindaian Sekuensial Ulang (Cold Boot Replay Rebuild)
    // -------------------------------------------------------------------------
    let start_replay = Instant::now();
    let mut reader = SegmentReader::open(&file_path).expect("Gagal membuka reader segmen");
    let read_footer = reader
        .read_footer()
        .expect("Gagal membaca catatan kaki segmen");
    assert_eq!(read_footer.total_records, footer.total_records);

    let mut scanned_records: u64 = 0;
    let mut current_offset: u64 = SEGMENT_HEADER_SIZE as u64;
    let end_offset = final_file_size - (SEGMENT_FOOTER_SIZE as u64);

    while current_offset + 161 <= end_offset {
        let _ = reader
            .read_record_at(current_offset)
            .expect("Gagal membaca record mutasi");
        current_offset += 161;
        scanned_records += 1;
    }

    let replay_duration_micros = start_replay.elapsed().as_micros();
    assert_eq!(scanned_records, written_records);

    // -------------------------------------------------------------------------
    // KALKULASI LAPORAN (Murni Integer Arithmetic)
    // -------------------------------------------------------------------------
    latencies_micros.sort_unstable();
    let sample_count = latencies_micros.len();
    let p50_batch = latencies_micros[sample_count * 50 / 100];
    let p95_batch = latencies_micros[sample_count * 95 / 100];
    let p99_batch = latencies_micros[sample_count * 99 / 100];

    let write_throughput_mb = (final_file_size as u128 * 1_000_000)
        .checked_div(write_duration_micros * 1024 * 1024)
        .unwrap_or(0);

    let write_tps = (written_records as u128 * 1_000_000)
        .checked_div(write_duration_micros)
        .unwrap_or(0);

    let replay_throughput_mb = (final_file_size as u128 * 1_000_000)
        .checked_div(replay_duration_micros * 1024 * 1024)
        .unwrap_or(0);

    println!("\n--- HASIL METRIK PERFORMA FISIK DISK ---");
    println!("Total Rekam Mutasi  : {} record (161-byte)", written_records);
    println!(
        "Ukuran Berkas Akhir : {} byte (Batas: 128 MB)",
        final_file_size
    );
    println!(
        "Waktu Tulis Total   : {} ms",
        write_duration_micros / 1_000
    );
    println!("Throughput Tulis    : {} MB/detik", write_throughput_mb);
    println!("Throughput Ingest   : {} record/detik (TPS)", write_tps);
    println!(
        "Latensi Batch 1K    : p50 = {} µs | p95 = {} µs | p99 = {} µs",
        p50_batch, p95_batch, p99_batch
    );
    println!(
        "Durasi Seal Footer  : {} µs (Flush & lock)",
        seal_duration_micros
    );
    println!(
        "Waktu Replay Scan   : {} ms",
        replay_duration_micros / 1_000
    );
    println!(
        "Throughput Replay   : {} MB/detik",
        replay_throughput_mb
    );
    println!("========================================================\n");
}
