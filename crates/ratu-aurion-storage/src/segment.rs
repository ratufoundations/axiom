//! Modul definisi tata letak biner segmen penyimpanan (header dan footer).

use ratu_aurion_primitives::crypto::Hash;

/// Batas kapasitas maksimal satu segmen log (128 MB = 134,217,728 byte).
pub const MAX_SEGMENT_SIZE: u64 = 128 * 1024 * 1024;

/// Ukuran tetap header berkas segmen (42 byte).
pub const SEGMENT_HEADER_SIZE: usize = 42;

/// Ukuran tetap catatan kaki segmen saat disegel (88 byte).
pub const SEGMENT_FOOTER_SIZE: usize = 88;

/// Pengenal biner header segmen (4 byte: "RAUR").
pub const SEGMENT_HEADER_MAGIC: [u8; 4] = *b"RAUR";

/// Pengenal biner footer segmen (8 byte: "RAUREND\x01").
pub const SEGMENT_FOOTER_MAGIC: [u8; 8] = *b"RAUREND\x01";

/// Header berkas segmen penyimpanan berukuran tetap 42 byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegmentHeader {
    /// Pengenal magic header segmen (wajib b"RAUR").
    pub magic: [u8; 4],
    /// Versi format segmen.
    pub version: u16,
    /// Penanda nomor epoch segmen (8 byte).
    pub epoch: u64,
    /// Indeks urutan segmen di dalam epoch yang sama (4 byte).
    pub segment_index: u32,
    /// Ruang cadangan untuk ekstensi mendatang (24 byte).
    pub reserved: [u8; 24],
}

impl SegmentHeader {
    /// Membuat SegmentHeader baru dengan magic bawaan.
    #[inline]
    pub const fn new(version: u16, epoch: u64, segment_index: u32) -> Self {
        Self {
            magic: SEGMENT_HEADER_MAGIC,
            version,
            epoch,
            segment_index,
            reserved: [0u8; 24],
        }
    }

    /// Serialisasi header ke dalam array tepat 42 byte (Little-Endian).
    pub fn to_bytes(&self) -> [u8; SEGMENT_HEADER_SIZE] {
        let mut buf = [0u8; SEGMENT_HEADER_SIZE];
        buf[0..4].copy_from_slice(&self.magic);
        buf[4..6].copy_from_slice(&self.version.to_le_bytes());
        buf[6..14].copy_from_slice(&self.epoch.to_le_bytes());
        buf[14..18].copy_from_slice(&self.segment_index.to_le_bytes());
        buf[18..42].copy_from_slice(&self.reserved);
        buf
    }

    /// Deserialisasi dan validasi header segmen dari 42 byte.
    pub fn from_bytes(bytes: &[u8; SEGMENT_HEADER_SIZE]) -> Result<Self, &'static str> {
        let magic_slice = &bytes[0..4];
        if magic_slice != SEGMENT_HEADER_MAGIC {
            return Err("Invalid segment header magic bytes");
        }

        let mut magic = [0u8; 4];
        magic.copy_from_slice(magic_slice);

        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        let epoch = u64::from_le_bytes([
            bytes[6], bytes[7], bytes[8], bytes[9],
            bytes[10], bytes[11], bytes[12], bytes[13],
        ]);
        let segment_index = u32::from_le_bytes([
            bytes[14], bytes[15], bytes[16], bytes[17],
        ]);

        let mut reserved = [0u8; 24];
        reserved.copy_from_slice(&bytes[18..42]);

        Ok(Self {
            magic,
            version,
            epoch,
            segment_index,
            reserved,
        })
    }
}

/// Catatan kaki segmen penyimpanan berukuran tetap 88 byte saat disegel (*sealed footer*).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegmentFooter {
    /// Total jumlah record yang tersimpan di dalam segmen.
    pub total_records: u64,
    /// Nomor epoch segmen.
    pub epoch: u64,
    /// Nomor urut mutasi pertama di dalam segmen ini.
    pub first_sequence: u64,
    /// Nomor urut mutasi terakhir di dalam segmen ini.
    pub last_sequence: u64,
    /// Digest status komitmen kriptografis segmen (32 byte).
    pub state_digest: Hash,
    /// Waktu penyegelan segmen dalam detik epoch Unix (8 byte).
    pub sealed_at: u64,
    /// Ruang cadangan penyesuaian batas struktur (8 byte).
    pub reserved: [u8; 8],
    /// Pengenal magic footer segmen (8 byte: "RAUREND\x01").
    pub seal_magic: [u8; 8],
}

impl SegmentFooter {
    /// Membuat SegmentFooter baru dengan magic bawaan.
    pub fn new(
        total_records: u64,
        epoch: u64,
        first_sequence: u64,
        last_sequence: u64,
        state_digest: Hash,
        sealed_at: u64,
    ) -> Self {
        Self {
            total_records,
            epoch,
            first_sequence,
            last_sequence,
            state_digest,
            sealed_at,
            reserved: [0u8; 8],
            seal_magic: SEGMENT_FOOTER_MAGIC,
        }
    }

    /// Serialisasi footer ke dalam array tepat 88 byte (Little-Endian).
    pub fn to_bytes(&self) -> [u8; SEGMENT_FOOTER_SIZE] {
        let mut buf = [0u8; SEGMENT_FOOTER_SIZE];
        buf[0..8].copy_from_slice(&self.total_records.to_le_bytes());
        buf[8..16].copy_from_slice(&self.epoch.to_le_bytes());
        buf[16..24].copy_from_slice(&self.first_sequence.to_le_bytes());
        buf[24..32].copy_from_slice(&self.last_sequence.to_le_bytes());
        buf[32..64].copy_from_slice(self.state_digest.as_bytes());
        buf[64..72].copy_from_slice(&self.sealed_at.to_le_bytes());
        buf[72..80].copy_from_slice(&self.reserved);
        buf[80..88].copy_from_slice(&self.seal_magic);
        buf
    }

    /// Deserialisasi dan validasi footer segmen dari 88 byte.
    pub fn from_bytes(bytes: &[u8; SEGMENT_FOOTER_SIZE]) -> Result<Self, &'static str> {
        let magic_slice = &bytes[80..88];
        if magic_slice != SEGMENT_FOOTER_MAGIC {
            return Err("Invalid segment footer magic bytes");
        }

        let total_records = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let epoch = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11],
            bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        let first_sequence = u64::from_le_bytes([
            bytes[16], bytes[17], bytes[18], bytes[19],
            bytes[20], bytes[21], bytes[22], bytes[23],
        ]);
        let last_sequence = u64::from_le_bytes([
            bytes[24], bytes[25], bytes[26], bytes[27],
            bytes[28], bytes[29], bytes[30], bytes[31],
        ]);

        let mut digest_bytes = [0u8; 32];
        digest_bytes.copy_from_slice(&bytes[32..64]);
        let state_digest = Hash::new(digest_bytes);

        let sealed_at = u64::from_le_bytes([
            bytes[64], bytes[65], bytes[66], bytes[67],
            bytes[68], bytes[69], bytes[70], bytes[71],
        ]);

        let mut reserved = [0u8; 8];
        reserved.copy_from_slice(&bytes[72..80]);

        let mut seal_magic = [0u8; 8];
        seal_magic.copy_from_slice(magic_slice);

        Ok(Self {
            total_records,
            epoch,
            first_sequence,
            last_sequence,
            state_digest,
            sealed_at,
            reserved,
            seal_magic,
        })
    }
}
