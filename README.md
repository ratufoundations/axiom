# Axiom (AXM)

Axiom adalah protokol buku besar terdistribusi yang dirancang dengan pendekatan *first principles*, memprioritaskan efisiensi penulisan data linier, evaluasi komputasi tertunda (*lazy evaluation*), serta keberlanjutan operasional jangka panjang di atas perangkat keras standar (termasuk media simpan mekanis).

---

## Status Siklus Pengembangan

> Bagian ini diperbarui secara berkala mengikuti perkembangan implementasi riil pada `task-register.md`.

* **Fase Saat Ini:** Fase 0 — *Scaffolding & Structural Blueprints*
* **Integritas Workspace:** Kerangka minimal independen (tanpa *cross-dependency* internal)
* **Status Kompilasi:** `cargo check` lolos pada tingkat stub

### Matriks Kesiapan Modul

| Modul Crate | Status Arsitektur | Cakupan Fungsi | Status Implementasi |
| :--- | :--- | :--- | :--- |
| `crates/axiom-primitives` | Draf Awal | Tipe dasar, serialisasi byte, kriptografi dasar | Tertunda (TR-01) |
| `crates/axiom-storage` | Draf Awal | *Append-only log engine*, pola tulis sekuensial | Belum Dimulai (TR-02) |
| `crates/axiom-index` | Draf Awal | Struktur indeks RAM, filter probabilitas | Belum Dimulai (TR-03) |
| `crates/axiom-archive` | Draf Awal | Segmentasi berkas, kompresi arsip dingin | Belum Dimulai (TR-04) |
| `crates/axiom-execution` | Draf Awal | Mesin validasi *witness* mandiri, evaluasi malas | Belum Dimulai (TR-05) |
| `crates/axiom-consensus` | Draf Awal | Aturan komitmen status jaringan | Belum Dimulai |
| `crates/axiom-network` | Draf Awal | Transmisi data peer-to-peer, *lazy sync* | Belum Dimulai (TR-06) |
| `bin/axiom-node` | Draf Awal | *Single-binary runner* modular monolitik | Belum Dimulai (TR-07) |

---

## Fondasi Arsitektur

* **Pola Tulis Linear (*Append-Only Log*):** Menghilangkan mutasi status acak di media simpan fisik. Aliran penulisan dirancang sekuensial agar ramah terhadap karakteristik mekanis hard disk.
* **Evaluasi Malas (*Lazy Evaluation*):** Validasi dan mutasi data hanya diproses ketika status tersebut diakses kembali.
* **Verifikasi Mandiri Klien (*Stateless Witness*):** Transaksi menyertakan bukti statusnya sendiri guna meringankan beban pencarian data oleh validator.
* **Modular Monolitik:** Isolasi ketat domain logika di tingkat crate, namun dikompilasi menjadi satu biner tunggal untuk meminimalkan *overhead* latensi internal.

---

## Struktur Repositori

```text
axiom/
├── AGENTS.md                    # Aturan kerja mutlak agen AI
├── Cargo.toml                   # Root workspace manifest
├── README.md                    # Ringkasan status dan orientasi proyek
├── task-register.md             # Pelacak pekerjaan teknis dan cetak biru
├── bin/
│   └── axiom-node/              # Biner eksekusi utama
└── crates/
    ├── axiom-primitives/        # Tipe data dasar & serialisasi biner
    ├── axiom-storage/           # Append-only storage engine
    ├── axiom-index/             # Indeks memori RAM
    ├── axiom-archive/           # Segmentasi & kompresi data arsip
    ├── axiom-execution/         # Evaluasi bukti malas (stateless witness)
    ├── axiom-consensus/         # Aturan komitmen konsensus
    └── axiom-network/           # Protokol transmisi data
```

---

## Pedoman Operasional Repositori

1. **Rujukan Tugas:** Seluruh pekerjaan teknis wajib merujuk secara eksplisit pada task yang terdaftar di `task-register.md`.
2. **Aturan Agen:** Kontributor agen terikat pada protokol kerja dan larangan dogma pada `AGENTS.md`.
3. **Pemeriksaan Kompilasi:**
```bash
cargo check --workspace
```
