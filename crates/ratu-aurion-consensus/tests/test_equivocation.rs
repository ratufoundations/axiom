//! Integration tests for Equivocation Detection & Double-Sign Slashing (OPT-CONSENSUS-01).

use ratu_aurion_consensus::error::ConsensusError;
use ratu_aurion_consensus::evidence::{EquivocationEvidence, VoteRecord};
use ratu_aurion_consensus::slashing::{SlashingLedger, HARD_SLASH_PENALTY_BPS};
use ratu_aurion_primitives::crypto::{AccountId, Hash, Signature};
use ratu_aurion_primitives::record::RECORD_KIND_SYSTEM_NOTIF;
use ratu_aurion_primitives::value::AurValue;
use ed25519_dalek::SigningKey;

fn create_validator(seed_byte: u8) -> (SigningKey, AccountId) {
    let mut seed = [0u8; 32];
    seed[0] = seed_byte;
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();
    let account = AccountId::new(verifying_key.to_bytes());
    (signing_key, account)
}

#[test]
fn test_valid_equivocation_evidence_verification() {
    let (signing_key, validator) = create_validator(1);
    let block_hash_x = Hash::new([0x11; 32]);
    let block_hash_y = Hash::new([0x22; 32]);

    let vote_a = VoteRecord::sign(validator, 1, 1, block_hash_x, &signing_key);
    let vote_b = VoteRecord::sign(validator, 1, 1, block_hash_y, &signing_key);

    let evidence = EquivocationEvidence::new(validator, 1, 1, vote_a, vote_b);
    assert!(evidence.verify().is_ok());
}

#[test]
fn test_reject_non_equivocation_identical_blocks() {
    let (signing_key, validator) = create_validator(2);
    let block_hash_x = Hash::new([0x33; 32]);

    let vote_a = VoteRecord::sign(validator, 1, 1, block_hash_x, &signing_key);
    let vote_b = VoteRecord::sign(validator, 1, 1, block_hash_x, &signing_key);

    let evidence = EquivocationEvidence::new(validator, 1, 1, vote_a, vote_b);
    let res = evidence.verify();
    assert_eq!(res, Err(ConsensusError::NonEquivocatingVotes));
}

#[test]
fn test_reject_differing_epochs_or_rounds() {
    let (signing_key, validator) = create_validator(3);
    let block_hash_x = Hash::new([0x44; 32]);
    let block_hash_y = Hash::new([0x55; 32]);

    // Round mismatch between votes (round 1 vs round 2)
    let vote_a = VoteRecord::sign(validator, 1, 1, block_hash_x, &signing_key);
    let vote_b = VoteRecord::sign(validator, 1, 2, block_hash_y, &signing_key);

    let evidence = EquivocationEvidence::new(validator, 1, 1, vote_a, vote_b);
    let res = evidence.verify();
    assert_eq!(res, Err(ConsensusError::InvalidEvidenceSlotMismatch));

    // Epoch mismatch between votes (epoch 1 vs epoch 2)
    let vote_c = VoteRecord::sign(validator, 1, 1, block_hash_x, &signing_key);
    let vote_d = VoteRecord::sign(validator, 2, 1, block_hash_y, &signing_key);

    let evidence_epoch = EquivocationEvidence::new(validator, 1, 1, vote_c, vote_d);
    let res_epoch = evidence_epoch.verify();
    assert_eq!(res_epoch, Err(ConsensusError::InvalidEvidenceSlotMismatch));
}

#[test]
fn test_reject_invalid_signature_in_evidence() {
    let (signing_key, validator) = create_validator(4);
    let block_hash_x = Hash::new([0x66; 32]);
    let block_hash_y = Hash::new([0x77; 32]);

    let vote_a = VoteRecord::sign(validator, 1, 1, block_hash_x, &signing_key);
    let mut vote_b = VoteRecord::sign(validator, 1, 1, block_hash_y, &signing_key);

    // Tamper 1 byte of the signature in Vote B
    let mut sig_bytes = vote_b.signature.to_bytes();
    sig_bytes[0] ^= 0xFF;
    vote_b.signature = Signature::new(sig_bytes);

    let evidence = EquivocationEvidence::new(validator, 1, 1, vote_a, vote_b);
    let res = evidence.verify();
    assert_eq!(res, Err(ConsensusError::InvalidSignature));
}

#[test]
fn test_deterministic_slashing_and_tombstone() {
    let (signing_key, validator) = create_validator(5);
    let block_hash_x = Hash::new([0x88; 32]);
    let block_hash_y = Hash::new([0x99; 32]);

    let vote_a = VoteRecord::sign(validator, 1, 1, block_hash_x, &signing_key);
    let vote_b = VoteRecord::sign(validator, 1, 1, block_hash_y, &signing_key);
    let evidence = EquivocationEvidence::new(validator, 1, 1, vote_a, vote_b);

    let mut slashing_ledger = SlashingLedger::new();
    let bonded_stake = AurValue::from_whole_aur(1_000).unwrap();

    // Apply verified EquivocationEvidence with 100% slashing penalty (10,000 BPS)
    let (slashed_amount, _) = slashing_ledger
        .process_evidence(&evidence, bonded_stake, HARD_SLASH_PENALTY_BPS)
        .unwrap();

    assert_eq!(slashed_amount, bonded_stake);
    let remaining_stake = bonded_stake.checked_sub(slashed_amount).unwrap();
    assert_eq!(remaining_stake, AurValue::ZERO);
    assert!(slashing_ledger.is_tombstoned(&validator));

    // Attempt to record any further votes from Validator V; assert returns ValidatorTombstoned
    let block_hash_z = Hash::new([0xAA; 32]);
    let vote_c = VoteRecord::sign(validator, 1, 2, block_hash_z, &signing_key);
    let res = slashing_ledger.record_vote(&vote_c);
    assert_eq!(res, Err(ConsensusError::ValidatorTombstoned));

    // Also assert that processing another evidence for the same tombstoned validator fails fast
    let res_double = slashing_ledger.process_evidence(&evidence, bonded_stake, HARD_SLASH_PENALTY_BPS);
    assert_eq!(res_double, Err(ConsensusError::ValidatorTombstoned));
}

#[test]
fn test_system_notification_mutation_generation() {
    let (signing_key, validator) = create_validator(6);
    let block_hash_x = Hash::new([0xBB; 32]);
    let block_hash_y = Hash::new([0xCC; 32]);

    let vote_a = VoteRecord::sign(validator, 1, 1, block_hash_x, &signing_key);
    let vote_b = VoteRecord::sign(validator, 1, 1, block_hash_y, &signing_key);
    let evidence = EquivocationEvidence::new(validator, 1, 1, vote_a, vote_b);

    let mut slashing_ledger = SlashingLedger::new();
    let bonded_stake = AurValue::from_whole_aur(500).unwrap();

    let (slashed_amount, mutation) = slashing_ledger
        .process_evidence(&evidence, bonded_stake, HARD_SLASH_PENALTY_BPS)
        .unwrap();

    assert_eq!(slashed_amount, bonded_stake);
    assert_eq!(mutation.record_kind, RECORD_KIND_SYSTEM_NOTIF);
    assert_eq!(mutation.sender, validator);
    assert_eq!(mutation.recipient, AccountId::new([0u8; 32]));
    assert_eq!(mutation.amount, bonded_stake);
    assert_eq!(mutation.epoch, 1);
    assert_ne!(mutation.signature.to_bytes(), [0u8; 64]);
}
