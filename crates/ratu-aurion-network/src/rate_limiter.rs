//! Modul pembatas laju transmisi (Rate Limiter) berbasis Token Bucket integer murni.

use std::collections::HashMap;

use crate::error::NetworkError;

/// Pembatas laju Token Bucket deterministik tanpa floating-point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenBucketLimiter {
    capacity: u64,
    tokens: u64,
    rate_per_sec: u64,
    last_refill_ms: u64,
}

impl TokenBucketLimiter {
    /// Mengonstruksi TokenBucketLimiter baru dengan kapasitas penuh.
    #[inline]
    pub fn new(capacity: u64, rate_per_sec: u64, start_time_ms: u64) -> Self {
        Self {
            capacity,
            tokens: capacity,
            rate_per_sec,
            last_refill_ms: start_time_ms,
        }
    }

    /// Mengambil jumlah token saat ini.
    #[inline]
    pub fn tokens(&self) -> u64 {
        self.tokens
    }

    /// Mengambil kapasitas maksimum token bucket.
    #[inline]
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Mengambil laju pengisian token per detik.
    #[inline]
    pub fn rate_per_sec(&self) -> u64 {
        self.rate_per_sec
    }

    /// Mengambil timestamp pengisian terakhir dalam milidetik.
    #[inline]
    pub fn last_refill_ms(&self) -> u64 {
        self.last_refill_ms
    }

    /// Mengisi ulang token berdasarkan waktu yang telah berlalu (elapsed_ms).
    pub fn refill_at(&mut self, current_ms: u64) {
        let elapsed_ms = current_ms.saturating_sub(self.last_refill_ms);
        if elapsed_ms == 0 || self.rate_per_sec == 0 {
            return;
        }

        let added_tokens = (elapsed_ms.saturating_mul(self.rate_per_sec)) / 1_000;
        if added_tokens > 0 {
            self.tokens = self.capacity.min(self.tokens.saturating_add(added_tokens));
            if self.tokens >= self.capacity {
                self.last_refill_ms = current_ms;
            } else {
                let consumed_ms = (added_tokens.saturating_mul(1_000)) / self.rate_per_sec;
                self.last_refill_ms = self.last_refill_ms.saturating_add(consumed_ms);
            }
        }
    }

    /// Mencoba mengonsumsi sejumlah token pada timestamp saat ini.
    pub fn try_consume(&mut self, current_ms: u64, count: u64) -> Result<(), NetworkError> {
        self.try_consume_peer(current_ms, count, "default")
    }

    /// Mencoba mengonsumsi sejumlah token dengan nama peer eksplisit untuk pelaporan error.
    pub fn try_consume_peer(
        &mut self,
        current_ms: u64,
        count: u64,
        peer: &str,
    ) -> Result<(), NetworkError> {
        self.refill_at(current_ms);
        if self.tokens >= count {
            self.tokens -= count;
            Ok(())
        } else {
            Err(NetworkError::RateLimitExceeded {
                peer: peer.to_string(),
            })
        }
    }
}

/// Tabel pemetaan token bucket per-peer berdasarkan pengenal string (IP atau PeerId).
#[derive(Debug, Clone)]
pub struct PeerRateLimiterTable {
    default_capacity: u64,
    default_rate_per_sec: u64,
    limiters: HashMap<String, TokenBucketLimiter>,
}

impl PeerRateLimiterTable {
    /// Mengonstruksi tabel baru dengan konfigurasi kapasitas dan laju default.
    #[inline]
    pub fn new(default_capacity: u64, default_rate_per_sec: u64) -> Self {
        Self {
            default_capacity,
            default_rate_per_sec,
            limiters: HashMap::new(),
        }
    }

    /// Memeriksa dan mengonsumsi kuota token untuk peer tertentu.
    pub fn check_peer(
        &mut self,
        peer: &str,
        current_ms: u64,
        count: u64,
    ) -> Result<(), NetworkError> {
        let cap = self.default_capacity;
        let rate = self.default_rate_per_sec;
        let limiter = self
            .limiters
            .entry(peer.to_string())
            .or_insert_with(|| TokenBucketLimiter::new(cap, rate, current_ms));

        limiter.try_consume_peer(current_ms, count, peer)
    }

    /// Menghapus pembatas laju untuk peer yang terputus.
    #[inline]
    pub fn remove_peer(&mut self, peer: &str) -> Option<TokenBucketLimiter> {
        self.limiters.remove(peer)
    }

    /// Jumlah peer yang sedang dipantau.
    #[inline]
    pub fn peer_count(&self) -> usize {
        self.limiters.len()
    }
}
