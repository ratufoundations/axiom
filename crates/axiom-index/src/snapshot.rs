//! Modul persistensi snapshot biner deterministik dan pemulihan cepat (RFC-0002).

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_primitives::value::AxmValue;

use crate::entry::{CompactAccountEntry, COMPACT_ACCOUNT_ENTRY_SIZE};
use crate::error::IndexError;
use crate::keydir::{Keydir, BUCKET_COUNT};

/// Ukuran tetap header snapshot berkas .snap (96 byte).
pub const SNAPSHOT_HEADER_SIZE: usize = 96;

/// Pengenal biner tetap format snapshot Axiom (8 byte: "AXMSNAP\x01").
pub const SNAPSHOT_MAGIC: [u8; 8] = *b"AXMSNAP\x01";

/// Versi format skema snapshot (1).
pub const SNAPSHOT_VERSION: u16 = 1;

/// Ukuran buffer baca/tulis streaming snapshot (128 KB = 131.072 byte).
pub const SNAPSHOT_STREAM_BUFFER_SIZE: usize = 128 * 1024;

/// Header berkas snapshot berukuran tepat 96 byte Little-Endian dengan perataan 16 byte.
///
/// Disusun tanpa celah compiler padding:
/// - `magic`: 8 byte (offset 0..8)
/// - `version`: 2 byte (offset 8..10)
/// - `reserved_pad1`: 6 byte (offset 10..16, pad ke 16B)
/// - `epoch`: 8 byte (offset 16..24)
/// - `segment_index`: 4 byte (offset 24..28)
/// - `reserved_pad2`: 4 byte (offset 28..32, pad ke 32B)
/// - `total_supply`: 16 byte (offset 32..48, align 16)
/// - `total_accounts`: 8 byte (offset 48..56, align 8)
/// - `state_digest`: 32 byte (offset 56..88)
/// - `created_at`: 8 byte (offset 88..96, align 8)
///
/// Total ukuran: tepat 96 byte (kelipatan 16 byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct SnapshotHeader {
    /// Pengenal magic snapshot (*b"AXMSNAP\x01").
    pub magic: [u8; 8],
    /// Versi format skema snapshot.
    pub version: u16,
    /// Padding perataan byte 0x00.
    pub reserved_pad1: [u8; 6],
    /// Nomor epoch saat checkpoint dibuat.
    pub epoch: u64,
    /// Indeks segmen log disk terakhir yang dikomit.
    pub segment_index: u32,
    /// Padding perataan byte 0x00.
    pub reserved_pad2: [u8; 4],
    /// Pasokan total koin moneter global (AxmValue terkuantisasi).
    pub total_supply: u128,
    /// Total akun terdaftar dalam checkpoint (N).
    pub total_accounts: u64,
    /// Intisari BLAKE3 atas seluruh rekaman 80-byte entri.
    pub state_digest: [u8; 32],
    /// Waktu UNIX timestamp pembuatan checkpoint.
    pub created_at: u64,
}

impl SnapshotHeader {
    /// Membuat SnapshotHeader baru dengan magic dan versi baku.
    #[inline]
    pub const fn new(
        epoch: u64,
        segment_index: u32,
        total_accounts: u64,
        total_supply: u128,
        state_digest: [u8; 32],
        created_at: u64,
    ) -> Self {
        Self {
            magic: SNAPSHOT_MAGIC,
            version: SNAPSHOT_VERSION,
            reserved_pad1: [0u8; 6],
            epoch,
            segment_index,
            reserved_pad2: [0u8; 4],
            total_supply,
            total_accounts,
            state_digest,
            created_at,
        }
    }

    /// Serialisasi header ke dalam array tepat 96 byte Little-Endian.
    pub fn to_bytes(&self) -> [u8; SNAPSHOT_HEADER_SIZE] {
        let mut buf = [0u8; SNAPSHOT_HEADER_SIZE];
        buf[0..8].copy_from_slice(&self.magic);
        buf[8..10].copy_from_slice(&self.version.to_le_bytes());
        buf[10..16].copy_from_slice(&self.reserved_pad1);
        buf[16..24].copy_from_slice(&self.epoch.to_le_bytes());
        buf[24..28].copy_from_slice(&self.segment_index.to_le_bytes());
        buf[28..32].copy_from_slice(&self.reserved_pad2);
        buf[32..48].copy_from_slice(&self.total_supply.to_le_bytes());
        buf[48..56].copy_from_slice(&self.total_accounts.to_le_bytes());
        buf[56..88].copy_from_slice(&self.state_digest);
        buf[88..96].copy_from_slice(&self.created_at.to_le_bytes());
        buf
    }

    /// Deserialisasi header dari array 96 byte Little-Endian dengan validasi magic dan versi.
    pub fn from_bytes(buf: &[u8; SNAPSHOT_HEADER_SIZE]) -> Result<Self, IndexError> {
        let mut magic = [0u8; 8];
        magic.copy_from_slice(&buf[0..8]);
        if magic != SNAPSHOT_MAGIC {
            return Err(IndexError::InvalidSnapshotMagic);
        }

        let version = u16::from_le_bytes([buf[8], buf[9]]);
        if version != SNAPSHOT_VERSION {
            return Err(IndexError::CorruptedSnapshotHeader {
                size: SNAPSHOT_HEADER_SIZE,
            });
        }

        let mut reserved_pad1 = [0u8; 6];
        reserved_pad1.copy_from_slice(&buf[10..16]);

        let mut epoch_bytes = [0u8; 8];
        epoch_bytes.copy_from_slice(&buf[16..24]);
        let epoch = u64::from_le_bytes(epoch_bytes);

        let mut seg_bytes = [0u8; 4];
        seg_bytes.copy_from_slice(&buf[24..28]);
        let segment_index = u32::from_le_bytes(seg_bytes);

        let mut reserved_pad2 = [0u8; 4];
        reserved_pad2.copy_from_slice(&buf[28..32]);

        let mut supply_bytes = [0u8; 16];
        supply_bytes.copy_from_slice(&buf[32..48]);
        let total_supply = u128::from_le_bytes(supply_bytes);

        let mut accounts_bytes = [0u8; 8];
        accounts_bytes.copy_from_slice(&buf[48..56]);
        let total_accounts = u64::from_le_bytes(accounts_bytes);

        let mut state_digest = [0u8; 32];
        state_digest.copy_from_slice(&buf[56..88]);

        let mut created_bytes = [0u8; 8];
        created_bytes.copy_from_slice(&buf[88..96]);
        let created_at = u64::from_le_bytes(created_bytes);

        Ok(Self {
            magic,
            version,
            reserved_pad1,
            epoch,
            segment_index,
            reserved_pad2,
            total_supply,
            total_accounts,
            state_digest,
            created_at,
        })
    }
}

/// Menulis snapshot status Keydir ke disk dengan persistensi atomik dua tahap (.tmp -> .snap).
pub fn write_snapshot<P: AsRef<Path>>(
    keydir: &Keydir,
    path: P,
    epoch: u64,
    segment_index: u32,
) -> Result<(), IndexError> {
    let target_path = path.as_ref().to_path_buf();
    let tmp_path = target_path.with_extension("tmp");

    let mut file = File::create(&tmp_path).map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;

    // Sisakan ruang 96 byte untuk header di awal berkas
    file.seek(SeekFrom::Start(SNAPSHOT_HEADER_SIZE as u64))
        .map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;

    let mut writer = BufWriter::with_capacity(SNAPSHOT_STREAM_BUFFER_SIZE, file);
    let mut hasher = blake3::Hasher::new();

    // Stream seluruh entri akun dari 256 bucket secara berurutan
    for bucket_idx in 0..BUCKET_COUNT {
        let bucket = &keydir.buckets[bucket_idx];
        for entry in bucket {
            let entry_bytes = entry.to_bytes();
            hasher.update(&entry_bytes);
            writer
                .write_all(&entry_bytes)
                .map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;
        }
    }

    writer
        .flush()
        .map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;
    let mut file = writer
        .into_inner()
        .map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;

    // Hitung intisari BLAKE3 atas seluruh payload entri
    let state_digest = *hasher.finalize().as_bytes();
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let header = SnapshotHeader::new(
        epoch,
        segment_index,
        keydir.account_count() as u64,
        keydir.total_supply().to_atomic(),
        state_digest,
        created_at,
    );

    // Tulis header di offset 0
    file.seek(SeekFrom::Start(0))
        .map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;
    file.write_all(&header.to_bytes())
        .map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;

    // Sinkronisasi data fisik disk
    file.sync_all()
        .map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;
    drop(file);

    // Ganti nama berkas sementara ke berkas target secara atomik
    if target_path.exists() {
        let _ = fs::remove_file(&target_path);
    }
    fs::rename(&tmp_path, &target_path).map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;

    Ok(())
}

/// Membaca dan merekonstruksi status Keydir dari berkas snapshot disk dalam O(N) streaming.
pub fn read_snapshot<P: AsRef<Path>>(path: P) -> Result<(Keydir, SnapshotHeader), IndexError> {
    let target_path = path.as_ref().to_path_buf();
    let mut file =
        File::open(&target_path).map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;

    let meta = file
        .metadata()
        .map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;
    let file_len = meta.len();

    // Validasi panjang minimum header
    if file_len < SNAPSHOT_HEADER_SIZE as u64 {
        return Err(IndexError::CorruptedSnapshotHeader {
            size: file_len as usize,
        });
    }

    let mut header_buf = [0u8; SNAPSHOT_HEADER_SIZE];
    file.read_exact(&mut header_buf)
        .map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;

    let header = SnapshotHeader::from_bytes(&header_buf)?;

    // Validasi panjang berkas fisik terhadap ekspektasi payload
    let expected_payload_len = header
        .total_accounts
        .checked_mul(COMPACT_ACCOUNT_ENTRY_SIZE as u64)
        .ok_or(IndexError::ArithmeticOverflow)?;
    let expected_file_len = (SNAPSHOT_HEADER_SIZE as u64)
        .checked_add(expected_payload_len)
        .ok_or(IndexError::ArithmeticOverflow)?;

    if file_len != expected_file_len {
        return Err(IndexError::CorruptedSnapshotHeader {
            size: file_len as usize,
        });
    }

    let mut reader = BufReader::with_capacity(SNAPSHOT_STREAM_BUFFER_SIZE, file);
    let mut hasher = blake3::Hasher::new();
    let mut keydir = Keydir::new();
    let mut entry_buf = [0u8; COMPACT_ACCOUNT_ENTRY_SIZE];

    // Rekonstruksi 256 bucket secara langsung tanpa pergeseran pengurutan (O(N) streaming)
    for _ in 0..header.total_accounts {
        reader
            .read_exact(&mut entry_buf)
            .map_err(|e| IndexError::SnapshotIoError(e.to_string()))?;
        hasher.update(&entry_buf);

        let entry = CompactAccountEntry::from_bytes(&entry_buf);
        let b_idx = Keydir::bucket_index_for(&entry.account);
        keydir.buckets[b_idx].push(entry);
    }

    // Set agregasi O(1) dari header terverifikasi
    keydir.total_accounts = header.total_accounts as usize;
    keydir.total_supply = AxmValue::from_atomic(header.total_supply);

    // Verifikasi intisari BLAKE3
    let actual_digest = *hasher.finalize().as_bytes();
    if actual_digest != header.state_digest {
        return Err(IndexError::SnapshotChecksumMismatch {
            expected: header.state_digest,
            actual: actual_digest,
        });
    }

    Ok((keydir, header))
}
