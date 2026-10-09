# Ratu Aurion Protocol (AUR)

Ratu Aurion Protocol adalah protokol buku besar terdistribusi yang dirancang dengan pendekatan *first principles*, memprioritaskan efisiensi penulisan data linier, evaluasi komputasi tertunda (*lazy evaluation*), serta keberlanjutan operasional jangka panjang di atas perangkat keras standar (termasuk media simpan mekanis).

---

## Status Siklus Pengembangan

> Bagian ini diperbarui secara berkala mengikuti perkembangan implementasi riil pada `task-register.md`.

* **Fase Saat Ini:** Fase 1 — *Core Subsystems Hardening & Protocol Optimization*
* **Integritas Workspace:** Kerangka modular independen (zero-panic, purely integer-based, `#![forbid(unsafe_code)]`)
* **Status Kompilasi:** `cargo check` & `cargo clippy` lolos tanpa peringatan

### Matriks Kesiapan Modul

| Modul Crate | Status Arsitektur | Cakupan Fungsi | Status Implementasi |
| :--- | :--- | :--- | :--- |
| `crates/ratu-aurion-primitives` | Produksi | Tipe dasar, serialisasi byte, kriptografi Ed25519/BLAKE3, token AUR | Tervalidasi (TR-01) |
| `crates/ratu-aurion-storage` | Produksi | *Append-only log engine*, pra-alokasi 128 MB, binary zero scan, durability tuning | Tervalidasi (TR-02) |
| `crates/ratu-aurion-index` | Produksi | Indeks RAM Bitcask 256-bucket, compact 80B entry, snapshot 96B, LRU disk paging | Tervalidasi (TR-03) |
| `crates/ratu-aurion-archive` | Produksi | Segmentasi berkas, kompresi arsip dingin zip deflate | Tervalidasi (TR-04) |
| `crates/ratu-aurion-engine` | Produksi | Pipeline eksekusi 3-tahap tanpa lock contention | Tervalidasi (TR-05) |
| `crates/ratu-aurion-consensus` | Produksi | Deteksi ekuivokasi & slashing 100%, pacemaker rotasi pemimpin deterministik | Tervalidasi (TR-06) |
| `crates/ratu-aurion-network` | Produksi | TCP framed wire protocol, token bucket rate limiter, bounded backpressure | Tervalidasi (TR-07) |
| `bin/ratu-aurion-node` | Produksi | Biner simpul p2p terpadu, IPC snapshot telemetri | Tervalidasi (TR-08) |
| `bin/ratu-aurion-cli` | Produksi | Antarmuka baris perintah, keygen, transfer desimal-to-atomik | Tervalidasi (TR-09) |

---

## Fondasi Arsitektur

* **Pola Tulis Linear (*Append-Only Log*):** Menghilangkan mutasi status acak di media simpan fisik. Aliran penulisan dirancang sekuensial agar ramah terhadap karakteristik mekanis hard disk.
* **Evaluasi Malas (*Lazy Evaluation*):** Validasi dan mutasi data hanya diproses ketika status tersebut diakses kembali.
* **Verifikasi Mandiri Klien (*Stateless Witness*):** Transaksi menyertakan bukti statusnya sendiri guna meringankan beban pencarian data oleh validator.
* **Modular Monolitik:** Isolasi ketat domain logika di tingkat crate, namun dikompilasi menjadi satu biner tunggal untuk meminimalkan *overhead* latensi internal.

---

## Struktur Repositori

```text
ratu-aurion/
├── AGENTS.md                    # Aturan kerja mutlak agen AI
├── Cargo.toml                   # Root workspace manifest
├── README.md                    # Ringkasan status dan orientasi proyek
├── task-register.md             # Pelacak pekerjaan teknis dan cetak biru
├── bin/
│   ├── ratu-aurion-node/        # Biner eksekusi utama node
│   └── ratu-aurion-cli/         # Biner antarmuka CLI dompet
└── crates/
    ├── ratu-aurion-primitives/  # Tipe data dasar & serialisasi biner
    ├── ratu-aurion-storage/     # Append-only storage engine
    ├── ratu-aurion-index/       # Indeks memori RAM Bitcask
    ├── ratu-aurion-archive/     # Segmentasi & kompresi data arsip
    ├── ratu-aurion-engine/      # Pipeline transaksi 3-tahap
    ├── ratu-aurion-consensus/   # Aturan komitmen konsensus & pacemaker
    └── ratu-aurion-network/     # Protokol transmisi data P2P
```

---

## Pedoman Operasional Repositori

1. **Rujukan Tugas:** Seluruh pekerjaan teknis wajib merujuk secara eksplisit pada task yang terdaftar di `task-register.md`.
2. **Aturan Agen:** Kontributor agen terikat pada protokol kerja dan larangan dogma pada `AGENTS.md`.
3. **Pemeriksaan Kompilasi:**
```bash
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python tools/guards.py
```
