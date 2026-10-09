//! Modul pemilah mutasi per akun (Account Partitioner) dari segmen log.

use std::collections::BTreeMap;

use ratu_aurion_primitives::crypto::AccountId;
use ratu_aurion_primitives::record::{MutationRecord, RECORD_SIZE};
use ratu_aurion_storage::reader::SegmentReader;
use ratu_aurion_storage::segment::{SegmentFooter, SEGMENT_HEADER_SIZE};

use crate::error::ArchiveError;

/// Pemilah mutasi biner yang mengelompokkan riwayat per AccountId.
#[derive(Debug, Clone)]
pub struct AccountPartitioner {
    /// Pemetaan terurut akun ke daftar mutasi yang melibatkan akun tersebut.
    pub accounts: BTreeMap<AccountId, Vec<MutationRecord>>,
    /// Informasi catatan kaki segmen yang disegel.
    pub footer: SegmentFooter,
}

impl AccountPartitioner {
    /// Memindai seluruh mutasi pada segmen bersegel dan memilahnya ke dalam pemetaan per akun.
    pub fn partition_segment(reader: &mut SegmentReader) -> Result<Self, ArchiveError> {
        let footer = reader.read_footer()?;
        let mut accounts: BTreeMap<AccountId, Vec<MutationRecord>> = BTreeMap::new();
        let mut offset = SEGMENT_HEADER_SIZE as u64;

        for _ in 0..footer.total_records {
            let record = reader.read_record_at(offset)?;

            accounts.entry(record.sender).or_default().push(record);
            if record.sender != record.recipient {
                accounts.entry(record.recipient).or_default().push(record);
            }

            offset = offset
                .checked_add(RECORD_SIZE as u64)
                .ok_or(ArchiveError::CorruptedSegment)?;
        }

        Ok(Self { accounts, footer })
    }

    /// Total akun unik yang terlibat dalam segmen ini.
    #[inline]
    pub fn total_accounts(&self) -> usize {
        self.accounts.len()
    }

    /// Total transaksi mutasi yang tercatat pada footer.
    #[inline]
    pub fn total_records(&self) -> u64 {
        self.footer.total_records
    }
}
