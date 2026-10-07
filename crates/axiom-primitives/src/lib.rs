#![forbid(unsafe_code)]

//! # Axiom Primitives
//!
//! Pustaka fondasi untuk tipe data biner, representasi moneter terproteksi,
//! identitas kriptografi, framing paket, dan rekaman mutasi berukuran tetap.

pub mod crypto;
pub mod framing;
pub mod record;
pub mod value;

pub use crypto::*;
pub use framing::*;
pub use record::*;
pub use value::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_axm_value_arithmetic_and_bounds() {
        let val1 = AxmValue::from_atomic(10_000_000_000); // 1.0 AXM
        let val2 = AxmValue::from_atomic(25_000_000_000); // 2.5 AXM

        let sum = val1.checked_add(val2).expect("Addition should succeed");
        assert_eq!(sum.to_atomic(), 35_000_000_000);

        let diff = val2.checked_sub(val1).expect("Subtraction should succeed");
        assert_eq!(diff.to_atomic(), 15_000_000_000);

        // Underflow test
        assert!(val1.checked_sub(val2).is_none());

        // Overflow test
        let max_val = AxmValue::MAX;
        assert!(max_val.checked_add(AxmValue::from_atomic(1)).is_none());
    }

    #[test]
    fn test_axm_value_ratio_multiplication() {
        let val = AxmValue::from_atomic(10_000_000_000); // 1.0 AXM

        // Hitung 25% = 25 / 100
        let portion = val.checked_mul_ratio(25, 100).expect("Ratio calculation should succeed");
        assert_eq!(portion.to_atomic(), 2_500_000_000);

        // Pembagian dengan 0 harus menghasilkan None
        assert!(val.checked_mul_ratio(1, 0).is_none());

        // Pengujian presisi tinggi tanpa floating point: 1/3 lalu 2/3
        let third = val.checked_mul_ratio(1, 3).expect("1/3 should succeed");
        assert_eq!(third.to_atomic(), 3_333_333_333);

        // Perkalian skala besar yang melebihi u128 sebelum pembagian:
        // u128::MAX * 1 / 2
        let half_max = AxmValue::MAX.checked_mul_ratio(1, 2).expect("Half MAX should succeed");
        assert_eq!(half_max.to_atomic(), u128::MAX / 2);
    }

    #[test]
    fn test_axm_value_serialization_round_trip() {
        let original = AxmValue::from_atomic(987_654_321_012_345);
        let bytes = original.to_le_bytes();
        let restored = AxmValue::from_le_bytes(bytes);
        assert_eq!(original, restored);
    }

    #[test]
    fn test_crypto_types_round_trip() {
        let hash_bytes = [0x5au8; 32];
        let hash = Hash::new(hash_bytes);
        assert_eq!(hash.to_bytes(), hash_bytes);
        assert_eq!(hash.as_bytes(), &hash_bytes);

        let account_bytes = [0xa5u8; 32];
        let account = AccountId::new(account_bytes);
        assert_eq!(account.to_bytes(), account_bytes);

        let sig_bytes = [0x33u8; 64];
        let sig = Signature::new(sig_bytes);
        assert_eq!(sig.to_bytes(), sig_bytes);
    }

    #[test]
    fn test_framing_round_trip_and_validation() {
        let checksum = Hash::new([0xeeu8; 32]);
        let header = FrameHeader::new(1, 1024, checksum);
        let bytes = header.to_bytes();

        assert_eq!(bytes.len(), HEADER_SIZE);
        assert_eq!(&bytes[0..4], &MAGIC_BYTES);

        let restored = FrameHeader::from_bytes(&bytes).expect("Valid header must deserialize");
        assert_eq!(header, restored);
        assert_eq!(restored.version, 1);
        assert_eq!(restored.payload_len, 1024);
        assert_eq!(restored.checksum, checksum);

        // Header penolakan jika magic bytes salah
        let mut corrupted_bytes = bytes;
        corrupted_bytes[0] = b'B';
        let err = FrameHeader::from_bytes(&corrupted_bytes);
        assert!(err.is_err());
        assert_eq!(err.unwrap_err(), "Invalid frame magic bytes");
    }

    #[test]
    fn test_mutation_record_round_trip_161_bytes() {
        let sender = AccountId::new([0x11u8; 32]);
        let recipient = AccountId::new([0x22u8; 32]);
        let amount = AxmValue::from_atomic(50_000_000_000);
        let signature = Signature::new([0x77u8; 64]);

        let record = MutationRecord {
            epoch: 42,
            sequence_number: 1001,
            record_kind: RECORD_KIND_TRANSFER,
            sender,
            recipient,
            amount,
            signature,
        };

        let bytes = record.to_bytes();
        assert_eq!(bytes.len(), RECORD_SIZE);
        assert_eq!(bytes.len(), 161);

        let restored = MutationRecord::from_bytes(&bytes);
        assert_eq!(record, restored);
        assert_eq!(restored.epoch, 42);
        assert_eq!(restored.sequence_number, 1001);
        assert_eq!(restored.record_kind, RECORD_KIND_TRANSFER);
        assert_eq!(restored.sender, sender);
        assert_eq!(restored.recipient, recipient);
        assert_eq!(restored.amount, amount);
        assert_eq!(restored.signature, signature);
    }

    #[test]
    fn test_mutation_record_signing_payload_97_bytes() {
        let sender = AccountId::new([0xaa; 32]);
        let recipient = AccountId::new([0xbb; 32]);
        let amount = AxmValue::from_atomic(10_000_000_000);
        let signature = Signature::new([0xcc; 64]);

        let record = MutationRecord {
            epoch: 1,
            sequence_number: 10,
            record_kind: 1,
            sender,
            recipient,
            amount,
            signature,
        };

        let signing_payload = record.signing_payload();
        assert_eq!(signing_payload.len(), MutationRecord::SIGNING_PAYLOAD_SIZE);
        assert_eq!(signing_payload.len(), 97);

        let full_bytes = record.to_bytes();
        assert_eq!(&signing_payload[..], &full_bytes[..97]);
    }
}
