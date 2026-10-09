//! Tipe data biner tetap untuk identitas dan kriptografi Axiom.

use core::fmt;

/// Hash kriptografis berukuran tetap 32 byte.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    /// Hash kosong (seluruh byte bernilai 0).
    pub const ZERO: Self = Self([0u8; 32]);

    /// Mengonstruksi Hash baru dari array 32 byte.
    #[inline]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Mengambil salinan byte mentah.
    #[inline]
    pub const fn to_bytes(self) -> [u8; 32] {
        self.0
    }

    /// Mengonstruksi Hash dari array 32 byte.
    #[inline]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Referensi irisan byte mentah.
    #[inline]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<[u8; 32]> for Hash {
    #[inline]
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<Hash> for [u8; 32] {
    #[inline]
    fn from(hash: Hash) -> Self {
        hash.0
    }
}

impl AsRef<[u8]> for Hash {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash(0x")?;
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        write!(f, ")")
    }
}

/// Identitas akun atau alamat berbasis kunci publik berukuran tetap 32 byte.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct AccountId(pub [u8; 32]);

impl AccountId {
    /// Alamat kosong / sistem (seluruh byte bernilai 0).
    pub const ZERO: Self = Self([0u8; 32]);

    /// Mengonstruksi AccountId baru dari array 32 byte.
    #[inline]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Mengambil salinan byte mentah.
    #[inline]
    pub const fn to_bytes(self) -> [u8; 32] {
        self.0
    }

    /// Mengonstruksi AccountId dari array 32 byte.
    #[inline]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Referensi irisan byte mentah.
    #[inline]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<[u8; 32]> for AccountId {
    #[inline]
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<AccountId> for [u8; 32] {
    #[inline]
    fn from(account: AccountId) -> Self {
        account.0
    }
}

impl AsRef<[u8]> for AccountId {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AccountId(0x")?;
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        write!(f, ")")
    }
}

/// Tanda tangan digital kriptografis berukuran tetap 64 byte.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Signature(pub [u8; 64]);

impl Signature {
    /// Tanda tangan kosong (seluruh byte bernilai 0).
    pub const ZERO: Self = Self([0u8; 64]);

    /// Mengonstruksi Signature baru dari array 64 byte.
    #[inline]
    pub const fn new(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Mengambil salinan byte mentah.
    #[inline]
    pub const fn to_bytes(self) -> [u8; 64] {
        self.0
    }

    /// Mengonstruksi Signature dari array 64 byte.
    #[inline]
    pub const fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Referensi irisan byte mentah.
    #[inline]
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl Default for Signature {
    #[inline]
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<[u8; 64]> for Signature {
    #[inline]
    fn from(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }
}

impl From<Signature> for [u8; 64] {
    #[inline]
    fn from(sig: Signature) -> Self {
        sig.0
    }
}

impl AsRef<[u8]> for Signature {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature(0x")?;
        for b in &self.0[..8] {
            write!(f, "{b:02x}")?;
        }
        write!(f, "..len=64)")
    }
}
