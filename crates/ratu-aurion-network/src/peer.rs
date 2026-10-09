//! Modul manajemen tabel peer (PeerTable) dan identitas simpul jaringan.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use ratu_aurion_primitives::crypto::AccountId;

use crate::error::NetworkError;

/// Identitas unik simpul peer jaringan yang dibungkus dari AccountId (32 byte).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PeerId(pub AccountId);

impl PeerId {
    /// Mengonstruksi PeerId baru dari AccountId.
    #[inline]
    pub const fn new(account_id: AccountId) -> Self {
        Self(account_id)
    }

    /// Mengambil referensi AccountId internal.
    #[inline]
    pub const fn as_account_id(&self) -> &AccountId {
        &self.0
    }
}

impl From<AccountId> for PeerId {
    #[inline]
    fn from(account: AccountId) -> Self {
        Self(account)
    }
}

/// Metadata dan status koneksi simpul peer aktif.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PeerInfo {
    /// Identitas peer.
    pub peer_id: PeerId,
    /// Alamat socket jaringan IP dan port (IPv4 atau IPv6).
    pub address: SocketAddr,
    /// Timestamp detik UNIX saat pertama kali terhubung.
    pub connected_at: u64,
    /// Epoch log terakhir yang dilaporkan atau tersinkronisasi oleh peer ini.
    pub last_seen_epoch: u64,
}

impl PeerInfo {
    /// Mengonstruksi metadata peer baru.
    #[inline]
    pub const fn new(
        peer_id: PeerId,
        address: SocketAddr,
        connected_at: u64,
        last_seen_epoch: u64,
    ) -> Self {
        Self {
            peer_id,
            address,
            connected_at,
            last_seen_epoch,
        }
    }
}

/// Tabel memori peer aktif (PeerTable) berbasis BTreeMap untuk penjelajahan deterministik.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PeerTable {
    peers: BTreeMap<PeerId, PeerInfo>,
}

impl PeerTable {
    /// Membuat PeerTable baru yang kosong.
    #[inline]
    pub fn new() -> Self {
        Self {
            peers: BTreeMap::new(),
        }
    }

    /// Mendaftarkan simpul peer baru ke dalam tabel.
    ///
    /// Menolak jika identitas peer sudah terdaftar (`NetworkError::PeerAlreadyExists`).
    pub fn insert(&mut self, peer: PeerInfo) -> Result<(), NetworkError> {
        if self.peers.contains_key(&peer.peer_id) {
            return Err(NetworkError::PeerAlreadyExists);
        }

        self.peers.insert(peer.peer_id, peer);
        Ok(())
    }

    /// Menghapus simpul peer berdasarkan identitasnya dan mengembalikan data lamanya jika ada.
    #[inline]
    pub fn remove(&mut self, peer_id: &PeerId) -> Option<PeerInfo> {
        self.peers.remove(peer_id)
    }

    /// Mengambil referensi data peer berdasarkan identitasnya.
    #[inline]
    pub fn get(&self, peer_id: &PeerId) -> Option<&PeerInfo> {
        self.peers.get(peer_id)
    }

    /// Memeriksa keberadaan peer dalam tabel.
    #[inline]
    pub fn contains(&self, peer_id: &PeerId) -> bool {
        self.peers.contains_key(peer_id)
    }

    /// Mengambil jumlah seluruh peer yang terdaftar.
    #[inline]
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Memeriksa apakah tabel peer kosong.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Memperbarui catatan epoch terakhir yang dilihat dari peer tertentu.
    pub fn update_last_seen(
        &mut self,
        peer_id: &PeerId,
        epoch: u64,
    ) -> Result<(), NetworkError> {
        let peer = self
            .peers
            .get_mut(peer_id)
            .ok_or(NetworkError::PeerNotFound)?;
        peer.last_seen_epoch = epoch;
        Ok(())
    }
}
