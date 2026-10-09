//! Modul pembersih segmen aktif disk (Pruning Engine) setelah verifikasi integritas arsip.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use ratu_aurion_storage::reader::SegmentReader;
use zip::ZipArchive;

use crate::error::ArchiveError;
use crate::manifest::{ArchiveManifestHeader, MANIFEST_HEADER_SIZE};

/// Memverifikasi kesesuaian digest status antara arsip ZIP dan segmen disk,
/// lalu memotong/menghapus segmen aktif dari disk jika integritas terbukti 100%.
pub fn verify_and_prune_segment<P: AsRef<Path>, Q: AsRef<Path>>(
    segment_path: P,
    archive_zip_path: Q,
) -> Result<(), ArchiveError> {
    let segment_path = segment_path.as_ref();
    let archive_zip_path = archive_zip_path.as_ref();

    // 1. Buka arsip ZIP dan baca 'manifest.bin'
    let zip_file = File::open(archive_zip_path)?;
    let mut zip_archive = ZipArchive::new(zip_file)?;

    let mut manifest_file = zip_archive
        .by_name("manifest.bin")
        .map_err(|e| ArchiveError::ZipError(e.to_string()))?;

    let mut manifest_buf = [0u8; MANIFEST_HEADER_SIZE];
    manifest_file.read_exact(&mut manifest_buf)?;
    let manifest = ArchiveManifestHeader::from_bytes(&manifest_buf)?;

    // 2. Buka segmen disk aktif dan baca catatan kaki
    let mut reader = SegmentReader::open(segment_path)?;
    let footer = reader.read_footer()?;

    // 3. Verifikasi integritas: pastikan state digest dan epoch identik
    if footer.state_digest != manifest.state_digest || footer.epoch != manifest.epoch {
        return Err(ArchiveError::DigestMismatch);
    }

    // 4. Jika verifikasi lolos secara deterministik, pangkas/hapus segmen aktif dari disk
    fs::remove_file(segment_path)?;

    Ok(())
}
