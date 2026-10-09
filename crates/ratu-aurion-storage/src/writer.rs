//! Modul penulis sekuensial linear (append-only) untuk segmen log Axiom
//! dengan manajemen buffer memori dan kebijakan durabilitas hardware.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use ratu_aurion_primitives::crypto::Hash;
use ratu_aurion_primitives::record::{MutationRecord, RECORD_SIZE};

use crate::error::StorageError;
use crate::segment::{
    SegmentFooter, SegmentHeader, MAX_SEGMENT_SIZE, SEGMENT_FOOTER_SIZE, SEGMENT_HEADER_SIZE,
};

/// Kapasitas buffer tulis memori terbuffer (128 KB = 131.072 byte).
pub const WRITE_BUFFER_CAPACITY: usize = 128 * 1024;

/// Kebijakan durabilitas dan sinkronisasi hardware disk untuk operasi append log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurabilityPolicy {
    /// Setiap record mutasi langsung ditulis dan disinkronisasi ke disk fisik via sync_data().
    Strict,
    /// Menahan mutasi dalam buffer memori dan mengeksekusi sync_data() setiap ambang batch_size tercapai.
    GroupCommit { batch_size: u32 },
    /// Mengandalkan buffer memori 128 KB tanpa fsync eksplisit per mutasi; flush otomatis saat buffer penuh.
    BufferedRelaxed,
}

/// Penulis segmen log append-only berkapasitas tetap (maks 128 MB) dengan manajemen buffer dan durabilitas.
pub struct SegmentWriter {
    file: File,
    header: SegmentHeader,
    current_offset: u64,
    flushed_offset: u64,
    total_records: u64,
    first_sequence: u64,
    last_sequence: u64,
    is_sealed: bool,
    policy: DurabilityPolicy,
    write_buffer: Vec<u8>,
    uncommitted_records: u32,
}

fn is_valid_committed_record(buf: &[u8; RECORD_SIZE]) -> bool {
    if buf.iter().all(|&b| b == 0) {
        return false;
    }
    let record_kind = buf[16];
    if record_kind != ratu_aurion_primitives::record::RECORD_KIND_TRANSFER
        && record_kind != ratu_aurion_primitives::record::RECORD_KIND_SYSTEM_NOTIF
    {
        return false;
    }
    // Signature bytes (97..161, 64 byte) pada record sah tidak boleh bernilai seluruhnya nol
    if buf[97..161].iter().all(|&b| b == 0) {
        return false;
    }
    true
}

impl SegmentWriter {
    /// Membuat berkas segmen baru dengan kebijakan durabilitas eksplisit
    /// dan pra-alokasi instan 128 MB.
    pub fn create_with_policy<P: AsRef<Path>>(
        path: P,
        version: u16,
        epoch: u64,
        segment_index: u32,
        policy: DurabilityPolicy,
    ) -> Result<Self, StorageError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        // Pre-alokasi 128 MB (134.217.728 byte) segera untuk mencegah fragmentasi berkas
        file.set_len(MAX_SEGMENT_SIZE)?;

        let header = SegmentHeader::new(version, epoch, segment_index);
        let header_bytes = header.to_bytes();
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header_bytes)?;
        file.sync_all()?;

        let current_offset = SEGMENT_HEADER_SIZE as u64;

        Ok(Self {
            file,
            header,
            current_offset,
            flushed_offset: current_offset,
            total_records: 0,
            first_sequence: 0,
            last_sequence: 0,
            is_sealed: false,
            policy,
            write_buffer: Vec::with_capacity(WRITE_BUFFER_CAPACITY),
            uncommitted_records: 0,
        })
    }

    /// Membuat berkas segmen baru pada jalur `path` dengan pra-alokasi instan 128 MB
    /// dan kebijakan default GroupCommit(batch_size: 100).
    pub fn create<P: AsRef<Path>>(
        path: P,
        version: u16,
        epoch: u64,
        segment_index: u32,
    ) -> Result<Self, StorageError> {
        Self::create_with_policy(
            path,
            version,
            epoch,
            segment_index,
            DurabilityPolicy::GroupCommit { batch_size: 100 },
        )
    }

    /// Menulis seluruh data di buffer memori ke berkas fisik tanpa memanggil sync_data.
    pub fn flush_buffer(&mut self) -> Result<(), StorageError> {
        if self.write_buffer.is_empty() {
            return Ok(());
        }

        self.file.seek(SeekFrom::Start(self.flushed_offset))?;
        self.file.write_all(&self.write_buffer)?;

        let buffer_len = self.write_buffer.len() as u64;
        self.flushed_offset = self
            .flushed_offset
            .checked_add(buffer_len)
            .ok_or(StorageError::OutOfBounds)?;

        self.write_buffer.clear();
        Ok(())
    }

    /// Mengosongkan buffer memori ke disk dan mengeksekusi hardware sync_data.
    pub fn flush_and_sync(&mut self) -> Result<(), StorageError> {
        self.flush_buffer()?;
        self.file.sync_data()?;
        self.uncommitted_records = 0;
        Ok(())
    }

    /// Membuka berkas segmen yang sudah ada dan melakukan pemulihan torn-write jika terjadi crash.
    ///
    /// Menegakkan invarian ukuran berkas:
    /// - Pada segmen bersegel (sealed): memuat metadata footer dan validitas payload.
    /// - Pada segmen pra-alokasi 128 MB: melakukan binary search scan atas nol biner (0x00)
    ///   untuk menemukan batas slot record terakhir yang sah dan menolkan torn-write parsial jika ada.
    /// - Pada segmen dinamis (fallback): memotong trailing bytes `(size - 42) % 161 == r`.
    pub fn recover_or_open<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.as_ref())?;

        let metadata = file.metadata()?;
        let raw_size = metadata.len();

        if raw_size < SEGMENT_HEADER_SIZE as u64 {
            return Err(StorageError::CorruptedHeader {
                size: raw_size as usize,
            });
        }

        file.seek(SeekFrom::Start(0))?;
        let mut header_buf = [0u8; SEGMENT_HEADER_SIZE];
        file.read_exact(&mut header_buf)?;

        let header =
            SegmentHeader::from_bytes(&header_buf).map_err(|_| StorageError::InvalidMagic)?;

        // Periksa apakah segmen telah disegel secara lengkap (memiliki footer valid)
        let is_sealed = if raw_size >= (SEGMENT_HEADER_SIZE + SEGMENT_FOOTER_SIZE) as u64 {
            let footer_offset = raw_size - SEGMENT_FOOTER_SIZE as u64;
            file.seek(SeekFrom::Start(footer_offset))?;
            let mut footer_buf = [0u8; SEGMENT_FOOTER_SIZE];
            if file.read_exact(&mut footer_buf).is_ok() {
                SegmentFooter::from_bytes(&footer_buf).is_ok()
            } else {
                false
            }
        } else {
            false
        };

        let (valid_size, total_records, is_sealed) = if is_sealed {
            let footer_offset = raw_size - SEGMENT_FOOTER_SIZE as u64;
            file.seek(SeekFrom::Start(footer_offset))?;
            let mut footer_buf = [0u8; SEGMENT_FOOTER_SIZE];
            file.read_exact(&mut footer_buf)?;
            let footer = SegmentFooter::from_bytes(&footer_buf)
                .map_err(|_| StorageError::InvalidMagic)?;
            (raw_size, footer.total_records, true)
        } else if raw_size == MAX_SEGMENT_SIZE {
            // Segmen unsealed pra-alokasi 128 MB: gunakan binary zero scan
            let max_slots = (MAX_SEGMENT_SIZE
                .checked_sub((SEGMENT_HEADER_SIZE + SEGMENT_FOOTER_SIZE) as u64)
                .ok_or(StorageError::OutOfBounds)?)
                / (RECORD_SIZE as u64);

            let mut low = 0usize;
            let mut high = max_slots as usize;

            while low < high {
                let mid = low + (high - low) / 2;
                let slot_offset =
                    SEGMENT_HEADER_SIZE as u64 + (mid as u64 * RECORD_SIZE as u64);
                file.seek(SeekFrom::Start(slot_offset))?;
                let mut slot_buf = [0u8; RECORD_SIZE];
                file.read_exact(&mut slot_buf)?;

                if is_valid_committed_record(&slot_buf) {
                    low = mid + 1;
                } else {
                    high = mid;
                }
            }

            let k = low;

            // Validasi slot K: jika tertulis sebagian (non-zero bytes tapi bukan record valid),
            // nolkan kembali 161 byte slot K dan sinkronisasi ke disk fisik.
            if k < max_slots as usize {
                let slot_offset =
                    SEGMENT_HEADER_SIZE as u64 + (k as u64 * RECORD_SIZE as u64);
                file.seek(SeekFrom::Start(slot_offset))?;
                let mut k_buf = [0u8; RECORD_SIZE];
                file.read_exact(&mut k_buf)?;

                if k_buf.iter().any(|&b| b != 0) {
                    file.seek(SeekFrom::Start(slot_offset))?;
                    let zeros = [0u8; RECORD_SIZE];
                    file.write_all(&zeros)?;
                    file.sync_all()?;
                }
            }

            let valid_records = k as u64;
            let current_off = SEGMENT_HEADER_SIZE as u64 + (valid_records * RECORD_SIZE as u64);
            (current_off, valid_records, false)
        } else {
            // Mode fallback untuk segmen dinamis (un-preallocated)
            let payload_bytes = raw_size
                .checked_sub(SEGMENT_HEADER_SIZE as u64)
                .ok_or(StorageError::OutOfBounds)?;
            let remainder = payload_bytes % (RECORD_SIZE as u64);
            if remainder > 0 {
                let truncated_size = raw_size
                    .checked_sub(remainder)
                    .ok_or(StorageError::OutOfBounds)?;
                file.set_len(truncated_size)?;
                file.sync_all()?;
                let valid_payload = truncated_size
                    .checked_sub(SEGMENT_HEADER_SIZE as u64)
                    .ok_or(StorageError::OutOfBounds)?;
                (truncated_size, valid_payload / (RECORD_SIZE as u64), false)
            } else {
                (raw_size, payload_bytes / (RECORD_SIZE as u64), false)
            }
        };

        // Rekonstruksi sequence number awal dan akhir dari record yang valid
        let (first_sequence, last_sequence) = if total_records > 0 {
            // Baca record pertama di offset 42
            file.seek(SeekFrom::Start(SEGMENT_HEADER_SIZE as u64))?;
            let mut first_buf = [0u8; RECORD_SIZE];
            file.read_exact(&mut first_buf)?;
            let first_rec = MutationRecord::from_bytes(&first_buf);

            // Baca record terakhir
            let last_offset = if is_sealed {
                valid_size
                    .checked_sub(SEGMENT_FOOTER_SIZE as u64)
                    .and_then(|s| s.checked_sub(RECORD_SIZE as u64))
                    .ok_or(StorageError::OutOfBounds)?
            } else {
                valid_size
                    .checked_sub(RECORD_SIZE as u64)
                    .ok_or(StorageError::OutOfBounds)?
            };

            file.seek(SeekFrom::Start(last_offset))?;
            let mut last_buf = [0u8; RECORD_SIZE];
            file.read_exact(&mut last_buf)?;
            let last_rec = MutationRecord::from_bytes(&last_buf);

            (first_rec.sequence_number, last_rec.sequence_number)
        } else {
            (0, 0)
        };

        // Posisikan kursor berkas di akhir data valid untuk penulisan sekuensial berikutnya
        file.seek(SeekFrom::Start(valid_size))?;

        Ok(Self {
            file,
            header,
            current_offset: valid_size,
            flushed_offset: valid_size,
            total_records,
            first_sequence,
            last_sequence,
            is_sealed,
            policy: DurabilityPolicy::GroupCommit { batch_size: 100 },
            write_buffer: Vec::with_capacity(WRITE_BUFFER_CAPACITY),
            uncommitted_records: 0,
        })
    }

    /// Menambahkan satu MutationRecord (161 byte) secara sekuensial ke ujung log.
    ///
    /// Menegakkan penanganan buffer 128 KB dan kebijakan durabilitas hardware.
    pub fn append_record(&mut self, record: &MutationRecord) -> Result<u64, StorageError> {
        if self.is_sealed {
            return Err(StorageError::SegmentAlreadySealed);
        }

        let record_bytes = record.to_bytes();
        let record_len = RECORD_SIZE as u64;

        let max_payload_offset = MAX_SEGMENT_SIZE
            .checked_sub(SEGMENT_FOOTER_SIZE as u64)
            .ok_or(StorageError::OutOfBounds)?;

        let next_offset = self
            .current_offset
            .checked_add(record_len)
            .ok_or(StorageError::SegmentFull)?;

        if next_offset > max_payload_offset {
            return Err(StorageError::SegmentFull);
        }

        // Jika buffer tidak memuat satu record lagi, flush buffer ke disk terlebih dahulu
        if self.write_buffer.len() + RECORD_SIZE > WRITE_BUFFER_CAPACITY {
            self.flush_buffer()?;
        }

        let write_offset = self.current_offset;
        self.write_buffer.extend_from_slice(&record_bytes);

        self.current_offset = next_offset;
        if self.total_records == 0 {
            self.first_sequence = record.sequence_number;
        }
        self.last_sequence = record.sequence_number;
        self.total_records = self
            .total_records
            .checked_add(1)
            .ok_or(StorageError::OutOfBounds)?;
        self.uncommitted_records = self
            .uncommitted_records
            .checked_add(1)
            .ok_or(StorageError::OutOfBounds)?;

        match self.policy {
            DurabilityPolicy::Strict => {
                self.flush_and_sync()?;
            }
            DurabilityPolicy::GroupCommit { batch_size } => {
                if self.uncommitted_records >= batch_size {
                    self.flush_and_sync()?;
                }
            }
            DurabilityPolicy::BufferedRelaxed => {
                // Jangan sinkronisasi; flush hanya saat buffer mencapai kapasitas penuh
            }
        }

        Ok(write_offset)
    }

    /// Menyegel berkas segmen dengan menuliskan SegmentFooter (88 byte) di ujung berkas.
    ///
    /// Memotong kelebihan ruang pra-alokasi 128 MB ke ukuran riil terpakai
    /// dan memanggil `sync_all` untuk memastikan seluruh byte tersimpan ke media simpan fisik.
    /// Setelah disegel, segmen berstatus read-only dan menolak penulisan data baru.
    pub fn seal_segment(
        &mut self,
        state_digest: Hash,
        sealed_at: u64,
    ) -> Result<SegmentFooter, StorageError> {
        if self.is_sealed {
            return Err(StorageError::SegmentAlreadySealed);
        }

        // Flush seluruh data yang tersisa di memory buffer sebelum menulis footer
        self.flush_buffer()?;

        let footer = SegmentFooter::new(
            self.total_records,
            self.header.epoch,
            self.first_sequence,
            self.last_sequence,
            state_digest,
            sealed_at,
        );

        let footer_bytes = footer.to_bytes();
        self.file.seek(SeekFrom::Start(self.current_offset))?;
        self.file.write_all(&footer_bytes)?;

        // Reklamasi ruang pra-alokasi yang tidak terpakai dengan memotong ke batas akhir footer
        let sealed_len = self
            .current_offset
            .checked_add(SEGMENT_FOOTER_SIZE as u64)
            .ok_or(StorageError::OutOfBounds)?;

        self.file.set_len(sealed_len)?;
        self.file.sync_all()?;

        self.is_sealed = true;
        self.current_offset = sealed_len;
        self.flushed_offset = sealed_len;
        self.uncommitted_records = 0;

        Ok(footer)
    }

    /// Memeriksa apakah segmen telah disegel.
    #[inline]
    pub fn is_sealed(&self) -> bool {
        self.is_sealed
    }

    /// Mengambil posisi byte offset aktif saat ini.
    #[inline]
    pub fn current_offset(&self) -> u64 {
        self.current_offset
    }

    /// Mengambil offset fisik byte terakhir yang telah dituliskan ke berkas disk.
    #[inline]
    pub fn flushed_offset(&self) -> u64 {
        self.flushed_offset
    }

    /// Mengambil total record yang telah dituliskan.
    #[inline]
    pub fn total_records(&self) -> u64 {
        self.total_records
    }

    /// Mengambil jumlah record yang belum disinkronisasi ke disk pada batch saat ini.
    #[inline]
    pub fn uncommitted_records(&self) -> u32 {
        self.uncommitted_records
    }

    /// Mengambil kebijakan durabilitas aktif pada writer.
    #[inline]
    pub fn policy(&self) -> DurabilityPolicy {
        self.policy
    }

    /// Mengambil referensi buffer tulis memori internal.
    #[inline]
    pub fn write_buffer(&self) -> &[u8] {
        &self.write_buffer
    }

    /// Mengambil referensi header segmen.
    #[inline]
    pub fn header(&self) -> &SegmentHeader {
        &self.header
    }
}

impl Drop for SegmentWriter {
    fn drop(&mut self) {
        if !self.is_sealed && !self.write_buffer.is_empty() {
            let _ = self.flush_buffer();
        }
    }
}
