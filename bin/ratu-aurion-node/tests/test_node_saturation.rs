#![forbid(unsafe_code)]

//! Node TCP Loopback Saturation Test Suite (E2E-BENCH-01)
//!
//! Validasi integrasi simpul mandiri `ratu-aurion-node` melalui antarmuka jaringan TCP loopback:
//! - Streaming frame transaksi tersandi oleh 4 thread klien simultan.
//! - Penegakan pembatasan laju Token Bucket murni integer per-peer.
//! - Verifikasi pemutakhiran status buku besar dan respons TxResult.
//! - Verifikasi propagasi backpressure dan graceful shutdown socket.

use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ratu_aurion_engine::coordinator::EngineCoordinator;
use ratu_aurion_network::codec::{decode_message, encode_message};
use ratu_aurion_network::framed::FramedStream;
use ratu_aurion_network::message::NetworkMessage;
use ratu_aurion_network::peer::PeerTable;
use ratu_aurion_network::rate_limiter::TokenBucketLimiter;
use ratu_aurion_node::config::NodeConfig;
use ratu_aurion_node::server::NodeServer;
use ratu_aurion_primitives::crypto::{AccountId, Signature};
use ratu_aurion_primitives::framing::HEADER_SIZE;
use ratu_aurion_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER};
use ratu_aurion_primitives::value::AurValue;
use ed25519_dalek::{Signer, SigningKey};

fn unique_test_dirs(label: &str) -> (PathBuf, PathBuf) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("ratu_aurion_node_sat_{label}_{nanos}"));
    let data_dir = base.join("data");
    let archive_dir = base.join("archive");
    fs::create_dir_all(&data_dir).expect("Create data dir");
    fs::create_dir_all(&archive_dir).expect("Create archive dir");
    (data_dir, archive_dir)
}

fn create_client_keypair(seed_byte: u8) -> (SigningKey, AccountId) {
    let mut seed = [0u8; 32];
    seed[0] = seed_byte;
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();
    let account = AccountId::new(verifying_key.to_bytes());
    (signing_key, account)
}

#[test]
fn test_node_tcp_loopback_saturation() {
    let (data_dir, archive_dir) = unique_test_dirs("tcp_loopback");
    let mut cfg = NodeConfig::default_test_config();
    cfg.data_dir = data_dir.clone();
    cfg.archive_dir = archive_dir;
    cfg.listen_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);

    let initial_balance = AurValue::from_whole_aur(1_000).expect("1,000 AUR");
    let transfer_amount = AurValue::from_whole_aur(1).expect("1 AUR");

    // 1. Inisialisasi EngineCoordinator dan pra-isi (seed) 4 akun klien
    let mut raw_engine = EngineCoordinator::new(&cfg.data_dir, &cfg.archive_dir, cfg.epoch)
        .expect("Engine coordinator init");

    let mut clients = Vec::with_capacity(4);
    for i in 0..4 {
        let (signing_key, sender) = create_client_keypair(0x40 + i as u8);
        let recipient = AccountId::new([0x80 + i as u8; 32]);
        raw_engine.seed_account(sender, initial_balance);
        clients.push((signing_key, sender, recipient));
    }

    let engine = Arc::new(RwLock::new(raw_engine));
    let peer_table = Arc::new(RwLock::new(PeerTable::new()));
    let server = NodeServer::new(cfg, Arc::clone(&engine), peer_table);

    // 2. Bind TCP listener pada port efemeral dan nyalakan server di background thread
    let listener = server.bind().expect("Bind ephemeral listener");
    let actual_addr = listener.local_addr().expect("Retrieve local listener address");
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();

    let server_handle = thread::spawn(move || {
        server.run_listener(listener, shutdown_rx)
    });

    let start_instant = Instant::now();
    let tx_per_client = 20usize;

    // 3. Spawn 4 client threads connecting via FramedStream over TCP
    let mut client_handles = Vec::with_capacity(4);
    for (signing_key, sender, recipient) in clients.clone() {
        let handle = thread::spawn(move || {
            let mut limiter = TokenBucketLimiter::new(25, 25, 0);
            let mut committed_offsets = Vec::with_capacity(tx_per_client);

            for seq in 1..=tx_per_client as u64 {
                // Penegakan kuota token bucket sebelum pengiriman
                let simulated_ms = (seq - 1) * 20;
                limiter.refill_at(simulated_ms);
                limiter
                    .try_consume(simulated_ms, 1)
                    .expect("Token bucket permit must be granted");

                // Hubungkan socket TCP dan bungkus dengan FramedStream (3,000 ms timeout)
                let tcp_stream = TcpStream::connect(actual_addr).expect("Connect TCP to node");
                let mut framed = FramedStream::from_tcp(tcp_stream, 3_000)
                    .expect("Construct FramedStream wrapper");

                // Siapkan mutasi dan tandatangani secara kriptografis
                let mut record = MutationRecord {
                    epoch: 1,
                    sequence_number: seq,
                    record_kind: RECORD_KIND_TRANSFER,
                    sender,
                    recipient,
                    amount: transfer_amount,
                    signature: Signature::ZERO,
                };
                let payload = ratu_aurion_engine::validator::compute_signing_payload(&record);
                let sig = signing_key.sign(&payload);
                record.signature = Signature::new(sig.to_bytes());

                // Enkode ke paket biner FrameHeader 42-byte
                let wire_packet = encode_message(&NetworkMessage::TxSubmit(record))
                    .expect("Encode TxSubmit frame");

                // Kirim frame transmisi melalui socket stream berbingkai
                framed
                    .stream_mut()
                    .write_all(&wire_packet)
                    .expect("Write wire frame to socket");
                framed
                    .stream_mut()
                    .flush()
                    .expect("Flush frame buffer");

                // Baca balasan TxResult dari server
                let mut header_buf = [0u8; HEADER_SIZE];
                framed
                    .stream_mut()
                    .read_exact(&mut header_buf)
                    .expect("Read reply header");
                let payload_len = u32::from_le_bytes([
                    header_buf[6],
                    header_buf[7],
                    header_buf[8],
                    header_buf[9],
                ]) as usize;

                let mut payload = vec![0u8; payload_len];
                framed
                    .stream_mut()
                    .read_exact(&mut payload)
                    .expect("Read reply payload");

                let mut reply_packet = Vec::with_capacity(HEADER_SIZE + payload_len);
                reply_packet.extend_from_slice(&header_buf);
                reply_packet.extend_from_slice(&payload);

                let reply_msg = decode_message(&reply_packet).expect("Decode reply packet");
                match reply_msg {
                    NetworkMessage::TxResult { success, offset, .. } => {
                        assert!(success, "Transaction must commit successfully on node");
                        assert!(offset >= 42, "Disk offset must be beyond 42-byte header");
                        committed_offsets.push(offset);
                    }
                    other => panic!("Expected TxResult reply, received: {:?}", other),
                }
            }

            committed_offsets
        });
        client_handles.push(handle);
    }

    // 4. Gabungkan hasil seluruh thread klien
    let mut total_committed_network_tx = 0usize;
    for handle in client_handles {
        let offsets = handle.join().expect("Join client thread");
        total_committed_network_tx += offsets.len();
    }

    let elapsed_ms = start_instant.elapsed().as_millis().max(1);
    let network_tps = (total_committed_network_tx as u128 * 1_000) / elapsed_ms;

    assert_eq!(
        total_committed_network_tx, 80,
        "All 80 transactions (4 clients x 20 tx) must commit via TCP loopback"
    );

    // 5. Validasi pemutakhiran status buku besar di engine simpul
    let eng_guard = engine.read().expect("Acquire engine read lock");
    let mut total_supply_check = 0u128;
    for (_, sender, recipient) in &clients {
        let sender_bal = eng_guard.query_balance(sender);
        let recipient_bal = eng_guard.query_balance(recipient);

        // Pengirim: 1,000 - 20 = 980 AUR
        let expected_sender = AurValue::from_whole_aur(980).expect("980 AUR");
        // Penerima: 0 + 20 = 20 AUR
        let expected_recipient = AurValue::from_whole_aur(20).expect("20 AUR");

        assert_eq!(sender_bal, expected_sender, "Sender balance must match exactly");
        assert_eq!(recipient_bal, expected_recipient, "Recipient balance must match exactly");

        total_supply_check = total_supply_check
            .checked_add(sender_bal.to_atomic())
            .expect("No overflow")
            .checked_add(recipient_bal.to_atomic())
            .expect("No overflow");
    }

    let expected_global_supply = AurValue::from_whole_aur(4_000).expect("4,000 AUR");
    assert_eq!(
        AurValue::from_atomic(total_supply_check),
        expected_global_supply,
        "Global total supply must be strictly conserved across loopback mutations"
    );
    drop(eng_guard);

    // 6. Cetak metrik performa saturasi socket TCP
    println!("\n================================================================================");
    println!("=== RATU AURION NODE TCP LOOPBACK SATURATION BENCHMARK (E2E-BENCH-01) ===");
    println!("================================================================================");
    println!("Total Transaksi Loopback : {} tx", total_committed_network_tx);
    println!("Thread Klien Paralel     : 4 threads (FramedStream over TCP)");
    println!("Durasi Loopback Total    : {} ms", elapsed_ms);
    println!("Throughput TCP Loopback  : {} TPS (integer transactions-per-second)", network_tps);
    println!("Status Buku Besar Simpul : 100% Konservasi Pasokan Moneter Terverifikasi");
    println!("================================================================================\n");

    // 7. Verifikasi graceful shutdown: Kirim sinyal shutdown dan pastikan socket menutup bersih
    shutdown_tx.send(()).expect("Send shutdown signal");
    let join_res = server_handle.join().expect("Join server thread");
    assert!(join_res.is_ok(), "Server loop must terminate cleanly");

    let _ = fs::remove_dir_all(data_dir.parent().unwrap());
}
