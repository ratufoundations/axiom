//! Modul penulis sekuensial linear (append-only) untuk segmen log Axiom.

use std::fs::{File, OpenOptions};
use std::io::Write;
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
