//! Modul enkoder dan dekoder biner deterministik berbasis FrameHeader Axiom.

use std::collections::BTreeMap;

use ratu_aurion_consensus::certificate::QuorumCertificate;
use ratu_aurion_consensus::proposal::{SegmentProposal, PROPOSAL_SIGNING_SIZE};
use ratu_aurion_consensus::vote::Vote;
use ratu_aurion_primitives::crypto::{AccountId, Hash, Signature};
use ratu_aurion_primitives::framing::{FrameHeader, HEADER_SIZE};
use ratu_aurion_primitives::record::{MutationRecord, RECORD_SIZE};

use crate::error::NetworkError;
use crate::message::{
    NetworkMessage, MSG_CERTIFICATE, MSG_PROPOSAL, MSG_SYNC_CHUNK, MSG_SYNC_REQ, MSG_TX_RESULT,
    MSG_TX_SUBMIT, MSG_VOTE,
};

/// Versi default frame transmisi jaringan.
pub const WIRE_PROTOCOL_VERSION: u16 = 1;

/// Mengonversi `NetworkMessage` ke dalam paket biner berbingkai `FrameHeader` 42-byte.
///
/// Format paket:
/// `[FrameHeader (42 byte) | Type ID (1 byte) | Serialized Payload (N byte)]`
pub fn encode_message(msg: &NetworkMessage) -> Result<Vec<u8>, NetworkError> {
    let mut payload = Vec::new();
    payload.push(msg.type_id());

    match msg {
        NetworkMessage::Proposal(proposal) => {
            payload.extend_from_slice(&proposal.signing_bytes());
        }
        NetworkMessage::Vote(vote) => {
            payload.extend_from_slice(vote.proposal_digest.as_bytes());
            payload.extend_from_slice(vote.validator.as_bytes());
            payload.extend_from_slice(vote.signature.as_bytes());
        }
        NetworkMessage::Certificate(cert) => {
            // 1. Proposal (84 byte)
            payload.extend_from_slice(&cert.proposal.signing_bytes());
            // 2. Accumulated weight (8 byte)
            payload.extend_from_slice(&cert.accumulated_weight.to_le_bytes());
            // 3. Jumlah tanda tangan (4 byte)
            let sig_count = cert.signatures.len() as u32;
            payload.extend_from_slice(&sig_count.to_le_bytes());
            // 4. Daftar tanda tangan terurut BTreeMap (96 byte per item)
            for (validator, sig) in &cert.signatures {
                payload.extend_from_slice(validator.as_bytes());
                payload.extend_from_slice(sig.as_bytes());
            }
        }
        NetworkMessage::SyncRequest {
            epoch,
            segment_index,
            from_offset,
        } => {
            payload.extend_from_slice(&epoch.to_le_bytes());
            payload.extend_from_slice(&segment_index.to_le_bytes());
            payload.extend_from_slice(&from_offset.to_le_bytes());
        }
        NetworkMessage::SyncChunk {
            epoch,
            segment_index,
            offset,
            data,
        } => {
            payload.extend_from_slice(&epoch.to_le_bytes());
            payload.extend_from_slice(&segment_index.to_le_bytes());
            payload.extend_from_slice(&offset.to_le_bytes());
            let data_len = data.len() as u32;
            payload.extend_from_slice(&data_len.to_le_bytes());
            payload.extend_from_slice(data);
        }
        NetworkMessage::TxSubmit(record) => {
            payload.extend_from_slice(&record.to_bytes());
        }
        NetworkMessage::TxResult {
            success,
            offset,
            message,
        } => {
            payload.push(if *success { 1 } else { 0 });
            payload.extend_from_slice(&offset.to_le_bytes());
            let msg_bytes = message.as_bytes();
            let msg_len = msg_bytes.len() as u32;
            payload.extend_from_slice(&msg_len.to_le_bytes());
            payload.extend_from_slice(msg_bytes);
        }
    }

    // Hitung intisari BLAKE3 untuk payload
    let hash = blake3::hash(&payload);
    let checksum = Hash::new(*hash.as_bytes());

    let payload_len = u32::try_from(payload.len()).map_err(|_| NetworkError::MalformedPayload)?;
    let header = FrameHeader::new(WIRE_PROTOCOL_VERSION, payload_len, checksum);

    let mut packet = Vec::with_capacity(
        HEADER_SIZE
            .checked_add(payload.len())
            .ok_or(NetworkError::MalformedPayload)?,
    );
    packet.extend_from_slice(&header.to_bytes());
    packet.extend_from_slice(&payload);

    Ok(packet)
}

/// Mendekode dan memverifikasi paket data biner menjadi varian `NetworkMessage` yang sesuai.
///
/// Melakukan validasi ketat:
/// 1. Verifikasi integritas header 42 byte dan magic bytes `AXM\x01`.
/// 2. Verifikasi kesesuaian BLAKE3 checksum payload.
/// 3. Validasi struktural data payload berdasarkan Type ID.
pub fn decode_message(bytes: &[u8]) -> Result<NetworkMessage, NetworkError> {
    if bytes.len() < HEADER_SIZE {
        return Err(NetworkError::MalformedPayload);
    }

    let mut header_buf = [0u8; HEADER_SIZE];
    header_buf.copy_from_slice(&bytes[0..HEADER_SIZE]);
    let header = FrameHeader::from_bytes(&header_buf).map_err(NetworkError::FramingError)?;

    let total_expected = (HEADER_SIZE as u64)
        .checked_add(header.payload_len as u64)
        .ok_or(NetworkError::MalformedPayload)?;

    if (bytes.len() as u64) != total_expected {
        return Err(NetworkError::MalformedPayload);
    }

    let payload = &bytes[HEADER_SIZE..];
    if payload.is_empty() {
        return Err(NetworkError::MalformedPayload);
    }

    // Verifikasi integritas BLAKE3 checksum
    let actual_hash = blake3::hash(payload);
    let actual_checksum = Hash::new(*actual_hash.as_bytes());
    if header.checksum != actual_checksum {
        return Err(NetworkError::ChecksumMismatch);
    }

    let type_id = payload[0];
    let body = &payload[1..];

    match type_id {
        MSG_PROPOSAL => {
            if body.len() != PROPOSAL_SIGNING_SIZE {
                return Err(NetworkError::MalformedPayload);
            }
            let mut prop_buf = [0u8; PROPOSAL_SIGNING_SIZE];
            prop_buf.copy_from_slice(body);
            let proposal = SegmentProposal::from_signing_bytes(&prop_buf);
            Ok(NetworkMessage::Proposal(proposal))
        }
        MSG_VOTE => {
            // Vote: 32 (digest) + 32 (validator) + 64 (sig) = 128 byte
            if body.len() != 128 {
                return Err(NetworkError::MalformedPayload);
            }
            let mut digest_buf = [0u8; 32];
            digest_buf.copy_from_slice(&body[0..32]);
            let proposal_digest = Hash::new(digest_buf);

            let mut val_buf = [0u8; 32];
            val_buf.copy_from_slice(&body[32..64]);
            let validator = AccountId::new(val_buf);

            let mut sig_buf = [0u8; 64];
            sig_buf.copy_from_slice(&body[64..128]);
            let signature = Signature::new(sig_buf);

            Ok(NetworkMessage::Vote(Vote::new(
                proposal_digest,
                validator,
                signature,
            )))
        }
        MSG_CERTIFICATE => {
            // Minimal: 84 (proposal) + 8 (weight) + 4 (sig_count) = 96 byte
            if body.len() < 96 {
                return Err(NetworkError::MalformedPayload);
            }

            let mut prop_buf = [0u8; PROPOSAL_SIGNING_SIZE];
            prop_buf.copy_from_slice(&body[0..84]);
            let proposal = SegmentProposal::from_signing_bytes(&prop_buf);

            let mut weight_bytes = [0u8; 8];
            weight_bytes.copy_from_slice(&body[84..92]);
            let accumulated_weight = u64::from_le_bytes(weight_bytes);

            let mut count_bytes = [0u8; 4];
            count_bytes.copy_from_slice(&body[92..96]);
            let sig_count = u32::from_le_bytes(count_bytes) as usize;

            let remaining = &body[96..];
            let expected_sig_bytes = sig_count
                .checked_mul(96)
                .ok_or(NetworkError::MalformedPayload)?;

            if remaining.len() != expected_sig_bytes {
                return Err(NetworkError::MalformedPayload);
            }

            let mut signatures = BTreeMap::new();
            let (chunks, _) = remaining.as_chunks::<96>();
            for chunk in chunks {
                let mut acct_bytes = [0u8; 32];
                acct_bytes.copy_from_slice(&chunk[0..32]);
                let acct = AccountId::new(acct_bytes);

                let mut sig_bytes = [0u8; 64];
                sig_bytes.copy_from_slice(&chunk[32..96]);
                let sig = Signature::new(sig_bytes);

                signatures.insert(acct, sig);
            }

            Ok(NetworkMessage::Certificate(QuorumCertificate {
                proposal,
                signatures,
                accumulated_weight,
            }))
        }
        MSG_SYNC_REQ => {
            // SyncRequest: 8 (epoch) + 4 (segment_index) + 8 (from_offset) = 20 byte
            if body.len() != 20 {
                return Err(NetworkError::MalformedPayload);
            }
            let mut epoch_bytes = [0u8; 8];
            epoch_bytes.copy_from_slice(&body[0..8]);
            let epoch = u64::from_le_bytes(epoch_bytes);

            let mut seg_bytes = [0u8; 4];
            seg_bytes.copy_from_slice(&body[8..12]);
            let segment_index = u32::from_le_bytes(seg_bytes);

            let mut off_bytes = [0u8; 8];
            off_bytes.copy_from_slice(&body[12..20]);
            let from_offset = u64::from_le_bytes(off_bytes);

            Ok(NetworkMessage::SyncRequest {
                epoch,
                segment_index,
                from_offset,
            })
        }
        MSG_SYNC_CHUNK => {
            // SyncChunk minimal header: 8 (epoch) + 4 (segment_index) + 8 (offset) + 4 (data_len) = 24 byte
            if body.len() < 24 {
                return Err(NetworkError::MalformedPayload);
            }
            let mut epoch_bytes = [0u8; 8];
            epoch_bytes.copy_from_slice(&body[0..8]);
            let epoch = u64::from_le_bytes(epoch_bytes);

            let mut seg_bytes = [0u8; 4];
            seg_bytes.copy_from_slice(&body[8..12]);
            let segment_index = u32::from_le_bytes(seg_bytes);

            let mut off_bytes = [0u8; 8];
            off_bytes.copy_from_slice(&body[12..20]);
            let offset = u64::from_le_bytes(off_bytes);

            let mut len_bytes = [0u8; 4];
            len_bytes.copy_from_slice(&body[20..24]);
            let data_len = u32::from_le_bytes(len_bytes) as usize;

            let data_slice = &body[24..];
            if data_slice.len() != data_len {
                return Err(NetworkError::MalformedPayload);
            }

            Ok(NetworkMessage::SyncChunk {
                epoch,
                segment_index,
                offset,
                data: data_slice.to_vec(),
            })
        }
        MSG_TX_SUBMIT => {
            if body.len() != RECORD_SIZE {
                return Err(NetworkError::MalformedPayload);
            }
            let mut record_buf = [0u8; RECORD_SIZE];
            record_buf.copy_from_slice(body);
            let record = MutationRecord::from_bytes(&record_buf);
            Ok(NetworkMessage::TxSubmit(record))
        }
        MSG_TX_RESULT => {
            if body.len() < 13 {
                return Err(NetworkError::MalformedPayload);
            }
            let success = body[0] != 0;
            let mut off_bytes = [0u8; 8];
            off_bytes.copy_from_slice(&body[1..9]);
            let offset = u64::from_le_bytes(off_bytes);

            let mut len_bytes = [0u8; 4];
            len_bytes.copy_from_slice(&body[9..13]);
            let msg_len = u32::from_le_bytes(len_bytes) as usize;

            let msg_slice = &body[13..];
            if msg_slice.len() != msg_len {
                return Err(NetworkError::MalformedPayload);
            }

            let message = String::from_utf8(msg_slice.to_vec())
                .map_err(|_| NetworkError::MalformedPayload)?;

            Ok(NetworkMessage::TxResult {
                success,
                offset,
                message,
            })
        }
        unknown => Err(NetworkError::UnknownMessageType(unknown)),
    }
}
