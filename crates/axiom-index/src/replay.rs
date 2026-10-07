//! Modul pemulihan status in-memory saat booting node (Replay Engine).

use axiom_primitives::record::RECORD_SIZE;
use axiom_storage::error::StorageError;
use axiom_storage::reader::SegmentReader;
use axiom_storage::segment::SEGMENT_HEADER_SIZE;

use crate::error::IndexError;
use crate::keydir::Keydir;

/// Memindai dan memutar ulang rekaman mutasi dari berkas segmen log ke Keydir di RAM.
///
/// Membaca setiap blok rekaman 161 byte secara sekuensial linear dari offset 42
/// dan menerapkan proyeksi status saldo dan pointer lokasi ke tabel indeks RAM.
pub fn rebuild_index_from_segment(
    reader: &mut SegmentReader,
    keydir: &mut Keydir,
    epoch: u64,
    segment_idx: u32,
) -> Result<u64, IndexError> {
    let mut offset = SEGMENT_HEADER_SIZE as u64;
    let mut count = 0u64;

    // Jika berkas segmen telah disegel, batasi iterasi hingga total_records pada footer
    let max_records = reader.read_footer().ok().map(|footer| footer.total_records);

    loop {
        if let Some(max) = max_records {
            if count >= max {
                break;
            }
        }

        match reader.read_record_at(offset) {
            Ok(record) => {
                keydir.apply_mutation(&record, epoch, segment_idx, offset)?;
                offset = offset
                    .checked_add(RECORD_SIZE as u64)
                    .ok_or(IndexError::ArithmeticOverflow)?;
                count = count
                    .checked_add(1)
                    .ok_or(IndexError::ArithmeticOverflow)?;
            }
            Err(StorageError::OutOfBounds) => break,
            Err(e) => return Err(IndexError::StorageError(e)),
        }
    }

    Ok(count)
}
