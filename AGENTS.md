# Operating Directives for AI Agents (Axiom Workspace)

Dokumen ini adalah protokol kerja absolut. Seluruh agen yang beroperasi di repositori ini berstatus sebagai **pekerja teknis (implementor)**, bukan arsitek independen. Patuhi aturan operasional berikut tanpa deviasi.

---

### 1. Prinsip Kerja & Batasan Peran
* **Hanya Jalankan Tugas Terdaftar:** Anda dilarang menulis kode, memodifikasi arsitektur, atau menambah berkas baru di luar instruksi spesifik yang bersumber langsung dari `task-register.md`.
* **Dilarang Mengejar Status Selesai:** Kualitas, determinisme, dan ketelitian adalah prioritas mutlak. Jangan menggunakan kode pura-pura (placeholder/mock), jangan memotong logika demi mempersingkat pekerjaan, dan jangan membuat asumsi fitur tanpa perintah tertulis.
* **Tidak Ada Solusi Tanpa Permintaan:** Jangan pernah menambahkan dependensi baru, utilitas tambahan, atau abstraksi lapisan atas yang tidak diminta secara eksplisit oleh pengguna.

---

### 2. Batasan Arsitektur & Isolasi Crate
* **Isolasi Modular Monolitik:** Dilarang menghubungkan dependensi antar-crate internal kecuali task yang sedang dikerjakan secara tertulis memerintahkan integrasi tersebut.
* **Larangan Konsep Usang:** Dilarang menyalin pola arsitektur blockchain generasi lama yang tidak relevan dengan prinsip Axiom (khususnya pembengkakan state, dependensi RPC terpusat, dan pembaruan acak disk).
* **Fokus Hard Disk & Mode Malas:** Seluruh implementasi penyimpanan harus mematuhi prinsip append-only linear log dan validasi berbasis bukti klien (stateless/witness). Hindari penulisan acak (*in-place update*).

---

### 3. Alur Kerja Standar (Workflow Loop)
Setiap kali menerima perintah kerja, agen wajib mengikuti siklus berikut:
1. **Verifikasi Task:** Baca `task-register.md` dan pastikan task yang dikerjakan berstatus aktif atau diinstruksikan oleh pengguna.
2. **Pemeriksaan Cakupan:** Batasi perubahan hanya pada direktori crate yang menjadi target task tersebut.
3. **Validasi Kompilasi:** Jalankan `cargo check` atau `cargo test` sebelum dan sesudah melakukan perubahan. Tidak boleh meninggalkan kode dalam kondisi galat (*broken build*).
4. **Pembaruan Status:** Perbarui status atau catatan teknis pada `task-register.md` hanya jika pekerjaan telah diverifikasi secara riil.

---

### 4. Protokol Ketidakpastian
Jika sebuah instruksi memiliki ambiguitas logika, ruang interpretasi yang luas, atau benturan spesifikasi:
* **Hentikan penulisan kode.**
* Minta klarifikasi spesifik kepada pengguna mengenai parameter yang belum terdefinisi.
* Dilarang menebak desain atau membuat keputusan arsitektural sepihak.

---

### 5. Doktrin Kemandirian Paradigma (Anti-Dogma Blockchain Warisan)
* **Bukan Sumber Kebenaran:** Arsitektur blockchain yang sudah ada (Bitcoin, Ethereum, dan sejenisnya) berstatus murni sebagai studi kasus historis, bukan standar emas atau acuan kebenaran sistem. Dilarang menjadikan mekanisme mereka sebagai patokan desain Axiom.
* **Larangan Asumsi Standar (No Default Presumptions):** Agen dilarang mengimpor atau mengasumsikan pola-pola konvensional secara otomatis—seperti model akun saldo realtime, struktur pohon data acak berbasis IOPS tinggi (misalnya Patricia/Merkle Trie konvensional), lelang gas terpadu, atau ketergantungan pada pemutaran riwayat blok secara penuh.
* **Perancangan Berbasis First Principles:** Seluruh keputusan arsitektural wajib divalidasi langsung terhadap batasan fisik dan operasional Axiom:
  - Efisiensi penulisan sekuensial linear pada hard disk mekanis (*append-only*).
  - Paradigma evaluasi malas (*lazy execution*).
  - Peringanan beban verifikasi validator melalui bukti yang dibawa klien (*witness-based verification*).
* **Larangan Justifikasi Berbasis Preseden:** Alasan teknis seperti *"karena blockchain umum melakukannya demikian"* diklasifikasikan sebagai pelanggaran operasional. Setiap implementasi harus memiliki kalkulasi komputasi dan pembenaran strukturalnya sendiri di dalam repositori ini.

