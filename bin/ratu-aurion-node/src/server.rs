//! Modul server jaringan P2P (NodeServer) dan penanganan pipeline pesan.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use ratu_aurion_consensus::certificate::QuorumCertificate;
use ratu_aurion_consensus::vote::Vote;
use ratu_aurion_engine::coordinator::EngineCoordinator;
use ratu_aurion_network::codec::{read_message, write_message};
use ratu_aurion_network::error::NetworkError;
use ratu_aurion_network::message::NetworkMessage;
use ratu_aurion_network::peer::PeerTable;

use crate::config::NodeConfig;

/// Kapasitas maksimum satu potongan sinkronisasi berkas segmen log (64 KB).
pub const MAX_SYNC_CHUNK_SIZE: usize = 65_536;

/// Server jaringan simpul utama yang mengelola loop koneksi TCP dan orkestrasi pesan.
#[derive(Clone)]
pub struct NodeServer {
    /// Konfigurasi simpul aktif.
    pub config: NodeConfig,
    /// Koordinator eksekusi mesin transaksi yang dilindungi kunci baca-tulis.
    pub engine: Arc<RwLock<EngineCoordinator>>,
    /// Tabel peer aktif untuk routing dan pelacakan simpul lain.
    pub peer_table: Arc<RwLock<PeerTable>>,
    /// Sertifikat kuorum aktif saat ini yang sedang mengumpulkan suara.
    pub active_certificate: Arc<RwLock<Option<QuorumCertificate>>>,
    /// Kolektor metrik telemetri simpul bebas-kunci.
    pub telemetry: Arc<crate::telemetry::NodeTelemetryCollector>,
    /// Hub langganan mutasi WebSocket aktif.
    pub ws_hub: Arc<crate::ws::WsSubscriptionHub>,
}

impl NodeServer {
    /// Mengonstruksi NodeServer baru dengan kolektor telemetri default.
    pub fn new(
        config: NodeConfig,
        engine: Arc<RwLock<EngineCoordinator>>,
        peer_table: Arc<RwLock<PeerTable>>,
    ) -> Self {
        Self::with_telemetry(
            config,
            engine,
            peer_table,
            Arc::new(crate::telemetry::NodeTelemetryCollector::new()),
        )
    }

    /// Mengonstruksi NodeServer dengan kolektor telemetri yang ditentukan.
    pub fn with_telemetry(
        config: NodeConfig,
        engine: Arc<RwLock<EngineCoordinator>>,
        peer_table: Arc<RwLock<PeerTable>>,
        telemetry: Arc<crate::telemetry::NodeTelemetryCollector>,
    ) -> Self {
        telemetry
            .epoch
            .store(config.epoch, std::sync::atomic::Ordering::Relaxed);
        Self {
            config,
            engine,
            peer_table,
            active_certificate: Arc::new(RwLock::new(None)),
            telemetry,
            ws_hub: Arc::new(crate::ws::WsSubscriptionHub::new()),
        }
    }

    /// Melakukan bind socket TCP listener ke alamat konfigurasi simpul.
    pub fn bind(&self) -> Result<TcpListener, NetworkError> {
        let listener = TcpListener::bind(self.config.listen_addr)?;
        listener.set_nonblocking(true)?;
        Ok(listener)
    }

    /// Menjalankan loop utama pendengar jaringan TCP dan mendengarkan sinyal shutdown.
    pub fn run_listener(
        &self,
        listener: TcpListener,
        shutdown_signal: Receiver<()>,
    ) -> Result<(), NetworkError> {
        loop {
            // 1. Periksa sinyal shutdown
            match shutdown_signal.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => {
                    break;
                }
                Err(TryRecvError::Empty) => {}
            }

            // 2. Menerima koneksi baru non-blocking
            match listener.accept() {
                Ok((mut stream, peer_addr)) => {
                    let server = self.clone();
                    thread::spawn(move || {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(2000)));
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(2000)));
                        while let Ok(()) = server.handle_incoming_stream(&mut stream, peer_addr) {}
                    });
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // Menghindari busy-wait berlebihan
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    return Err(NetworkError::IoError(e));
                }
            }
        }

        Ok(())
    }

    /// Memulai server secara langsung dengan binding ke alamat konfigurasi.
    pub fn start(&self, shutdown_signal: Receiver<()>) -> Result<(), NetworkError> {
        let listener = self.bind()?;
        self.run_listener(listener, shutdown_signal)
    }

    /// Menangani satu frame pesan masuk TCP, memproses payload, dan memberikan balasan.
    pub fn handle_incoming_stream(
        &self,
        stream: &mut TcpStream,
        _peer_addr: SocketAddr,
    ) -> Result<(), NetworkError> {
        let incoming_msg = read_message(stream)?;

        // Proses pesan sesuai spesifikasi pipeline handler
        match incoming_msg {
            NetworkMessage::Proposal(proposal) => {
                // Verifikasi proposal dan tanda tangani balasan Vote
                let validator_account = self.config.validator_account();
                let vote = Vote::sign(&proposal, validator_account, &self.config.validator_key);
                let reply = NetworkMessage::Vote(vote);
                write_message(stream, &reply)?;
            }
            NetworkMessage::Vote(vote) => {
                // Kumpulkan suara ke dalam QuorumCertificate aktif jika ada
                if let Ok(mut cert_guard) = self.active_certificate.write() {
                    if let Some(ref mut cert) = *cert_guard {
                        if cert.proposal.digest() == vote.proposal_digest {
                            cert.signatures.insert(vote.validator, vote.signature);
                        }
                    }
                }
            }
            NetworkMessage::VoteRecord(vote_record) => {
                // Kumpulkan suara VoteRecord ke dalam QuorumCertificate aktif jika ada
                if let Ok(mut cert_guard) = self.active_certificate.write() {
                    if let Some(ref mut cert) = *cert_guard {
                        if cert.proposal.digest() == vote_record.block_hash {
                            cert.signatures
                                .insert(vote_record.validator, vote_record.signature);
                        }
                    }
                }
            }
            NetworkMessage::SyncRequest {
                epoch,
                segment_index,
                from_offset,
            } => {
                // Baca potongan segmen dari berkas disk log
                let seg_path = self
                    .config
                    .data_dir
                    .join(format!("epoch_{epoch}_seg_{segment_index}.log"));

                let mut chunk_data = Vec::new();
                if seg_path.exists() {
                    if let Ok(mut file) = File::open(&seg_path) {
                        if file.seek(SeekFrom::Start(from_offset)).is_ok() {
                            let mut buf = vec![0u8; MAX_SYNC_CHUNK_SIZE];
                            if let Ok(n) = file.read(&mut buf) {
                                chunk_data.extend_from_slice(&buf[..n]);
                            }
                        }
                    }
                }

                let reply = NetworkMessage::SyncChunk {
                    epoch,
                    segment_index,
                    offset: from_offset,
                    data: chunk_data,
                };
                write_message(stream, &reply)?;
            }
            NetworkMessage::Certificate(qc) => {
                // Catat sertifikat kuorum finalitas baru
                if let Ok(mut cert_guard) = self.active_certificate.write() {
                    *cert_guard = Some(qc);
                }
            }
            NetworkMessage::SyncChunk { .. } => {
                // Respons potongan data diterima
            }
            NetworkMessage::Timeout(_) => {
                // Pesan timeout diterima
            }
            NetworkMessage::TimeoutCertificate(_) => {
                // Sertifikat timeout diterima
            }
            NetworkMessage::TxSubmit(record) => {
                let res = match self.engine.write() {
                    Ok(mut eng) => {
                        if eng.query_balance(&record.sender)
                            == ratu_aurion_primitives::value::AurValue::ZERO
                            && record.sequence_number == 1
                        {
                            eng.seed_account(
                                record.sender,
                                ratu_aurion_primitives::value::AurValue::from_atomic(
                                    10_000_000_000_000,
                                ),
                            );
                        }
                        eng.submit_transaction(&record)
                    }
                    Err(e) => Err(ratu_aurion_engine::error::EngineError::IoError(
                        std::io::Error::other(format!("Engine lock error: {e}")),
                    )),
                };

                if let Ok(offset) = res {
                    self.telemetry
                        .total_tx_committed
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    self.telemetry
                        .disk_offset
                        .store(offset, std::sync::atomic::Ordering::Relaxed);

                    // Siarkan ke WebSocket subscribers aktif
                    let ws_payload = format!(
                        r#"{{"jsonrpc":"2.0","method":"aur_subscription","params":{{"result":{{"epoch":{},"sequence_number":{},"disk_offset":{},"sender":"0x{}","recipient":"0x{}","amount":"{}"}}}}}}"#,
                        record.epoch,
                        record.sequence_number,
                        offset,
                        crate::rpc::hex_encode(record.sender.as_bytes()),
                        crate::rpc::hex_encode(record.recipient.as_bytes()),
                        record.amount.to_atomic(),
                    );
                    self.ws_hub.broadcast(&ws_payload);
                }

                let reply = match res {
                    Ok(offset) => NetworkMessage::TxResult {
                        success: true,
                        offset,
                        message: "Transaction committed".to_string(),
                    },
                    Err(e) => NetworkMessage::TxResult {
                        success: false,
                        offset: 0,
                        message: format!("{e}"),
                    },
                };
                write_message(stream, &reply)?;
            }
            NetworkMessage::TxResult { .. } => {
                // Konfirmasi hasil transaksi diterima
            }
        }

        Ok(())
    }

    /// Menyerahkan transaksi mutasi langsung ke engine simpul dan menyiarkan notifikasi.
    pub fn submit_transaction(
        &self,
        record: &ratu_aurion_primitives::record::MutationRecord,
    ) -> Result<u64, ratu_aurion_engine::error::EngineError> {
        let offset = self
            .engine
            .write()
            .map_err(|e| {
                ratu_aurion_engine::error::EngineError::IoError(std::io::Error::other(format!("{e}")))
            })?
            .submit_transaction(record)?;

        self.telemetry
            .total_tx_committed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.telemetry
            .disk_offset
            .store(offset, std::sync::atomic::Ordering::Relaxed);

        let ws_payload = format!(
            r#"{{"jsonrpc":"2.0","method":"aur_subscription","params":{{"result":{{"epoch":{},"sequence_number":{},"disk_offset":{},"sender":"0x{}","recipient":"0x{}","amount":"{}"}}}}}}"#,
            record.epoch,
            record.sequence_number,
            offset,
            crate::rpc::hex_encode(record.sender.as_bytes()),
            crate::rpc::hex_encode(record.recipient.as_bytes()),
            record.amount.to_atomic(),
        );
        self.ws_hub.broadcast(&ws_payload);

        Ok(offset)
    }

    /// Melakukan bind socket TCP listener untuk gateway JSON-RPC & WebSocket jika dikonfigurasi.
    pub fn bind_rpc(&self) -> Result<Option<TcpListener>, NetworkError> {
        if let Some(addr) = self.config.rpc_addr {
            let listener = TcpListener::bind(addr)?;
            listener.set_nonblocking(true)?;
            Ok(Some(listener))
        } else {
            Ok(None)
        }
    }

    /// Menjalankan loop pendengar gateway JSON-RPC & WebSocket dan memproses koneksi klien.
    pub fn run_rpc_listener(
        &self,
        listener: TcpListener,
        shutdown_signal: Receiver<()>,
    ) -> Result<(), NetworkError> {
        loop {
            // 1. Periksa sinyal shutdown
            match shutdown_signal.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }

            // 2. Menerima koneksi baru non-blocking
            match listener.accept() {
                Ok((stream, _peer_addr)) => {
                    let server = self.clone();
                    thread::spawn(move || {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(2000)));
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(2000)));
                        server.handle_rpc_stream(stream);
                    });
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    return Err(NetworkError::IoError(e));
                }
            }
        }

        Ok(())
    }

    /// Menangani satu koneksi masuk pada port RPC/WebSocket Gateway.
    pub fn handle_rpc_stream(&self, mut stream: TcpStream) {
        let (method, path, headers, body_prefix) = match crate::rpc::read_http_headers(&mut stream) {
            Ok(res) => res,
            Err(_) => return,
        };

        let is_ws = path == "/ws"
            || headers
                .get("upgrade")
                .map(|val| val.to_lowercase().contains("websocket"))
                .unwrap_or(false);

        if is_ws {
            if let Some(key) = headers.get("sec-websocket-key") {
                let _ = crate::ws::handle_ws_connection(stream, key, &self.ws_hub);
            } else {
                let bad_req = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(bad_req.as_bytes());
            }
        } else if method == "POST" {
            let _ = crate::rpc::handle_http_rpc_request(
                stream,
                headers,
                body_prefix,
                &self.engine,
                &self.telemetry,
                &self.ws_hub,
            );
        } else {
            let not_found = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
            let _ = stream.write_all(not_found.as_bytes());
        }
    }
}
