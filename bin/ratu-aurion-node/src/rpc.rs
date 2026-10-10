//! Modul JSON-RPC 2.0 dan parser HTTP/1.1 minimal untuk Ratu Aurion Gateway.
//!
//! Menangani request publik HTTP POST, parsing batas muatan 64 KB,
//! dan routing RPC deterministik tanpa ketergantungan eksternal (zero-dependency).

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};

use ratu_aurion_engine::coordinator::EngineCoordinator;
use ratu_aurion_primitives::crypto::AccountId;
use ratu_aurion_primitives::record::{MutationRecord, RECORD_SIZE};

use crate::telemetry::NodeTelemetryCollector;
use crate::ws::WsSubscriptionHub;

/// Batas maksimum ukuran muatan body HTTP/RPC (64 KB = 65.536 byte).
pub const MAX_RPC_BODY_SIZE: usize = 65_536;

/// Identitas unik rantai Ratu Aurion ("RAUR" dalam hex: 0x52415552).
pub const AUR_CHAIN_ID: &str = "0x52415552";

// ============================================================================
// PARSER DAN SERIALIZER JSON IN-TREE (PURE INTEGER, NO FLOATS)
// ============================================================================

/// Representasi nilai JSON murni dengan tipe numerik integer 128-bit terproteksi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(i128),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// Mengambil referensi nilai dalam objek berdasarkan kunci nama.
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(pairs) => {
                for (k, v) in pairs {
                    if k == key {
                        return Some(v);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Mengambil string jika varian bertipe String.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Mengambil nilai integer i128 jika varian bertipe Number.
    pub fn as_i128(&self) -> Option<i128> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Mengambil nilai u64 jika varian bertipe Number dan positif.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            JsonValue::Number(n) if *n >= 0 => Some(*n as u64),
            _ => None,
        }
    }

    /// Mengambil slice array jika varian bertipe Array.
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// Serialisasi JsonValue menjadi string JSON deterministik.
    pub fn to_json_string(&self) -> String {
        match self {
            JsonValue::Null => "null".to_string(),
            JsonValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            JsonValue::Number(n) => format!("{n}"),
            JsonValue::String(s) => {
                let mut out = String::with_capacity(s.len() + 2);
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        other => out.push(other),
                    }
                }
                out.push('"');
                out
            }
            JsonValue::Array(arr) => {
                let mut out = String::from("[");
                for (i, item) in arr.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&item.to_json_string());
                }
                out.push(']');
                out
            }
            JsonValue::Object(pairs) => {
                let mut out = String::from("{");
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push('"');
                    out.push_str(k);
                    out.push_str("\":");
                    out.push_str(&v.to_json_string());
                }
                out.push('}');
                out
            }
        }
    }

    /// Melakukan parsing string teks menjadi JsonValue secara deterministik.
    pub fn parse(input: &str) -> Result<Self, String> {
        let chars: Vec<char> = input.chars().collect();
        let mut idx = 0;
        skip_ws(&chars, &mut idx);
        let val = parse_value(&chars, &mut idx)?;
        skip_ws(&chars, &mut idx);
        if idx != chars.len() {
            return Err("Trailing characters after JSON value".to_string());
        }
        Ok(val)
    }
}

fn skip_ws(chars: &[char], idx: &mut usize) {
    while *idx < chars.len() && chars[*idx].is_whitespace() {
        *idx += 1;
    }
}

fn parse_value(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    skip_ws(chars, idx);
    if *idx >= chars.len() {
        return Err("Unexpected end of JSON input".to_string());
    }

    match chars[*idx] {
        'n' => parse_null(chars, idx),
        't' | 'f' => parse_bool(chars, idx),
        '"' => parse_string(chars, idx).map(JsonValue::String),
        '[' => parse_array(chars, idx),
        '{' => parse_object(chars, idx),
        '-' | '0'..='9' => parse_number(chars, idx),
        c => Err(format!("Unexpected character: '{c}' at position {}", *idx)),
    }
}

fn parse_null(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    if *idx + 4 <= chars.len() && chars[*idx..*idx + 4] == ['n', 'u', 'l', 'l'] {
        *idx += 4;
        Ok(JsonValue::Null)
    } else {
        Err("Invalid null token".to_string())
    }
}

fn parse_bool(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    if *idx + 4 <= chars.len() && chars[*idx..*idx + 4] == ['t', 'r', 'u', 'e'] {
        *idx += 4;
        Ok(JsonValue::Bool(true))
    } else if *idx + 5 <= chars.len() && chars[*idx..*idx + 5] == ['f', 'a', 'l', 's', 'e'] {
        *idx += 5;
        Ok(JsonValue::Bool(false))
    } else {
        Err("Invalid boolean token".to_string())
    }
}

fn parse_string(chars: &[char], idx: &mut usize) -> Result<String, String> {
    if *idx >= chars.len() || chars[*idx] != '"' {
        return Err("Expected string opening quote".to_string());
    }
    *idx += 1; // Lewati pembuka '"'
    let mut s = String::new();

    while *idx < chars.len() {
        let c = chars[*idx];
        *idx += 1;
        if c == '"' {
            return Ok(s);
        } else if c == '\\' {
            if *idx >= chars.len() {
                return Err("Unexpected end in escape sequence".to_string());
            }
            let esc = chars[*idx];
            *idx += 1;
            match esc {
                '"' => s.push('"'),
                '\\' => s.push('\\'),
                '/' => s.push('/'),
                'b' => s.push('\x08'),
                'f' => s.push('\x0c'),
                'n' => s.push('\n'),
                'r' => s.push('\r'),
                't' => s.push('\t'),
                'u' => {
                    // 4 hex digits Unicode
                    if *idx + 4 > chars.len() {
                        return Err("Invalid unicode escape".to_string());
                    }
                    let hex_str: String = chars[*idx..*idx + 4].iter().collect();
                    *idx += 4;
                    let code = u32::from_str_radix(&hex_str, 16)
                        .map_err(|e| format!("Invalid unicode escape: {e}"))?;
                    if let Some(ch) = char::from_u32(code) {
                        s.push(ch);
                    }
                }
                other => s.push(other),
            }
        } else {
            s.push(c);
        }
    }

    Err("Unterminated string in JSON".to_string())
}

fn parse_number(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    let start = *idx;
    if chars[*idx] == '-' {
        *idx += 1;
    }
    if *idx >= chars.len() || !chars[*idx].is_ascii_digit() {
        return Err("Invalid number format".to_string());
    }
    while *idx < chars.len() && chars[*idx].is_ascii_digit() {
        *idx += 1;
    }

    // Larang tipe floating point atau eksponen ilmiah dalam protokol integer Ratu Aurion
    if *idx < chars.len() && (chars[*idx] == '.' || chars[*idx] == 'e' || chars[*idx] == 'E') {
        return Err("Floating-point types and scientific notation are forbidden in pure-integer gateway".to_string());
    }

    let num_str: String = chars[start..*idx].iter().collect();
    let num = num_str
        .parse::<i128>()
        .map_err(|e| format!("Integer parse error: {e}"))?;
    Ok(JsonValue::Number(num))
}

fn parse_array(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    *idx += 1; // Lewati '['
    let mut items = Vec::new();
    skip_ws(chars, idx);

    if *idx < chars.len() && chars[*idx] == ']' {
        *idx += 1;
        return Ok(JsonValue::Array(items));
    }

    loop {
        let val = parse_value(chars, idx)?;
        items.push(val);
        skip_ws(chars, idx);

        if *idx >= chars.len() {
            return Err("Unterminated array in JSON".to_string());
        }

        if chars[*idx] == ',' {
            *idx += 1;
        } else if chars[*idx] == ']' {
            *idx += 1;
            break;
        } else {
            return Err(format!("Expected ',' or ']' in array at {}", *idx));
        }
    }

    Ok(JsonValue::Array(items))
}

fn parse_object(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    *idx += 1; // Lewati '{'
    let mut pairs = Vec::new();
    skip_ws(chars, idx);

    if *idx < chars.len() && chars[*idx] == '}' {
        *idx += 1;
        return Ok(JsonValue::Object(pairs));
    }

    loop {
        skip_ws(chars, idx);
        let key = parse_string(chars, idx)?;
        skip_ws(chars, idx);

        if *idx >= chars.len() || chars[*idx] != ':' {
            return Err("Expected ':' after key in object".to_string());
        }
        *idx += 1; // Lewati ':'

        let val = parse_value(chars, idx)?;
        pairs.push((key, val));
        skip_ws(chars, idx);

        if *idx >= chars.len() {
            return Err("Unterminated object in JSON".to_string());
        }

        if chars[*idx] == ',' {
            *idx += 1;
        } else if chars[*idx] == '}' {
            *idx += 1;
            break;
        } else {
            return Err(format!("Expected ',' or '}}' in object at {}", *idx));
        }
    }

    Ok(JsonValue::Object(pairs))
}

// ============================================================================
// FUNGSI BANTU HEX ENCODING / DECODING
// ============================================================================

/// Mengonversi byte slice ke string hex lowercase.
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX_CHARS[(b >> 4) as usize] as char);
        s.push(HEX_CHARS[(b & 0x0f) as usize] as char);
    }
    s
}

/// Melakukan decoding string hex (opsional awalan 0x/0X) ke byte vector.
pub fn hex_decode(input: &str) -> Result<Vec<u8>, String> {
    let clean = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
        .unwrap_or(input);

    if !clean.len().is_multiple_of(2) {
        return Err("Hex string length must be even".to_string());
    }

    let mut bytes = Vec::with_capacity(clean.len() / 2);
    let chars = clean.as_bytes();
    for chunk in chars.chunks(2) {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        bytes.push((hi << 4) | lo);
    }
    Ok(bytes)
}

fn hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(format!("Invalid hex character: {}", byte as char)),
    }
}

// ============================================================================
// FORMATTER ERROR DAN RESPON JSON-RPC 2.0
// ============================================================================

/// Membentuk respons sukses JSON-RPC 2.0: `{"jsonrpc":"2.0","result":...,"id":N}`.
pub fn make_rpc_success(result: JsonValue, id: JsonValue) -> String {
    let obj = vec![
        ("jsonrpc".to_string(), JsonValue::String("2.0".to_string())),
        ("result".to_string(), result),
        ("id".to_string(), id),
    ];
    JsonValue::Object(obj).to_json_string()
}

/// Membentuk respons galat JSON-RPC 2.0: `{"jsonrpc":"2.0","error":{"code":C,"message":"M"},"id":N}`.
pub fn make_rpc_error(code: i64, message: &str, id: JsonValue) -> String {
    let err_obj = vec![
        ("code".to_string(), JsonValue::Number(code as i128)),
        ("message".to_string(), JsonValue::String(message.to_string())),
    ];
    let obj = vec![
        ("jsonrpc".to_string(), JsonValue::String("2.0".to_string())),
        ("error".to_string(), JsonValue::Object(err_obj)),
        ("id".to_string(), id),
    ];
    JsonValue::Object(obj).to_json_string()
}

/// Memformat respons HTTP/1.1 lengkap dengan header Content-Length.
pub fn format_http_response(status_code: u16, status_text: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        status_code,
        status_text,
        body.len(),
        body
    )
}

// ============================================================================
// DISPATCHER METODE JSON-RPC 2.0
// ============================================================================

/// Menjalankan eksekusi pemanggilan metode JSON-RPC 2.0.
pub fn dispatch_json_rpc(
    req: &JsonValue,
    engine: &Arc<RwLock<EngineCoordinator>>,
    telemetry: &Arc<NodeTelemetryCollector>,
    ws_hub: &Arc<WsSubscriptionHub>,
) -> String {
    // 1. Validasi struktur dasar JSON-RPC 2.0 Request
    let id = req.get("id").cloned().unwrap_or(JsonValue::Null);

    let version = match req.get("jsonrpc") {
        Some(JsonValue::String(v)) if v == "2.0" => v,
        _ => return make_rpc_error(-32600, "Invalid Request: missing or invalid jsonrpc version", id),
    };
    let _ = version;

    let method = match req.get("method") {
        Some(JsonValue::String(m)) => m.as_str(),
        _ => return make_rpc_error(-32600, "Invalid Request: missing method name", id),
    };

    let params = req.get("params");

    // 2. Routing metode yang didukung
    match method {
        "aur_chainId" => {
            make_rpc_success(JsonValue::String(AUR_CHAIN_ID.to_string()), id)
        }
        "aur_blockHeight" => {
            let epoch = match engine.read() {
                Ok(eng) => eng.current_epoch(),
                Err(_) => 1,
            };
            let seq = telemetry.total_tx_committed.load(Ordering::Relaxed);

            let res = JsonValue::Object(vec![
                ("epoch".to_string(), JsonValue::Number(epoch as i128)),
                ("sequence_number".to_string(), JsonValue::Number(seq as i128)),
            ]);
            make_rpc_success(res, id)
        }
        "aur_getBalance" => {
            let addr_str = match params.and_then(|p| p.as_array()).and_then(|arr| arr.first()) {
                Some(JsonValue::String(s)) => s,
                _ => return make_rpc_error(-32602, "Invalid params: expected [account_hex]", id),
            };

            let addr_bytes = match hex_decode(addr_str) {
                Ok(b) if b.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&b);
                    AccountId::new(arr)
                }
                _ => return make_rpc_error(-32602, "Invalid params: address must be 32 bytes hex", id),
            };

            let balance = match engine.read() {
                Ok(eng) => eng.query_balance(&addr_bytes),
                Err(e) => return make_rpc_error(-32000, &format!("Engine read lock error: {e}"), id),
            };

            // Kembalikan representasi unit atomik exact sebagai JSON String
            make_rpc_success(JsonValue::String(balance.to_atomic().to_string()), id)
        }
        "aur_getAccountLocation" => {
            let addr_str = match params.and_then(|p| p.as_array()).and_then(|arr| arr.first()) {
                Some(JsonValue::String(s)) => s,
                _ => return make_rpc_error(-32602, "Invalid params: expected [account_hex]", id),
            };

            let addr_bytes = match hex_decode(addr_str) {
                Ok(b) if b.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&b);
                    AccountId::new(arr)
                }
                _ => return make_rpc_error(-32602, "Invalid params: address must be 32 bytes hex", id),
            };

            let location_opt = match engine.read() {
                Ok(eng) => eng.query_last_location(&addr_bytes),
                Err(e) => return make_rpc_error(-32000, &format!("Engine read lock error: {e}"), id),
            };

            match location_opt {
                Some(loc) => {
                    let res = JsonValue::Object(vec![
                        ("epoch".to_string(), JsonValue::Number(loc.epoch as i128)),
                        ("segment_index".to_string(), JsonValue::Number(loc.segment_idx as i128)),
                        ("offset".to_string(), JsonValue::Number(loc.offset as i128)),
                        ("sequence_number".to_string(), JsonValue::Number(loc.sequence_number as i128)),
                    ]);
                    make_rpc_success(res, id)
                }
                None => make_rpc_success(JsonValue::Null, id),
            }
        }
        "aur_sendRawTransaction" => {
            let tx_hex = match params.and_then(|p| p.as_array()).and_then(|arr| arr.first()) {
                Some(JsonValue::String(s)) => s,
                _ => return make_rpc_error(-32602, "Invalid params: expected [raw_tx_hex]", id),
            };

            let tx_bytes = match hex_decode(tx_hex) {
                Ok(b) if b.len() == RECORD_SIZE => b,
                Ok(b) => {
                    return make_rpc_error(
                        -32602,
                        &format!("Invalid params: expected exactly {RECORD_SIZE} bytes, got {}", b.len()),
                        id,
                    )
                }
                Err(e) => return make_rpc_error(-32602, &format!("Invalid hex string: {e}"), id),
            };

            let mut record_arr = [0u8; RECORD_SIZE];
            record_arr.copy_from_slice(&tx_bytes);
            let record = MutationRecord::from_bytes(&record_arr);

            let commit_result = match engine.write() {
                Ok(mut eng) => eng.submit_transaction(&record),
                Err(e) => return make_rpc_error(-32000, &format!("Engine write lock error: {e}"), id),
            };

            match commit_result {
                Ok(disk_offset) => {
                    telemetry.total_tx_committed.fetch_add(1, Ordering::Relaxed);
                    telemetry.disk_offset.store(disk_offset, Ordering::Relaxed);

                    // Siarkan mutasi baru ke seluruh pelanggan WebSocket aktif
                    let ws_payload = format!(
                        r#"{{"jsonrpc":"2.0","method":"aur_subscription","params":{{"result":{{"epoch":{},"sequence_number":{},"disk_offset":{},"sender":"0x{}","recipient":"0x{}","amount":"{}"}}}}}}"#,
                        record.epoch,
                        record.sequence_number,
                        disk_offset,
                        hex_encode(record.sender.as_bytes()),
                        hex_encode(record.recipient.as_bytes()),
                        record.amount.to_atomic(),
                    );
                    ws_hub.broadcast(&ws_payload);

                    let res = JsonValue::Object(vec![
                        ("disk_offset".to_string(), JsonValue::Number(disk_offset as i128)),
                        ("sequence_number".to_string(), JsonValue::Number(record.sequence_number as i128)),
                    ]);
                    make_rpc_success(res, id)
                }
                Err(e) => make_rpc_error(-32000, &format!("Execution error: {e}"), id),
            }
        }
        unknown => {
            make_rpc_error(-32601, &format!("Method not found: {unknown}"), id)
        }
    }
}

// ============================================================================
// PARSER HTTP/1.1 DAN LOOP PENANGAN KONEKSI
// ============================================================================

/// Tipe hasil parsing header HTTP: (Method, Path, Headers, BodyPrefix).
pub type HttpHeaderResult = (String, String, BTreeMap<String, String>, Vec<u8>);

/// Membaca header HTTP/1.1 dari TcpStream sampai baris kosong `\r\n\r\n`.
pub fn read_http_headers(
    stream: &mut TcpStream,
) -> Result<HttpHeaderResult, std::io::Error> {
    let mut buf = Vec::with_capacity(4096);
    let mut temp = [0u8; 1024];

    let header_end_pos;
    loop {
        let n = stream.read(&mut temp)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Connection closed while reading headers",
            ));
        }
        buf.extend_from_slice(&temp[..n]);

        if let Some(pos) = find_header_delimiter(&buf) {
            header_end_pos = pos;
            break;
        }

        if buf.len() > 16_384 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP headers exceed 16 KB limit",
            ));
        }
    }

    let header_bytes = &buf[..header_end_pos];
    let body_prefix = buf[header_end_pos + 4..].to_vec();

    let header_str = String::from_utf8_lossy(header_bytes);
    let mut lines = header_str.lines();

    let request_line = lines
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Empty request line"))?;

    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .unwrap_or("")
        .to_uppercase();
    let path = parts
        .next()
        .unwrap_or("")
        .to_string();

    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }

    Ok((method, path, headers, body_prefix))
}

fn find_header_delimiter(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Menangani satu permintaan HTTP JSON-RPC melalui TcpStream.
pub fn handle_http_rpc_request(
    mut stream: TcpStream,
    headers: BTreeMap<String, String>,
    mut body_buf: Vec<u8>,
    engine: &Arc<RwLock<EngineCoordinator>>,
    telemetry: &Arc<NodeTelemetryCollector>,
    ws_hub: &Arc<WsSubscriptionHub>,
) -> Result<(), std::io::Error> {
    let content_len: usize = headers
        .get("content-length")
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(0);

    // Verifikasi batas muatan 64 KB (DoS Protection)
    if content_len > MAX_RPC_BODY_SIZE {
        let err_json = make_rpc_error(
            -32600,
            "Payload exceeds maximum RPC body size (64 KB)",
            JsonValue::Null,
        );
        let resp = format_http_response(200, "OK", &err_json);
        stream.write_all(resp.as_bytes())?;
        stream.flush()?;
        return Ok(());
    }

    // Baca sisa data body jika belum terbaca lengkap di header buffer
    while body_buf.len() < content_len {
        let mut temp = vec![0u8; content_len - body_buf.len()];
        let n = stream.read(&mut temp)?;
        if n == 0 {
            break;
        }
        body_buf.extend_from_slice(&temp[..n]);
    }

    let body_str = String::from_utf8_lossy(&body_buf[..content_len]);

    // Parsing JSON request
    let resp_body = match JsonValue::parse(&body_str) {
        Ok(json_req) => dispatch_json_rpc(&json_req, engine, telemetry, ws_hub),
        Err(err) => make_rpc_error(-32700, &format!("Parse error: {err}"), JsonValue::Null),
    };

    let http_resp = format_http_response(200, "OK", &resp_body);
    stream.write_all(http_resp.as_bytes())?;
    stream.flush()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_parser_and_serializer_round_trip() {
        let raw = r#"{"jsonrpc":"2.0","method":"aur_getBalance","params":["0x1234"],"id":42}"#;
        let parsed = JsonValue::parse(raw).expect("JSON should parse");

        assert_eq!(parsed.get("jsonrpc").unwrap().as_str().unwrap(), "2.0");
        assert_eq!(parsed.get("method").unwrap().as_str().unwrap(), "aur_getBalance");
        assert_eq!(parsed.get("id").unwrap().as_u64().unwrap(), 42);

        let params = parsed.get("params").unwrap().as_array().unwrap();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].as_str().unwrap(), "0x1234");
    }

    #[test]
    fn test_json_parser_reject_floats_and_malformed() {
        // Floating point dilarang secara ketat
        let float_json = r#"{"amount": 10.5}"#;
        assert!(JsonValue::parse(float_json).is_err());

        // Malformed JSON
        let bad_json = r#"{"key": unquoted}"#;
        assert!(JsonValue::parse(bad_json).is_err());
    }

    #[test]
    fn test_hex_encode_decode_round_trip() {
        let original = vec![0x01, 0x52, 0x41, 0x55, 0x52, 0xff];
        let encoded = hex_encode(&original);
        assert_eq!(encoded, "0152415552ff");

        let decoded = hex_decode(&encoded).expect("Decode should succeed");
        assert_eq!(decoded, original);

        // Uji dengan prefix 0x
        let decoded_prefixed = hex_decode("0x0152415552ff").expect("Prefix 0x should succeed");
        assert_eq!(decoded_prefixed, original);
    }
}
