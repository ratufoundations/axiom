//! Modul konfigurasi runtime simpul utama Axiom.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use ratu_aurion_primitives::crypto::AccountId;
use ed25519_dalek::SigningKey;

/// Konfigurasi runtime untuk simpul ratu-aurion-node.
#[derive(Clone, Debug)]
pub struct NodeConfig {
    /// Direktori penyimpanan berkas segmen log aktif.
    pub data_dir: PathBuf,
    /// Direktori penyimpanan berkas arsip bulanan (.zip).
    pub archive_dir: PathBuf,
    /// Alamat socket TCP lokal untuk mendengarkan koneksi P2P.
    pub listen_addr: SocketAddr,
    /// Nomor epoch awal simpul.
    pub epoch: u64,
    /// Kunci privat penandatanganan validator simpul (Ed25519).
    pub validator_key: SigningKey,
    /// Path berkas Unix Domain Socket untuk telemetri IPC (opsional).
    pub ipc_socket: Option<PathBuf>,
    /// Alamat socket TCP lokal untuk gateway JSON-RPC & WebSocket (opsional).
    pub rpc_addr: Option<SocketAddr>,
}

impl NodeConfig {
    /// Mengonstruksi konfigurasi simpul baru secara eksplisit.
    pub fn new(
        data_dir: PathBuf,
        archive_dir: PathBuf,
        listen_addr: SocketAddr,
        epoch: u64,
        validator_key: SigningKey,
    ) -> Self {
        Self {
            data_dir,
            archive_dir,
            listen_addr,
            epoch,
            validator_key,
            ipc_socket: None,
            rpc_addr: None,
        }
    }

    /// Menentukan alamat listening RPC/WebSocket gateway (opsional).
    pub fn with_rpc_addr(mut self, addr: SocketAddr) -> Self {
        self.rpc_addr = Some(addr);
        self
    }

    /// Menghasilkan konfigurasi bawaan yang aman untuk pengujian.
    pub fn default_test_config() -> Self {
        let mut seed = [0u8; 32];
        seed[0] = 0x01;
        let validator_key = SigningKey::from_bytes(&seed);
        let listen_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9000);

        Self {
            data_dir: PathBuf::from("data"),
            archive_dir: PathBuf::from("archive"),
            listen_addr,
            epoch: 1,
            validator_key,
            ipc_socket: None,
            rpc_addr: None,
        }
    }

    /// Mengambil identitas publik AccountId dari validator simpul ini.
    #[inline]
    pub fn validator_account(&self) -> AccountId {
        let verifying_key = self.validator_key.verifying_key();
        AccountId::new(verifying_key.to_bytes())
    }

    /// Melakukan parsing argumen baris perintah dari iterator string.
    pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Result<Self, String> {
        let mut data_dir = PathBuf::from("data");
        let mut archive_dir = PathBuf::from("archive");
        let mut listen_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9000);
        let mut epoch: u64 = 1;
        let mut seed_byte: u8 = 0x01;
        let mut ipc_socket: Option<PathBuf> = None;
        let mut rpc_addr: Option<SocketAddr> = Some(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9545),
        );

        let args_vec: Vec<String> = args.into_iter().collect();
        let mut i = 0;
        while i < args_vec.len() {
            match args_vec[i].as_str() {
                "--data-dir" => {
                    i = i.checked_add(1).ok_or("Arg index overflow")?;
                    if i >= args_vec.len() {
                        return Err("Missing argument for --data-dir".to_string());
                    }
                    data_dir = PathBuf::from(&args_vec[i]);
                }
                "--archive-dir" => {
                    i = i.checked_add(1).ok_or("Arg index overflow")?;
                    if i >= args_vec.len() {
                        return Err("Missing argument for --archive-dir".to_string());
                    }
                    archive_dir = PathBuf::from(&args_vec[i]);
                }
                "--listen" => {
                    i = i.checked_add(1).ok_or("Arg index overflow")?;
                    if i >= args_vec.len() {
                        return Err("Missing argument for --listen".to_string());
                    }
                    listen_addr = args_vec[i]
                        .parse::<SocketAddr>()
                        .map_err(|e| format!("Invalid --listen address: {e}"))?;
                }
                "--rpc-addr" => {
                    i = i.checked_add(1).ok_or("Arg index overflow")?;
                    if i >= args_vec.len() {
                        return Err("Missing argument for --rpc-addr".to_string());
                    }
                    rpc_addr = Some(
                        args_vec[i]
                            .parse::<SocketAddr>()
                            .map_err(|e| format!("Invalid --rpc-addr address: {e}"))?,
                    );
                }
                "--epoch" => {
                    i = i.checked_add(1).ok_or("Arg index overflow")?;
                    if i >= args_vec.len() {
                        return Err("Missing argument for --epoch".to_string());
                    }
                    epoch = args_vec[i]
                        .parse::<u64>()
                        .map_err(|e| format!("Invalid --epoch number: {e}"))?;
                }
                "--seed-byte" => {
                    i = i.checked_add(1).ok_or("Arg index overflow")?;
                    if i >= args_vec.len() {
                        return Err("Missing argument for --seed-byte".to_string());
                    }
                    seed_byte = args_vec[i]
                        .parse::<u8>()
                        .map_err(|e| format!("Invalid --seed-byte value: {e}"))?;
                }
                "--ipc-socket" => {
                    i = i.checked_add(1).ok_or("Arg index overflow")?;
                    if i >= args_vec.len() {
                        return Err("Missing argument for --ipc-socket".to_string());
                    }
                    ipc_socket = Some(PathBuf::from(&args_vec[i]));
                }
                "--help" | "-h" => {
                    return Err("Usage: ratu-aurion-node [--data-dir PATH] [--archive-dir PATH] [--listen IP:PORT] [--rpc-addr IP:PORT] [--epoch NUM] [--seed-byte U8] [--ipc-socket PATH]".to_string());
                }
                unknown => {
                    return Err(format!("Unknown argument: {unknown}"));
                }
            }
            i = i.checked_add(1).ok_or("Arg index overflow")?;
        }

        let mut seed = [0u8; 32];
        seed[0] = seed_byte;
        let validator_key = SigningKey::from_bytes(&seed);

        Ok(Self {
            data_dir,
            archive_dir,
            listen_addr,
            epoch,
            validator_key,
            ipc_socket,
            rpc_addr,
        })
    }

    /// Parsing argumen baris perintah dari lingkungan eksekusi standar (`std::env::args`).
    pub fn parse_args() -> Result<Self, String> {
        Self::from_args(std::env::args().skip(1))
    }
}
