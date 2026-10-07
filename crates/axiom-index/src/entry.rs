//! Modul definisi penunjuk lokasi mutasi dan status saldo akun di RAM.

use axiom_primitives::value::AxmValue;

/// Penunjuk lokasi rekaman mutasi terakhir akun di media simpan fisik.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountLocation {
    /// Nomor epoch segmen log tempat mutasi dicatat.
    pub epoch: u64,
    /// Indeks urutan segmen di dalam epoch tersebut.
    pub segment_idx: u32,
    /// Posisi byte offset tempat record 161 byte berada di berkas segmen.
    pub offset: u64,
    /// Nomor urut transaksi / nonce mutasi terakhir akun.
    pub sequence_number: u64,
}

impl AccountLocation {
    /// Membuat AccountLocation baru.
    #[inline]
    pub const fn new(epoch: u64, segment_idx: u32, offset: u64, sequence_number: u64) -> Self {
        Self {
            epoch,
            segment_idx,
            offset,
            sequence_number,
        }
    }
}

/// Status proyeksi mutakhir akun di memori RAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountState {
    /// Saldo moneter terkini akun berbasis aritmatika integer terproteksi.
    pub balance: AxmValue,
    /// Penunjuk lokasi mutasi terakhir di piringan disk.
    pub location: AccountLocation,
}

impl AccountState {
    /// Membuat AccountState baru.
    #[inline]
    pub const fn new(balance: AxmValue, location: AccountLocation) -> Self {
        Self { balance, location }
    }
}
