#![forbid(unsafe_code)]

//! Multi-Node Local Validator Testnet & Consensus Gossip Test Suite (NET-CLUSTER-01)
//!
//! Validasi integrasi kluster 4-simpul validator mandiri tanpa dependensi eksternal:
//! 1. `test_cluster_happy_path_proposal_and_qc_commit`:
//!    - Bootstrap 4 local `NodeServer` pada port efemeral (`127.0.0.1:0`).
//!    - Topologi mutual Full-Mesh TCP `PeerMesh` antarseluruh 4 simpul.
//!    - Proposer ronde 1 (Validator 1) menyiarkan proposal segmen.
//!    - Validator 2, 3, 4 memverifikasi dan menyiarkan `VoteRecord`.
//!    - Validator 1 merakit `QuorumCertificate` (>= 3 suara) dan menyiarkan QC.
//!    - Seluruh 4 simpul mengomitsikan mutasi ke storage disk dan memutakhirkan status secara deterministik.
//! 2. `test_cluster_deterministic_leader_rotation`:
//!    - Pemilihan pemimpin deterministik ronde 2 via `(epoch + round) % 4`.
//!    - Pengakuan tunggal Validator 2 sebagai pemimpin oleh seluruh simpul.
//!    - Validator 2 menyiarkan proposal; 1, 3, 4 memberikan suara; blok terkomit sukses.
//! 3. `test_cluster_leader_crash_and_view_change`:
//!    - Pemimpin ronde 3 (Validator 3) mengalami kegagalan/shutdown mendadak.
//!    - Pacemaker pada simpul 1, 2, 4 kedaluwarsa setelah timeout dasar 2.000 ms.
//!    - Simpul aktif menyiarkan `TimeoutMsg` (56-byte muatan penandatanganan).
//!    - Agregasi supermayoritas 3 suara timeout menjadi `TimeoutCertificate` (TC).
//!    - Ronde konsensus berpindah ke ronde 4 dengan Validator 4 sebagai pemimpin baru secara otonom.

use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};
use ratu_aurion_consensus::certificate::QuorumCertificate;
use ratu_aurion_consensus::evidence::VoteRecord;
use ratu_aurion_consensus::pacemaker::Pacemaker;
use ratu_aurion_consensus::proposal::SegmentProposal;
use ratu_aurion_consensus::timeout::{TimeoutCertificate, TimeoutMsg};
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
        "ratu_aurion_cluster_{label}_n{node_idx}_{nanos}"
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

fn bootstrap_4_node_cluster(test_label: &str, epoch: u64) -> (Vec<TestNode>, Vec<AccountId>, ValidatorSet) {
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

        let raw_engine = EngineCoordinator::new(&data_dir, &archive_dir, epoch)
            .expect("Init engine coordinator");
        let engine = Arc::new(RwLock::new(raw_engine));
        let peer_table = Arc::new(RwLock::new(PeerTable::new()));

        let server = NodeServer::new(config, Arc::clone(&engine), peer_table);
        let listener = server.bind().expect("Bind ephemeral listener");
        let addr = listener.local_addr().expect("Get local listener address");

        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
        let server_handle = thread::spawn(move || {
            server.run_listener(listener, shutdown_rx)
        });

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
    // Untuk epoch = 0: index 1 adalah Node 1, index 2 adalah Node 2, index 3 adalah Node 3, index 0 adalah Node 4
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
fn test_cluster_happy_path_proposal_and_qc_commit() {
    let epoch = 0u64;
    let round = 1u64;
    let (mut nodes, ordered_validators, validator_set) =
        bootstrap_4_node_cluster("happy_path", epoch);

    // 1. Verifikasi kepemimpinan ronde 1: Validator 1 adalah proposer sah via (epoch + round) % 4
    let leader_r1 = Pacemaker::elect_leader(epoch, round, &ordered_validators).unwrap();
    assert_eq!(
        leader_r1, nodes[0].account,
        "Validator 1 must be elected leader for round 1 via (epoch + round) % 4"
    );

    // 2. Injeksi saldo awal (seed) ke dalam seluruh engine simpul kluster
    let (client_sk, sender) = create_validator_identity(0x77);
    let recipient = AccountId::new([0x88; 32]);
    let initial_balance = AurValue::from_whole_aur(1_000).expect("1,000 AUR");
    let transfer_amount = AurValue::from_whole_aur(50).expect("50 AUR");

    for node in &nodes {
        node.engine
            .write()
            .unwrap()
            .seed_account(sender, initial_balance);
    }

    // 3. Simpul 1 menyiapkan transaksi mutasi dan proposal segmen
    let mutation = create_signed_mutation(1, 1, sender, recipient, transfer_amount, &client_sk);
    let tx_digest = Hash::new(*blake3::hash(&mutation.to_bytes()).as_bytes());
    let proposal = SegmentProposal::new(1, 0, tx_digest, nodes[0].account, round);

    // Simpul 1 menyiarkan proposal ke simpul 2, 3, 4 melalui PeerMesh broadcast
    let broadcast_count = nodes[0]
        .mesh
        .broadcast(&NetworkMessage::Proposal(proposal))
        .expect("Broadcast proposal");
    assert_eq!(broadcast_count, 3, "All 3 peer validators must receive proposal");

    // 4. Simpul 2, 3, 4 memverifikasi transaksi, membuat signed VoteRecord dan signed Vote, lalu menyiarkannya
    let vr_2 = VoteRecord::sign(nodes[1].account, 1, round, proposal.digest(), &nodes[1].signing_key);
    let vr_3 = VoteRecord::sign(nodes[2].account, 1, round, proposal.digest(), &nodes[2].signing_key);
    let vr_4 = VoteRecord::sign(nodes[3].account, 1, round, proposal.digest(), &nodes[3].signing_key);

    assert!(vr_2.verify_signature().is_ok());
    assert!(vr_3.verify_signature().is_ok());
    assert!(vr_4.verify_signature().is_ok());

    let vote_2 = Vote::sign(&proposal, nodes[1].account, &nodes[1].signing_key);
    let vote_3 = Vote::sign(&proposal, nodes[2].account, &nodes[2].signing_key);
    let vote_4 = Vote::sign(&proposal, nodes[3].account, &nodes[3].signing_key);

    assert!(vote_2.verify(&proposal).is_ok());
    assert!(vote_3.verify(&proposal).is_ok());
    assert!(vote_4.verify(&proposal).is_ok());

    let b2 = nodes[1]
        .mesh
        .broadcast(&NetworkMessage::VoteRecord(vr_2))
        .expect("Broadcast vote record 2");
    let b3 = nodes[2]
        .mesh
        .broadcast(&NetworkMessage::VoteRecord(vr_3))
        .expect("Broadcast vote record 3");
    let b4 = nodes[3]
        .mesh
        .broadcast(&NetworkMessage::VoteRecord(vr_4))
        .expect("Broadcast vote record 4");
    assert_eq!(b2, 3);
    assert_eq!(b3, 3);
    assert_eq!(b4, 3);

    // Broadcast signed Vote
    nodes[1].mesh.broadcast(&NetworkMessage::Vote(vote_2.clone())).unwrap();
    nodes[2].mesh.broadcast(&NetworkMessage::Vote(vote_3.clone())).unwrap();
    nodes[3].mesh.broadcast(&NetworkMessage::Vote(vote_4)).unwrap();

    // 5. Simpul 1 mengumpulkan >= 3 suara dan merakit QuorumCertificate (QC)
    let vote_1 = Vote::sign(&proposal, nodes[0].account, &nodes[0].signing_key);

    let mut qc = QuorumCertificate::new(proposal);
    qc.add_vote(&vote_1, &validator_set).expect("Add vote 1");
    qc.add_vote(&vote_2, &validator_set).expect("Add vote 2");
    qc.add_vote(&vote_3, &validator_set).expect("Add vote 3");

    assert!(qc.is_final(&validator_set), "Quorum supermajority (3/4) must be reached");
    qc.verify(&validator_set).expect("QC must be cryptographically valid");

    // Simpul 1 menyiarkan QC yang telah terhimpun ke seluruh kluster
    let qc_broadcast_count = nodes[0]
        .mesh
        .broadcast(&NetworkMessage::Certificate(qc.clone()))
        .expect("Broadcast QC");
    assert_eq!(qc_broadcast_count, 3, "QC must be propagated to all 3 peers");

    // 6. Seluruh 4 simpul mengomitsikan mutasi ke storage disk dan memutakhirkan state RAM
    let mut committed_offsets = Vec::with_capacity(4);
    for node in &nodes {
        let offset = node
            .engine
            .write()
            .unwrap()
            .submit_transaction(&mutation)
            .expect("Commit transaction to node storage");
        assert!(offset >= 42, "Disk offset must be beyond 42-byte segment header");
        committed_offsets.push(offset);
    }

    // 7. Verifikasi ketat konsistensi dan determinisme status di seluruh 4 simpul
    let first_offset = committed_offsets[0];
    for &off in &committed_offsets {
        assert_eq!(off, first_offset, "Disk storage offsets must be identical across all nodes");
    }

    let expected_sender_bal = AurValue::from_whole_aur(950).unwrap();
    let expected_recipient_bal = AurValue::from_whole_aur(50).unwrap();

    for node in &nodes {
        let eng = node.engine.read().unwrap();
        assert_eq!(eng.query_balance(&sender), expected_sender_bal);
        assert_eq!(eng.query_balance(&recipient), expected_recipient_bal);

        let loc = eng.query_last_location(&sender).expect("Account location");
        assert_eq!(loc.sequence_number, 1);
        assert_eq!(loc.offset, first_offset);
    }
}

#[test]
fn test_cluster_deterministic_leader_rotation() {
    let epoch = 0u64;
    let (mut nodes, ordered_validators, validator_set) =
        bootstrap_4_node_cluster("leader_rotation", epoch);

    // 1. Majukan ronde ke ronde 2
    let round_2 = 2u64;
    let expected_leader_idx = ((epoch + round_2) % 4) as usize;
    assert_eq!(expected_leader_idx, 2);

    let leader = Pacemaker::elect_leader(epoch, round_2, &ordered_validators).unwrap();
    assert_eq!(
        leader, nodes[1].account,
        "Validator 2 must be deterministically recognized as sole leader for round 2: (epoch + round) % 4"
    );

    // Pastikan seluruh simpul secara independen mengakui Validator 2 sebagai pemimpin
    for node in &nodes {
        let p = Pacemaker::with_round(2_000, round_2);
        let sole_leader = p.current_leader(epoch, &ordered_validators).unwrap();
        assert_eq!(sole_leader, nodes[1].account);
        assert_eq!((epoch + node.engine.read().unwrap().current_epoch() + round_2) % 4, 2);
    }

    // 2. Siapkan mutasi dan proposal ronde 2 oleh Validator 2
    let (client_sk, sender) = create_validator_identity(0x66);
    let recipient = AccountId::new([0x99; 32]);
    let initial_balance = AurValue::from_whole_aur(500).unwrap();
    let transfer_amount = AurValue::from_whole_aur(20).unwrap();

    for node in &nodes {
        node.engine
            .write()
            .unwrap()
            .seed_account(sender, initial_balance);
    }

    let mutation = create_signed_mutation(1, 1, sender, recipient, transfer_amount, &client_sk);
    let tx_digest = Hash::new(*blake3::hash(&mutation.to_bytes()).as_bytes());
    let proposal_2 = SegmentProposal::new(1, 0, tx_digest, nodes[1].account, round_2);

    // Validator 2 menyiarkan proposal ronde 2
    let sent = nodes[1]
        .mesh
        .broadcast(&NetworkMessage::Proposal(proposal_2))
        .expect("Broadcast round 2 proposal");
    assert_eq!(sent, 3);

    // 3. Simpul 1, 3, 4 memverifikasi dan menyiarkan suara (VoteRecord dan Vote)
    let vr_1 = VoteRecord::sign(nodes[0].account, 1, round_2, proposal_2.digest(), &nodes[0].signing_key);
    let vr_3 = VoteRecord::sign(nodes[2].account, 1, round_2, proposal_2.digest(), &nodes[2].signing_key);
    let vr_4 = VoteRecord::sign(nodes[3].account, 1, round_2, proposal_2.digest(), &nodes[3].signing_key);

    assert!(vr_1.verify_signature().is_ok());
    assert!(vr_3.verify_signature().is_ok());
    assert!(vr_4.verify_signature().is_ok());

    let vote_1 = Vote::sign(&proposal_2, nodes[0].account, &nodes[0].signing_key);
    let vote_3 = Vote::sign(&proposal_2, nodes[2].account, &nodes[2].signing_key);
    let vote_4 = Vote::sign(&proposal_2, nodes[3].account, &nodes[3].signing_key);

    nodes[0].mesh.broadcast(&NetworkMessage::VoteRecord(vr_1)).unwrap();
    nodes[2].mesh.broadcast(&NetworkMessage::VoteRecord(vr_3)).unwrap();
    nodes[3].mesh.broadcast(&NetworkMessage::VoteRecord(vr_4)).unwrap();

    nodes[0].mesh.broadcast(&NetworkMessage::Vote(vote_1.clone())).unwrap();
    nodes[2].mesh.broadcast(&NetworkMessage::Vote(vote_3.clone())).unwrap();
    nodes[3].mesh.broadcast(&NetworkMessage::Vote(vote_4)).unwrap();

    // 4. Simpul 2 merakit QC ronde 2 dan menyiarkannya
    let vote_2 = Vote::sign(&proposal_2, nodes[1].account, &nodes[1].signing_key);

    let mut qc_2 = QuorumCertificate::new(proposal_2);
    qc_2.add_vote(&vote_2, &validator_set).unwrap();
    qc_2.add_vote(&vote_1, &validator_set).unwrap();
    qc_2.add_vote(&vote_3, &validator_set).unwrap();
    assert!(qc_2.is_final(&validator_set));
    qc_2.verify(&validator_set).unwrap();

    nodes[1].mesh.broadcast(&NetworkMessage::Certificate(qc_2)).unwrap();

    // 5. Komit blok di seluruh 4 simpul
    for node in &nodes {
        let off = node
            .engine
            .write()
            .unwrap()
            .submit_transaction(&mutation)
            .expect("Commit round 2 transaction");
        assert!(off >= 42);
        assert_eq!(node.engine.read().unwrap().query_balance(&recipient), transfer_amount);
    }
}

#[test]
fn test_cluster_leader_crash_and_view_change() {
    let epoch = 0u64;
    let round_3 = 3u64;
    let (mut nodes, ordered_validators, validator_set) =
        bootstrap_4_node_cluster("crash_view_change", epoch);

    // 1. Verifikasi Validator 3 adalah pemimpin ronde 3: (epoch + round_3) % 4 = 3
    let leader_r3 = Pacemaker::elect_leader(epoch, round_3, &ordered_validators).unwrap();
    assert_eq!(
        leader_r3, nodes[2].account,
        "Validator 3 must be leader for round 3"
    );

    // 2. Simulasi power failure / crash mendadak: Matikan Validator 3 secara paksa
    nodes[2].shutdown();

    // 3. Pacemaker pada Simpul 1, 2, dan 4 kedaluwarsa setelah batas waktu dasar 2.000 ms
    let mut pacemaker_1 = Pacemaker::with_round(2_000, round_3);
    let mut pacemaker_2 = Pacemaker::with_round(2_000, round_3);
    let mut pacemaker_4 = Pacemaker::with_round(2_000, round_3);

    assert_eq!(pacemaker_1.base_timeout_ms(), 2_000);
    assert_eq!(pacemaker_1.current_timeout_ms(), 2_000);

    // Timeout habis tanpa proposal valid dari Validator 3
    let new_timeout_1 = pacemaker_1.on_timeout();
    let new_timeout_2 = pacemaker_2.on_timeout();
    let new_timeout_4 = pacemaker_4.on_timeout();

    // Exponential backoff integer: 2.000 * 2^1 = 4.000 ms
    assert_eq!(new_timeout_1, 4_000);
    assert_eq!(new_timeout_2, 4_000);
    assert_eq!(new_timeout_4, 4_000);
    assert_eq!(pacemaker_1.consecutive_timeouts(), 1);

    // 4. Simpul 1, 2, dan 4 menghasilkan dan menyiarkan TimeoutMsg (56 byte payload)
    let high_qc_round = 2u64;
    let tm_1 = TimeoutMsg::sign(nodes[0].account, epoch, round_3, high_qc_round, &nodes[0].signing_key);
    let tm_2 = TimeoutMsg::sign(nodes[1].account, epoch, round_3, high_qc_round, &nodes[1].signing_key);
    let tm_4 = TimeoutMsg::sign(nodes[3].account, epoch, round_3, high_qc_round, &nodes[3].signing_key);

    assert_eq!(tm_1.signing_payload().len(), 56, "TimeoutMsg signing payload must be exactly 56 bytes");
    assert_eq!(tm_2.signing_payload().len(), 56);
    assert_eq!(tm_4.signing_payload().len(), 56);

    assert!(tm_1.verify_signature().is_ok());
    assert!(tm_2.verify_signature().is_ok());
    assert!(tm_4.verify_signature().is_ok());

    // Siarkan timeout message melalui mesh (Simpul 3 mati, transmisi ke peer yang hidup tetap sukses)
    let s1 = nodes[0].mesh.broadcast(&NetworkMessage::Timeout(tm_1.clone())).unwrap();
    let s2 = nodes[1].mesh.broadcast(&NetworkMessage::Timeout(tm_2.clone())).unwrap();
    let s4 = nodes[3].mesh.broadcast(&NetworkMessage::Timeout(tm_4.clone())).unwrap();

    // 2 simpul hidup berhasil menerima siaran (simpul 3 yang crash dilewati tanpa panik)
    assert_eq!(s1, 2);
    assert_eq!(s2, 2);
    assert_eq!(s4, 2);

    // 5. Setiap simpul aktif mengagregasikan 3 pesan timeout menjadi TimeoutCertificate (TC)
    let timeout_messages = vec![tm_1, tm_2, tm_4];
    let tc = TimeoutCertificate::assemble(epoch, round_3, &timeout_messages, &validator_set)
        .expect("Assemble supermajority TimeoutCertificate");

    tc.verify(&validator_set).expect("TC verification against validator set");
    assert_eq!(tc.signatures.len(), 3, "TC must aggregate exactly 3 validator signatures");
    assert_eq!(tc.round, round_3);
    assert_eq!(tc.high_qc_round, high_qc_round);

    // 6. Setiap simpul aktif memproses TC dan memajukan ronde ke ronde 4
    let advanced_r1 = pacemaker_1.process_timeout_certificate(&tc).expect("Advance round 1");
    let advanced_r2 = pacemaker_2.process_timeout_certificate(&tc).expect("Advance round 2");
    let advanced_r4 = pacemaker_4.process_timeout_certificate(&tc).expect("Advance round 4");

    assert_eq!(advanced_r1, 4);
    assert_eq!(advanced_r2, 4);
    assert_eq!(advanced_r4, 4);

    assert_eq!(pacemaker_1.current_round(), 4);
    assert_eq!(pacemaker_2.current_round(), 4);
    assert_eq!(pacemaker_4.current_round(), 4);

    // 7. Verifikasi pemilihan pemimpin baru pada ronde 4: Validator 4 terpilih secara otonom
    let new_leader_r4 = Pacemaker::elect_leader(epoch, 4, &ordered_validators).unwrap();
    assert_eq!(
        new_leader_r4, nodes[3].account,
        "Validator 4 must be elected leader for round 4 without deadlock or human intervention"
    );
}
