#![forbid(unsafe_code)]

//! Suite Pengujian & Benchmark Pelepasan Produksi (PERF-RELEASE-01)
//!
//! Validasi kinerja saturasi perangkat keras, throughput transaksi puncak & berkelanjutan (TPS),
//! serta profil latensi integer tanpa floating-point (p50, p95, p99) pada profil rilis teroptimasi.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};
use ratu_aurion_engine::pipeline::PipelineCoordinator;
use ratu_aurion_index::entry::AccountLocation;
use ratu_aurion_index::keydir::Keydir;
use ratu_aurion_primitives::{
    crypto::{AccountId, Signature},
    record::{MutationRecord, RECORD_KIND_TRANSFER},
    value::AurValue,
};
use ratu_aurion_storage::reader::SegmentReader;
use ratu_aurion_storage::writer::SegmentWriter;

/// Struktur pelacak distribusi latensi berbasis integer mikrodetik murni (Zero-Float).
#[derive(Debug, Clone, Default)]
pub struct LatencyHistogram {
    pub samples_micros: Vec<u64>,
}

impl LatencyHistogram {
    /// Membuat instance baru LatencyHistogram.
    pub fn new() -> Self {
        Self {
            samples_micros: Vec::new(),
        }
    }

    /// Membuat instance baru dengan alokasi kapasitas awal.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            samples_micros: Vec::with_capacity(capacity),
        }
    }

    /// Mencatat satu sampel latensi dalam mikrodetik.
    pub fn record(&mut self, micros: u64) {
        self.samples_micros.push(micros);
    }

    /// Menghitung persentil latensi integer: (min, p50, p95, p99, max).
    /// Menggunakan formula indeks urutan integer: k = (P * N) / 100.
    pub fn compute_percentiles(&mut self) -> (u64, u64, u64, u64, u64) {
        if self.samples_micros.is_empty() {
            return (0, 0, 0, 0, 0);
        }
        self.samples_micros.sort_unstable();
        let len = self.samples_micros.len();
        let min = self.samples_micros[0];
        let max = self.samples_micros[len.saturating_sub(1)];

        let p50_idx = ((50 * len) / 100).min(len.saturating_sub(1));
        let p95_idx = ((95 * len) / 100).min(len.saturating_sub(1));
        let p99_idx = ((99 * len) / 100).min(len.saturating_sub(1));

        let p50 = self.samples_micros[p50_idx];
        let p95 = self.samples_micros[p95_idx];
        let p99 = self.samples_micros[p99_idx];

        (min, p50, p95, p99, max)
    }
}

fn unique_test_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ratu_aurion_perf_rel_{label}_{nanos}"));
    fs::create_dir_all(&path).expect("Create test dir");
    path
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
    amount_atomic: u128,
) -> MutationRecord {
    let mut record = MutationRecord {
        epoch,
        sequence_number: seq,
        record_kind: RECORD_KIND_TRANSFER,
        sender,
        recipient,
        amount: AurValue::from_atomic(amount_atomic),
        signature: Signature::ZERO,
    };
    let payload = record.signing_payload();
    let dalek_sig = signing_key.sign(&payload);
    record.signature = Signature::new(dalek_sig.to_bytes());
    record
}

#[test]
fn test_release_pipeline_saturation() {
    let dir = unique_test_dir("pipeline_saturation");
    let seg_path = dir.join("epoch_1_seg_0.log");
    let cold_path = dir.join("cold_state.idx");

    let writer = SegmentWriter::create(&seg_path, 1, 1, 0).expect("Create writer");
    let mut keydir =
        Keydir::with_budget(&cold_path, 1_000).expect("Create keydir with 1,000 hot budget");

    let initial_balance = AurValue::from_whole_aur(10_000).expect("10,000 AUR");
    let transfer_amount = AurValue::from_whole_aur(1).expect("1 AUR").to_atomic();

    // 1. Seed 50 client accounts dengan 10,000 AUR per akun (Total: 500,000 AUR)
    let mut clients = Vec::with_capacity(50);
    for i in 0..50 {
        let (signing_key, account) = create_keypair(0x10 + i as u8);
        keydir.seed_account(
            account,
            initial_balance,
            AccountLocation::new(1, 0, 0, 0),
        );
        clients.push((signing_key, account));
    }

    let expected_total_supply = AurValue::from_whole_aur(500_000).expect("500,000 AUR");
    assert_eq!(keydir.total_supply(), expected_total_supply);

    // 2. Spawn PipelineCoordinator dengan 16 verifier threads, 1 sequencer, dan 1 storage writer
    let coordinator = Arc::new(
        PipelineCoordinator::spawn(writer, keydir, 16)
            .expect("Spawn pipeline coordinator with 16 verifiers"),
    );

    let start_wall_clock = Instant::now();

    // 3. Spawn 16 concurrent client worker threads, masing-masing mengirim 625 transaksi (Total: 10,000 tx)
    let num_workers = 16;
    let tx_per_worker = 625;
    let total_expected_tx = num_workers * tx_per_worker; // 10,000

    let mut worker_handles = Vec::with_capacity(num_workers);
    for thread_idx in 0..num_workers {
        let coord = Arc::clone(&coordinator);
        let sender_key = clients[thread_idx].0.clone();
        let sender = clients[thread_idx].1;
        let recipient = clients[(thread_idx + 16) % 50].1;

        let handle = std::thread::spawn(move || {
            let mut latencies = Vec::with_capacity(tx_per_worker);
            for seq in 1..=tx_per_worker as u64 {
                let tx = sign_mutation(
                    &sender_key,
                    1,
                    seq,
                    sender,
                    recipient,
                    transfer_amount,
                );
                let tx_start = Instant::now();
                let receipt = coord.submit(tx).expect("Submit valid tx under release saturation");
                let tx_micros = tx_start.elapsed().as_micros() as u64;
                assert_eq!(receipt.sequence_number, seq, "Sequence must match strictly");
                latencies.push(tx_micros);
            }
            latencies
        });
        worker_handles.push(handle);
    }

    let mut histogram = LatencyHistogram::with_capacity(total_expected_tx);
    for handle in worker_handles {
        let latencies = handle.join().expect("Worker thread join");
        for lat in latencies {
            histogram.record(lat);
        }
    }

    let elapsed_duration = start_wall_clock.elapsed();
    let elapsed_micros = elapsed_duration.as_micros().max(1);
    let elapsed_ms = elapsed_duration.as_millis().max(1);
    let tps_integer = (total_expected_tx as u128 * 1_000_000) / elapsed_micros;

    // 4. Assert seluruh 10,000 transaksi berhasil dikomit
    assert_eq!(
        histogram.samples_micros.len(),
        total_expected_tx,
        "All 10,000 transactions must be committed"
    );

    // 5. Assert pasokan koin global tetap terkonservasi murni pada 500,000 AUR
    let mut query_sum_atomic: u128 = 0;
    for (_, account) in &clients {
        let bal = coordinator.query_balance(account).expect("Query account balance");
        query_sum_atomic = query_sum_atomic
            .checked_add(bal.to_atomic())
            .expect("Supply overflow");
    }
    assert_eq!(
        AurValue::from_atomic(query_sum_atomic),
        expected_total_supply,
        "Total monetary supply must be strictly conserved at 500,000 AUR"
    );

    // 6. Hitung dan tampilkan profil latensi integer (min, p50, p95, p99, max)
    let (min_lat, p50_lat, p95_lat, p99_lat, max_lat) = histogram.compute_percentiles();

    println!("\n================================================================================");
    println!("=== RATU AURION PRODUCTION RELEASE PIPELINE BENCHMARK (PERF-RELEASE-01) ===");
    println!("================================================================================");
    println!("Total Mutasi Transaksi  : {} tx (161-byte per record)", total_expected_tx);
    println!("Konfigurasi Pipeline    : 16 verifier threads, 1 sequencer, 1 storage writer");
    println!("Kapasitas RAM Keydir    : 1,000 hot accounts (didukung ColdStore)");
    println!("Durasi Total Eksekusi   : {} ms ({} us)", elapsed_ms, elapsed_micros);
    println!("Throughput Terukur      : {} TPS (pure integer transactions-per-second)", tps_integer);
    println!("Distribusi Latensi (us) : min={} us | p50={} us | p95={} us | p99={} us | max={} us",
        min_lat, p50_lat, p95_lat, p99_lat, max_lat);
    println!("Konservasi Pasokan Koin : {} AUR (Invarian Terverifikasi)", expected_total_supply);
    println!("================================================================================\n");

    // 7. Verifikasi integritas segmen disk dan footer
    let coordinator_instance = Arc::try_unwrap(coordinator)
        .map_err(|_| "Arc still held")
        .expect("Coordinator unwrapped successfully");
    coordinator_instance.shutdown().expect("Coordinator shutdown cleanly");

    let mut reader = SegmentReader::open(&seg_path).expect("Open sealed segment");
    let footer = reader.read_footer().expect("Read footer");
    assert_eq!(footer.total_records, total_expected_tx as u64);

    let _ = fs::remove_dir_all(&dir);
}
