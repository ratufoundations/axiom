# RFC-0001: Spesifikasi Tata Letak Biner Deterministik Ratu Aurion v1.0

* **Status:** Standar / Final
* **Versi:** 1.0.0
* **Tanggal Berlaku:** 8 Oktober 2026
* **Penulis:** Inti Protokol Ratu Aurion
* **Endinansi:** Little-Endian (LE) Murni
* **Invarian Integritas:** Bebas Tipe Floating-Point, Zero Unsafe, Padding Eksplisit

---

## 1. Konvensi & Prinsip Serialisasi

Protokol Ratu Aurion menerapkan aturan serialisasi biner deterministik tetap (*fixed-width binary layout*). Tidak ada kompresi variabel dinamis (seperti Varint atau LEB128) pada struktur data inti untuk memastikan offset memori dan disk dapat dihitung secara langsung tanpa pemindaian parsial:

1. **Urutan Byte:** Seluruh tipe integer numerik (`u16`, `u32`, `u64`, `u128`) diserialisasikan secara Little-Endian.
2. **Ketiadaan Alignment Padding Tak Terdefinisi:** Semua struktur biner tidak bergantung pada *compiler struct memory padding*. Setiap byte cadangan dideklarasikan eksplisit sebagai `reserved` bernilai `0x00`.
3. **Representasi Moneter:** Nilai `AurValue` disimpan sebagai `u128` (16 byte LE) yang merepresentasikan unit terkecil integer dengan faktor pengali desimal 10^10 (1 AUR = 10.000.000.000 unit atomik).

---

## 2. Spesifikasi Bingkai Wire Jaringan (FrameHeader)

Setiap transmisi soket TCP pada lapisan jaringan (`ratu-aurion-network`) dibungkus oleh bingkai berukuran tetap 42 byte, diikuti oleh payload dinamis berukuran N byte.

### 2.1 Tata Letak Struktur Biner (42 Byte)

```text
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       Magic: b"AUR\x01"                       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|         Version (u16)         |       Payload Length (u32)    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|       (Payload Length)        |                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+                               +
|                                                               |
+                                                               +
|                    BLAKE3 Checksum (32 Byte)                  |
+                                                               +
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### 2.2 Rincian Bidang

| Offset Byte | Panjang | Bidang | Tipe | Deskripsi & Nilai Sah |
| --- | --- | --- | --- | --- |
| `0x00 .. 0x03` | 4 Byte | `magic` | `[u8; 4]` | Identifier protokol: `0x41, 0x55, 0x52, 0x01` (`b"AUR\x01"`). |
| `0x04 .. 0x05` | 2 Byte | `version` | `u16` LE | Versi protokol framing wire: `1` (`0x0001`). |
| `0x06 .. 0x09` | 4 Byte | `payload_len` | `u32` LE | Ukuran panjang byte payload setelah header (maksimal 64 MB). |
| `0x0A .. 0x29` | 32 Byte | `checksum` | `[u8; 32]` | Intisari BLAKE3 murni dari seluruh byte payload. |

---

## 3. Spesifikasi Rekam Mutasi Transaksi (MutationRecord)

Rekam mutasi transaksi pada `ratu-aurion-primitives` dan penulisan disk `ratu-aurion-storage` memiliki ukuran kanonikal tepat 161 byte.

### 3.1 Tata Letak Struktur Biner (161 Byte)

```text
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                          Epoch (u64)                          |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                     Sequence Number (u64)                     |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   Kind (u8)   |                                               |
+-+-+-+-+-+-+-+-+                                               +
|                                                               |
+                     Sender AccountId (32 Byte)                +
|                                                               |
+               +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|               |                                               |
+-+-+-+-+-+-+-+-+                                               +
|                                                               |
+                   Recipient AccountId (32 Byte)               +
|                                                               |
+                               +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                               |                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+                               +
|                        Amount (u128 LE)                       |
+                               +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                               |                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+                               +
|                                                               |
+                                                               +
|                 Ed25519 Signature (64 Byte)                   |
+                                                               +
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### 3.2 Rincian Bidang

| Offset Byte | Panjang | Bidang | Tipe | Deskripsi |
| --- | --- | --- | --- | --- |
| `0x00 .. 0x07` | 8 Byte | `epoch` | `u64` LE | Siklus bulan kalender deterministik transaksi. |
| `0x08 .. 0x0F` | 8 Byte | `sequence_number` | `u64` LE | Nonce akun pengirim yang harus meningkat secara ketat. |
| `0x10` | 1 Byte | `record_kind` | `u8` | Tipe mutasi (`0x01`: Transfer Standar, `0x02`: Genesis/Mint). |
| `0x11 .. 0x30` | 32 Byte | `sender` | `[u8; 32]` | Kunci publik Ed25519 pengirim. |
| `0x31 .. 0x50` | 32 Byte | `recipient` | `[u8; 32]` | Kunci publik Ed25519 penerima. |
| `0x51 .. 0x60` | 16 Byte | `amount` | `u128` LE | Besaran nilai mutasi dalam satuan atomik (10^-10 AUR). |
| `0x61 .. 0xA0` | 64 Byte | `signature` | `[u8; 64]` | Tanda tangan digital Ed25519 atas payload 97 byte. |

---

## 4. Spesifikasi Segmen Log Fisik Disk (ratu-aurion-storage)

Setiap segmen log aktif memiliki batas ukuran maksimum 128 MB (134.217.728 byte), diawali oleh 42-byte Header, aliran linier mutasi 161 byte, dan ditutup oleh 88-byte Catatan Kaki (Sealed Footer).

### 4.1 Header Segmen (SegmentHeader - 42 Byte)

| Offset Byte | Panjang | Bidang | Tipe | Deskripsi & Nilai Kanonikal |
| --- | --- | --- | --- | --- |
| `0x00 .. 0x03` | 4 Byte | `magic` | `[u8; 4]` | Identifier segmen: `0x52, 0x41, 0x55, 0x52` (`b"AURS"`). |
| `0x04 .. 0x05` | 2 Byte | `version` | `u16` LE | Versi penyimpanan disk: `0x0001`. |
| `0x06 .. 0x0D` | 8 Byte | `epoch` | `u64` LE | Siklus bulan segmen log. |
| `0x0E .. 0x11` | 4 Byte | `segment_index` | `u32` LE | Indeks urutan segmen dalam bulan berjalan. |
| `0x12 .. 0x29` | 24 Byte | `reserved` | `[u8; 24]` | Byte cadangan ekspansi, wajib bernilai `0x00`. |

### 4.2 Catatan Kaki Segmen Bersegel (SegmentFooter - 88 Byte)

```text
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      Total Records (u64)                      |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                          Epoch (u64)                          |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                     First Sequence (u64)                      |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      Last Sequence (u64)                      |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
+                                                               +
|                   State Digest Hash (32 Byte)                 |
+                                                               +
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       Sealed At (u64)                         |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        Reserved (8 Byte)                      |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                   Seal Magic: b"AUREND\x01\x00"               |
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

| Offset Byte Relatif | Panjang | Bidang | Tipe | Deskripsi |
| --- | --- | --- | --- | --- |
| `0x00 .. 0x07` | 8 Byte | `total_records` | `u64` LE | Jumlah mutasi 161-byte dalam segmen. |
| `0x08 .. 0x0F` | 8 Byte | `epoch` | `u64` LE | Nomor epoch bulan berjalan. |
| `0x10 .. 0x17` | 8 Byte | `first_sequence` | `u64` LE | Nonce mutasi pertama di dalam segmen. |
| `0x18 .. 0x1F` | 8 Byte | `last_sequence` | `u64` LE | Nonce mutasi terakhir di dalam segmen. |
| `0x20 .. 0x3F` | 32 Byte | `state_digest` | `[u8; 32]` | Intisari BLAKE3 kumulatif dari isi segmen. |
| `0x40 .. 0x47` | 8 Byte | `sealed_at` | `u64` LE | Waktu stempel UNIX detik saat penyegelan disk. |
| `0x48 .. 0x4F` | 8 Byte | `reserved` | `[u8; 8]` | Byte cadangan, wajib bernilai `0x00`. |
| `0x50 .. 0x57` | 8 Byte | `seal_magic` | `[u8; 8]` | Penanda akhir segmen: `b"AUREND\x01\x00"`. |

---

## 5. Ringkasan Ukuran Struktur Biner

| Struktur Data | Ukuran Tetap | Identifikasi Magic | Ranah Implementasi |
| --- | --- | --- | --- |
| **`FrameHeader`** | 42 Byte | `b"AUR\x01"` (4 Byte) | Wire framing jaringan (`ratu-aurion-network`) |
| **`MutationRecord`** | 161 Byte | Tidak ada (Payload murni) | Inti status & audit disk (`ratu-aurion-primitives`) |
| **`SegmentHeader`** | 42 Byte | `b"AURS"` (4 Byte) | Metadata awal berkas disk (`ratu-aurion-storage`) |
| **`SegmentFooter`** | 88 Byte | `b"AUREND\x01\x00"` (8 Byte) | Catatan kaki pengunci disk (`ratu-aurion-storage`) |
| **`ArchiveManifest`** | 80 Byte | `b"AXAM"` (4 Byte) | Metadata arsip bulanan ZIP (`ratu-aurion-archive`) |
