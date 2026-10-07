//! Modul framing paket data deterministik untuk protokol Axiom.

use crate::crypto::Hash;

/// Pengenal paket data Axiom (4 byte: "AXM\x01").
pub const MAGIC_BYTES: [u8; 4] = *b"AXM\x01";

/// Ukuran tetap header frame protokol (42 byte).
///
/// Komposisi:
/// - Magic: 4 byte
/// - Version: 2 byte
/// - PayloadLen: 4 byte
/// - Checksum: 32 byte
pub const HEADER_SIZE: usize = 42;

/// Header bingkai transmisi atau penulisan data Axiom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameHeader {
    /// Nilai pengenal bingkai protokol (wajib b"AXM\x01").
    pub magic: [u8; 4],
    /// Versi protokol transmisi.
    pub version: u16,
    /// Panjang payload data biner yang mengikuti header (Little-Endian).
    pub payload_len: u32,
    /// Hash ringkasan atau checksum payload.
    pub checksum: Hash,
}

impl FrameHeader {
    /// Membuat FrameHeader baru dengan magic bytes default.
    #[inline]
    pub const fn new(version: u16, payload_len: u32, checksum: Hash) -> Self {
        Self {
            magic: MAGIC_BYTES,
            version,
            payload_len,
            checksum,
        }
    }

    /// Serialisasi header ke dalam representasi biner tepat 42 byte (Little-Endian).
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..4].copy_from_slice(&self.magic);
        buf[4..6].copy_from_slice(&self.version.to_le_bytes());
        buf[6..10].copy_from_slice(&self.payload_len.to_le_bytes());
        buf[10..42].copy_from_slice(self.checksum.as_bytes());
        buf
    }

    /// Deserialisasi dan validasi header dari array 42 byte.
    ///
    /// Mengembalikan galat jika magic bytes tidak cocok dengan `MAGIC_BYTES`.
    pub fn from_bytes(bytes: &[u8; HEADER_SIZE]) -> Result<Self, &'static str> {
        let magic_slice = &bytes[0..4];
        if magic_slice != MAGIC_BYTES {
            return Err("Invalid frame magic bytes");
        }


        let mut magic = [0u8; 4];
        magic.copy_from_slice(magic_slice);

        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        let payload_len = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);

        let mut checksum_bytes = [0u8; 32];
        checksum_bytes.copy_from_slice(&bytes[10..42]);
        let checksum = Hash::new(checksum_bytes);

        Ok(Self {
            magic,
            version,
            payload_len,
            checksum,
        })
    }
}
