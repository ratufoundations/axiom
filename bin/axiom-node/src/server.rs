//! Modul server jaringan P2P (NodeServer) dan penanganan pipeline pesan.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use axiom_consensus::certificate::QuorumCertificate;
use axiom_consensus::vote::Vote;
use axiom_engine::coordinator::EngineCoordinator;
use axiom_network::codec::{decode_message, encode_message};
use axiom_network::error::NetworkError;
use axiom_network::message::NetworkMessage;
use axiom_network::peer::PeerTable;
use axiom_primitives::framing::HEADER_SIZE;

use crate::config::NodeConfig;

/// Kapasitas maksimum satu potongan sinkronisasi berkas segmen log (64 KB).
pub const MAX_SYNC_CHUNK_SIZE: usize = 65_536;

/// Server jaringan simpul utama yang mengelola loop koneksi TCP dan orkestrasi pesan.
pub struct NodeServer {
    /// Konfigurasi simpul aktif.
    pub config: NodeConfig,
    /// Koordinator eksekusi mesin transaksi yang dilindungi kunci baca-tulis.
    pub engine: Arc<RwLock<EngineCoordinator>>,
    /// Tabel peer aktif untuk routing dan pelacakan simpul lain.
    pub peer_table: Arc<RwLock<PeerTable>>,
    /// Sertifikat kuorum aktif saat ini yang sedang mengumpulkan suara.
    pub active_certificate: Arc<RwLock<Option<QuorumCertificate>>>,
}

impl NodeServer {
    /// Mengonstruksi NodeServer baru.
    pub fn new(
        config: NodeConfig,
        engine: Arc<RwLock<EngineCoordinator>>,
        peer_table: Arc<RwLock<PeerTable>>,
    ) -> Self {
        Self {
            config,
            engine,
            peer_table,
            active_certificate: Arc::new(RwLock::new(None)),
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
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                    let _ = self.handle_incoming_stream(&mut stream, peer_addr);
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

    /// Menangani satu koneksi masuk TCP, membaca frame paket biner, dan memproses payload.
    fn handle_incoming_stream(
        &self,
        stream: &mut TcpStream,
        _peer_addr: SocketAddr,
    ) -> Result<(), NetworkError> {
        // 1. Baca 42 byte FrameHeader
        let mut header_buf = [0u8; HEADER_SIZE];
        stream.read_exact(&mut header_buf)?;

        // Ekstraksi panjang payload dari bytes [6..10]
        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&header_buf[6..10]);
        let payload_len = u32::from_le_bytes(len_bytes) as usize;

        // 2. Baca isi payload secara lengkap
        let mut payload = vec![0u8; payload_len];
        stream.read_exact(&mut payload)?;

        // 3. Gabungkan header dan payload untuk verifikasi checksum dan deserialisasi
        let mut full_packet = Vec::with_capacity(
            HEADER_SIZE
                .checked_add(payload_len)
                .ok_or(NetworkError::MalformedPayload)?,
        );
        full_packet.extend_from_slice(&header_buf);
        full_packet.extend_from_slice(&payload);

        let incoming_msg = decode_message(&full_packet)?;

        // 4. Proses pesan sesuai spesifikasi pipeline handler
        match incoming_msg {
            NetworkMessage::Proposal(proposal) => {
                // Verifikasi proposal dan tanda tangani balasan Vote
                let validator_account = self.config.validator_account();
                let vote = Vote::sign(&proposal, validator_account, &self.config.validator_key);
                let reply = NetworkMessage::Vote(vote);
                let reply_packet = encode_message(&reply)?;
                stream.write_all(&reply_packet)?;
                stream.flush()?;
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
                let reply_packet = encode_message(&reply)?;
                stream.write_all(&reply_packet)?;
                stream.flush()?;
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
            NetworkMessage::TxSubmit(record) => {
                let res = match self.engine.write() {
                    Ok(mut eng) => eng.submit_transaction(&record),
                    Err(e) => Err(axiom_engine::error::EngineError::IoError(std::io::Error::other(
                        format!("Engine lock error: {e}"),
                    ))),
                };

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
                let reply_packet = encode_message(&reply)?;
                stream.write_all(&reply_packet)?;
                stream.flush()?;
            }
            NetworkMessage::TxResult { .. } => {
                // Konfirmasi hasil transaksi diterima
            }
        }

        Ok(())
    }
}
