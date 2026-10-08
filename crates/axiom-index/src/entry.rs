//! Modul definisi penunjuk lokasi mutasi, tata letak biner padat, dan status saldo akun di RAM.

use axiom_primitives::crypto::AccountId;
use axiom_primitives::value::AxmValue;

/// Tata letak biner padat status akun di RAM berukuran tepat 80 byte dengan 0 padding.
///
/// Disusun dengan perataan alami 16-byte (dari `AxmValue`/`u128`) dan 0 byte padding compiler:
/// - `account`: 32 byte (offset 0..32)
/// - `balance`: 16 byte (offset 32..48, align 16)
/// - `sequence_number`: 8 byte (offset 48..56, align 8)
/// - `epoch`: 8 byte (offset 56..64, align 8)
/// - `offset`: 8 byte (offset 64..72, align 8)
/// - `segment_index`: 4 byte (offset 72..76, align 4)
/// - `flags`: 4 byte (offset 76..80, align 4)
///
/// Total ukuran: tepat 80 byte (kelipatan 16 byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct CompactAccountEntry {
    /// Kunci publik Ed25519 akun (32 byte).
    pub account: AccountId,
    /// Saldo moneter terkini akun (16 byte).
    pub balance: AxmValue,
    /// Nomor urut transaksi terakhir akun (anti-replay, 8 byte).
    pub sequence_number: u64,
    /// Nomor epoch segmen log tempat mutasi dicatat (8 byte).
    pub epoch: u64,
    /// Posisi byte fisik mutasi pada segmen disk (8 byte).
    pub offset: u64,
    /// Indeks segmen di dalam epoch (4 byte).
    pub segment_index: u32,
    /// Bitmask status akun (misal: aktif, pasak, reserved, 4 byte).
    pub flags: u32,
}

impl CompactAccountEntry {
    /// Membuat CompactAccountEntry baru.
    #[inline]
    pub const fn new(
        account: AccountId,
        balance: AxmValue,
        sequence_number: u64,
        epoch: u64,
        offset: u64,
        segment_index: u32,
        flags: u32,
    ) -> Self {
        Self {
            account,
            balance,
            sequence_number,
            epoch,
            offset,
            segment_index,
            flags,
        }
    }

    /// Mengambil penunjuk lokasi penyimpanan fisik disk.
    #[inline]
    pub const fn location(&self) -> AccountLocation {
        AccountLocation {
            epoch: self.epoch,
            segment_idx: self.segment_index,
            offset: self.offset,
            sequence_number: self.sequence_number,
        }
    }

    /// Mengonversi entri padat ke AccountState untuk kompatibilitas API publik.
    #[inline]
    pub const fn to_account_state(&self) -> AccountState {
        AccountState {
            balance: self.balance,
            location: self.location(),
        }
    }

    /// Mengonstruksi CompactAccountEntry dari AccountId dan AccountState.
    #[inline]
    pub const fn from_state(account: AccountId, state: AccountState, flags: u32) -> Self {
        Self {
            account,
            balance: state.balance,
            sequence_number: state.location.sequence_number,
            epoch: state.location.epoch,
            offset: state.location.offset,
            segment_index: state.location.segment_idx,
            flags,
        }
    }
}

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
