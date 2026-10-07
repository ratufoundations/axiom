//! Modul pembaca segmen log Axiom berbasis offset dan catatan kaki.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use axiom_primitives::record::{MutationRecord, RECORD_SIZE};

use crate::error::StorageError;
use crate::segment::{SegmentFooter, SegmentHeader, SEGMENT_FOOTER_SIZE, SEGMENT_HEADER_SIZE};

/// Pembaca segmen log biner untuk inspeksi status dan pencarian data acak.
pub struct SegmentReader {
    file: File,
    header: SegmentHeader,
}

impl SegmentReader {
    /// Membuka berkas segmen yang ada dan memvalidasi keabsahan SegmentHeader.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let mut file = File::open(path)?;
        let mut header_buf = [0u8; SEGMENT_HEADER_SIZE];
        file.read_exact(&mut header_buf)?;

        let header =
            SegmentHeader::from_bytes(&header_buf).map_err(|_| StorageError::InvalidMagic)?;

        Ok(Self { file, header })
    }

    /// Membaca satu MutationRecord tepat 161 byte dari posisi offset byte tertentu.
    pub fn read_record_at(&mut self, offset: u64) -> Result<MutationRecord, StorageError> {
        if offset < SEGMENT_HEADER_SIZE as u64 {
            return Err(StorageError::OutOfBounds);
        }

        let file_len = self.file.metadata()?.len();
        let end_offset = offset
            .checked_add(RECORD_SIZE as u64)
            .ok_or(StorageError::OutOfBounds)?;

        if end_offset > file_len {
            return Err(StorageError::OutOfBounds);
        }

        self.file.seek(SeekFrom::Start(offset))?;
        let mut buf = [0u8; RECORD_SIZE];
        self.file.read_exact(&mut buf)?;

        Ok(MutationRecord::from_bytes(&buf))
    }

    /// Membaca catatan kaki (SegmentFooter, 88 byte) dari ujung berkas segmen yang telah disegel.
    pub fn read_footer(&mut self) -> Result<SegmentFooter, StorageError> {
        let file_len = self.file.metadata()?.len();
        let min_len = (SEGMENT_HEADER_SIZE + SEGMENT_FOOTER_SIZE) as u64;

        if file_len < min_len {
            return Err(StorageError::OutOfBounds);
        }

        let footer_offset = file_len
            .checked_sub(SEGMENT_FOOTER_SIZE as u64)
            .ok_or(StorageError::OutOfBounds)?;

        self.file.seek(SeekFrom::Start(footer_offset))?;
        let mut buf = [0u8; SEGMENT_FOOTER_SIZE];
        self.file.read_exact(&mut buf)?;

        SegmentFooter::from_bytes(&buf).map_err(|_| StorageError::InvalidMagic)
    }

    /// Mengambil referensi header segmen.
    #[inline]
    pub fn header(&self) -> &SegmentHeader {
        &self.header
    }
}
