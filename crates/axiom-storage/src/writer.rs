//! Modul penulis sekuensial linear (append-only) untuk segmen log Axiom.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use axiom_primitives::crypto::Hash;
use axiom_primitives::record::{MutationRecord, RECORD_SIZE};

use crate::error::StorageError;
use crate::segment::{
    SegmentFooter, SegmentHeader, MAX_SEGMENT_SIZE, SEGMENT_FOOTER_SIZE, SEGMENT_HEADER_SIZE,
};

/// Penulis segmen log append-only berkapasitas tetap (maks 128 MB).
pub struct SegmentWriter {
    file: File,
    header: SegmentHeader,
    current_offset: u64,
    total_records: u64,
    first_sequence: u64,
    last_sequence: u64,
    is_sealed: bool,
}

impl SegmentWriter {
    /// Membuat berkas segmen baru pada jalur `path` dan menginisialisasi header 42 byte.
    pub fn create<P: AsRef<Path>>(
        path: P,
        version: u16,
        epoch: u64,
        segment_index: u32,
    ) -> Result<Self, StorageError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        let header = SegmentHeader::new(version, epoch, segment_index);
        let header_bytes = header.to_bytes();
        file.write_all(&header_bytes)?;
        file.flush()?;

        let current_offset = SEGMENT_HEADER_SIZE as u64;

        Ok(Self {
            file,
            header,
            current_offset,
            total_records: 0,
            first_sequence: 0,
            last_sequence: 0,
            is_sealed: false,
        })
    }

    /// Membuka berkas segmen yang sudah ada dan melakukan pemulihan torn-write jika terjadi crash.
    ///
    /// Menegakkan invarian ukuran berkas: `(size - 42) % 161 == 0`.
    /// Jika terdapat trailing bytes parsial `r > 0`, berkas dipotong (*truncate*) ke `size - r`
    /// dan disinkronisasi ke disk fisik sebelum penulisan berikutnya.
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

        let (valid_size, total_records) = if is_sealed {
            let payload_bytes = raw_size
                .checked_sub((SEGMENT_HEADER_SIZE + SEGMENT_FOOTER_SIZE) as u64)
                .ok_or(StorageError::OutOfBounds)?;
            let remainder = payload_bytes % (RECORD_SIZE as u64);
            if remainder > 0 {
                return Err(StorageError::RecoveryFailed(
                    "Sealed segment contains misaligned record payload".to_string(),
                ));
            }
            let records = payload_bytes / (RECORD_SIZE as u64);
            (raw_size, records)
        } else {
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
                (truncated_size, valid_payload / (RECORD_SIZE as u64))
            } else {
                (raw_size, payload_bytes / (RECORD_SIZE as u64))
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
            total_records,
            first_sequence,
            last_sequence,
            is_sealed,
        })
    }

    /// Menambahkan satu MutationRecord (161 byte) secara sekuensial ke ujung berkas segmen.
    ///
    /// Mengembalikan offset byte awal penulisan record tersebut.
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

        let write_offset = self.current_offset;
        self.file.write_all(&record_bytes)?;

        self.current_offset = next_offset;
        if self.total_records == 0 {
            self.first_sequence = record.sequence_number;
        }
        self.last_sequence = record.sequence_number;
        self.total_records = self
            .total_records
            .checked_add(1)
            .ok_or(StorageError::OutOfBounds)?;

        Ok(write_offset)
    }

    /// Menyegel berkas segmen dengan menuliskan SegmentFooter (88 byte) di ujung berkas.
    ///
    /// Memanggil `flush` untuk memastikan seluruh byte tersimpan ke media simpan fisik.
    /// Setelah disegel, segmen berstatus read-only dan menolak penulisan data baru.
    pub fn seal_segment(
        &mut self,
        state_digest: Hash,
        sealed_at: u64,
    ) -> Result<SegmentFooter, StorageError> {
        if self.is_sealed {
            return Err(StorageError::SegmentAlreadySealed);
        }

        let footer = SegmentFooter::new(
            self.total_records,
            self.header.epoch,
            self.first_sequence,
            self.last_sequence,
            state_digest,
            sealed_at,
        );

        let footer_bytes = footer.to_bytes();
        self.file.write_all(&footer_bytes)?;
        self.file.flush()?;

        self.is_sealed = true;
        self.current_offset = self
            .current_offset
            .checked_add(SEGMENT_FOOTER_SIZE as u64)
            .ok_or(StorageError::OutOfBounds)?;

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

    /// Mengambil total record yang telah dituliskan.
    #[inline]
    pub fn total_records(&self) -> u64 {
        self.total_records
    }

    /// Mengambil referensi header segmen.
    #[inline]
    pub fn header(&self) -> &SegmentHeader {
        &self.header
    }
}
