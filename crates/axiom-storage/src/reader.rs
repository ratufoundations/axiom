//! Modul pembaca segmen log Axiom berbasis offset, catatan kaki, dan streaming terbuffer.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use axiom_primitives::record::MutationRecord;

use crate::error::StorageError;
use crate::segment::{
    SegmentFooter, SegmentHeader, SEGMENT_FOOTER_SIZE, SEGMENT_HEADER_SIZE,
};

/// Ukuran buffer baca streaming default (128 KB = 131.072 byte).
pub const DEFAULT_STREAM_BUFFER_SIZE: usize = 128 * 1024;

/// Pembaca segmen log biner untuk inspeksi status, pembacaan acak, dan pemindaian terbuffer.
pub struct SegmentReader {
    pub(crate) file: File,
    pub(crate) path: PathBuf,
    pub(crate) header: SegmentHeader,
}

impl SegmentReader {
    /// Membuka berkas segmen yang ada dan memvalidasi keabsahan SegmentHeader.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let path_buf = path.as_ref().to_path_buf();
        let mut file = File::open(&path_buf)?;
        let mut header_buf = [0u8; SEGMENT_HEADER_SIZE];
        file.read_exact(&mut header_buf)?;

        let header =
            SegmentHeader::from_bytes(&header_buf).map_err(|_| StorageError::InvalidMagic)?;

        Ok(Self {
            file,
            path: path_buf,
            header,
        })
    }

    /// Mengambil referensi header segmen.
    #[inline]
    pub fn header(&self) -> &SegmentHeader {
        &self.header
    }

    /// Membaca satu MutationRecord tepat 161 byte dari posisi offset byte tertentu.
    pub fn read_record_at(&mut self, offset: u64) -> Result<MutationRecord, StorageError> {
        let meta = self.file.metadata()?;
        let file_len = meta.len();

        let end_record_offset = offset
            .checked_add(MutationRecord::RECORD_SIZE as u64)
            .ok_or(StorageError::OutOfBounds)?;

        if offset < SEGMENT_HEADER_SIZE as u64 || end_record_offset > file_len {
            return Err(StorageError::OutOfBounds);
        }

        // Batas data tidak boleh menembus footer jika segmen sudah disegel
        if file_len >= (SEGMENT_HEADER_SIZE + SEGMENT_FOOTER_SIZE) as u64 {
            let footer_offset = file_len - SEGMENT_FOOTER_SIZE as u64;
            if end_record_offset > footer_offset && self.read_footer().is_ok() {
                return Err(StorageError::OutOfBounds);
            }
        }

        self.file.seek(SeekFrom::Start(offset))?;
        let mut record_buf = [0u8; MutationRecord::RECORD_SIZE];
        self.file.read_exact(&mut record_buf)?;

        Ok(MutationRecord::from_bytes(&record_buf))
    }

    /// Membaca catatan kaki (SegmentFooter, 88 byte) dari ujung berkas segmen yang telah disegel.
    pub fn read_footer(&mut self) -> Result<SegmentFooter, StorageError> {
        let meta = self.file.metadata()?;
        let file_len = meta.len();

        if file_len < (SEGMENT_HEADER_SIZE + SEGMENT_FOOTER_SIZE) as u64 {
            return Err(StorageError::OutOfBounds);
        }

        let footer_offset = file_len - SEGMENT_FOOTER_SIZE as u64;
        self.file.seek(SeekFrom::Start(footer_offset))?;

        let mut footer_buf = [0u8; SEGMENT_FOOTER_SIZE];
        self.file.read_exact(&mut footer_buf)?;

        SegmentFooter::from_bytes(&footer_buf).map_err(|_| StorageError::InvalidMagic)
    }

    /// Membuka stream baca terbuffer (chunk streaming) dari offset 42 hingga batas footer.
    pub fn stream_records(&self) -> Result<RecordStream, StorageError> {
        self.stream_records_with_capacity(DEFAULT_STREAM_BUFFER_SIZE)
    }

    /// Membuka stream baca terbuffer dengan kapasitas buffer kustom.
    pub fn stream_records_with_capacity(
        &self,
        buffer_capacity: usize,
    ) -> Result<RecordStream, StorageError> {
        let mut file = File::open(&self.path)?;
        let file_len = file.metadata()?.len();

        let has_footer = if file_len >= (SEGMENT_HEADER_SIZE + SEGMENT_FOOTER_SIZE) as u64 {
            let footer_offset = file_len - SEGMENT_FOOTER_SIZE as u64;
            let mut footer_buf = [0u8; SEGMENT_FOOTER_SIZE];
            if file.seek(SeekFrom::Start(footer_offset)).is_ok()
                && file.read_exact(&mut footer_buf).is_ok()
            {
                SegmentFooter::from_bytes(&footer_buf).is_ok()
            } else {
                false
            }
        } else {
            false
        };

        let data_end_offset = if has_footer {
            file_len - SEGMENT_FOOTER_SIZE as u64
        } else {
            file_len
        };

        file.seek(SeekFrom::Start(SEGMENT_HEADER_SIZE as u64))?;
        let reader = BufReader::with_capacity(buffer_capacity, file);

        Ok(RecordStream {
            reader,
            current_offset: SEGMENT_HEADER_SIZE as u64,
            end_offset: data_end_offset,
        })
    }
}

/// Iterator pembaca stream mutasi terbuffer linear.
pub struct RecordStream {
    reader: BufReader<File>,
    current_offset: u64,
    end_offset: u64,
}

impl RecordStream {
    /// Mengambil offset posisi mutasi saat ini di dalam berkas segmen.
    pub fn current_offset(&self) -> u64 {
        self.current_offset
    }
}

impl Iterator for RecordStream {
    type Item = Result<(u64, MutationRecord), StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        let next_offset = self
            .current_offset
            .checked_add(MutationRecord::RECORD_SIZE as u64)?;
        if next_offset > self.end_offset {
            return None;
        }

        let record_offset = self.current_offset;
        let mut record_buf = [0u8; MutationRecord::RECORD_SIZE];

        match self.reader.read_exact(&mut record_buf) {
            Ok(()) => {
                if record_buf.iter().all(|&b| b == 0) {
                    return None;
                }
                self.current_offset = next_offset;
                let rec = MutationRecord::from_bytes(&record_buf);
                Some(Ok((record_offset, rec)))
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    None
                } else {
                    Some(Err(StorageError::IoError(e)))
                }
            }
        }
    }
}
