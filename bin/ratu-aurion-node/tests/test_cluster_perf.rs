#![forbid(unsafe_code)]

//! Suite Pengujian & Benchmark Pelepasan Produksi Kluster 4-Simpul (PERF-RELEASE-01)
//!
//! Validasi kinerja throughput konsensus Full-Mesh P2P, agregasi Quorum Certificate (QC),
//! serta latensi komitmen disk sinkron lintas 4 simpul validator pada profil rilis teroptimasi.

use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};
use ratu_aurion_consensus::certificate::QuorumCertificate;
use ratu_aurion_consensus::evidence::VoteRecord;
use ratu_aurion_consensus::pacemaker::Pacemaker;
use ratu_aurion_consensus::proposal::SegmentProposal;
use ratu_aurion_consensus::validator_set::{ValidatorInfo, ValidatorSet};
use ratu_aurion_consensus::vote::Vote;
use ratu_aurion_engine::coordinator::EngineCoordinator;
use ratu_aurion_network::error::NetworkError;
use ratu_aurion_network::mesh::PeerMesh;
use ratu_aurion_network::message::NetworkMessage;
use ratu_aurion_network::peer::PeerTable;
use ratu_aurion_node::config::NodeConfig;
use ratu_aurion_node::server::NodeServer;
use ratu_aurion_primitives::crypto::{AccountId, Hash, Signature};
use ratu_aurion_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER};
use ratu_aurion_primitives::value::AurValue;

fn unique_cluster_dirs(label: &str, node_idx: usize) -> (PathBuf, PathBuf) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "ratu_aurion_cluster_perf_{label}_n{node_idx}_{nanos}"
    ));
    let data_dir = base.join("data");
    let archive_dir = base.join("archive");
    fs::create_dir_all(&data_dir).expect("Create data dir");
    fs::create_dir_all(&archive_dir).expect("Create archive dir");
    (data_dir, archive_dir)
}

fn create_validator_identity(seed_byte: u8) -> (SigningKey, AccountId) {
    let mut seed = [0u8; 32];
    seed[0] = seed_byte;
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();
    let account = AccountId::new(verifying_key.to_bytes());
    (signing_key, account)
}

struct TestNode {
    account: AccountId,
    signing_key: SigningKey,
    engine: Arc<RwLock<EngineCoordinator>>,
    mesh: PeerMesh,
    addr: SocketAddr,
    shutdown_tx: Option<Sender<()>>,
    server_handle: Option<JoinHandle<Result<(), NetworkError>>>,
    data_dir: PathBuf,
}

impl TestNode {
    fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.server_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for TestNode {
    fn drop(&mut self) {
        self.shutdown();
        if let Some(parent) = self.data_dir.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

fn bootstrap_4_node_cluster(
    test_label: &str,
    epoch: u64,
) -> (Vec<TestNode>, Vec<AccountId>, ValidatorSet) {
    let seeds = [0x01u8, 0x02u8, 0x03u8, 0x04u8];
    let mut nodes = Vec::with_capacity(4);
    let mut val_accounts = Vec::with_capacity(4);
    let mut val_infos = Vec::with_capacity(4);

    for (i, &seed) in seeds.iter().enumerate() {
        let (signing_key, account) = create_validator_identity(seed);
        let (data_dir, archive_dir) = unique_cluster_dirs(test_label, i);

        let mut config = NodeConfig::default_test_config();
        config.data_dir = data_dir.clone();
        config.archive_dir = archive_dir.clone();
        config.listen_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);
        config.epoch = epoch;
        config.validator_key = signing_key.clone();

        let raw_engine =
            EngineCoordinator::new(&data_dir, &archive_dir, epoch).expect("Init engine coordinator");
        let engine = Arc::new(RwLock::new(raw_engine));
        let peer_table = Arc::new(RwLock::new(PeerTable::new()));

        let server = NodeServer::new(config, Arc::clone(&engine), peer_table);
        let listener = server.bind().expect("Bind ephemeral listener");
        let addr = listener.local_addr().expect("Get local listener address");

        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
        let server_handle = thread::spawn(move || server.run_listener(listener, shutdown_rx));

        let mesh = PeerMesh::new(account);

        nodes.push(TestNode {
            account,
            signing_key,
            engine,
            mesh,
            addr,
            shutdown_tx: Some(shutdown_tx),
            server_handle: Some(server_handle),
            data_dir,
        });

        val_accounts.push(account);
        val_infos.push(ValidatorInfo::new(account, 1));
    }

    // Topologi rotasi terurut: (epoch + round) % 4
    let ordered_validators = vec![
        val_accounts[3], // index 0 (Validator 4)
        val_accounts[0], // index 1 (Validator 1)
        val_accounts[1], // index 2 (Validator 2)
        val_accounts[2], // index 3 (Validator 3)
    ];

    let validator_set = ValidatorSet::new(val_infos).expect("Construct ValidatorSet");
    assert_eq!(validator_set.quorum_threshold(), 3);

    // Membangun topologi mutual full-mesh TCP antarseluruh 4 simpul
    for i in 0..4 {
        for j in 0..4 {
            if i != j {
                let peer_account = nodes[j].account;
                let peer_addr = nodes[j].addr;
                nodes[i]
                    .mesh
                    .connect_peer(peer_account, peer_addr)
                    .expect("Establish full-mesh connection");
            }
        }
        assert_eq!(nodes[i].mesh.connected_peer_count(), 3);
    }

    (nodes, ordered_validators, validator_set)
}

fn create_signed_mutation(
    epoch: u64,
    seq: u64,
    sender: AccountId,
    recipient: AccountId,
    amount: AurValue,
    sender_key: &SigningKey,
) -> MutationRecord {
    let mut record = MutationRecord {
        epoch,
        sequence_number: seq,
        record_kind: RECORD_KIND_TRANSFER,
        sender,
        recipient,
        amount,
        signature: Signature::ZERO,
    };
    let payload = ratu_aurion_engine::validator::compute_signing_payload(&record);
    let sig = sender_key.sign(&payload);
    record.signature = Signature::new(sig.to_bytes());
    record
}

#[test]
fn test_release_cluster_throughput() {
    let epoch = 0u64;
    let (mut nodes, ordered_validators, validator_set) =
        bootstrap_4_node_cluster("release_perf", epoch);

    // 1. Seed 10 accounts across all 4 node engine instances (1,000 AUR per akun = 10,000 AUR total)
    let initial_balance = AurValue::from_whole_aur(1_000).expect("1,000 AUR");
    let transfer_amount = AurValue::from_whole_aur(10).expect("10 AUR");
    let expected_total_supply = AurValue::from_whole_aur(10_000).expect("10,000 AUR");

    let mut client_accounts = Vec::with_capacity(10);
    for i in 0..10 {
        let (sk, acc) = create_validator_identity(0x60 + i as u8);
        client_accounts.push((sk, acc));

        for node in &nodes {
            node.engine
                .write()
                .unwrap()
                .seed_account(acc, initial_balance);
        }
    }

    // Verifikasi saldo awal pada seluruh 4 simpul
    for node in &nodes {
        let eng = node.engine.read().unwrap();
        let mut sum_atomic = 0u128;
        for (_, acc) in &client_accounts {
            sum_atomic += eng.query_balance(acc).to_atomic();
        }
        assert_eq!(AurValue::from_atomic(sum_atomic), expected_total_supply);
    }

    // 2. Jalankan sekuens 5 ronde konsensus di bawah beban rilis
    let total_rounds = 5u64;
    let mut round_latencies_micros = Vec::with_capacity(total_rounds as usize);
    let cluster_benchmark_start = Instant::now();

    for round in 1..=total_rounds {
        let round_start = Instant::now();

        // Identifikasi pemimpin sah ronde ini via (epoch + round) % 4
        let leader_acc = Pacemaker::elect_leader(epoch, round, &ordered_validators)
            .expect("Elect leader for round");
        let leader_idx = nodes
            .iter()
            .position(|n| n.account == leader_acc)
            .expect("Leader found in cluster");

        // Siapkan transaksi untuk ronde ini
        let sender_idx = ((round - 1) as usize) % 10;
        let recipient_idx = (sender_idx + 1) % 10;
        let (ref sender_sk, sender) = client_accounts[sender_idx];
        let (_, recipient) = client_accounts[recipient_idx];

        let mutation = create_signed_mutation(
            1,
            round,
            sender,
            recipient,
            transfer_amount,
            sender_sk,
        );
        let tx_digest = Hash::new(*blake3::hash(&mutation.to_bytes()).as_bytes());
        let proposal = SegmentProposal::new(1, 0, tx_digest, leader_acc, round);

        // Leader menyiarkan proposal ke seluruh peer
        let broadcast_count = nodes[leader_idx]
            .mesh
            .broadcast(&NetworkMessage::Proposal(proposal))
            .expect("Broadcast proposal");
        assert_eq!(broadcast_count, 3, "All 3 peer nodes must receive proposal");

        // Follower nodes memverifikasi dan menyiarkan signed VoteRecord serta signed Vote
        let mut votes = Vec::with_capacity(4);
        for (i, node) in nodes.iter_mut().enumerate() {
            let vote = Vote::sign(&proposal, node.account, &node.signing_key);
            assert!(vote.verify(&proposal).is_ok());

            if i != leader_idx {
                let vr = VoteRecord::sign(
                    node.account,
                    1,
                    round,
                    proposal.digest(),
                    &node.signing_key,
                );
                assert!(vr.verify_signature().is_ok());
                let b = node
                    .mesh
                    .broadcast(&NetworkMessage::VoteRecord(vr))
                    .expect("Broadcast vote record");
                assert_eq!(b, 3);
            }
            votes.push(vote);
        }

        // Leader mengumpulkan >= 3 suara dan merakit QuorumCertificate (QC)
        let mut qc = QuorumCertificate::new(proposal);
        for vote in &votes[0..3] {
            qc.add_vote(vote, &validator_set).expect("Add vote to QC");
        }
        assert!(qc.is_final(&validator_set), "Supermajority quorum must be reached");
        qc.verify(&validator_set).expect("QC must be cryptographically valid");

        // Leader menyiarkan QC yang telah terhimpun
        let qc_broadcast = nodes[leader_idx]
            .mesh
            .broadcast(&NetworkMessage::Certificate(qc))
            .expect("Broadcast QC");
        assert_eq!(qc_broadcast, 3, "All 3 peer nodes must receive QC");

        // Seluruh 4 simpul mengomitsikan mutasi ke storage disk
        let mut round_offsets = Vec::with_capacity(4);
        for node in &nodes {
            let offset = node
                .engine
                .write()
                .unwrap()
                .submit_transaction(&mutation)
                .expect("Commit transaction to node storage");
            round_offsets.push(offset);
        }

        // Verifikasi determinisme offset disk antar seluruh 4 simpul
        let first_off = round_offsets[0];
        for &off in &round_offsets {
            assert_eq!(off, first_off, "Disk offsets must be identical across all nodes");
        }

        let round_micros = round_start.elapsed().as_micros() as u64;
        round_latencies_micros.push(round_micros);
    }

    let cluster_elapsed = cluster_benchmark_start.elapsed();
    let cluster_total_micros = cluster_elapsed.as_micros().max(1) as u64;
    let avg_round_micros = cluster_total_micros / total_rounds;

    // 3. Verifikasi akhir: konsistensi pasokan moneter pada seluruh 4 simpul
    for node in &nodes {
        let eng = node.engine.read().unwrap();
        let mut final_sum_atomic = 0u128;
        for (_, acc) in &client_accounts {
            final_sum_atomic += eng.query_balance(acc).to_atomic();
        }
        assert_eq!(
            AurValue::from_atomic(final_sum_atomic),
            expected_total_supply,
            "Total monetary supply must be conserved across all nodes"
        );
    }

    println!("\n================================================================================");
    println!("=== RATU AURION 4-NODE CLUSTER PRODUCTION BENCHMARK (PERF-RELEASE-01) ===");
    println!("================================================================================");
    println!("Jumlah Simpul Kluster   : 4 simpul (Topologi Mutual Full-Mesh TCP)");
    println!("Total Ronde Konsensus   : {} ronde konsensus penuh (Proposal -> QC Commit)", total_rounds);
    for (r_idx, lat) in round_latencies_micros.iter().enumerate() {
        println!("  - Ronde {} Latensi      : {} us ({} ms)", r_idx + 1, lat, lat / 1_000);
    }
    println!("Rata-rata Durasi Ronde  : {} us ({} ms)", avg_round_micros, avg_round_micros / 1_000);
    println!("Total Waktu Benchmark   : {} ms", cluster_elapsed.as_millis());
    println!("Konservasi Pasokan Koin : {} AUR (Identik di seluruh 4 Simpul)", expected_total_supply);
    println!("================================================================================\n");
}
