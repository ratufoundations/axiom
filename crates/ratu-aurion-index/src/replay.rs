//! Modul pemulihan status in-memory saat booting node (Replay Engine).

use crate::error::IndexError;
use crate::keydir::Keydir;
use ratu_aurion_storage::reader::SegmentReader;

/// Membangun ulang seluruh status in-memory Keydir menggunakan pemindaian terbuffer cepat.
pub fn rebuild_index_from_segment(
    reader: &mut SegmentReader,
    keydir: &mut Keydir,
    epoch: u64,
    segment_idx: u32,
) -> Result<u64, IndexError> {
    let stream = reader.stream_records()?;
    let mut applied_count: u64 = 0;

    for item in stream {
        let (offset, record) = item.map_err(IndexError::StorageError)?;
        keydir.apply_mutation(&record, epoch, segment_idx, offset)?;
        applied_count = applied_count
            .checked_add(1)
            .ok_or(IndexError::ArithmeticOverflow)?;
    }

    Ok(applied_count)
}
