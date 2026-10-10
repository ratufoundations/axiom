#![forbid(unsafe_code)]

//! # Axiom Node
//!
//! Simpul jaringan mandiri dan executable binary runtime Axiom.
//! Mengorkestrasi engine eksekusi transaksi, indeks RAM, pengarsipan bulanan,
//! konsensus pembuktian kuorum, dan server TCP P2P.

pub mod config;
pub mod rpc;
pub mod server;
pub mod telemetry;
pub mod ws;

use std::fs;
use std::sync::mpsc;
use std::sync::{Arc, RwLock};

use ratu_aurion_consensus::validator_set::{ValidatorInfo, ValidatorSet};
use ratu_aurion_engine::coordinator::EngineCoordinator;
use ratu_aurion_network::peer::PeerTable;

use crate::config::NodeConfig;
use crate::server::NodeServer;

fn main() {
    println!("[ratu-aurion-node] Memulai bootstrap simpul Axiom...");

    // 1. Parsing konfigurasi CLI
    let mut config = match NodeConfig::parse_args() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("[ratu-aurion-node ERROR] {err}");
            std::process::exit(1);
        }
    };

    // Jika --keystore disediakan, dekripsi kunci validator langsung dari berkas keystore
    if let Some(ref keystore_path) = config.keystore_path {
        let pass = config.keystore_pass.as_deref().unwrap_or("");
        match ratu_aurion_primitives::keystore::load_keystore_file(keystore_path, pass) {
            Ok((key, account)) => {
                println!(
                    "[ratu-aurion-node] Kunci validator berhasil didekripsi dari keystore {:?} (AccountId: 0x{})",
                    keystore_path,
                    crate::rpc::hex_encode(account.as_bytes())
                );
                config.validator_key = key;
            }
            Err(e) => {
                eprintln!(
                    "[ratu-aurion-node ERROR] Gagal mendekripsi berkas keystore {:?}: {e}",
                    keystore_path
                );
                std::process::exit(1);
            }
        }
    }

    println!(
        "[ratu-aurion-node] Konfigurasi dimuat: listen={}, epoch={}, data_dir={:?}, archive_dir={:?}",
        config.listen_addr, config.epoch, config.data_dir, config.archive_dir
    );

    // 2. Setup direktori penyimpanan dan arsip
    if let Err(e) = fs::create_dir_all(&config.data_dir) {
        eprintln!("[ratu-aurion-node ERROR] Gagal membuat data_dir: {e}");
        std::process::exit(1);
    }
    if let Err(e) = fs::create_dir_all(&config.archive_dir) {
        eprintln!("[ratu-aurion-node ERROR] Gagal membuat archive_dir: {e}");
        std::process::exit(1);
    }

    // 3. Inisialisasi EngineCoordinator
    let engine = match EngineCoordinator::new(&config.data_dir, &config.archive_dir, config.epoch) {
        Ok(eng) => Arc::new(RwLock::new(eng)),
        Err(e) => {
            eprintln!("[ratu-aurion-node ERROR] Inisialisasi EngineCoordinator gagal: {e}");
            std::process::exit(1);
        }
    };

    // 4. Inisialisasi ValidatorSet dengan validator lokal simpul
    let local_validator = ValidatorInfo::new(config.validator_account(), 100);
    let validator_set = match ValidatorSet::new(vec![local_validator]) {
        Ok(vs) => vs,
        Err(e) => {
            eprintln!("[ratu-aurion-node ERROR] Inisialisasi ValidatorSet gagal: {e}");
            std::process::exit(1);
        }
    };

    println!(
        "[ratu-aurion-node] ValidatorSet aktif: total_weight={}, quorum_threshold={}",
        validator_set.total_weight(),
        validator_set.quorum_threshold()
    );

    // 5. Inisialisasi PeerTable
    let peer_table = Arc::new(RwLock::new(PeerTable::new()));

    // 6. Jalankan Server Jaringan P2P dan Telemetri IPC
    let telemetry = Arc::new(telemetry::NodeTelemetryCollector::new());
    if let Some(ref ipc_path) = config.ipc_socket {
        if let Err(e) = telemetry::IpcServer::start(ipc_path, telemetry.clone()) {
            eprintln!("[ratu-aurion-node WARNING] Gagal mengikat UDS telemetri di {ipc_path:?}: {e}");
        } else {
            println!("[ratu-aurion-node] IPC Telemetri aktif di {:?}", ipc_path);
        }
    }

    let server = NodeServer::with_telemetry(config.clone(), engine, peer_table, telemetry);
    let (_shutdown_tx, shutdown_rx) = mpsc::channel();

    println!(
        "[ratu-aurion-node] Server P2P aktif mendengarkan di {}",
        config.listen_addr
    );
    if let Err(e) = server.start(shutdown_rx) {
        eprintln!("[ratu-aurion-node ERROR] Server loop berhenti dengan galat: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    use ratu_aurion_consensus::proposal::SegmentProposal;
    use ratu_aurion_network::codec::{decode_message, encode_message};
    use ratu_aurion_network::message::NetworkMessage;
    use ratu_aurion_primitives::crypto::{AccountId, Hash, Signature};
    use ratu_aurion_primitives::framing::HEADER_SIZE;
    use ratu_aurion_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER};
    use ratu_aurion_primitives::value::AurValue;
    use ed25519_dalek::{Signer, SigningKey};

    fn unique_test_dirs(label: &str) -> (PathBuf, PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("ratu_aurion_node_{label}_{nanos}"));
        let data_dir = base.join("data");
        let archive_dir = base.join("archive");
        (data_dir, archive_dir)
    }

    #[test]
    fn test_node_config_parsing() {
        let args = vec![
            "--data-dir".to_string(),
            "my_data_path".to_string(),
            "--archive-dir".to_string(),
            "my_archive_path".to_string(),
            "--listen".to_string(),
            "127.0.0.1:9099".to_string(),
            "--epoch".to_string(),
            "7".to_string(),
            "--seed-byte".to_string(),
            "3".to_string(),
        ];

        let cfg = NodeConfig::from_args(args).expect("Config parse from args");
        assert_eq!(cfg.data_dir, PathBuf::from("my_data_path"));
        assert_eq!(cfg.archive_dir, PathBuf::from("my_archive_path"));
        assert_eq!(
            cfg.listen_addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9099)
        );
        assert_eq!(cfg.epoch, 7);
    }

    #[test]
    fn test_node_server_bootstrap_and_graceful_shutdown() {
        let (data_dir, archive_dir) = unique_test_dirs("bootstrap");
        let mut cfg = NodeConfig::default_test_config();
        cfg.data_dir = data_dir.clone();
        cfg.archive_dir = archive_dir;
        cfg.listen_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);

        let engine = Arc::new(RwLock::new(
            EngineCoordinator::new(&cfg.data_dir, &cfg.archive_dir, cfg.epoch)
                .expect("Engine init"),
        ));
        let peer_table = Arc::new(RwLock::new(PeerTable::new()));
        let server = NodeServer::new(cfg, engine, peer_table);

        let listener = server.bind().expect("Bind server");
        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            server.run_listener(listener, shutdown_rx)
        });

        // Kirim sinyal shutdown
        shutdown_tx.send(()).expect("Send shutdown signal");
        let result = handle.join().expect("Join server thread");
        assert!(result.is_ok());

        let _ = fs::remove_dir_all(data_dir.parent().unwrap());
    }

    #[test]
    fn test_node_e2e_proposal_and_vote_pipeline() {
        let (data_dir, archive_dir) = unique_test_dirs("prop_vote");
        let mut cfg = NodeConfig::default_test_config();
        cfg.data_dir = data_dir.clone();
        cfg.archive_dir = archive_dir;
        cfg.listen_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);

        let node_account = cfg.validator_account();

        let engine = Arc::new(RwLock::new(
            EngineCoordinator::new(&cfg.data_dir, &cfg.archive_dir, cfg.epoch)
                .expect("Engine init"),
        ));
        let peer_table = Arc::new(RwLock::new(PeerTable::new()));
        let server = NodeServer::new(cfg, engine, peer_table);

        let listener = server.bind().expect("Bind server");
        let actual_addr = listener.local_addr().expect("Local addr");
        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            server.run_listener(listener, shutdown_rx)
        });

        // Klien: Hubungi server dan ajukan proposal segmen
        let mut stream = TcpStream::connect(actual_addr).expect("Connect to server");

        let proposal = SegmentProposal::new(
            1,
            0,
            Hash::new([0xee; 32]),
            AccountId::new([0x99; 32]),
            1,
        );

        let req_msg = NetworkMessage::Proposal(proposal);
        let req_packet = encode_message(&req_msg).expect("Encode proposal");
        stream.write_all(&req_packet).expect("Send proposal");
        stream.flush().expect("Flush stream");

        // Klien: Terima balasan Vote
        let mut header_buf = [0u8; HEADER_SIZE];
        stream.read_exact(&mut header_buf).expect("Read header");
        let payload_len = u32::from_le_bytes([
            header_buf[6],
            header_buf[7],
            header_buf[8],
            header_buf[9],
        ]) as usize;

        let mut payload = vec![0u8; payload_len];
        stream.read_exact(&mut payload).expect("Read payload");

        let mut reply_packet = Vec::new();
        reply_packet.extend_from_slice(&header_buf);
        reply_packet.extend_from_slice(&payload);

        let reply_msg = decode_message(&reply_packet).expect("Decode vote reply");
        match reply_msg {
            NetworkMessage::Vote(vote) => {
                // Verifikasi bahwa vote ditandatangani oleh validator node dan cocok dengan proposal
                assert_eq!(vote.validator, node_account);
                assert!(vote.verify(&proposal).is_ok());
            }
            other => panic!("Expected Vote reply, got: {:?}", other),
        }

        drop(stream);
        shutdown_tx.send(()).expect("Send shutdown signal");
        let _ = handle.join().expect("Join server thread");

        let _ = fs::remove_dir_all(data_dir.parent().unwrap());
    }

    #[test]
    fn test_node_e2e_sync_request_and_chunk_pipeline() {
        let (data_dir, archive_dir) = unique_test_dirs("sync_chunk");
        let mut cfg = NodeConfig::default_test_config();
        cfg.data_dir = data_dir.clone();
        cfg.archive_dir = archive_dir;
        cfg.listen_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);

        // Buat mutasi transaksi awal pada engine agar berkas segmen fisik terbentuk di disk
        let mut raw_engine = EngineCoordinator::new(&cfg.data_dir, &cfg.archive_dir, cfg.epoch)
            .expect("Engine init");

        let mut seed = [0u8; 32];
        seed[0] = 0x55;
        let alice_key = SigningKey::from_bytes(&seed);
        let alice = AccountId::new(alice_key.verifying_key().to_bytes());
        let bob = AccountId::new([0x66; 32]);

        raw_engine.seed_account(alice, AurValue::from_atomic(100_000_000_000));

        let mut tx = MutationRecord {
            epoch: 1,
            sequence_number: 1,
            record_kind: RECORD_KIND_TRANSFER,
            sender: alice,
            recipient: bob,
            amount: AurValue::from_atomic(20_000_000_000),
            signature: Signature::ZERO,
        };
        let payload = ratu_aurion_engine::validator::compute_signing_payload(&tx);
        let sig = alice_key.sign(&payload);
        tx.signature = Signature::new(sig.to_bytes());

        raw_engine.submit_transaction(&tx).expect("Submit tx");

        let engine = Arc::new(RwLock::new(raw_engine));
        let peer_table = Arc::new(RwLock::new(PeerTable::new()));
        let server = NodeServer::new(cfg, engine, peer_table);

        let listener = server.bind().expect("Bind server");
        let actual_addr = listener.local_addr().expect("Local addr");
        let (shutdown_tx, shutdown_rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            server.run_listener(listener, shutdown_rx)
        });

        // Klien: Kirim SyncRequest
        let mut stream = TcpStream::connect(actual_addr).expect("Connect to server");
        let req_msg = NetworkMessage::SyncRequest {
            epoch: 1,
            segment_index: 0,
            from_offset: 0,
        };
        let req_packet = encode_message(&req_msg).expect("Encode sync req");
        stream.write_all(&req_packet).expect("Send sync req");
        stream.flush().expect("Flush stream");

        // Klien: Baca balasan SyncChunk
        let mut header_buf = [0u8; HEADER_SIZE];
        stream.read_exact(&mut header_buf).expect("Read header");
        let payload_len = u32::from_le_bytes([
            header_buf[6],
            header_buf[7],
            header_buf[8],
            header_buf[9],
        ]) as usize;

        let mut payload = vec![0u8; payload_len];
        stream.read_exact(&mut payload).expect("Read payload");

        let mut reply_packet = Vec::new();
        reply_packet.extend_from_slice(&header_buf);
        reply_packet.extend_from_slice(&payload);

        let reply_msg = decode_message(&reply_packet).expect("Decode sync chunk reply");
        match reply_msg {
            NetworkMessage::SyncChunk {
                epoch,
                segment_index,
                offset,
                data,
            } => {
                assert_eq!(epoch, 1);
                assert_eq!(segment_index, 0);
                assert_eq!(offset, 0);
                assert!(!data.is_empty());
                // Byte 0..4 dari segmen berkas log adalah magic bytes "RAUR"
                assert_eq!(&data[0..4], b"RAUR");
            }
            other => panic!("Expected SyncChunk reply, got: {:?}", other),
        }

        drop(stream);
        shutdown_tx.send(()).expect("Send shutdown signal");
        let _ = handle.join().expect("Join server thread");

        let _ = fs::remove_dir_all(data_dir.parent().unwrap());
    }
}
