//! Modul struktur mutasi dan rekaman peristiwa berukuran tetap (161 byte).

use crate::crypto::{AccountId, Signature};
use crate::value::AurValue;

/// Ukuran pasti satu MutationRecord dalam byte (161 byte).
pub const RECORD_SIZE: usize = 161;

/// Jenis record: Transfer mutasi nilai antar-akun.
pub const RECORD_KIND_TRANSFER: u8 = 1;

/// Jenis record: Notifikasi status sistem / operasi jaringan.
pub const RECORD_KIND_SYSTEM_NOTIF: u8 = 2;

/// Jenis record: Pemusnahan biaya transaksi (Fee Burn).
pub const RECORD_KIND_FEE_BURN: u8 = 0x02;

/// Jenis record: Emisi reward blok ke validator leader (Block Reward).
pub const RECORD_KIND_BLOCK_REWARD: u8 = 0x03;

/// Penyebut basis poin deterministik (10_000 basis points = 100%).
pub const BPS_DENOMINATOR: u128 = 10_000;

/// Rasio pembakaran biaya transaksi baku (70% = 7_000 BPS).
pub const DEFAULT_BURN_RATIO_BPS: u128 = 7_000;

/// Rasio tip prioritas transaksi untuk leader (30% = 3_000 BPS).
pub const DEFAULT_TIP_RATIO_BPS: u128 = 3_000;

/// Target inflasi tahunan moneter (5% = 500 BPS).
pub const ANNUAL_INFLATION_BPS: u128 = 500;

/// Estimasi jumlah blok per tahun (1 detik per blok = 31.536.000 blok).
pub const BLOCKS_PER_YEAR: u128 = 31_536_000;

/// Biaya dasar per transaksi dalam unit atomik (0.001 AUR = 10_000_000 atomik).
pub const BASE_TX_FEE_ATOMIC: u128 = 10_000_000; // 0.001 AUR

/// Biaya dasar per transaksi sebagai representasi AurValue.
pub const BASE_TX_FEE: AurValue = AurValue::from_atomic(BASE_TX_FEE_ATOMIC);

/// Rekaman mutasi status berukuran tetap (fixed-width 161 byte).
///
/// Dirancang secara linear untuk efisiensi penulisan sekuensial disk mekanis
/// dan kesederhanaan rotasi/pemotongan arsip (pruning) per-epoch.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MutationRecord {
    /// Penanda nomor epoch / segmen rotasi waktu (8 byte).
    pub epoch: u64,
    /// Nomor urut transaksi / mutasi deterministik (8 byte).
    pub sequence_number: u64,
    /// Kategori rekaman (1 byte, 1 = Transfer, 2 = NotifikasiSistem).
    pub record_kind: u8,
    /// Alamat pengirim mutasi (32 byte).
    pub sender: AccountId,
    /// Alamat penerima mutasi (32 byte).
    pub recipient: AccountId,
    /// Nilai mutasi moneter (16 byte, basis 10 desimal integer).
    pub amount: AurValue,
    /// Tanda tangan kriptografis autorisasi (64 byte).
    pub signature: Signature,
}

impl MutationRecord {
    /// Ukuran pasti satu MutationRecord dalam byte (161 byte, RFC-0001 §3).
    pub const RECORD_SIZE: usize = RECORD_SIZE;

    /// Panjang data mutasi yang ditandatangani oleh pengirim (97 byte, RFC-0001 §3.3).
    pub const SIGNING_PAYLOAD_SIZE: usize = 97;

    /// Menghasilkan 97 byte pertama payload mutasi untuk verifikasi tanda tangan kriptografi Ed25519.
    pub fn signing_payload(&self) -> [u8; Self::SIGNING_PAYLOAD_SIZE] {
        let bytes = self.to_bytes();
        let mut payload = [0u8; Self::SIGNING_PAYLOAD_SIZE];
        payload.copy_from_slice(&bytes[0..Self::SIGNING_PAYLOAD_SIZE]);
        payload
    }

    /// Serialisasi record ke dalam representasi biner tepat 161 byte (Little-Endian).
    pub fn to_bytes(&self) -> [u8; RECORD_SIZE] {
        let mut buf = [0u8; RECORD_SIZE];
        buf[0..8].copy_from_slice(&self.epoch.to_le_bytes());
        buf[8..16].copy_from_slice(&self.sequence_number.to_le_bytes());
        buf[16] = self.record_kind;
        buf[17..49].copy_from_slice(self.sender.as_bytes());
        buf[49..81].copy_from_slice(self.recipient.as_bytes());
        buf[81..97].copy_from_slice(&self.amount.to_le_bytes());
        buf[97..161].copy_from_slice(self.signature.as_bytes());
        buf
    }

    /// Deserialisasi record dari representasi biner 161 byte (Little-Endian).
    pub fn from_bytes(bytes: &[u8; RECORD_SIZE]) -> Self {
        let epoch = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let sequence_number = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11],
            bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        let record_kind = bytes[16];

        let mut sender_bytes = [0u8; 32];
        sender_bytes.copy_from_slice(&bytes[17..49]);
        let sender = AccountId::new(sender_bytes);

        let mut recipient_bytes = [0u8; 32];
        recipient_bytes.copy_from_slice(&bytes[49..81]);
        let recipient = AccountId::new(recipient_bytes);

        let mut amount_bytes = [0u8; 16];
        amount_bytes.copy_from_slice(&bytes[81..97]);
        let amount = AurValue::from_le_bytes(amount_bytes);

        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(&bytes[97..161]);
        let signature = Signature::new(sig_bytes);

        Self {
            epoch,
            sequence_number,
            record_kind,
            sender,
            recipient,
            amount,
            signature,
        }
    }
}
