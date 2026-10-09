//! Modul definisi penunjuk lokasi mutasi, tata letak biner padat, dan status saldo akun di RAM.

use ratu_aurion_primitives::crypto::AccountId;
use ratu_aurion_primitives::value::AurValue;

/// Tata letak biner padat status akun di RAM berukuran tepat 80 byte dengan 0 padding.
///
/// Disusun dengan perataan alami 16-byte (dari `AurValue`/`u128`) dan 0 byte padding compiler:
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
    pub balance: AurValue,
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
        balance: AurValue,
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

    /// Serialisasi entri ke array tepat 80 byte Little-Endian.
    #[inline]
    pub fn to_bytes(&self) -> [u8; COMPACT_ACCOUNT_ENTRY_SIZE] {
        let mut buf = [0u8; COMPACT_ACCOUNT_ENTRY_SIZE];
        buf[0..32].copy_from_slice(self.account.as_bytes());
        buf[32..48].copy_from_slice(&self.balance.to_le_bytes());
        buf[48..56].copy_from_slice(&self.sequence_number.to_le_bytes());
        buf[56..64].copy_from_slice(&self.epoch.to_le_bytes());
        buf[64..72].copy_from_slice(&self.offset.to_le_bytes());
        buf[72..76].copy_from_slice(&self.segment_index.to_le_bytes());
        buf[76..80].copy_from_slice(&self.flags.to_le_bytes());
        buf
    }

    /// Deserialisasi entri dari array tepat 80 byte Little-Endian.
    #[inline]
    pub fn from_bytes(buf: &[u8; COMPACT_ACCOUNT_ENTRY_SIZE]) -> Self {
        let mut account_bytes = [0u8; 32];
        account_bytes.copy_from_slice(&buf[0..32]);
        let account = AccountId::new(account_bytes);

        let mut balance_bytes = [0u8; 16];
        balance_bytes.copy_from_slice(&buf[32..48]);
        let balance = AurValue::from_le_bytes(balance_bytes);

        let mut seq_bytes = [0u8; 8];
        seq_bytes.copy_from_slice(&buf[48..56]);
        let sequence_number = u64::from_le_bytes(seq_bytes);

        let mut epoch_bytes = [0u8; 8];
        epoch_bytes.copy_from_slice(&buf[56..64]);
        let epoch = u64::from_le_bytes(epoch_bytes);

        let mut offset_bytes = [0u8; 8];
        offset_bytes.copy_from_slice(&buf[64..72]);
        let offset = u64::from_le_bytes(offset_bytes);

        let mut seg_bytes = [0u8; 4];
        seg_bytes.copy_from_slice(&buf[72..76]);
        let segment_index = u32::from_le_bytes(seg_bytes);

        let mut flags_bytes = [0u8; 4];
        flags_bytes.copy_from_slice(&buf[76..80]);
        let flags = u32::from_le_bytes(flags_bytes);

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
}

/// Ukuran tetap biner satu entri akun di memori RAM (80 byte).
pub const COMPACT_ACCOUNT_ENTRY_SIZE: usize = 80;

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
    pub balance: AurValue,
    /// Penunjuk lokasi mutasi terakhir di piringan disk.
    pub location: AccountLocation,
}

impl AccountState {
    /// Membuat AccountState baru.
    #[inline]
    pub const fn new(balance: AurValue, location: AccountLocation) -> Self {
        Self { balance, location }
    }
}
