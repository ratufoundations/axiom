//! Modul nilai moneter terproteksi dan aritmatika integer terdeterminasi.

use core::fmt;

/// Jumlah angka desimal tetap untuk token Axiom (10 desimal).
pub const AXM_DECIMALS: u32 = 10;

/// Faktor pengali unit atomik (10^10 = 10_000_000_000).
pub const ATOMIC_UNIT_FACTOR: u128 = 10_000_000_000;

/// Representasi nilai moneter Axiom berbasis unit atomik integer 128-bit.
///
/// Dilarang keras menggunakan tipe data floating-point. Seluruh komputasi
/// dilakukan pada skala integer atomik tetap (fixed-point integer scale).
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct AxmValue(pub u128);

impl AxmValue {
    /// Nilai nol.
    pub const ZERO: Self = Self(0);

    /// Nilai maksimum yang dapat ditampung.
    pub const MAX: Self = Self(u128::MAX);

    /// Mengonstruksi AxmValue langsung dari unit atomik.
    #[inline]
    pub const fn from_atomic(amount: u128) -> Self {
        Self(amount)
    }

    /// Mengambil unit atomik mentah.
    #[inline]
    pub const fn to_atomic(self) -> u128 {
        self.0
    }

    /// Mengonstruksi AxmValue dari satuan utuh AXM (dikalikan 10^10).
    pub const fn from_whole_axm(whole: u128) -> Option<Self> {
        match whole.checked_mul(ATOMIC_UNIT_FACTOR) {
            Some(val) => Some(Self(val)),
            None => None,
        }
    }

    /// Serialisasi nilai ke dalam 16 byte Little-Endian.
    #[inline]
    pub const fn to_le_bytes(self) -> [u8; 16] {
        self.0.to_le_bytes()
    }

    /// Deserialisasi nilai dari 16 byte Little-Endian.
    #[inline]
    pub const fn from_le_bytes(bytes: [u8; 16]) -> Self {
        Self(u128::from_le_bytes(bytes))
    }

    /// Penambahan terproteksi terhadap overflow.
    #[inline]
    pub const fn checked_add(self, other: Self) -> Option<Self> {
        match self.0.checked_add(other.0) {
            Some(res) => Some(Self(res)),
            None => None,
        }
    }

    /// Pengurangan terproteksi terhadap underflow.
    #[inline]
    pub const fn checked_sub(self, other: Self) -> Option<Self> {
        match self.0.checked_sub(other.0) {
            Some(res) => Some(Self(res)),
            None => None,
        }
    }

    /// Perkalian rasio integer deterministik: `(self * numerator) / denominator`.
    ///
    /// Menggunakan promosi 256-bit penuh untuk perkalian sebelum pembagian,
    /// mencegah pembulatan prematur tanpa menggunakan tipe data float.
    pub fn checked_mul_ratio(self, numerator: u128, denominator: u128) -> Option<Self> {
        if denominator == 0 {
            return None;
        }
        let (high, low) = mul_u128_to_u256(self.0, numerator);
        let (quotient, _) = div_rem_u256_by_u128(high, low, denominator)?;
        Some(Self(quotient))
    }
}

impl fmt::Display for AxmValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / ATOMIC_UNIT_FACTOR;
        let frac = self.0 % ATOMIC_UNIT_FACTOR;
        write!(f, "{whole}.{frac:010}")
    }
}

/// Perkalian dua bilangan integer 128-bit menghasilkan representasi 256-bit `(high, low)`.
pub const fn mul_u128_to_u256(a: u128, b: u128) -> (u128, u128) {
    let a_lo = (a as u64) as u128;
    let a_hi = a >> 64;
    let b_lo = (b as u64) as u128;
    let b_hi = b >> 64;


    let p0 = a_lo * b_lo;
    let p1 = a_lo * b_hi;
    let p2 = a_hi * b_lo;
    let p3 = a_hi * b_hi;

    let mid = p1 + (p0 >> 64);
    let mid_lo = (mid as u64) as u128;
    let mid_hi = mid >> 64;

    let mid2 = mid_lo + p2;
    let low = ((mid2 as u64) as u128) << 64 | ((p0 as u64) as u128);
    let high = p3 + mid_hi + (mid2 >> 64);

    (high, low)
}

/// Pembagian integer 256-bit `(high, low)` dengan pembagi `denom: u128`.
/// Mengembalikan `Some((quotient, remainder))` jika hasil pembagian muat dalam `u128`.
pub fn div_rem_u256_by_u128(high: u128, low: u128, denom: u128) -> Option<(u128, u128)> {
    if denom == 0 {
        return None;
    }
    // Jika bagian high >= denom, maka hasil bagi melebihi kapasitas u128
    if high >= denom {
        return None;
    }

    let mut rem = high;
    let mut quotient: u128 = 0;

    let mut i = 128;
    while i > 0 {
        i -= 1;
        let bit = (low >> i) & 1;
        let rem_overflow = (rem >> 127) != 0;
        rem = (rem << 1) | bit;
        if rem_overflow || rem >= denom {
            rem = rem.wrapping_sub(denom);
            quotient |= 1 << i;
        }
    }

    Some((quotient, rem))
}
