#![forbid(unsafe_code)]

//! Suite pengujian integrasi untuk tiket GATEWAY-RPC-01: JSON-RPC & WebSocket Gateway Interface.
//!
//! Menguji kepatuhan JSON-RPC 2.0 deterministik, batasan payload 64 KB,
//! handshake RFC 6455 in-tree, dan langganan WebSocket real-time mutasi baru.

use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};
use ratu_aurion_engine::coordinator::EngineCoordinator;
use ratu_aurion_network::error::NetworkError;
use ratu_aurion_network::peer::PeerTable;
use ratu_aurion_node::config::NodeConfig;
use ratu_aurion_node::rpc::{hex_encode, JsonValue};
use ratu_aurion_node::server::NodeServer;
use ratu_aurion_node::ws::{compute_websocket_accept, read_ws_frame, WsFrame};
use ratu_aurion_primitives::crypto::{AccountId, Signature};
use ratu_aurion_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER};
use ratu_aurion_primitives::value::AurValue;

fn unique_test_dirs(label: &str) -> (PathBuf, PathBuf) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("ratu_aurion_rpc_{label}_{nanos}"));
    let data_dir = base.join("data");
    let archive_dir = base.join("archive");
    (data_dir, archive_dir)
}

struct TestRpcNode {
    server: NodeServer,
    rpc_addr: SocketAddr,
    shutdown_tx: Option<Sender<()>>,
    rpc_handle: Option<JoinHandle<Result<(), NetworkError>>>,
    data_dir: PathBuf,
}

impl Drop for TestRpcNode {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.rpc_handle.take() {
            let _ = handle.join();
        }
        if let Some(parent) = self.data_dir.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

fn bootstrap_test_rpc_node(label: &str) -> TestRpcNode {
    let (data_dir, archive_dir) = unique_test_dirs(label);
    let mut config = NodeConfig::default_test_config();
    config.data_dir = data_dir.clone();
    config.archive_dir = archive_dir.clone();
    config.listen_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);
    config.rpc_addr = Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0));

    let engine = Arc::new(RwLock::new(
        EngineCoordinator::new(&data_dir, &archive_dir, 1).expect("Engine init"),
    ));
    let peer_table = Arc::new(RwLock::new(PeerTable::new()));
    let server = NodeServer::new(config, engine, peer_table);

    let rpc_listener = server
        .bind_rpc()
        .expect("Bind rpc")
        .expect("RPC addr configured");
    let rpc_addr = rpc_listener.local_addr().expect("Local RPC addr");

    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    let srv_clone = server.clone();
    let rpc_handle = thread::spawn(move || {
        srv_clone.run_rpc_listener(rpc_listener, shutdown_rx)
    });

    TestRpcNode {
        server,
        rpc_addr,
        shutdown_tx: Some(shutdown_tx),
        rpc_handle: Some(rpc_handle),
        data_dir,
    }
}

fn post_rpc(addr: SocketAddr, json_body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("Connect to RPC");
    stream.set_read_timeout(Some(Duration::from_millis(2000))).unwrap();
    stream.set_write_timeout(Some(Duration::from_millis(2000))).unwrap();

    let request = format!(
        "POST / HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        addr,
        json_body.len(),
        json_body
    );
    stream.write_all(request.as_bytes()).expect("Write request");
    stream.flush().expect("Flush request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("Read response");

    let status_code = if let Some(first_line) = response.lines().next() {
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() >= 2 {
            parts[1].parse::<u16>().unwrap_or(0)
        } else {
            0
        }
    } else {
        0
    };

    let body = if let Some(pos) = response.find("\r\n\r\n") {
        response[pos + 4..].to_string()
    } else {
        response
    };

    (status_code, body)
}

fn node_encode_client_frame(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mask = [0x12, 0x34, 0x56, 0x78];
    let mut frame = Vec::new();
    frame.push(0x81); // FIN + Text
    if bytes.len() < 126 {
        frame.push(0x80 | (bytes.len() as u8));
    } else {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    for (i, &b) in bytes.iter().enumerate() {
        frame.push(b ^ mask[i % 4]);
    }
    frame
}

#[test]
fn test_rpc_chain_id_and_height() {
    let node = bootstrap_test_rpc_node("chain_id_height");

    // 1. Send HTTP POST request calling aur_chainId
    let (status, body) = post_rpc(
        node.rpc_addr,
        r#"{"jsonrpc":"2.0","method":"aur_chainId","id":1}"#,
    );
    assert_eq!(status, 200);
    let json = JsonValue::parse(&body).expect("Parse chainId response");
    assert_eq!(json.get("jsonrpc").unwrap().as_str().unwrap(), "2.0");
    assert_eq!(json.get("result").unwrap().as_str().unwrap(), "0x52415552");
    assert_eq!(json.get("id").unwrap().as_u64().unwrap(), 1);

    // 2. Call aur_blockHeight; assert returns current epoch and committed sequence number
    let (status, body) = post_rpc(
        node.rpc_addr,
        r#"{"jsonrpc":"2.0","method":"aur_blockHeight","id":1}"#,
    );
    assert_eq!(status, 200);
    let json = JsonValue::parse(&body).expect("Parse blockHeight response");
    assert_eq!(json.get("jsonrpc").unwrap().as_str().unwrap(), "2.0");
    let result = json.get("result").unwrap();
    assert_eq!(result.get("epoch").unwrap().as_u64().unwrap(), 1);
    assert_eq!(result.get("sequence_number").unwrap().as_u64().unwrap(), 0);
    assert_eq!(json.get("id").unwrap().as_u64().unwrap(), 1);
}

#[test]
fn test_rpc_get_balance_and_account_location() {
    let node = bootstrap_test_rpc_node("balance_location");

    // Seed an account with 100 AUR (100 * 10^10 = 1_000_000_000_000 atomic units)
    let mut seed = [0u8; 32];
    seed[0] = 0xAA;
    let alice_key = SigningKey::from_bytes(&seed);
    let alice_id = AccountId::new(alice_key.verifying_key().to_bytes());
    let alice_hex = hex_encode(alice_id.as_bytes());

    let initial_balance = AurValue::from_atomic(1_000_000_000_000);
    node.server
        .engine
        .write()
        .unwrap()
        .seed_account(alice_id, initial_balance);

    // Call aur_getBalance with hex address parameter; assert returns exact atomic unit representation
    let req = format!(
        r#"{{"jsonrpc":"2.0","method":"aur_getBalance","params":["0x{}"],"id":1}}"#,
        alice_hex
    );
    let (status, body) = post_rpc(node.rpc_addr, &req);
    assert_eq!(status, 200);
    let json = JsonValue::parse(&body).expect("Parse getBalance response");
    assert_eq!(json.get("jsonrpc").unwrap().as_str().unwrap(), "2.0");
    assert_eq!(json.get("result").unwrap().as_str().unwrap(), "1000000000000");
    assert_eq!(json.get("id").unwrap().as_u64().unwrap(), 1);

    // Call aur_getAccountLocation; assert returns { "epoch": 1, "segment_index": 0, "offset": ..., "sequence_number": ... }
    let req_loc = format!(
        r#"{{"jsonrpc":"2.0","method":"aur_getAccountLocation","params":["0x{}"],"id":1}}"#,
        alice_hex
    );
    let (status, body) = post_rpc(node.rpc_addr, &req_loc);
    assert_eq!(status, 200);
    let json = JsonValue::parse(&body).expect("Parse getAccountLocation response");
    let loc = json.get("result").unwrap();
    assert_eq!(loc.get("epoch").unwrap().as_u64().unwrap(), 1);
    assert_eq!(loc.get("segment_index").unwrap().as_u64().unwrap(), 0);
    assert!(loc.get("offset").is_some());
    assert!(loc.get("sequence_number").is_some());
}

#[test]
fn test_rpc_send_raw_transaction_success_and_error() {
    let node = bootstrap_test_rpc_node("send_raw_tx");

    let mut seed_alice = [0u8; 32];
    seed_alice[0] = 0xA1;
    let alice_key = SigningKey::from_bytes(&seed_alice);
    let alice = AccountId::new(alice_key.verifying_key().to_bytes());

    let bob = AccountId::new([0xB2; 32]);

    // Seed Alice with 100 AUR
    node.server
        .engine
        .write()
        .unwrap()
        .seed_account(alice, AurValue::from_atomic(1_000_000_000_000));

    // Construct valid signed MutationRecord (161 bytes), encode to hex string
    let mut tx = MutationRecord {
        epoch: 1,
        sequence_number: 1,
        record_kind: RECORD_KIND_TRANSFER,
        sender: alice,
        recipient: bob,
        amount: AurValue::from_atomic(300_000_000_000), // 30 AUR
        signature: Signature::ZERO,
    };
    let payload = ratu_aurion_engine::validator::compute_signing_payload(&tx);
    let sig = alice_key.sign(&payload);
    tx.signature = Signature::new(sig.to_bytes());

    let tx_bytes = tx.to_bytes();
    let tx_hex = hex_encode(&tx_bytes);

    // Call aur_sendRawTransaction via HTTP POST; assert returns {"jsonrpc":"2.0","result":{"disk_offset":...,"sequence_number":1},"id":2}
    let req = format!(
        r#"{{"jsonrpc":"2.0","method":"aur_sendRawTransaction","params":["0x{}"],"id":2}}"#,
        tx_hex
    );
    let (status, body) = post_rpc(node.rpc_addr, &req);
    assert_eq!(status, 200);
    let json = JsonValue::parse(&body).expect("Parse sendRawTransaction response");
    assert_eq!(json.get("jsonrpc").unwrap().as_str().unwrap(), "2.0");
    assert_eq!(json.get("id").unwrap().as_u64().unwrap(), 2);
    let result = json.get("result").unwrap();
    assert!(result.get("disk_offset").unwrap().as_u64().unwrap() >= 42);
    assert_eq!(result.get("sequence_number").unwrap().as_u64().unwrap(), 1);

    // Query aur_getBalance to verify recipient balance reflects transfer
    let bob_hex = hex_encode(bob.as_bytes());
    let req_bob = format!(
        r#"{{"jsonrpc":"2.0","method":"aur_getBalance","params":["0x{}"],"id":3}}"#,
        bob_hex
    );
    let (status, body) = post_rpc(node.rpc_addr, &req_bob);
    assert_eq!(status, 200);
    let json_bob = JsonValue::parse(&body).expect("Parse bob balance response");
    assert_eq!(
        json_bob.get("result").unwrap().as_str().unwrap(),
        "300000000000"
    );

    // Also verify Alice balance decreased to 700_000_000_000
    let alice_hex = hex_encode(alice.as_bytes());
    let req_alice = format!(
        r#"{{"jsonrpc":"2.0","method":"aur_getBalance","params":["0x{}"],"id":4}}"#,
        alice_hex
    );
    let (_, body_alice) = post_rpc(node.rpc_addr, &req_alice);
    let json_alice = JsonValue::parse(&body_alice).expect("Parse alice balance response");
    assert_eq!(
        json_alice.get("result").unwrap().as_str().unwrap(),
        "700000000000"
    );

    // Re-send exact same transaction (stale sequence number); assert returns JSON-RPC error -32000
    let (status, body_stale) = post_rpc(node.rpc_addr, &req);
    assert_eq!(status, 200);
    let json_stale = JsonValue::parse(&body_stale).expect("Parse stale tx error response");
    assert_eq!(json_stale.get("jsonrpc").unwrap().as_str().unwrap(), "2.0");
    assert_eq!(json_stale.get("id").unwrap().as_u64().unwrap(), 2);
    let err = json_stale.get("error").unwrap();
    assert_eq!(err.get("code").unwrap().as_i128().unwrap(), -32000);
    assert!(err
        .get("message")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("Stale"));
}

#[test]
fn test_rpc_reject_invalid_json_and_oversized_payload() {
    let node = bootstrap_test_rpc_node("reject_invalid");

    // 1. Send malformed JSON body; assert returns parse error -32700
    let (status, body) = post_rpc(node.rpc_addr, r#"{"jsonrpc":"2.0", invalid_json"#);
    assert_eq!(status, 200);
    let json = JsonValue::parse(&body).expect("Parse error response");
    assert_eq!(json.get("jsonrpc").unwrap().as_str().unwrap(), "2.0");
    let err = json.get("error").unwrap();
    assert_eq!(err.get("code").unwrap().as_i128().unwrap(), -32700);

    // 2. Send payload exceeding 64 KB; assert connection closed or returns -32600
    let oversized_len = 65_537; // > 65_536 bytes
    let mut stream = TcpStream::connect(node.rpc_addr).expect("Connect to RPC");
    stream
        .set_read_timeout(Some(Duration::from_millis(2000)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_millis(2000)))
        .unwrap();

    let oversized_header = format!(
        "POST / HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        node.rpc_addr, oversized_len
    );
    stream
        .write_all(oversized_header.as_bytes())
        .expect("Write header");
    stream.flush().expect("Flush header");

    let mut resp = String::new();
    let _ = stream.read_to_string(&mut resp);

    // Assert returns -32600 error or connection closed
    if !resp.is_empty() {
        if let Some(pos) = resp.find("\r\n\r\n") {
            let body = &resp[pos + 4..];
            let json = JsonValue::parse(body).expect("Parse oversized error response");
            let err = json.get("error").unwrap();
            assert_eq!(err.get("code").unwrap().as_i128().unwrap(), -32600);
        }
    }
}

#[test]
fn test_websocket_handshake_and_subscription_stream() {
    let node = bootstrap_test_rpc_node("ws_stream");

    // 1. Connect client socket to gateway endpoint /ws
    let mut stream = TcpStream::connect(node.rpc_addr).expect("Connect to WS endpoint");
    stream
        .set_read_timeout(Some(Duration::from_millis(2000)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_millis(2000)))
        .unwrap();

    // 2. Perform RFC 6455 handshake (Upgrade: websocket, Sec-WebSocket-Key)
    let client_key = "dGhlIHNhbXBsZSBub25jZQ==";
    let expected_accept = compute_websocket_accept(client_key);

    let handshake_req = format!(
        "GET /ws HTTP/1.1\r\n\
         Host: {}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {}\r\n\
         Sec-WebSocket-Version: 13\r\n\
         \r\n",
        node.rpc_addr, client_key
    );
    stream
        .write_all(handshake_req.as_bytes())
        .expect("Send WS handshake");
    stream.flush().expect("Flush handshake");

    // Read handshake response
    let mut resp_buf = [0u8; 1024];
    let n = stream
        .read(&mut resp_buf)
        .expect("Read handshake response");
    let resp_str = String::from_utf8_lossy(&resp_buf[..n]);

    // Assert server returns 101 Switching Protocols with valid Sec-WebSocket-Accept
    assert!(
        resp_str.contains("101 Switching Protocols"),
        "Got: {resp_str}"
    );
    assert!(
        resp_str.contains(&format!("Sec-WebSocket-Accept: {expected_accept}")),
        "Got: {resp_str}"
    );

    // 3. Send subscription request {"jsonrpc":"2.0","method":"aur_subscribe","params":["newMutations"],"id":1}
    let sub_req = r#"{"jsonrpc":"2.0","method":"aur_subscribe","params":["newMutations"],"id":1}"#;
    let frame_bytes = node_encode_client_frame(sub_req);
    stream.write_all(&frame_bytes).expect("Send sub frame");
    stream.flush().expect("Flush sub frame");

    // Read subscription ack frame
    match read_ws_frame(&mut stream).expect("Read subscription reply frame") {
        WsFrame::Text(text) => {
            let json = JsonValue::parse(&text).expect("Parse sub ack");
            assert_eq!(json.get("result").unwrap().as_u64().unwrap(), 1);
            assert_eq!(json.get("id").unwrap().as_u64().unwrap(), 1);
        }
        other => panic!("Expected Text frame for sub ack, got: {other:?}"),
    }

    // 4. Submit transaction on node
    let mut seed = [0u8; 32];
    seed[0] = 0x55;
    let sender_key = SigningKey::from_bytes(&seed);
    let sender = AccountId::new(sender_key.verifying_key().to_bytes());
    let recipient = AccountId::new([0x66; 32]);

    node.server
        .engine
        .write()
        .unwrap()
        .seed_account(sender, AurValue::from_atomic(500_000_000_000));

    let mut tx = MutationRecord {
        epoch: 1,
        sequence_number: 1,
        record_kind: RECORD_KIND_TRANSFER,
        sender,
        recipient,
        amount: AurValue::from_atomic(100_000_000_000),
        signature: Signature::ZERO,
    };
    let payload = ratu_aurion_engine::validator::compute_signing_payload(&tx);
    let sig = sender_key.sign(&payload);
    tx.signature = Signature::new(sig.to_bytes());

    let offset = node.server.submit_transaction(&tx).expect("Submit tx");
    assert!(offset >= 42);

    // 5. Assert client receives WebSocket text frame containing committed mutation details
    match read_ws_frame(&mut stream).expect("Read mutation notification frame") {
        WsFrame::Text(text) => {
            let json = JsonValue::parse(&text).expect("Parse mutation notification");
            assert_eq!(
                json.get("method").unwrap().as_str().unwrap(),
                "aur_subscription"
            );
            let params = json.get("params").unwrap();
            let result = params.get("result").unwrap();
            assert_eq!(result.get("epoch").unwrap().as_u64().unwrap(), 1);
            assert_eq!(result.get("sequence_number").unwrap().as_u64().unwrap(), 1);
            assert_eq!(
                result.get("disk_offset").unwrap().as_u64().unwrap(),
                offset
            );
        }
        other => panic!("Expected Text frame for mutation notif, got: {other:?}"),
    }
}
