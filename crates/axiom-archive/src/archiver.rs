//! Mesin pengemas arsip bulanan deterministik ke format berkas .zip.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_storage::reader::SegmentReader;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;
use zip::ZipWriter;

use crate::error::ArchiveError;
use crate::manifest::ArchiveManifestHeader;
use crate::partitioner::AccountPartitioner;

/// Membangun berkas arsip bulanan berformat .zip dari berkas segmen bersegel.
pub fn create_monthly_archive<P: AsRef<Path>, Q: AsRef<Path>>(
    epoch: u64,
    segment_path: P,
    output_zip_path: Q,
) -> Result<ArchiveManifestHeader, ArchiveError> {
    let segment_path = segment_path.as_ref();
    let output_zip_path = output_zip_path.as_ref();

    // 1. Buka dan validasi berkas segmen
    let mut reader = SegmentReader::open(segment_path)?;

    // 2. Partisi mutasi per akun
    let partitioner = AccountPartitioner::partition_segment(&mut reader)?;
    if partitioner.footer.epoch != epoch {
        return Err(ArchiveError::CorruptedSegment);
    }

    // 3. Inisialisasi penulis berkas ZIP
    let zip_file = File::create(output_zip_path)?;
    let mut zip_writer = ZipWriter::new(zip_file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    // 4. Tulis berkas salinan segmen asli ke 'segments/{filename}'
    let segment_filename = segment_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("segment.sealed");
    let segment_entry_name = format!("segments/{segment_filename}");
    zip_writer.start_file(segment_entry_name, options)?;

    let mut segment_raw = Vec::new();
    File::open(segment_path)?.read_to_end(&mut segment_raw)?;
    zip_writer.write_all(&segment_raw)?;

    // 5. Tulis riwayat mutasi per akun ke 'accounts/{account_hex}.bin'
    for (account_id, records) in &partitioner.accounts {
        let mut hex_str = String::with_capacity(64);
        for b in account_id.as_bytes() {
            use core::fmt::Write;
            let _ = write!(&mut hex_str, "{b:02x}");
        }

        let account_entry = format!("accounts/{hex_str}.bin");
        zip_writer.start_file(account_entry, options)?;
        for record in records {
            zip_writer.write_all(&record.to_bytes())?;
        }
    }

    // 6. Tulis manifes biner ke 'manifest.bin' pada root arsip
    let archived_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let manifest_header = ArchiveManifestHeader::new(
        1,
        epoch,
        partitioner.total_accounts() as u32,
        partitioner.total_records(),
        partitioner.footer.state_digest,
        archived_at,
    );

    zip_writer.start_file("manifest.bin", options)?;
    zip_writer.write_all(&manifest_header.to_bytes())?;

    // 7. Tuntaskan kompresi
    zip_writer.finish()?;

    Ok(manifest_header)
}
