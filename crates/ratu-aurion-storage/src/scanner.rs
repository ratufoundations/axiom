#![forbid(unsafe_code)]

//! Modul pemindaian integritas dan verifikasi invarian struktural log segmen (Fail-Fast).
//!
//! Menggunakan streaming I/O terbuffer 128 KB dengan konsumsi memori konstan O(1)
//! dan verifikasi BLAKE3 bit-rot serta validasi tanda tangan kriptografi Ed25519.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use ratu_aurion_primitives::{
    crypto::Hash,
    record::{MutationRecord, RECORD_KIND_SYSTEM_NOTIF, RECORD_KIND_TRANSFER, RECORD_SIZE},
};
use ed25519_dalek::Verifier;

use crate::{
    error::StorageError,
    segment::{
        SegmentFooter, SegmentHeader, SEGMENT_FOOTER_MAGIC, SEGMENT_FOOTER_SIZE,
        SEGMENT_HEADER_SIZE,
    },
};

/// Kapasitas buffer baca streaming pemindaian (128 KB = 131,072 byte).
pub const SCAN_BUFFER_CAPACITY: usize = 128 * 1024;

/// Laporan hasil pemindaian segmen log penyimpanan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentScanReport {
    /// Total record mutasi valid yang berhasil dipindai.
    pub total_records_scanned: u64,
    /// Nomor urut mutasi pertama yang terdeteksi.
    pub first_sequence: u64,
    /// Nomor urut mutasi terakhir yang terdeteksi.
    pub last_sequence: u64,
    /// Status apakah segmen telah disegel secara fisik (sealed footer).
    pub is_sealed: bool,
    /// Intisari BLAKE3 yang dihitung secara streaming dari seluruh payload record.
    pub calculated_digest: Hash,
    /// Intisari yang tercantum pada footer segmen jika telah disegel.
    pub footer_digest: Option<Hash>,
    /// Menandakan apakah seluruh invarian struktural dan integritas terverifikasi valid.
    pub is_valid: bool,
}

/// Pemindai cepat invarian struktural dan pendeteksi bit-rot pada segmen penyimpanan.
pub struct SegmentScanner;

impl SegmentScanner {
    /// Memindai segmen log secara linear dengan streaming fail-fast.
    ///
    /// Memvalidasi:
    /// - Header 42 byte dan identifikasi footer 88 byte.
    /// - Invarian kesesuaian epoch terhadap header segmen.
    /// - Nomor urut mutasi sekuensial deterministik tanpa celah.
    /// - Validitas jenis mutasi (`RECORD_KIND_TRANSFER` atau `RECORD_KIND_SYSTEM_NOTIF`).
    /// - Tanda tangan bukan nol biner.
    /// - Verifikasi BLAKE3 bit-rot terhadap `state_digest` pada segmen bersegel.
    pub fn scan_segment<P: AsRef<Path>>(path: P) -> Result<SegmentScanReport, StorageError> {
        let mut file = File::open(path)?;
        let file_len = file.metadata()?.len();

        if file_len < SEGMENT_HEADER_SIZE as u64 {
            return Err(StorageError::CorruptedHeader {
                size: file_len as usize,
            });
        }

        let mut header_buf = [0u8; SEGMENT_HEADER_SIZE];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut header_buf)?;

        let header =
            SegmentHeader::from_bytes(&header_buf).map_err(|_| StorageError::InvalidMagic)?;

        // Periksa apakah segmen telah disegel dengan memeriksa 88 byte terakhir
        let mut footer_opt = None;
        let is_sealed = if file_len >= (SEGMENT_HEADER_SIZE + SEGMENT_FOOTER_SIZE) as u64 {
            let footer_offset = file_len - SEGMENT_FOOTER_SIZE as u64;
            let mut footer_buf = [0u8; SEGMENT_FOOTER_SIZE];
            file.seek(SeekFrom::Start(footer_offset))?;
            if file.read_exact(&mut footer_buf).is_ok()
                && footer_buf[80..88] == SEGMENT_FOOTER_MAGIC
            {
                match SegmentFooter::from_bytes(&footer_buf) {
                    Ok(footer) => {
                        footer_opt = Some(footer);
                        true
                    }
                    Err(_) => false,
                }
            } else {
                false
            }
        } else {
            false
        };

        let data_end_offset = if is_sealed {
            file_len - SEGMENT_FOOTER_SIZE as u64
        } else {
            file_len
        };

        if is_sealed && !(data_end_offset - SEGMENT_HEADER_SIZE as u64).is_multiple_of(RECORD_SIZE as u64) {
            return Err(StorageError::StructuralInvariantViolation {
                offset: data_end_offset,
                reason: "Sealed segment payload size is not a multiple of 161 bytes".to_string(),
            });
        }

        file.seek(SeekFrom::Start(SEGMENT_HEADER_SIZE as u64))?;
        let mut reader = BufReader::with_capacity(SCAN_BUFFER_CAPACITY, file);
        let mut hasher = blake3::Hasher::new();

        let mut total_records_scanned: u64 = 0;
        let mut first_sequence: u64 = 0;
        let mut last_sequence: u64 = 0;
        let mut expected_seq: u64 = 0;
        let mut current_offset: u64 = SEGMENT_HEADER_SIZE as u64;
        let mut record_buf = [0u8; RECORD_SIZE];

        while current_offset + (RECORD_SIZE as u64) <= data_end_offset {
            match reader.read_exact(&mut record_buf) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    break;
                }
                Err(e) => return Err(StorageError::IoError(e)),
            }

            // Pada segmen yang belum disegel, deteksi pengisian nol pra-alokasi sebagai batas commit
            if !is_sealed && record_buf.iter().all(|&b| b == 0) {
                break;
            }

            let record = MutationRecord::from_bytes(&record_buf);

            // Invarian 1: record.epoch == header.epoch
            if record.epoch != header.epoch {
                return Err(StorageError::EpochMismatch {
                    expected: header.epoch,
                    found: record.epoch,
                    offset: current_offset,
                });
            }

            // Invarian 2: record.sequence_number == expected_seq
            if total_records_scanned == 0 {
                first_sequence = record.sequence_number;
                expected_seq = record.sequence_number;
            }

            if record.sequence_number != expected_seq {
                return Err(StorageError::SequenceMismatch {
                    expected: expected_seq,
                    found: record.sequence_number,
                    offset: current_offset,
                });
            }

            // Invarian 3: record_kind == RECORD_KIND_TRANSFER || record_kind == RECORD_KIND_SYSTEM_NOTIF
            if record.record_kind != RECORD_KIND_TRANSFER
                && record.record_kind != RECORD_KIND_SYSTEM_NOTIF
            {
                return Err(StorageError::StructuralInvariantViolation {
                    offset: current_offset,
                    reason: format!("Invalid record kind: {}", record.record_kind),
                });
            }

            // Invarian 4: Tanda tangan tidak boleh seluruhnya nol
            if record.signature.as_bytes().iter().all(|&b| b == 0) {
                return Err(StorageError::StructuralInvariantViolation {
                    offset: current_offset,
                    reason: "Signature contains only zero bytes".to_string(),
                });
            }

            // Perbarui akumulator hash BLAKE3
            hasher.update(&record_buf);

            last_sequence = record.sequence_number;
            total_records_scanned += 1;
            expected_seq += 1;
            current_offset += RECORD_SIZE as u64;
        }

        if is_sealed && current_offset != data_end_offset {
            return Err(StorageError::StructuralInvariantViolation {
                offset: current_offset,
                reason: format!(
                    "Sealed segment scan terminated prematurely at offset {}, expected {}",
                    current_offset, data_end_offset
                ),
            });
        }

        let blake_hash = hasher.finalize();
        let calculated_digest = Hash::new(*blake_hash.as_bytes());

        let footer_digest = if let Some(footer) = footer_opt {
            let footer_offset = file_len - SEGMENT_FOOTER_SIZE as u64;

            if footer.total_records != total_records_scanned {
                return Err(StorageError::StructuralInvariantViolation {
                    offset: footer_offset,
                    reason: format!(
                        "Footer record count mismatch: expected {}, scanned {}",
                        footer.total_records, total_records_scanned
                    ),
                });
            }

            if total_records_scanned > 0 {
                if footer.first_sequence != first_sequence {
                    return Err(StorageError::StructuralInvariantViolation {
                        offset: footer_offset,
                        reason: format!(
                            "Footer first sequence mismatch: expected {}, scanned {}",
                            footer.first_sequence, first_sequence
                        ),
                    });
                }
                if footer.last_sequence != last_sequence {
                    return Err(StorageError::StructuralInvariantViolation {
                        offset: footer_offset,
                        reason: format!(
                            "Footer last sequence mismatch: expected {}, scanned {}",
                            footer.last_sequence, last_sequence
                        ),
                    });
                }
            }

            if footer.state_digest != calculated_digest {
                return Err(StorageError::BitRotDetected {
                    offset: footer_offset,
                    expected: *footer.state_digest.as_bytes(),
                    actual: *calculated_digest.as_bytes(),
                });
            }

            Some(footer.state_digest)
        } else {
            None
        };

        Ok(SegmentScanReport {
            total_records_scanned,
            first_sequence,
            last_sequence,
            is_sealed,
            calculated_digest,
            footer_digest,
            is_valid: true,
        })
    }

    /// Menjalankan pemindaian struktural diikuti verifikasi tanda tangan kriptografi Ed25519
    /// atas payload 97 byte untuk setiap record yang tersimpan.
    pub fn deep_verify_signatures<P: AsRef<Path>>(
        path: P,
    ) -> Result<SegmentScanReport, StorageError> {
        let report = Self::scan_segment(&path)?;

        if report.total_records_scanned == 0 {
            return Ok(report);
        }

        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(SEGMENT_HEADER_SIZE as u64))?;
        let mut reader = BufReader::with_capacity(SCAN_BUFFER_CAPACITY, file);
        let mut record_buf = [0u8; RECORD_SIZE];
        let mut current_offset = SEGMENT_HEADER_SIZE as u64;

        for _ in 0..report.total_records_scanned {
            reader.read_exact(&mut record_buf)?;
            let record = MutationRecord::from_bytes(&record_buf);

            let payload = record.signing_payload();
            let pubkey_bytes = record.sender.as_bytes();
            let sig_bytes = record.signature.as_bytes();

            let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(pubkey_bytes).map_err(
                |_| StorageError::SignatureVerificationFailed {
                    offset: current_offset,
                },
            )?;

            let dalek_sig = ed25519_dalek::Signature::from_bytes(sig_bytes);

            verifying_key.verify(&payload, &dalek_sig).map_err(|_| {
                StorageError::SignatureVerificationFailed {
                    offset: current_offset,
                }
            })?;

            current_offset += RECORD_SIZE as u64;
        }

        Ok(report)
    }
}
