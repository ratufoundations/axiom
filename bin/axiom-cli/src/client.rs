//! Modul komunikasi TCP klien untuk mengirim transaksi ke simpul node Axiom.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use axiom_network::codec::{decode_message, encode_message};
use axiom_network::message::NetworkMessage;
use axiom_primitives::framing::HEADER_SIZE;
use axiom_primitives::record::MutationRecord;

use crate::error::CliError;

/// Mengirimkan transaksi mutasi ke simpul jaringan via TCP dan mengembalikan offset byte fisik disk.
pub fn submit_transaction(node_addr: SocketAddr, record: MutationRecord) -> Result<u64, CliError> {
    // 1. Hubungi simpul node
    let mut stream = TcpStream::connect_timeout(&node_addr, Duration::from_secs(5))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    // 2. Susun pesan TxSubmit dan bungkus ke frame paket biner
    let msg = NetworkMessage::TxSubmit(record);
    let packet = encode_message(&msg)?;

    // 3. Kirim paket melalui stream
    stream.write_all(&packet)?;
    stream.flush()?;

    // 4. Baca respon balasan dari simpul
    let mut header_buf = [0u8; HEADER_SIZE];
    stream.read_exact(&mut header_buf)?;

    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(&header_buf[6..10]);
    let payload_len = u32::from_le_bytes(len_bytes) as usize;

    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload)?;

    let mut reply_packet = Vec::with_capacity(
        HEADER_SIZE
            .checked_add(payload_len)
            .ok_or(CliError::InvalidAmountFormat("Payload size overflow"))?,
    );
    reply_packet.extend_from_slice(&header_buf);
    reply_packet.extend_from_slice(&payload);

    let reply_msg = decode_message(&reply_packet)?;

    // 5. Evaluasi hasil eksekusi transaksi dari respon simpul
    match reply_msg {
        NetworkMessage::TxResult {
            success: true,
            offset,
            ..
        } => Ok(offset),
        NetworkMessage::TxResult {
            success: false,
            message,
            ..
        } => Err(CliError::NodeRejectedTransaction(message)),
        other => Err(CliError::NodeRejectedTransaction(format!(
            "Unexpected response from node: {other:?}"
        ))),
    }
}
