//! Modul parsing nilai desimal ke integer atomik (zero-float) dan konversi heksadesimal.

use axiom_primitives::value::AxmValue;

use crate::error::CliError;

/// Presisi maksimum pecahan moneter AXM (10 digit desimal).
pub const AXM_DECIMAL_PLACES: usize = 10;
/// Faktor pengali 1 AXM utuh ke satuan atomik terkecil ($10^{10}$).
pub const AXM_SCALE: u128 = 10_000_000_000;

/// Mengonversi nilai representasi string desimal (misal `"150.2500000000"`) menjadi `AxmValue`
/// murni berbasis integer terproteksi tanpa keterlibatan tipe floating-point (`f32`/`f64`).
pub fn parse_axm_decimal(input: &str) -> Result<AxmValue, CliError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(CliError::InvalidAmountFormat("Amount cannot be empty"));
    }

    // 1. Validasi karakter numerik dan titik desimal tunggal
    let mut dot_count = 0;
    for c in trimmed.chars() {
        if c == '.' {
            dot_count += 1;
            if dot_count > 1 {
                return Err(CliError::InvalidAmountFormat("Multiple decimal points found"));
            }
        } else if !c.is_ascii_digit() {
            return Err(CliError::InvalidAmountFormat("Non-numeric character in amount"));
        }
    }

    // 2. Pisahkan bagian bilangan utuh (whole) dan pecahan (fraction)
    let (whole_str, frac_str) = match trimmed.split_once('.') {
        Some((w, f)) => {
            let whole = if w.is_empty() { "0" } else { w };
            (whole, f)
        }
        None => (trimmed, ""),
    };

    // 3. Validasi batas panjang pecahan (maksimal 10 digit)
    if frac_str.len() > AXM_DECIMAL_PLACES {
        return Err(CliError::PrecisionExceeded);
    }

    // 4. Lakukan right-padding dengan '0' hingga tepat 10 digit
    let mut padded_frac = String::with_capacity(AXM_DECIMAL_PLACES);
    padded_frac.push_str(frac_str);
    while padded_frac.len() < AXM_DECIMAL_PLACES {
        padded_frac.push('0');
    }

    // 5. Parse whole string ke u128 dan kalikan dengan 10^10 secara aman
    let whole_val: u128 = whole_str
        .parse()
        .map_err(|_| CliError::InvalidAmountFormat("Integer part overflows integer capacity"))?;
    let whole_atomic = whole_val
        .checked_mul(AXM_SCALE)
        .ok_or(CliError::InvalidAmountFormat("Total amount overflows u128"))?;

    // 6. Parse fraction string ke u128 dan tambahkan ke nilai atomik utuh
    let frac_val: u128 = padded_frac
        .parse()
        .map_err(|_| CliError::InvalidAmountFormat("Fraction part overflows integer capacity"))?;
    let total_atomic = whole_atomic
        .checked_add(frac_val)
        .ok_or(CliError::InvalidAmountFormat("Total amount overflows u128"))?;

    Ok(AxmValue::from_atomic(total_atomic))
}

/// Mengonversi string heksadesimal 64 karakter menjadi array 32 byte.
pub fn hex_to_bytes_32(hex_str: &str) -> Result<[u8; 32], CliError> {
    let clean = hex_str.trim().trim_start_matches("0x").trim_start_matches("0X");
    if clean.len() != 64 {
        return Err(CliError::InvalidKeyLength);
    }

    let mut bytes = [0u8; 32];
    for (i, byte_slot) in bytes.iter_mut().enumerate() {
        let start = i.checked_mul(2).ok_or(CliError::InvalidHexFormat)?;
        let end = start.checked_add(2).ok_or(CliError::InvalidHexFormat)?;
        let hex_pair = &clean[start..end];
        *byte_slot = u8::from_str_radix(hex_pair, 16).map_err(|_| CliError::InvalidHexFormat)?;
    }

    Ok(bytes)
}

/// Mengonversi irisan byte menjadi representasi string heksadesimal huruf kecil.
pub fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}
