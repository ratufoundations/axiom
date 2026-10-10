# Task Register - Ratu Aurion Protocol

Dokumen cetak blueprint dan pelacak tugas rekayasa sistem untuk proyek blockchain modular Ratu Aurion Protocol.

| ID Task | Domain Sistem | Deskripsi Spesifikasi Teknis | Ketergantungan | Status |
| :--- | :--- | :--- | :--- | :--- |
| TR-01 | Primitives | Definisi tipe data dasar, representasi biner, address, hash primitives, serta abstraksi data serialisasi standar. | None | Tervalidasi |
| TR-02 | Storage | Implementasi append-only log engine untuk persistensi data blok, transaksi, write-ahead logging (WAL), dan flush scheduler. | None | Tervalidasi |
| TR-03 | Index | Desain dan implementasi struktur data indeks berbasis RAM untuk pencarian cepat status/state dan pemetaan kunci-nilai performa tinggi. | None | Tervalidasi |
| TR-04 | Archive | Mesin kompresi dan segmentasi arsip data historis, cold storage management, serta strategi pruning data blok. | None | Tervalidasi |
| TR-05 | Engine | Mesin orkestrasi dan validasi transaksi (kriptografi, storage, in-memory index, dan pengarsipan). | None | Tervalidasi |
| TR-06 | Consensus | Penegakan aturan komitmen jaringan, validasi finalitas blok, algoritma konsensus modular, dan integritas state commitment. | None | Tervalidasi |
| TR-07 | Network | Protokol komunikasi data P2P, serialization framing, transport abstraction layer, penanganan koneksi peer, dan propagasi pesan. | None | Tervalidasi |
| TR-08 | Node | Integrasi runtime simpul biner utama (ratu-aurion-node), loop server TCP P2P, pipeline handler, dan graceful shutdown. | TR-01 - TR-07 | Tervalidasi |
| REFACTOR-REBRAND-01 | Workspace Rebranding | Rebranding menyeluruh workspace dan protokol biner ke Ratu Aurion Protocol (`ratu-aurion-*`, ticker `AUR`, `AurValue`, magic bytes baru). | None | Tervalidasi |
| E2E-BENCH-01 | E2E Benchmark | End-to-end stress test dan saturasi throughput pipeline in-memory, loopback socket TCP, serta ketahanan injeksi anomali. | TR-05, TR-07, TR-08 | Tervalidasi |
| NET-CLUSTER-01 | Network / Cluster | 4-Node Testnet Cluster & Consensus Gossip: In-tree Full-Mesh TCP coordinator (`PeerMesh`), gossip wire framing, QC commit, deterministic leader rotation, dan view change failure recovery. | TR-06, TR-07, TR-08 | Tervalidasi |
| GATEWAY-RPC-01 | Gateway / RPC | In-tree HTTP/1.1 JSON-RPC 2.0 & RFC 6455 WebSocket Gateway Interface: Dispatcher deterministik (`aur_chainId`, `aur_blockHeight`, `aur_getBalance`, `aur_getAccountLocation`, `aur_sendRawTransaction`), batas body 64 KB, SHA-1 integer in-tree, dan langganan mutasi real-time. | TR-05, TR-07, TR-08 | Tervalidasi |
| SEC-KEYSTORE-01 | Keystore / Security | In-tree ChaCha20-BLAKE3 Encrypted Keystore & Validator Key Management: Berkas 128 byte tetap (`KEYSTORE_FILE_SIZE = 128`), header 64B magic `*b"RAURKEY\x01"`, KDF PBKDF-BLAKE3 100.000 iterasi, enkripsi ChaCha20 RFC 8439 integer murni, BLAKE3 Keyed MAC fail-fast, sub-perintah CLI (`keystore generate`, `inspect`, `export`), dan integrasi runtime simpul node (`--keystore`, `--keystore-pass`). | TR-01, TR-08 | Tervalidasi |
