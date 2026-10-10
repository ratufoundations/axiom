#![forbid(unsafe_code)]

//! Modul koordinator koneksi peer Full-Mesh in-tree (PeerMesh).
//!
//! Mengelola koneksi TCP aktif dua arah ke seluruh simpul validator yang dikenal,
//! memelihara pemetaan socket berdasarkan AccountId, dan memfasilitasi broadcast
//! serta transmisi pesan terarah dengan socket timeout guard.

use std::collections::BTreeMap;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use ratu_aurion_primitives::crypto::AccountId;

use crate::codec::{encode_message, read_message, write_message};
use crate::error::NetworkError;
use crate::framed::FramedStream;
use crate::message::NetworkMessage;

/// Batas waktu default socket I/O untuk koneksi mesh (2.000 ms).
pub const DEFAULT_MESH_TIMEOUT_MS: u64 = 2_000;

/// Struktur koneksi individu ke simpul validator peer.
#[derive(Debug)]
struct PeerConnection {
    addr: SocketAddr,
    framed: FramedStream<TcpStream>,
}

/// Koordinator koneksi jaringan Full-Mesh untuk kluster validator konsensus.
#[derive(Debug)]
pub struct PeerMesh {
    local_account: AccountId,
    peers: BTreeMap<AccountId, PeerConnection>,
    timeout_ms: u64,
}

impl PeerMesh {
    /// Mengonstruksi koordinator `PeerMesh` baru untuk akun validator lokal
    /// dengan timeout default 2.000 ms.
    pub fn new(local_account: AccountId) -> Self {
        Self {
            local_account,
            peers: BTreeMap::new(),
            timeout_ms: DEFAULT_MESH_TIMEOUT_MS,
        }
    }

    /// Mengonfigurasi batas waktu I/O socket untuk koneksi baru.
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Mengambil identitas akun validator lokal.
    #[inline]
    pub const fn local_account(&self) -> &AccountId {
        &self.local_account
    }

    /// Membuka koneksi TCP baru ke alamat socket peer dan mendaftarkannya ke mesh.
    ///
    /// Menolak pendaftaran jika peer sudah terhubung (`NetworkError::PeerAlreadyExists`).
    pub fn connect_peer(
        &mut self,
        peer_account: AccountId,
        addr: SocketAddr,
    ) -> Result<(), NetworkError> {
        if self.peers.contains_key(&peer_account) {
            return Err(NetworkError::PeerAlreadyExists);
        }

        let stream = TcpStream::connect_timeout(&addr, Duration::from_millis(self.timeout_ms))
            .map_err(NetworkError::IoError)?;

        let framed = FramedStream::from_tcp(stream, self.timeout_ms)?;

        self.peers.insert(
            peer_account,
            PeerConnection { addr, framed },
        );

        Ok(())
    }

    /// Menyiarkan pesan jaringan ke seluruh peer yang terhubung.
    ///
    /// Enkode pesan ke dalam frame biner protokol (FrameHeader 42-byte).
    /// Melakukan iterasi ke setiap peer dan menulis frame dengan batas timeout socket.
    /// Mengembalikan jumlah peer yang berhasil menerima transmisi pesan secara penuh.
    pub fn broadcast(&mut self, message: &NetworkMessage) -> Result<usize, NetworkError> {
        let wire_frame = encode_message(message)?;
        let mut notified_count = 0usize;

        for conn in self.peers.values_mut() {
            if conn.framed.stream_mut().write_all(&wire_frame).is_ok()
                && conn.framed.stream_mut().flush().is_ok()
            {
                notified_count = notified_count.saturating_add(1);
            }
        }

        Ok(notified_count)
    }

    /// Mengirimkan pesan jaringan secara terarah ke satu validator peer tertentu.
    pub fn send_to(
        &mut self,
        peer_account: &AccountId,
        message: &NetworkMessage,
    ) -> Result<(), NetworkError> {
        let conn = self
            .peers
            .get_mut(peer_account)
            .ok_or(NetworkError::PeerNotFound)?;

        write_message(conn.framed.stream_mut(), message)
    }

    /// Membaca satu pesan jaringan dari simpul peer tertentu.
    pub fn receive_from(
        &mut self,
        peer_account: &AccountId,
    ) -> Result<NetworkMessage, NetworkError> {
        let conn = self
            .peers
            .get_mut(peer_account)
            .ok_or(NetworkError::PeerNotFound)?;

        read_message(conn.framed.stream_mut())
    }

    /// Memutuskan dan menghapus koneksi ke simpul validator peer.
    pub fn disconnect_peer(&mut self, peer_account: &AccountId) {
        if let Some(mut conn) = self.peers.remove(peer_account) {
            conn.framed.close();
        }
    }

    /// Mengambil jumlah peer yang saat ini aktif terhubung dalam mesh.
    #[inline]
    pub fn connected_peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Memeriksa apakah simpul validator peer tertentu terdaftar dalam mesh.
    #[inline]
    pub fn has_peer(&self, peer_account: &AccountId) -> bool {
        self.peers.contains_key(peer_account)
    }

    /// Mengambil daftar seluruh AccountId validator peer yang terhubung.
    pub fn connected_peers(&self) -> Vec<AccountId> {
        self.peers.keys().copied().collect()
    }

    /// Mengambil alamat socket dari simpul peer tertentu jika terdaftar.
    pub fn peer_address(&self, peer_account: &AccountId) -> Option<SocketAddr> {
        self.peers.get(peer_account).map(|conn| conn.addr)
    }
}
