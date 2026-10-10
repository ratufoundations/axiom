//! Modul protokol WebSocket RFC 6455 in-tree untuk Ratu Aurion Gateway.
//!
//! Menyediakan handshake deterministik Sec-WebSocket-Accept berbasis SHA-1 integer murni
//! dan Base64 encoder tanpa ketergantungan eksternal (zero-dependency).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::Sender;
use std::sync::RwLock;
use std::time::Duration;

/// GUID kanonikal RFC 6455 untuk handshake WebSocket.
pub const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Karakter lookup Base64 standar (RFC 4648).
const BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Implementasi SHA-1 deterministik berbasis integer murni (FIPS 180-1).
/// Bebas dari pustaka eksternal dan tipe data floating-point.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    let len = data.len();
    let bit_len = (len as u64).wrapping_mul(8);

    // Padding RFC 3174: data + 0x80 + 0x00... sampai (len % 64 == 56) + 8 byte bit_len
    let mut padded = Vec::with_capacity(len.saturating_add(72));
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let (chunks, _) = padded.as_chunks::<64>();
    for chunk in chunks {
        let mut w = [0u32; 80];
        for (i, item) in w.iter_mut().enumerate().take(16) {
            let j = i * 4;
            *item = u32::from_be_bytes([chunk[j], chunk[j + 1], chunk[j + 2], chunk[j + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for (i, &w_i) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w_i);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

/// Melakukan encoding Base64 deterministik tanpa pustaka luar.
pub fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        let idx0 = (b0 >> 2) as usize;
        let idx1 = (((b0 & 0x03) << 4) | (b1 >> 4)) as usize;
        let idx2 = (((b1 & 0x0F) << 2) | (b2 >> 6)) as usize;
        let idx3 = (b2 & 0x3F) as usize;

        out.push(BASE64_TABLE[idx0] as char);
        out.push(BASE64_TABLE[idx1] as char);
        if chunk.len() > 1 {
            out.push(BASE64_TABLE[idx2] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(BASE64_TABLE[idx3] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Menghitung kunci verifikasi `Sec-WebSocket-Accept` sesuai RFC 6455 §4.2.2.
pub fn compute_websocket_accept(client_key: &str) -> String {
    let mut combined = String::with_capacity(client_key.trim().len() + WS_GUID.len());
    combined.push_str(client_key.trim());
    combined.push_str(WS_GUID);
    let digest = sha1(combined.as_bytes());
    base64_encode(&digest)
}

/// Membingkai pesan teks unmasked dari server ke klien (RFC 6455 Opcode 0x1).
pub fn encode_ws_text_frame(payload: &str) -> Vec<u8> {
    let bytes = payload.as_bytes();
    let len = bytes.len();
    let mut frame = Vec::with_capacity(len + 10);
    // FIN (bit 7 = 1) + Opcode 0x1 (Text)
    frame.push(0x81);
    if len < 126 {
        frame.push(len as u8);
    } else if len <= 0xFFFF {
        frame.push(126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    frame.extend_from_slice(bytes);
    frame
}

/// Representasi frame WebSocket yang diterima dari soket TCP.
#[derive(Debug, PartialEq, Eq)]
pub enum WsFrame {
    /// Frame teks (Opcode 0x1).
    Text(String),
    /// Frame penutupan koneksi (Opcode 0x8).
    Close,
    /// Frame Ping (Opcode 0x9).
    Ping(Vec<u8>),
    /// Frame Pong (Opcode 0xA).
    Pong(Vec<u8>),
    /// Frame opcode lain atau biner.
    Other(u8, Vec<u8>),
}

/// Membaca satu frame WebSocket dari TcpStream (mendukung unmasking klien).
pub fn read_ws_frame(stream: &mut TcpStream) -> Result<WsFrame, std::io::Error> {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head)?;

    let opcode = head[0] & 0x0F;
    let is_masked = (head[1] & 0x80) != 0;
    let len_code = (head[1] & 0x7F) as usize;

    let payload_len: usize = match len_code {
        126 => {
            let mut buf = [0u8; 2];
            stream.read_exact(&mut buf)?;
            u16::from_be_bytes(buf) as usize
        }
        127 => {
            let mut buf = [0u8; 8];
            stream.read_exact(&mut buf)?;
            u64::from_be_bytes(buf) as usize
        }
        n => n,
    };

    let mask_key = if is_masked {
        let mut key = [0u8; 4];
        stream.read_exact(&mut key)?;
        Some(key)
    } else {
        None
    };

    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload)?;

    if let Some(mask) = mask_key {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[i % 4];
        }
    }

    match opcode {
        0x1 => {
            let text = String::from_utf8(payload)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Ok(WsFrame::Text(text))
        }
        0x8 => Ok(WsFrame::Close),
        0x9 => Ok(WsFrame::Ping(payload)),
        0xA => Ok(WsFrame::Pong(payload)),
        other => Ok(WsFrame::Other(other, payload)),
    }
}

/// Hub langganan WebSocket yang mengelola kanal pengiriman mutasi aktif.
pub struct WsSubscriptionHub {
    subscribers: RwLock<Vec<Sender<String>>>,
}

impl Default for WsSubscriptionHub {
    fn default() -> Self {
        Self::new()
    }
}

impl WsSubscriptionHub {
    /// Membuat hub baru tanpa pelanggan.
    pub fn new() -> Self {
        Self {
            subscribers: RwLock::new(Vec::new()),
        }
    }

    /// Mendaftarkan kanal pelanggan baru untuk mutasi stream.
    pub fn subscribe(&self, tx: Sender<String>) {
        if let Ok(mut subs) = self.subscribers.write() {
            subs.push(tx);
        }
    }

    /// Menyiarkan pesan notifikasi ke seluruh pelanggan aktif, membersihkan yang terputus.
    pub fn broadcast(&self, message: &str) {
        if let Ok(mut subs) = self.subscribers.write() {
            subs.retain(|tx| tx.send(message.to_string()).is_ok());
        }
    }

    /// Menghitung jumlah pelanggan aktif saat ini.
    pub fn subscriber_count(&self) -> usize {
        self.subscribers
            .read()
            .map(|s| s.len())
            .unwrap_or_default()
    }
}

/// Menangani sesi koneksi WebSocket setelah handshake HTTP diterima.
pub fn handle_ws_connection(
    mut stream: TcpStream,
    sec_websocket_key: &str,
    ws_hub: &WsSubscriptionHub,
) -> Result<(), std::io::Error> {
    let accept_key = compute_websocket_accept(sec_websocket_key);
    let handshake_resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        accept_key
    );

    stream.write_all(handshake_resp.as_bytes())?;
    stream.flush()?;

    let (tx, rx) = std::sync::mpsc::channel::<String>();
    // Timeout baca pendek 50 ms agar thread responsif terhadap siaran mutasi baru
    stream.set_read_timeout(Some(Duration::from_millis(50)))?;

    loop {
        // 1. Kirim pesan notifikasi yang tertampung di kanal rx
        while let Ok(msg) = rx.try_recv() {
            let frame = encode_ws_text_frame(&msg);
            if stream.write_all(&frame).is_err() || stream.flush().is_err() {
                return Ok(());
            }
        }

        // 2. Baca frame dari klien
        match read_ws_frame(&mut stream) {
            Ok(WsFrame::Text(text)) => {
                // Periksa jika permintaan langganan JSON-RPC
                if text.contains("aur_subscribe") && text.contains("newMutations") {
                    ws_hub.subscribe(tx.clone());

                    // Ekstraksi id permintaan jika ada (format sederhana)
                    let id_val = extract_json_rpc_id(&text).unwrap_or(1);
                    let reply = format!(r#"{{"jsonrpc":"2.0","result":1,"id":{}}}"#, id_val);
                    let reply_frame = encode_ws_text_frame(&reply);
                    stream.write_all(&reply_frame)?;
                    stream.flush()?;
                }
            }
            Ok(WsFrame::Ping(payload)) => {
                let mut pong = vec![0x8A, payload.len() as u8];
                pong.extend_from_slice(&payload);
                stream.write_all(&pong)?;
                stream.flush()?;
            }
            Ok(WsFrame::Close) => break,
            Ok(WsFrame::Pong(_)) | Ok(WsFrame::Other(_, _)) => {}
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // Timeout pembacaan wajar, lanjutkan loop polling
            }
            Err(_) => break,
        }
    }

    Ok(())
}

/// Ekstraksi nilai id dari string JSON sederhana tanpa pustaka luar.
fn extract_json_rpc_id(text: &str) -> Option<u64> {
    if let Some(pos) = text.find("\"id\"") {
        let after = &text[pos + 4..];
        let start = after.find(|c: char| c.is_ascii_digit())?;
        let num_str = &after[start..];
        let end = num_str
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(num_str.len());
        num_str[..end].parse::<u64>().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha1_standard_vectors() {
        // Vektor kosong: da39a3ee5e6b4b0d3255bfef95601890afd80709
        let empty_hash = sha1(b"");
        let empty_hex: String = empty_hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(empty_hex, "da39a3ee5e6b4b0d3255bfef95601890afd80709");

        // Vektor FIPS "The quick brown fox jumps over the lazy dog"
        let fox_hash = sha1(b"The quick brown fox jumps over the lazy dog");
        let fox_hex: String = fox_hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(fox_hex, "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12");
    }

    #[test]
    fn test_rfc6455_websocket_accept_computation() {
        // Vektor RFC 6455 §1.3:
        // Key: "dGhlIHNhbXBsZSBub25jZQ==" -> Accept: "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        let client_key = "dGhlIHNhbXBsZSBub25jZQ==";
        let accept = compute_websocket_accept(client_key);
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn test_encode_ws_text_frame_small_and_medium() {
        let payload = "Hello";
        let frame = encode_ws_text_frame(payload);
        assert_eq!(frame[0], 0x81);
        assert_eq!(frame[1], 5);
        assert_eq!(&frame[2..], b"Hello");

        // Ukuran sedang (130 byte > 125)
        let med = "a".repeat(130);
        let frame_med = encode_ws_text_frame(&med);
        assert_eq!(frame_med[0], 0x81);
        assert_eq!(frame_med[1], 126);
        assert_eq!(u16::from_be_bytes([frame_med[2], frame_med[3]]), 130);
        assert_eq!(&frame_med[4..], med.as_bytes());
    }

    #[test]
    fn test_ws_subscription_hub_lifecycle() {
        let hub = WsSubscriptionHub::new();
        assert_eq!(hub.subscriber_count(), 0);

        let (tx1, rx1) = std::sync::mpsc::channel();
        let (tx2, rx2) = std::sync::mpsc::channel();

        hub.subscribe(tx1);
        hub.subscribe(tx2);
        assert_eq!(hub.subscriber_count(), 2);

        hub.broadcast("tx_committed:1");
        assert_eq!(rx1.recv().unwrap(), "tx_committed:1");
        assert_eq!(rx2.recv().unwrap(), "tx_committed:1");

        // Drop rx1, broadcast lagi, tx1 harus dipangkas
        drop(rx1);
        hub.broadcast("tx_committed:2");
        assert_eq!(rx2.recv().unwrap(), "tx_committed:2");
        assert_eq!(hub.subscriber_count(), 1);
    }
}
