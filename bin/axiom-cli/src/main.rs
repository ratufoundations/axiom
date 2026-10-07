#![forbid(unsafe_code)]

//! # Axiom CLI
//!
//! Antarmuka baris perintah (CLI) untuk manajemen dompet kunci Ed25519,
//! parsing nilai nominal moneter deterministik, dan pengiriman transaksi mutasi ke simpul jaringan.

pub mod client;
pub mod error;
pub mod parser;
pub mod wallet;

use std::net::SocketAddr;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_primitives::crypto::AccountId;
use axiom_primitives::record::{MutationRecord, RECORD_KIND_TRANSFER};

use crate::client::submit_transaction;
use crate::error::CliError;
use crate::parser::{bytes_to_hex, hex_to_bytes_32, parse_axm_decimal};
use crate::wallet::Wallet;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        return;
    }

    if let Err(e) = run_cli(&args[1..]) {
        eprintln!("[axiom-cli ERROR] {e}");
        std::process::exit(1);
    }
}

fn print_usage() {
    println!("Axiom CLI - Antarmuka Dompet & Transaksi Simpul");
    println!("Penggunaan:");
    println!("  axiom-cli keygen --out <path>");
    println!("  axiom-cli inspect --key <path>");
    println!("  axiom-cli transfer --key <path> --to <hex> --amount <axm> --epoch <u64> --seq <u64> --node <ip:port>");
    println!("  axiom-cli status --ipc <socket_path>");
}

fn run_cli(args: &[String]) -> Result<(), CliError> {
    let command = args[0].as_str();

    match command {
        "keygen" => {
            let out_path = find_arg(args, "--out").ok_or(CliError::MissingArgument("--out"))?;
            let mut seed = [0u8; 32];
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| CliError::NodeRejectedTransaction(e.to_string()))?
                .as_nanos();
            seed[0..16].copy_from_slice(&nanos.to_le_bytes());
            seed[16] = 0xac;

            let wallet = Wallet::generate_from_entropy(seed);
            wallet.save_to_file(Path::new(out_path))?;

            println!("[axiom-cli] Kunci dompet berhasil dibuat.");
            println!("Account ID: 0x{}", bytes_to_hex(wallet.account_id().as_bytes()));
            println!(
                "Alamat Akun (AccountId): 0x{}",
                bytes_to_hex(wallet.account_id().as_bytes())
            );
            println!("Berkas Kunci: {out_path}");
            Ok(())
        }
        "inspect" => {
            let key_path = find_arg(args, "--key").ok_or(CliError::MissingArgument("--key"))?;
            let wallet = Wallet::load_from_file(Path::new(key_path))?;

            println!("Account ID: 0x{}", bytes_to_hex(wallet.account_id().as_bytes()));
            println!(
                "Alamat Akun (AccountId): 0x{}",
                bytes_to_hex(wallet.account_id().as_bytes())
            );
            Ok(())
        }
        "transfer" => {
            let key_path = find_arg(args, "--key").ok_or(CliError::MissingArgument("--key"))?;
            let to_hex = find_arg(args, "--to").ok_or(CliError::MissingArgument("--to"))?;
            let amount_str =
                find_arg(args, "--amount").ok_or(CliError::MissingArgument("--amount"))?;
            let epoch_str =
                find_arg(args, "--epoch").ok_or(CliError::MissingArgument("--epoch"))?;
            let seq_str = find_arg(args, "--seq").ok_or(CliError::MissingArgument("--seq"))?;
            let node_str =
                find_arg(args, "--node").ok_or(CliError::MissingArgument("--node"))?;

            let wallet = Wallet::load_from_file(Path::new(key_path))?;
            let recipient_bytes = hex_to_bytes_32(to_hex)?;
            let recipient = AccountId::new(recipient_bytes);
            let amount = parse_axm_decimal(amount_str)?;

            let epoch: u64 = epoch_str
                .parse()
                .map_err(|_| CliError::InvalidAmountFormat("Invalid epoch number"))?;
            let seq: u64 = seq_str
                .parse()
                .map_err(|_| CliError::InvalidAmountFormat("Invalid sequence number"))?;
            let node_addr: SocketAddr = node_str
                .parse()
                .map_err(|_| CliError::InvalidAmountFormat("Invalid node address IP:PORT"))?;

            let signature = wallet.sign_mutation_payload(
                epoch,
                seq,
                RECORD_KIND_TRANSFER,
                &recipient,
                amount,
            );

            let record = MutationRecord {
                epoch,
                sequence_number: seq,
                record_kind: RECORD_KIND_TRANSFER,
                sender: wallet.account_id(),
                recipient,
                amount,
                signature,
            };

            println!(
                "[axiom-cli] Mengirim mutasi transfer {} AXM dari 0x{} ke 0x{}...",
                amount_str,
                bytes_to_hex(wallet.account_id().as_bytes()),
                bytes_to_hex(recipient.as_bytes())
            );

            let offset = submit_transaction(node_addr, record)?;
            println!("[axiom-cli] Transaksi berhasil dikomit ke simpul!");
            println!("Offset Fisik Disk: {offset} byte");
            Ok(())
        }
        "status" => {
            let ipc_path = find_arg(args, "--ipc").ok_or(CliError::MissingArgument("--ipc"))?;
            query_node_status(Path::new(ipc_path))
                .map_err(|e| CliError::NodeRejectedTransaction(e.to_string()))?;
            Ok(())
        }
        unknown => Err(CliError::UnknownCommand(unknown.to_string())),
    }
}

fn find_arg<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let mut iter = args.iter();
    while let Some(item) = iter.next() {
        if item == flag {
            return iter.next().map(|s| s.as_str());
        }
    }
    None
}

/// Menginspeksi metrik simpul secara instan melalui Unix Domain Socket.
#[cfg(unix)]
pub fn query_node_status(socket_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path)?;

    // Kirim request 8-byte
    let req = [b'A', b'X', b'T', b'I', 1u8, 0u8, 0u8, 0u8];
    stream.write_all(&req)?;

    // Terima 128-byte snapshot
    let mut resp = [0u8; 128];
    stream.read_exact(&mut resp)?;

    if &resp[0..4] != b"AXTR" {
        return Err("Magic balasan IPC tidak valid".into());
    }

    let mut epoch_bytes = [0u8; 8];
    epoch_bytes.copy_from_slice(&resp[8..16]);
    let epoch = u64::from_le_bytes(epoch_bytes);

    let mut seg_idx_bytes = [0u8; 4];
    seg_idx_bytes.copy_from_slice(&resp[16..20]);
    let seg_idx = u32::from_le_bytes(seg_idx_bytes);

    let mut disk_offset_bytes = [0u8; 8];
    disk_offset_bytes.copy_from_slice(&resp[20..28]);
    let disk_offset = u64::from_le_bytes(disk_offset_bytes);

    let mut tx_total_bytes = [0u8; 8];
    tx_total_bytes.copy_from_slice(&resp[28..36]);
    let tx_total = u64::from_le_bytes(tx_total_bytes);

    let mut tps_bytes = [0u8; 4];
    tps_bytes.copy_from_slice(&resp[36..40]);
    let tps = u32::from_le_bytes(tps_bytes);

    let mut p50_bytes = [0u8; 4];
    p50_bytes.copy_from_slice(&resp[40..44]);
    let p50 = u32::from_le_bytes(p50_bytes);

    let mut p99_bytes = [0u8; 4];
    p99_bytes.copy_from_slice(&resp[44..48]);
    let p99 = u32::from_le_bytes(p99_bytes);

    let mut peers_bytes = [0u8; 4];
    peers_bytes.copy_from_slice(&resp[48..52]);
    let peers = u32::from_le_bytes(peers_bytes);

    let mut uptime_bytes = [0u8; 8];
    uptime_bytes.copy_from_slice(&resp[60..68]);
    let uptime = u64::from_le_bytes(uptime_bytes);

    println!("=== AXIOM NODE TELEMETRY SNAPSHOT ===");
    println!("Uptime Operasional : {} detik", uptime);
    println!("Epoch Aktif        : {}", epoch);
    println!("Segmen Disk Aktif  : Segmen #{}, Offset: {} byte", seg_idx, disk_offset);
    println!("Transaksi Komit    : {} mutasi", tx_total);
    println!("Throughput Ingest  : {} TPS", tps);
    println!("Latensi Disk Commit: p50 = {} µs | p99 = {} µs", p50, p99);
    println!("Peer Terhubung     : {} simpul", peers);
    Ok(())
}

/// Fallback untuk sistem non-Unix.
#[cfg(not(unix))]
pub fn query_node_status(_socket_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Err("Unix Domain Socket IPC is only supported on Unix/Linux systems".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_axm_decimal_bounds() {
        // 1. Nol mutlak
        let zero = parse_axm_decimal("0").expect("Parse 0");
        assert_eq!(zero.to_atomic(), 0);

        let zero_dot = parse_axm_decimal("0.0").expect("Parse 0.0");
        assert_eq!(zero_dot.to_atomic(), 0);

        // 2. Satu AXM utuh ($10^{10}$ atomic)
        let one = parse_axm_decimal("1").expect("Parse 1");
        assert_eq!(one.to_atomic(), 10_000_000_000);

        let one_dot = parse_axm_decimal("1.0000000000").expect("Parse 1.0000000000");
        assert_eq!(one_dot.to_atomic(), 10_000_000_000);

        // 3. Satu unit atomik terkecil (0.0000000001)
        let min_atomic = parse_axm_decimal("0.0000000001").expect("Parse 1 atomic");
        assert_eq!(min_atomic.to_atomic(), 1);

        // 4. Pecahan parsial dengan right padding ("100.5" -> 100.5000000000)
        let half = parse_axm_decimal("100.5").expect("Parse 100.5");
        assert_eq!(half.to_atomic(), 1_005_000_000_000);

        // 5. Penolakan presisi melebihi 10 desimal
        let overflow_prec = parse_axm_decimal("1.00000000001");
        assert!(matches!(overflow_prec, Err(CliError::PrecisionExceeded)));

        // 6. Penolakan input non-numerik
        let invalid_char = parse_axm_decimal("12a.5");
        assert!(matches!(invalid_char, Err(CliError::InvalidAmountFormat(_))));

        // 7. Penolakan tanda titik ganda
        let double_dot = parse_axm_decimal("1.2.3");
        assert!(matches!(double_dot, Err(CliError::InvalidAmountFormat(_))));
    }

    #[test]
    fn test_wallet_signing_and_verification() {
        let mut seed = [0u8; 32];
        seed[0] = 0x77;
        let wallet = Wallet::generate_from_entropy(seed);

        let recipient = AccountId::new([0x88; 32]);
        let amount = parse_axm_decimal("50.2500000000").expect("Parse 50.25");

        let signature = wallet.sign_mutation_payload(1, 10, RECORD_KIND_TRANSFER, &recipient, amount);

        let record = MutationRecord {
            epoch: 1,
            sequence_number: 10,
            record_kind: RECORD_KIND_TRANSFER,
            sender: wallet.account_id(),
            recipient,
            amount,
            signature,
        };

        // Verifikasi langsung menggunakan modul validator dari axiom-engine
        let verify_result = axiom_engine::validator::verify_record_signature(&record);
        assert!(verify_result.is_ok());

        // Verifikasi bahwa jika nominal transaksi dimanipulasi, verifikasi gagal
        let mut tampered_record = record;
        tampered_record.amount = parse_axm_decimal("50.2600000000").unwrap();
        let tampered_result = axiom_engine::validator::verify_record_signature(&tampered_record);
        assert!(tampered_result.is_err());
    }

    #[test]
    fn test_hex_encoding_decoding_roundtrip() {
        let original_bytes = [
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
            0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11,
            0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
        ];

        let hex_string = bytes_to_hex(&original_bytes);
        assert_eq!(hex_string.len(), 64);

        let restored_bytes = hex_to_bytes_32(&hex_string).expect("Hex to bytes 32");
        assert_eq!(original_bytes, restored_bytes);

        // Uji dengan prefix 0x
        let prefixed_hex = format!("0x{hex_string}");
        let restored_from_prefixed = hex_to_bytes_32(&prefixed_hex).expect("Hex with prefix");
        assert_eq!(original_bytes, restored_from_prefixed);

        // Penolakan hex dengan panjang tidak tepat 64 karakter
        assert!(matches!(hex_to_bytes_32("1234"), Err(CliError::InvalidKeyLength)));

        // Penolakan karakter heksadesimal invalid
        let invalid_hex = "zz".repeat(32);
        assert!(matches!(hex_to_bytes_32(&invalid_hex), Err(CliError::InvalidHexFormat)));
    }
}
