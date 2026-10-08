//! Modul pengatur ritme konsensus (Pacemaker) dan rotasi pemimpin deterministik.

use axiom_primitives::crypto::AccountId;

use crate::error::ConsensusError;
use crate::timeout::TimeoutCertificate;

/// Durasi timeout awal standar (2.000 ms).
pub const DEFAULT_BASE_TIMEOUT_MS: u64 = 2_000;

/// Batas eksponen pengali backoff maksimal (6, yaitu 2^6 = 64x multiplier -> 128.000 ms).
pub const MAX_BACKOFF_EXPONENT: u32 = 6;

/// Pengatur tempo putaran ronde konsensus dengan backoff eksponensial integer terproteksi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pacemaker {
    /// Durasi batas waktu dasar dalam milidetik.
    base_timeout_ms: u64,
    /// Jumlah timeout berturut-turut yang telah dialami node tanpa adanya commit sukses.
    consecutive_timeouts: u32,
    /// Nomor ronde konsensus saat ini.
    current_round: u64,
}

impl Pacemaker {
    /// Mengonstruksi Pacemaker baru dengan ronde awal 1 dan consecutive_timeouts 0.
    #[inline]
    pub fn new(base_timeout_ms: u64) -> Self {
        Self {
            base_timeout_ms,
            consecutive_timeouts: 0,
            current_round: 1,
        }
    }

    /// Mengonstruksi Pacemaker baru dengan ronde tertentu yang ditentukan.
    #[inline]
    pub fn with_round(base_timeout_ms: u64, current_round: u64) -> Self {
        Self {
            base_timeout_ms,
            consecutive_timeouts: 0,
            current_round,
        }
    }

    /// Mengambil nomor ronde saat ini.
    #[inline]
    pub fn current_round(&self) -> u64 {
        self.current_round
    }

    /// Mengambil jumlah timeout berturut-turut saat ini.
    #[inline]
    pub fn consecutive_timeouts(&self) -> u32 {
        self.consecutive_timeouts
    }

    /// Mengambil durasi basis timeout.
    #[inline]
    pub fn base_timeout_ms(&self) -> u64 {
        self.base_timeout_ms
    }

    /// Menghitung durasi batas waktu saat ini dengan backoff eksponensial integer murni:
    ///
    /// `timeout = base_timeout_ms * (1 << min(consecutive_timeouts, MAX_BACKOFF_EXPONENT))`
    #[inline]
    pub fn current_timeout_ms(&self) -> u64 {
        let exponent = self.consecutive_timeouts.min(MAX_BACKOFF_EXPONENT);
        let multiplier = 1u64 << exponent;
        self.base_timeout_ms.saturating_mul(multiplier)
    }

    /// Dipanggil saat timer ronde kedaluwarsa: menambah consecutive_timeouts dan mengembalikan timeout baru.
    pub fn on_timeout(&mut self) -> u64 {
        self.consecutive_timeouts = self.consecutive_timeouts.saturating_add(1);
        self.current_timeout_ms()
    }

    /// Dipanggil saat sebuah proposal segmen berhasil mencapai QuorumCertificate (QC):
    /// memperbarui nomor ronde dan mereset penghitung timeout kembali ke 0.
    pub fn on_success(&mut self, new_round: u64) {
        self.current_round = new_round;
        self.consecutive_timeouts = 0;
    }

    /// Memajukan nomor ronde secara eksplisit.
    pub fn advance_round(&mut self, new_round: u64) {
        self.current_round = new_round;
    }

    /// Memproses TimeoutCertificate untuk memajukan nomor ronde dari R ke R + 1 secara deterministik.
    pub fn process_timeout_certificate(
        &mut self,
        tc: &TimeoutCertificate,
    ) -> Result<u64, ConsensusError> {
        let next_round = tc
            .round
            .checked_add(1)
            .ok_or(ConsensusError::ArithmeticOverflow)?;

        if next_round > self.current_round {
            self.current_round = next_round;
            self.consecutive_timeouts = self.consecutive_timeouts.saturating_add(1);
        }

        Ok(self.current_round)
    }

    /// Memilih pemimpin proposal (proposer / leader) secara deterministik tanpa overhead komunikasi:
    ///
    /// `leader_idx = (epoch + round) % N`
    ///
    /// Menolak dengan `NoActiveValidators` jika daftar validator aktif kosong.
    pub fn elect_leader(
        epoch: u64,
        round: u64,
        active_validators: &[AccountId],
    ) -> Result<AccountId, ConsensusError> {
        if active_validators.is_empty() {
            return Err(ConsensusError::NoActiveValidators);
        }

        let total = active_validators.len() as u64;
        let slot = epoch
            .checked_add(round)
            .ok_or(ConsensusError::ArithmeticOverflow)?;
        let index = (slot % total) as usize;

        Ok(active_validators[index])
    }

    /// Mengambil pemimpin saat ini untuk ronde dan epoch aktif.
    #[inline]
    pub fn current_leader(
        &self,
        epoch: u64,
        active_validators: &[AccountId],
    ) -> Result<AccountId, ConsensusError> {
        Self::elect_leader(epoch, self.current_round, active_validators)
    }
}

impl Default for Pacemaker {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_TIMEOUT_MS)
    }
}
