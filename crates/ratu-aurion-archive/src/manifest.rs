//! Modul manifes deterministik untuk berkas arsip bulanan (.zip).

use ratu_aurion_primitives::crypto::Hash;

use crate::error::ArchiveError;

/// Pengenal biner manifes arsip (4 byte: "AXAM").
pub const MANIFEST_MAGIC: [u8; 4] = *b"AXAM";

/// Ukuran pasti header manifes arsip dalam byte (80 byte).
pub const MANIFEST_HEADER_SIZE: usize = 80;

/// Header biner manifes arsip berukuran tetap 80 byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArchiveManifestHeader {
    /// Magic pengenal manifes (wajib b"AXAM").
    pub magic: [u8; 4],
    /// Versi format arsip.
    pub version: u16,
    /// Nomor epoch segmen yang diarsipkan.
    pub epoch: u64,
    /// Total akun unik yang tercatat di dalam arsip ini.
    pub total_accounts: u32,
    /// Total rekaman mutasi yang dikemas.
    pub total_records: u64,
    /// Digest status komitmen segmen (32 byte, dicocokkan dari sealed footer).
    pub state_digest: Hash,
    /// Waktu pembuatan arsip dalam detik epoch Unix.
    pub archived_at: u64,
    /// Ruang cadangan penyesuaian batas struktur 80 byte (14 byte).
    pub reserved: [u8; 14],
}

impl ArchiveManifestHeader {
    /// Membuat ArchiveManifestHeader baru dengan magic bawaan.
    pub fn new(
        version: u16,
        epoch: u64,
        total_accounts: u32,
        total_records: u64,
        state_digest: Hash,
        archived_at: u64,
    ) -> Self {
        Self {
            magic: MANIFEST_MAGIC,
            version,
            epoch,
            total_accounts,
            total_records,
            state_digest,
            archived_at,
            reserved: [0u8; 14],
        }
    }

    /// Serialisasi header manifes ke dalam array 80 byte (Little-Endian).
    pub fn to_bytes(&self) -> [u8; MANIFEST_HEADER_SIZE] {
        let mut buf = [0u8; MANIFEST_HEADER_SIZE];
        buf[0..4].copy_from_slice(&self.magic);
        buf[4..6].copy_from_slice(&self.version.to_le_bytes());
        buf[6..14].copy_from_slice(&self.epoch.to_le_bytes());
        buf[14..18].copy_from_slice(&self.total_accounts.to_le_bytes());
        buf[18..26].copy_from_slice(&self.total_records.to_le_bytes());
        buf[26..58].copy_from_slice(self.state_digest.as_bytes());
        buf[58..66].copy_from_slice(&self.archived_at.to_le_bytes());
        buf[66..80].copy_from_slice(&self.reserved);
        buf
    }

    /// Deserialisasi dan validasi header manifes dari array 80 byte.
    pub fn from_bytes(bytes: &[u8; MANIFEST_HEADER_SIZE]) -> Result<Self, ArchiveError> {
        let magic_slice = &bytes[0..4];
        if magic_slice != MANIFEST_MAGIC {
            return Err(ArchiveError::InvalidMagic);
        }

        let mut magic = [0u8; 4];
        magic.copy_from_slice(magic_slice);

        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        let epoch = u64::from_le_bytes([
            bytes[6], bytes[7], bytes[8], bytes[9],
            bytes[10], bytes[11], bytes[12], bytes[13],
        ]);
        let total_accounts = u32::from_le_bytes([
            bytes[14], bytes[15], bytes[16], bytes[17],
        ]);
        let total_records = u64::from_le_bytes([
            bytes[18], bytes[19], bytes[20], bytes[21],
            bytes[22], bytes[23], bytes[24], bytes[25],
        ]);

        let mut digest_bytes = [0u8; 32];
        digest_bytes.copy_from_slice(&bytes[26..58]);
        let state_digest = Hash::new(digest_bytes);

        let archived_at = u64::from_le_bytes([
            bytes[58], bytes[59], bytes[60], bytes[61],
            bytes[62], bytes[63], bytes[64], bytes[65],
        ]);

        let mut reserved = [0u8; 14];
        reserved.copy_from_slice(&bytes[66..80]);

        Ok(Self {
            magic,
            version,
            epoch,
            total_accounts,
            total_records,
            state_digest,
            archived_at,
            reserved,
        })
    }
}
