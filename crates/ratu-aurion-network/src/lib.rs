#![forbid(unsafe_code)]

//! # Axiom Network
//!
//! Modul komunikasi data P2P, serialization framing biner deterministik,
//! manajemen koneksi peer node, dan Full-Mesh coordinator untuk protokol Axiom.

pub mod codec;
pub mod error;
pub mod framed;
pub mod mesh;
pub mod message;
pub mod peer;
pub mod rate_limiter;

pub use codec::*;
pub use error::*;
pub use framed::*;
pub use mesh::*;
pub use message::*;
pub use peer::*;
pub use rate_limiter::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
    use std::thread;

    use ratu_aurion_consensus::certificate::QuorumCertificate;
    use ratu_aurion_consensus::evidence::VoteRecord;
    use ratu_aurion_consensus::proposal::SegmentProposal;
    use ratu_aurion_consensus::timeout::{TimeoutCertificate, TimeoutMsg};
    use ratu_aurion_consensus::vote::Vote;
    use ratu_aurion_primitives::crypto::{AccountId, Hash, Signature};
    use ratu_aurion_primitives::framing::HEADER_SIZE;

    #[test]
    fn test_network_message_roundtrip_all_variants() {
        let proposer = AccountId::new([0x01; 32]);
        let val1 = AccountId::new([0x02; 32]);
        let val2 = AccountId::new([0x03; 32]);
        let digest = Hash::new([0xaa; 32]);
        let sig1 = Signature::new([0x11; 64]);
        let sig2 = Signature::new([0x22; 64]);

        let proposal = SegmentProposal::new(1, 0, digest, proposer, 1);

        // 1. Proposal Message
        let msg_proposal = NetworkMessage::Proposal(proposal);
        let enc_proposal = encode_message(&msg_proposal).expect("Encode proposal");
        let dec_proposal = decode_message(&enc_proposal).expect("Decode proposal");
        assert_eq!(msg_proposal, dec_proposal);

        // 2. Vote Message
        let vote = Vote::new(digest, val1, sig1);
        let msg_vote = NetworkMessage::Vote(vote);
        let enc_vote = encode_message(&msg_vote).expect("Encode vote");
        let dec_vote = decode_message(&enc_vote).expect("Decode vote");
        assert_eq!(msg_vote, dec_vote);

        // 3. QuorumCertificate Message
        let mut signatures = BTreeMap::new();
        signatures.insert(val1, sig1);
        signatures.insert(val2, sig2);
        let cert = QuorumCertificate {
            proposal,
            signatures,
            accumulated_weight: 75,
        };
        let msg_cert = NetworkMessage::Certificate(cert);
        let enc_cert = encode_message(&msg_cert).expect("Encode certificate");
        let dec_cert = decode_message(&enc_cert).expect("Decode certificate");
        assert_eq!(msg_cert, dec_cert);

        // 4. SyncRequest Message
        let msg_sync_req = NetworkMessage::SyncRequest {
            epoch: 5,
            segment_index: 2,
            from_offset: 4096,
        };
        let enc_sync_req = encode_message(&msg_sync_req).expect("Encode sync req");
        let dec_sync_req = decode_message(&enc_sync_req).expect("Decode sync req");
        assert_eq!(msg_sync_req, dec_sync_req);

        // 5. SyncChunk Message
        let chunk_data = vec![0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0xba, 0xbe];
        let msg_sync_chunk = NetworkMessage::SyncChunk {
            epoch: 5,
            segment_index: 2,
            offset: 4096,
            data: chunk_data,
        };
        let enc_sync_chunk = encode_message(&msg_sync_chunk).expect("Encode sync chunk");
        let dec_sync_chunk = decode_message(&enc_sync_chunk).expect("Decode sync chunk");
        assert_eq!(msg_sync_chunk, dec_sync_chunk);

        // 6. TxSubmit Message
        let record = ratu_aurion_primitives::record::MutationRecord {
            epoch: 1,
            sequence_number: 1,
            record_kind: ratu_aurion_primitives::record::RECORD_KIND_TRANSFER,
            sender: val1,
            recipient: val2,
            amount: ratu_aurion_primitives::value::AurValue::from_atomic(10_000_000_000),
            signature: sig1,
        };
        let msg_tx = NetworkMessage::TxSubmit(record);
        let enc_tx = encode_message(&msg_tx).expect("Encode tx submit");
        let dec_tx = decode_message(&enc_tx).expect("Decode tx submit");
        assert_eq!(msg_tx, dec_tx);

        // 7. TxResult Message
        let msg_res = NetworkMessage::TxResult {
            success: true,
            offset: 1024,
            message: "Committed".to_string(),
        };
        let enc_res = encode_message(&msg_res).expect("Encode tx result");
        let dec_res = decode_message(&enc_res).expect("Decode tx result");
        assert_eq!(msg_res, dec_res);

        // 8. VoteRecord Message
        let vr = VoteRecord::new(val1, 1, 1, digest, sig1);
        let msg_vr = NetworkMessage::VoteRecord(vr);
        let enc_vr = encode_message(&msg_vr).expect("Encode vote record");
        let dec_vr = decode_message(&enc_vr).expect("Decode vote record");
        assert_eq!(msg_vr, dec_vr);

        // 9. Timeout Message
        let t_msg = TimeoutMsg::new(val1, 1, 1, 0, sig1);
        let msg_t = NetworkMessage::Timeout(t_msg);
        let enc_t = encode_message(&msg_t).expect("Encode timeout");
        let dec_t = decode_message(&enc_t).expect("Decode timeout");
        assert_eq!(msg_t, dec_t);

        // 10. TimeoutCertificate Message
        let tc = TimeoutCertificate::new(1, 1, 0, vec![(val1, sig1), (val2, sig2)]);
        let msg_tc = NetworkMessage::TimeoutCertificate(tc);
        let enc_tc = encode_message(&msg_tc).expect("Encode timeout certificate");
        let dec_tc = decode_message(&enc_tc).expect("Decode timeout certificate");
        assert_eq!(msg_tc, dec_tc);
    }

    #[test]
    fn test_peer_mesh_lifecycle() {
        let local_account = AccountId::new([0x01; 32]);
        let peer_account = AccountId::new([0x02; 32]);

        let listener = TcpListener::bind("127.0.0.1:0").expect("Bind ephemeral listener");
        let listen_addr = listener.local_addr().expect("Local address");

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("Accept incoming connection");
            let incoming = read_message(&mut stream).expect("Read message");
            match incoming {
                NetworkMessage::Vote(vote) => {
                    let reply = NetworkMessage::TxResult {
                        success: true,
                        offset: 42,
                        message: "Vote acknowledged".to_string(),
                    };
                    write_message(&mut stream, &reply).expect("Write reply");
                    vote
                }
                _ => panic!("Unexpected message variant received"),
            }
        });

        let mut mesh = PeerMesh::new(local_account);
        assert_eq!(mesh.connected_peer_count(), 0);

        mesh.connect_peer(peer_account, listen_addr)
            .expect("Connect to peer");
        assert_eq!(mesh.connected_peer_count(), 1);
        assert!(mesh.has_peer(&peer_account));

        // Penolakan duplikasi
        let dup = mesh.connect_peer(peer_account, listen_addr);
        assert!(matches!(dup, Err(NetworkError::PeerAlreadyExists)));

        let vote = Vote::new(
            Hash::new([0xbb; 32]),
            local_account,
            Signature::new([0x99; 64]),
        );
        let vote_msg = NetworkMessage::Vote(vote.clone());

        mesh.send_to(&peer_account, &vote_msg)
            .expect("Send directed message");

        let reply = mesh
            .receive_from(&peer_account)
            .expect("Receive reply from peer");
        match reply {
            NetworkMessage::TxResult { success, offset, .. } => {
                assert!(success);
                assert_eq!(offset, 42);
            }
            _ => panic!("Expected TxResult"),
        }

        let received_vote = handle.join().expect("Join server thread");
        assert_eq!(received_vote, vote);

        mesh.disconnect_peer(&peer_account);
        assert_eq!(mesh.connected_peer_count(), 0);
        assert!(!mesh.has_peer(&peer_account));
    }

    #[test]
    fn test_packet_rejection_checksum_mismatch() {
        let msg = NetworkMessage::SyncRequest {
            epoch: 1,
            segment_index: 0,
            from_offset: 42,
        };
        let mut packet = encode_message(&msg).expect("Encode message");

        // Manipulasi 1 byte pada isi payload setelah FrameHeader
        packet[HEADER_SIZE + 2] ^= 0xff;

        let result = decode_message(&packet);
        assert!(matches!(result, Err(NetworkError::ChecksumMismatch)));
    }

    #[test]
    fn test_packet_rejection_invalid_magic() {
        let msg = NetworkMessage::SyncRequest {
            epoch: 1,
            segment_index: 0,
            from_offset: 42,
        };
        let mut packet = encode_message(&msg).expect("Encode message");

        // Rusak magic byte pertama pada FrameHeader
        packet[0] = b'X';

        let result = decode_message(&packet);
        assert!(matches!(result, Err(NetworkError::FramingError(_))));
    }

    #[test]
    fn test_peer_table_lifecycle() {
        let mut table = PeerTable::new();
        assert!(table.is_empty());

        let peer1_id = PeerId::new(AccountId::new([0x10; 32]));
        let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8001);
        let peer1 = PeerInfo::new(peer1_id, addr1, 1000, 1);

        let peer2_id = PeerId::new(AccountId::new([0x20; 32]));
        let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8002);
        let peer2 = PeerInfo::new(peer2_id, addr2, 1005, 1);

        // 1. Penambahan peer
        table.insert(peer1.clone()).expect("Insert peer1");
        table.insert(peer2.clone()).expect("Insert peer2");
        assert_eq!(table.len(), 2);
        assert!(!table.is_empty());

        // 2. Penolakan duplikasi peer yang sama
        let dup_err = table.insert(peer1.clone());
        assert!(matches!(dup_err, Err(NetworkError::PeerAlreadyExists)));

        // 3. Pencarian peer
        let retrieved = table.get(&peer1_id).expect("Get peer1");
        assert_eq!(retrieved.address, addr1);
        assert_eq!(retrieved.last_seen_epoch, 1);

        // 4. Pembaruan last seen epoch
        table
            .update_last_seen(&peer1_id, 5)
            .expect("Update last seen");
        assert_eq!(table.get(&peer1_id).unwrap().last_seen_epoch, 5);

        // 5. Penghapusan peer
        let removed = table.remove(&peer1_id).expect("Remove peer1");
        assert_eq!(removed.peer_id, peer1_id);
        assert_eq!(table.len(), 1);
        assert!(!table.contains(&peer1_id));
        assert!(table.contains(&peer2_id));
    }
}
