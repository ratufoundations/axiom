//! Modul penyimpanan dingin berbasis disk (ColdStore) dengan penataan partisi 256-bucket.

use std::fs::{self, File};
use std::sync::Mutex;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use axiom_primitives::crypto::AccountId;

use crate::entry::{CompactAccountEntry, COMPACT_ACCOUNT_ENTRY_SIZE};
use crate::error::IndexError;
use crate::keydir::BUCKET_COUNT;

/// Pengenal biner berkas indeks dingin Axiom (8 byte: "AXMCOLD\x01").
pub const COLD_MAGIC: [u8; 8] = *b"AXMCOLD\x01";

/// Versi format skema indeks dingin (1).
pub const COLD_VERSION: u16 = 1;

/// Ukuran fisik satu deskriptor bucket (16 byte: offset u64 + count u32 + reserved u32).
pub const BUCKET_DESCRIPTOR_SIZE: usize = 16;

/// Ukuran tetap header berkas indeks dingin di disk (4.112 byte).
///
/// Komposisi:
/// - magic: 8 byte
/// - version: 2 byte
/// - reserved: 6 byte
/// - 256 bucket descriptors: 256 * 16 = 4.096 byte
///
/// Total: 8 + 2 + 6 + 4.096 = 4.112 byte (kelipatan 16 byte).
pub const COLD_HEADER_SIZE: usize = 8 + 2 + 6 + (BUCKET_COUNT * BUCKET_DESCRIPTOR_SIZE);

/// Deskriptor partisi bucket pada media simpan dingin di disk (16 byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct BucketDescriptor {
    /// Posisi byte offset tempat partisi bucket dimulai pada berkas.
    pub offset: u64,
    /// Jumlah entri 80-byte yang tersimpan di dalam bucket ini.
    pub entry_count: u32,
    /// Ruang cadangan perataan biner.
    pub reserved: u32,
}

impl BucketDescriptor {
    /// Deskriptor kosong (offset 0, entry_count 0).
    pub const ZERO: Self = Self {
        offset: 0,
        entry_count: 0,
        reserved: 0,
    };

    /// Serialisasi deskriptor ke dalam 16 byte Little-Endian.
    #[inline]
    pub fn to_bytes(&self) -> [u8; BUCKET_DESCRIPTOR_SIZE] {
        let mut buf = [0u8; BUCKET_DESCRIPTOR_SIZE];
        buf[0..8].copy_from_slice(&self.offset.to_le_bytes());
        buf[8..12].copy_from_slice(&self.entry_count.to_le_bytes());
        buf[12..16].copy_from_slice(&self.reserved.to_le_bytes());
        buf
    }

    /// Deserialisasi deskriptor dari 16 byte Little-Endian.
    #[inline]
    pub fn from_bytes(buf: &[u8; BUCKET_DESCRIPTOR_SIZE]) -> Self {
        let mut off_bytes = [0u8; 8];
        off_bytes.copy_from_slice(&buf[0..8]);
        let offset = u64::from_le_bytes(off_bytes);

        let mut cnt_bytes = [0u8; 4];
        cnt_bytes.copy_from_slice(&buf[8..12]);
        let entry_count = u32::from_le_bytes(cnt_bytes);

        let mut res_bytes = [0u8; 4];
        res_bytes.copy_from_slice(&buf[12..16]);
        let reserved = u32::from_le_bytes(res_bytes);

        Self {
            offset,
            entry_count,
            reserved,
        }
    }
}

/// Pengelola indeks akun pasif pada media simpan dingin (ColdStore).
#[derive(Debug)]
pub struct ColdStore {
    file: Mutex<Option<File>>,
    path: PathBuf,
    descriptors: [BucketDescriptor; BUCKET_COUNT],
    total_cold_accounts: u64,
}

impl ColdStore {
    /// Membuka berkas cold storage yang ada atau membuat baru jika belum ada.
    pub fn create_or_open<P: AsRef<Path>>(path: P) -> Result<Self, IndexError> {
        let path_buf = path.as_ref().to_path_buf();
        if path_buf.exists() {
            Self::open(path_buf)
        } else {
            Self::create(path_buf)
        }
    }

    /// Membuat berkas cold store baru dengan header 4.112 byte kosong.
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self, IndexError> {
        let path_buf = path.as_ref().to_path_buf();
        let mut file = File::create(&path_buf)
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;

        let mut descriptors = [BucketDescriptor::ZERO; BUCKET_COUNT];
        for d in &mut descriptors {
            d.offset = COLD_HEADER_SIZE as u64;
        }

        let mut header_buf = vec![0u8; COLD_HEADER_SIZE];
        header_buf[0..8].copy_from_slice(&COLD_MAGIC);
        header_buf[8..10].copy_from_slice(&COLD_VERSION.to_le_bytes());

        for (b, desc) in descriptors.iter().enumerate() {
            let start = 16 + b * BUCKET_DESCRIPTOR_SIZE;
            header_buf[start..start + BUCKET_DESCRIPTOR_SIZE].copy_from_slice(&desc.to_bytes());
        }

        file.write_all(&header_buf)
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;
        file.sync_all()
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;

        let file_reopened = File::options()
            .read(true)
            .write(true)
            .open(&path_buf)
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;

        Ok(Self {
            file: Mutex::new(Some(file_reopened)),
            path: path_buf,
            descriptors,
            total_cold_accounts: 0,
        })
    }

    /// Membuka berkas cold store yang ada dan memvalidasi integritas strukturnya.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, IndexError> {
        let path_buf = path.as_ref().to_path_buf();
        let mut file = File::options()
            .read(true)
            .write(true)
            .open(&path_buf)
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;

        let meta = file
            .metadata()
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;
        let file_len = meta.len();

        if file_len < COLD_HEADER_SIZE as u64 {
            return Err(IndexError::CorruptedColdIndex(format!(
                "Cold index file size smaller than header: {file_len} < {COLD_HEADER_SIZE}"
            )));
        }

        let mut header_buf = vec![0u8; COLD_HEADER_SIZE];
        file.read_exact(&mut header_buf)
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;

        if header_buf[0..8] != COLD_MAGIC {
            return Err(IndexError::CorruptedColdIndex(
                "Invalid cold storage magic bytes".to_string(),
            ));
        }

        let version = u16::from_le_bytes([header_buf[8], header_buf[9]]);
        if version != COLD_VERSION {
            return Err(IndexError::CorruptedColdIndex(format!(
                "Unsupported cold storage version: {version}"
            )));
        }

        let mut descriptors = [BucketDescriptor::ZERO; BUCKET_COUNT];
        let mut total_cold_accounts = 0u64;

        for (b, desc_slot) in descriptors.iter_mut().enumerate() {
            let start = 16 + b * BUCKET_DESCRIPTOR_SIZE;
            let mut desc_bytes = [0u8; BUCKET_DESCRIPTOR_SIZE];
            desc_bytes.copy_from_slice(&header_buf[start..start + BUCKET_DESCRIPTOR_SIZE]);
            let desc = BucketDescriptor::from_bytes(&desc_bytes);
            *desc_slot = desc;
            total_cold_accounts = total_cold_accounts
                .checked_add(desc.entry_count as u64)
                .ok_or(IndexError::ArithmeticOverflow)?;
        }

        let expected_min_len = (COLD_HEADER_SIZE as u64)
            .checked_add(
                total_cold_accounts
                    .checked_mul(COMPACT_ACCOUNT_ENTRY_SIZE as u64)
                    .ok_or(IndexError::ArithmeticOverflow)?,
            )
            .ok_or(IndexError::ArithmeticOverflow)?;

        if file_len < expected_min_len {
            return Err(IndexError::CorruptedColdIndex(format!(
                "Cold index file truncated: expected at least {expected_min_len} bytes, found {file_len}"
            )));
        }

        Ok(Self {
            file: Mutex::new(Some(file)),
            path: path_buf,
            descriptors,
            total_cold_accounts,
        })
    }

    /// Membaca entri akun tertentu via on-disk binary search O(log2 M) seek.
    pub fn get_account(
        &self,
        account: &AccountId,
    ) -> Result<Option<CompactAccountEntry>, IndexError> {
        let b = account.as_bytes()[0] as usize;
        let desc = self.descriptors[b];
        if desc.entry_count == 0 {
            return Ok(None);
        }

        let mut low = 0u32;
        let mut high = desc.entry_count;
        let mut entry_buf = [0u8; COMPACT_ACCOUNT_ENTRY_SIZE];
        let mut file_guard = self
            .file
            .lock()
            .map_err(|_| IndexError::ColdStoreIoError("Failed to acquire cold store lock".to_string()))?;
        let file = file_guard
            .as_mut()
            .ok_or_else(|| IndexError::ColdStoreIoError("Cold store file handle is closed".to_string()))?;

        while low < high {
            let mid = low + (high - low) / 2;
            let pos = desc
                .offset
                .checked_add(
                    (mid as u64)
                        .checked_mul(COMPACT_ACCOUNT_ENTRY_SIZE as u64)
                        .ok_or(IndexError::ArithmeticOverflow)?,
                )
                .ok_or(IndexError::ArithmeticOverflow)?;

            file.seek(SeekFrom::Start(pos))
                .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;
            file.read_exact(&mut entry_buf)
                .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;

            let entry = CompactAccountEntry::from_bytes(&entry_buf);

            match entry.account.cmp(account) {
                std::cmp::Ordering::Equal => return Ok(Some(entry)),
                std::cmp::Ordering::Less => low = mid + 1,
                std::cmp::Ordering::Greater => high = mid,
            }
        }

        Ok(None)
    }

    /// Membaca seluruh entri dari satu partisi bucket tertentu.
    fn read_bucket_entries(
        &self,
        b: usize,
    ) -> Result<Vec<CompactAccountEntry>, IndexError> {
        let desc = self.descriptors[b];
        if desc.entry_count == 0 {
            return Ok(Vec::new());
        }

        let mut file_guard = self
            .file
            .lock()
            .map_err(|_| IndexError::ColdStoreIoError("Failed to acquire cold store lock".to_string()))?;
        let file = file_guard
            .as_mut()
            .ok_or_else(|| IndexError::ColdStoreIoError("Cold store file handle is closed".to_string()))?;
        file.seek(SeekFrom::Start(desc.offset))
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;

        let mut entries = Vec::with_capacity(desc.entry_count as usize);
        let mut entry_buf = [0u8; COMPACT_ACCOUNT_ENTRY_SIZE];

        for _ in 0..desc.entry_count {
            file.read_exact(&mut entry_buf)
                .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;
            entries.push(CompactAccountEntry::from_bytes(&entry_buf));
        }

        Ok(entries)
    }

    /// Menyimpan entri-entri baru hasil evakuasi RAM ke media simpan dingin secara atomik.
    pub fn flush_evicted_entries(
        &mut self,
        entries: &[CompactAccountEntry],
    ) -> Result<(), IndexError> {
        let mut all_buckets: [Vec<CompactAccountEntry>; BUCKET_COUNT] =
            core::array::from_fn(|_| Vec::new());

        // 1. Baca entri yang sudah ada di disk untuk seluruh 256 bucket
        for (b, bucket) in all_buckets.iter_mut().enumerate() {
            *bucket = self.read_bucket_entries(b)?;
        }

        // 2. Sisipkan entri baru hasil evakuasi ke bucket yang sesuai dalam urutan tersortir
        for entry in entries {
            let b = entry.account.as_bytes()[0] as usize;
            match all_buckets[b].binary_search_by(|e| e.account.cmp(&entry.account)) {
                Ok(idx) => all_buckets[b][idx] = *entry,
                Err(idx) => all_buckets[b].insert(idx, *entry),
            }
        }

        // 3. Tulis ulang berkas cold store secara atomik (.tmp -> path)
        self.rewrite_all_buckets(&all_buckets)
    }

    /// Menghapus satu entri akun dingin dari disk (misal saat ditarik kembali / paged in ke RAM).
    pub fn remove_entry(
        &mut self,
        account: &AccountId,
    ) -> Result<Option<CompactAccountEntry>, IndexError> {
        let found = self.get_account(account)?;
        let removed_entry = match found {
            Some(e) => e,
            None => return Ok(None),
        };

        let target_b = account.as_bytes()[0] as usize;
        let mut all_buckets: [Vec<CompactAccountEntry>; BUCKET_COUNT] =
            core::array::from_fn(|_| Vec::new());

        for (b, bucket) in all_buckets.iter_mut().enumerate() {
            let mut entries = self.read_bucket_entries(b)?;
            if b == target_b {
                entries.retain(|e| e.account != *account);
            }
            *bucket = entries;
        }

        self.rewrite_all_buckets(&all_buckets)?;
        Ok(Some(removed_entry))
    }

    /// Menulis ulang seluruh 256 bucket ke berkas sementara lalu mengganti secara atomik.
    fn rewrite_all_buckets(
        &mut self,
        all_buckets: &[Vec<CompactAccountEntry>; BUCKET_COUNT],
    ) -> Result<(), IndexError> {
        let tmp_path = self.path.with_extension("tmp");
        let mut tmp_file = File::create(&tmp_path)
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;

        // Hitung deskriptor baru
        let mut new_descriptors = [BucketDescriptor::ZERO; BUCKET_COUNT];
        let mut current_offset = COLD_HEADER_SIZE as u64;
        let mut total_accounts = 0u64;

        for b in 0..BUCKET_COUNT {
            let count = all_buckets[b].len() as u32;
            new_descriptors[b] = BucketDescriptor {
                offset: current_offset,
                entry_count: count,
                reserved: 0,
            };
            current_offset = current_offset
                .checked_add(
                    (count as u64)
                        .checked_mul(COMPACT_ACCOUNT_ENTRY_SIZE as u64)
                        .ok_or(IndexError::ArithmeticOverflow)?,
                )
                .ok_or(IndexError::ArithmeticOverflow)?;
            total_accounts = total_accounts
                .checked_add(count as u64)
                .ok_or(IndexError::ArithmeticOverflow)?;
        }

        // Tulis header 4.112 byte
        let mut header_buf = vec![0u8; COLD_HEADER_SIZE];
        header_buf[0..8].copy_from_slice(&COLD_MAGIC);
        header_buf[8..10].copy_from_slice(&COLD_VERSION.to_le_bytes());

        for (b, desc) in new_descriptors.iter().enumerate() {
            let start = 16 + b * BUCKET_DESCRIPTOR_SIZE;
            header_buf[start..start + BUCKET_DESCRIPTOR_SIZE].copy_from_slice(&desc.to_bytes());
        }

        tmp_file
            .write_all(&header_buf)
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;

        // Tulis entri seluruh bucket secara berurutan kontigu
        for bucket in all_buckets {
            for entry in bucket {
                let entry_bytes = entry.to_bytes();
                tmp_file
                    .write_all(&entry_bytes)
                    .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;
            }
        }

        tmp_file
            .sync_all()
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;
        drop(tmp_file);

        // Tutup file lama sebelum me-rename pada Windows
        {
            let mut file_guard = self
                .file
                .lock()
                .map_err(|_| IndexError::ColdStoreIoError("Failed to acquire cold store lock".to_string()))?;
            *file_guard = None;
        }

        if self.path.exists() {
            let _ = fs::remove_file(&self.path);
        }
        fs::rename(&tmp_path, &self.path)
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;

        // Buka kembali file yang telah diperbarui
        let reopened = File::options()
            .read(true)
            .write(true)
            .open(&self.path)
            .map_err(|e| IndexError::ColdStoreIoError(e.to_string()))?;

        {
            let mut file_guard = self
                .file
                .lock()
                .map_err(|_| IndexError::ColdStoreIoError("Failed to acquire cold store lock".to_string()))?;
            *file_guard = Some(reopened);
        }

        self.descriptors = new_descriptors;
        self.total_cold_accounts = total_accounts;

        Ok(())
    }

    /// Total akun yang tersimpan dalam media simpan dingin.
    #[inline]
    pub fn total_cold_accounts(&self) -> u64 {
        self.total_cold_accounts
    }

    /// Deskriptor 256 bucket di media simpan dingin.
    #[inline]
    pub fn descriptors(&self) -> &[BucketDescriptor; BUCKET_COUNT] {
        &self.descriptors
    }

    /// Jalur berkas fisik media simpan dingin.
    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }
}
