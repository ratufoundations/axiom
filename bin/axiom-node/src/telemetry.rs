//! Modul telemetri IPC berbasis Unix Domain Socket dan metrik atomik tanpa kunci.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::net::UnixListener;
#[cfg(unix)]
use std::thread;

/// Magic identifikasi permintaan IPC klien (4 byte: "AXTI").
pub const IPC_REQ_MAGIC: [u8; 4] = *b"AXTI";

/// Magic identifikasi balasan telemetri node (4 byte: "AXTR").
pub const IPC_RESP_MAGIC: [u8; 4] = *b"AXTR";

/// Ukuran pasti payload snapshot telemetri (128 byte).
pub const TELEMETRY_PAYLOAD_SIZE: usize = 128;

/// Pengumpul metrik performa simpul secara atomik tanpa penguncian mutex/RwLock.
#[derive(Default)]
pub struct NodeTelemetryCollector {
    /// Epoch aktif saat ini.
    pub epoch: AtomicU64,
    /// Indeks segmen log aktif yang sedang ditulis.
    pub segment_idx: AtomicU32,
    /// Posisi offset fisik byte terakhir pada segmen.
    pub disk_offset: AtomicU64,
    /// Akumulasi total transaksi mutasi sukses.
    pub total_tx_committed: AtomicU64,
    /// Laju transaksi per detik (TPS).
    pub instant_tps: AtomicU32,
    /// Median latensi commit penulisan disk (mikrodetik).
    pub p50_micros: AtomicU32,
    /// Persentil 99 latensi commit penulisan disk (mikrodetik).
    pub p99_micros: AtomicU32,
    /// Jumlah peer aktif yang terhubung.
    pub active_peers: AtomicU32,
    /// Nomor putaran sertifikat kuorum terakhir.
    pub quorum_round: AtomicU64,
    /// Waktu mulai simpul untuk menghitung uptime.
    pub start_instant: Option<Instant>,
}

impl NodeTelemetryCollector {
    /// Membuat kolektor metrik baru dengan stempel waktu mulai saat ini.
    pub fn new() -> Self {
        Self {
            start_instant: Some(Instant::now()),
            ..Default::default()
        }
    }

    /// Serialisasi snapshot telemetri ke dalam paket biner kanonikal tepat 128 byte.
    pub fn serialize_snapshot(&self) -> [u8; TELEMETRY_PAYLOAD_SIZE] {
        let mut buf = [0u8; TELEMETRY_PAYLOAD_SIZE];
        buf[0..4].copy_from_slice(&IPC_RESP_MAGIC);
        buf[4..6].copy_from_slice(&1u16.to_le_bytes()); // version
        buf[6..8].copy_from_slice(&1u16.to_le_bytes()); // status: healthy (1)

        buf[8..16].copy_from_slice(&self.epoch.load(Ordering::Relaxed).to_le_bytes());
        buf[16..20].copy_from_slice(&self.segment_idx.load(Ordering::Relaxed).to_le_bytes());
        buf[20..28].copy_from_slice(&self.disk_offset.load(Ordering::Relaxed).to_le_bytes());
        buf[28..36].copy_from_slice(&self.total_tx_committed.load(Ordering::Relaxed).to_le_bytes());
        buf[36..40].copy_from_slice(&self.instant_tps.load(Ordering::Relaxed).to_le_bytes());
        buf[40..44].copy_from_slice(&self.p50_micros.load(Ordering::Relaxed).to_le_bytes());
        buf[44..48].copy_from_slice(&self.p99_micros.load(Ordering::Relaxed).to_le_bytes());
        buf[48..52].copy_from_slice(&self.active_peers.load(Ordering::Relaxed).to_le_bytes());
        buf[52..60].copy_from_slice(&self.quorum_round.load(Ordering::Relaxed).to_le_bytes());

        let uptime = self.start_instant.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        buf[60..68].copy_from_slice(&uptime.to_le_bytes());

        // Byte 68..96 adalah reserved (28 byte), default bernilai 0x00

        // Checksum BLAKE3 atas 96 byte data pertama (0..96)
        let hash = blake3::hash(&buf[0..96]);
        buf[96..128].copy_from_slice(hash.as_bytes());

        buf
    }
}

/// Server listener soket IPC untuk telemetri simpul.
pub struct IpcServer {
    pub socket_path: PathBuf,
    pub collector: Arc<NodeTelemetryCollector>,
}

impl IpcServer {
    /// Menjalankan server UDS telemetri pada thread terpisah (platform Unix).
    #[cfg(unix)]
    pub fn start<P: AsRef<Path>>(
        socket_path: P,
        collector: Arc<NodeTelemetryCollector>,
    ) -> std::io::Result<()> {
        let path = socket_path.as_ref().to_path_buf();
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }

        let listener = UnixListener::bind(&path)?;

        // Batasi izin berkas soket ke grup axiom saja (0660)
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660));
        }

        thread::Builder::new()
            .name("axiom-ipc-telemetry".to_string())
            .spawn(move || {
                for stream in listener.incoming() {
                    match stream {
                        Ok(mut client) => {
                            let mut req_buf = [0u8; 8];
                            if client.read_exact(&mut req_buf).is_ok()
                                && &req_buf[0..4] == &IPC_REQ_MAGIC
                            {
                                let resp = collector.serialize_snapshot();
                                let _ = client.write_all(&resp);
                            }
                        }
                        Err(_) => break,
                    }
                }
            })?;

        Ok(())
    }

    /// Implementasi fallback untuk sistem non-Unix.
    #[cfg(not(unix))]
    pub fn start<P: AsRef<Path>>(
        _socket_path: P,
        _collector: Arc<NodeTelemetryCollector>,
    ) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_snapshot_serialization_and_checksum() {
        let collector = NodeTelemetryCollector::new();
        collector.epoch.store(5, Ordering::Relaxed);
        collector.segment_idx.store(2, Ordering::Relaxed);
        collector.disk_offset.store(1048576, Ordering::Relaxed);
        collector.total_tx_committed.store(6500, Ordering::Relaxed);
        collector.instant_tps.store(1250, Ordering::Relaxed);
        collector.p50_micros.store(320, Ordering::Relaxed);
        collector.p99_micros.store(890, Ordering::Relaxed);
        collector.active_peers.store(4, Ordering::Relaxed);
        collector.quorum_round.store(10, Ordering::Relaxed);

        let snapshot = collector.serialize_snapshot();
        assert_eq!(snapshot.len(), TELEMETRY_PAYLOAD_SIZE);
        assert_eq!(&snapshot[0..4], &IPC_RESP_MAGIC);

        // Verifikasi checksum BLAKE3
        let expected_hash = blake3::hash(&snapshot[0..96]);
        assert_eq!(&snapshot[96..128], expected_hash.as_bytes());

        // Verifikasi field extraction
        let epoch = u64::from_le_bytes(snapshot[8..16].try_into().unwrap());
        let seg_idx = u32::from_le_bytes(snapshot[16..20].try_into().unwrap());
        let disk_offset = u64::from_le_bytes(snapshot[20..28].try_into().unwrap());
        let total_tx = u64::from_le_bytes(snapshot[28..36].try_into().unwrap());
        let tps = u32::from_le_bytes(snapshot[36..40].try_into().unwrap());

        assert_eq!(epoch, 5);
        assert_eq!(seg_idx, 2);
        assert_eq!(disk_offset, 1048576);
        assert_eq!(total_tx, 6500);
        assert_eq!(tps, 1250);
    }
}
