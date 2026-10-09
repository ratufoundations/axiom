//! Integration tests for Deterministic View Change & Pacemaker (OPT-CONSENSUS-02).

use ratu_aurion_consensus::error::ConsensusError;
use ratu_aurion_consensus::evidence::{EquivocationEvidence, VoteRecord};
use ratu_aurion_consensus::pacemaker::Pacemaker;
use ratu_aurion_consensus::slashing::{SlashingLedger, HARD_SLASH_PENALTY_BPS};
use ratu_aurion_consensus::timeout::{TimeoutCertificate, TimeoutMsg};
use ratu_aurion_consensus::validator_set::{ValidatorInfo, ValidatorSet};
use ratu_aurion_primitives::crypto::{AccountId, Hash};
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
fn test_deterministic_leader_election() {
    let (_, val_a) = create_validator(1);
    let (sk_b, val_b) = create_validator(2);
    let (_, val_c) = create_validator(3);
    let (_, val_d) = create_validator(4);

    let all_validators = vec![val_a, val_b, val_c, val_d];
    let epoch = 1;

    // Assert leader selection for rounds 1, 2, 3, 4, 5 rotates strictly: (epoch + round) % 4
    for round in 1..=5 {
        let leader = Pacemaker::elect_leader(epoch, round, &all_validators).unwrap();
        let expected_idx = ((epoch + round) % 4) as usize;
        assert_eq!(leader, all_validators[expected_idx]);
    }

    // Tombstone validator B via verified equivocation evidence
    let vote_b1 = VoteRecord::sign(val_b, 1, 1, Hash::new([0x11; 32]), &sk_b);
    let vote_b2 = VoteRecord::sign(val_b, 1, 1, Hash::new([0x22; 32]), &sk_b);
    let evidence = EquivocationEvidence::new(val_b, 1, 1, vote_b1, vote_b2);

    let mut slashing_ledger = SlashingLedger::new();
    let bonded_stake = AurValue::from_whole_aur(1_000).unwrap();
    slashing_ledger
        .process_evidence(&evidence, bonded_stake, HARD_SLASH_PENALTY_BPS)
        .unwrap();

    assert!(slashing_ledger.is_tombstoned(&val_b));

    // Leader selection skips B and rotates across {A, C, D}
    let active_validators: Vec<AccountId> = all_validators
        .iter()
        .copied()
        .filter(|v| !slashing_ledger.is_tombstoned(v))
        .collect();

    assert_eq!(active_validators.len(), 3);
    assert!(!active_validators.contains(&val_b));

    for round in 1..=5 {
        let leader = Pacemaker::elect_leader(epoch, round, &active_validators).unwrap();
        let expected_idx = ((epoch + round) % 3) as usize;
        assert_eq!(leader, active_validators[expected_idx]);
        assert_ne!(leader, val_b);
    }
}

#[test]
fn test_timeout_certificate_supermajority_assembly() {
    let (sk_a, val_a) = create_validator(11);
    let (sk_b, val_b) = create_validator(12);
    let (sk_c, val_c) = create_validator(13);
    let (_, val_d) = create_validator(14);

    let val_set = ValidatorSet::new(vec![
        ValidatorInfo::new(val_a, 1),
        ValidatorInfo::new(val_b, 1),
        ValidatorInfo::new(val_c, 1),
        ValidatorInfo::new(val_d, 1),
    ])
    .unwrap();

    assert_eq!(val_set.quorum_threshold(), 3);

    let msg_a = TimeoutMsg::sign(val_a, 1, 1, 0, &sk_a);
    let msg_b = TimeoutMsg::sign(val_b, 1, 1, 1, &sk_b);

    // 2 votes out of 4: insufficient quorum
    let res = TimeoutCertificate::assemble(1, 1, &[msg_a.clone(), msg_b.clone()], &val_set);
    assert_eq!(
        res,
        Err(ConsensusError::InsufficientTimeoutQuorum {
            required: 3,
            actual: 2
        })
    );

    // Add 3rd valid vote: assembly succeeds
    let msg_c = TimeoutMsg::sign(val_c, 1, 1, 3, &sk_c);
    let tc = TimeoutCertificate::assemble(1, 1, &[msg_a, msg_b, msg_c], &val_set).unwrap();

    assert_eq!(tc.epoch, 1);
    assert_eq!(tc.round, 1);
    assert_eq!(tc.high_qc_round, 3);
    assert_eq!(tc.signatures.len(), 3);
}

#[test]
fn test_reject_timeout_mismatch_or_duplicate() {
    let (sk_a, val_a) = create_validator(21);
    let (sk_b, val_b) = create_validator(22);
    let (sk_c, val_c) = create_validator(23);
    let (_, val_d) = create_validator(24);

    let val_set = ValidatorSet::new(vec![
        ValidatorInfo::new(val_a, 1),
        ValidatorInfo::new(val_b, 1),
        ValidatorInfo::new(val_c, 1),
        ValidatorInfo::new(val_d, 1),
    ])
    .unwrap();

    // 1. Duplicate votes from the same validator
    let msg_a1 = TimeoutMsg::sign(val_a, 1, 1, 0, &sk_a);
    let msg_a2 = TimeoutMsg::sign(val_a, 1, 1, 0, &sk_a);
    let msg_b = TimeoutMsg::sign(val_b, 1, 1, 0, &sk_b);

    let res_dup = TimeoutCertificate::assemble(1, 1, &[msg_a1, msg_a2, msg_b], &val_set);
    assert_eq!(res_dup, Err(ConsensusError::DuplicateTimeoutVote(val_a)));

    // 2. Differing round
    let msg_a = TimeoutMsg::sign(val_a, 1, 1, 0, &sk_a);
    let msg_b_diff_round = TimeoutMsg::sign(val_b, 1, 2, 0, &sk_b);
    let msg_c = TimeoutMsg::sign(val_c, 1, 1, 0, &sk_c);

    let res_round =
        TimeoutCertificate::assemble(1, 1, &[msg_a.clone(), msg_b_diff_round, msg_c.clone()], &val_set);
    assert_eq!(res_round, Err(ConsensusError::InvalidTimeoutSlotMismatch));

    // 3. Differing epoch
    let msg_b_diff_epoch = TimeoutMsg::sign(val_b, 2, 1, 0, &sk_b);
    let res_epoch = TimeoutCertificate::assemble(1, 1, &[msg_a, msg_b_diff_epoch, msg_c], &val_set);
    assert_eq!(res_epoch, Err(ConsensusError::InvalidTimeoutSlotMismatch));
}

#[test]
fn test_pacemaker_exponential_backoff_and_reset() {
    let mut pacemaker = Pacemaker::new(2_000);
    assert_eq!(pacemaker.current_timeout_ms(), 2_000);

    // Verify consecutive timeout progression
    assert_eq!(pacemaker.on_timeout(), 4_000);
    assert_eq!(pacemaker.on_timeout(), 8_000);
    assert_eq!(pacemaker.on_timeout(), 16_000);
    assert_eq!(pacemaker.on_timeout(), 32_000);
    assert_eq!(pacemaker.on_timeout(), 64_000);
    assert_eq!(pacemaker.on_timeout(), 128_000);

    // Further timeouts stay capped at 128,000 ms
    assert_eq!(pacemaker.on_timeout(), 128_000);
    assert_eq!(pacemaker.on_timeout(), 128_000);
    assert_eq!(pacemaker.current_timeout_ms(), 128_000);

    // Successful commit resets timeout back to 2,000 ms
    pacemaker.on_success(2);
    assert_eq!(pacemaker.current_timeout_ms(), 2_000);
    assert_eq!(pacemaker.consecutive_timeouts(), 0);
    assert_eq!(pacemaker.current_round(), 2);
}

#[test]
fn test_advance_round_on_timeout_certificate() {
    let mut pacemaker = Pacemaker::with_round(2_000, 3);
    assert_eq!(pacemaker.current_round(), 3);

    let tc = TimeoutCertificate::new(1, 3, 2, vec![]);
    let next_round = pacemaker.process_timeout_certificate(&tc).unwrap();
    assert_eq!(next_round, 4);
    assert_eq!(pacemaker.current_round(), 4);

    let (_, val_a) = create_validator(31);
    let (_, val_b) = create_validator(32);
    let (_, val_c) = create_validator(33);
    let (_, val_d) = create_validator(34);
    let active_validators = vec![val_a, val_b, val_c, val_d];

    let epoch = 1;
    let leader = pacemaker.current_leader(epoch, &active_validators).unwrap();
    let expected_leader = active_validators[((epoch + 4) % 4) as usize];
    assert_eq!(leader, expected_leader);
}
