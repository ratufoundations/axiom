# Task Register - Axiom Blockchain

Dokumen cetak biru dan pelacak tugas rekayasa sistem untuk proyek blockchain modular Axiom.

| ID Task | Domain Sistem | Deskripsi Spesifikasi Teknis | Ketergantungan | Status |
| :--- | :--- | :--- | :--- | :--- |
| TR-01 | Primitives | Definisi tipe data dasar, representasi biner, address, hash primitives, serta abstraksi data serialisasi standar. | None | Tervalidasi |
| TR-02 | Storage | Implementasi append-only log engine untuk persistensi data blok, transaksi, write-ahead logging (WAL), dan flush scheduler. | None | Tervalidasi |
| TR-03 | Index | Desain dan implementasi struktur data indeks berbasis RAM untuk pencarian cepat status/state dan pemetaan kunci-nilai performa tinggi. | None | Tervalidasi |
| TR-04 | Archive | Mesin kompresi dan segmentasi arsip data historis, cold storage management, serta strategi pruning data blok. | None | Tervalidasi |
| TR-05 | Engine | Mesin orkestrasi dan validasi transaksi (kriptografi, storage, in-memory index, dan pengarsipan). | None | Tervalidasi |
| TR-06 | Consensus | Penegakan aturan komitmen jaringan, validasi finalitas blok, algoritma konsensus modular, dan integritas state commitment. | None | Tervalidasi |
| TR-07 | Network | Protokol komunikasi data P2P, serialization framing, transport abstraction layer, penanganan koneksi peer, dan propagasi pesan. | None | Tervalidasi |
| TR-08 | Node | Integrasi runtime simpul biner utama (axiom-node), loop server TCP P2P, pipeline handler, dan graceful shutdown. | TR-01 - TR-07 | Dalam Review |
