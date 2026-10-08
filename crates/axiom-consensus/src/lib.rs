#![forbid(unsafe_code)]

//! # Axiom Consensus
//!
//! Modul aturan komitmen jaringan, validasi finalitas linier atas intisari segmen log disk,
//! dan pembuktian kuorum validator supermayoritas (> 2/3 bobot) berbasis Ed25519.

pub mod certificate;
pub mod error;
pub mod evidence;
pub mod pacemaker;
pub mod proposal;
pub mod slashing;
pub mod timeout;
pub mod validator_set;
pub mod vote;

pub use certificate::*;
pub use error::*;
pub use evidence::*;
pub use pacemaker::*;
pub use proposal::*;
pub use slashing::*;
pub use timeout::*;
pub use validator_set::*;
pub use vote::*;

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_primitives::crypto::{AccountId, Hash, Signature};
    use ed25519_dalek::SigningKey;

    fn create_validator_keypair(seed_byte: u8) -> (SigningKey, AccountId) {
        let mut seed = [0u8; 32];
        seed[0] = seed_byte;
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        let account = AccountId::new(verifying_key.to_bytes());
        (signing_key, account)
    }

    #[test]
    fn test_deterministic_integer_quorum_threshold() {
        // 1 validator: bobot 1 -> ambang (1*2)/3 + 1 = 1
        let (_, v1) = create_validator_keypair(1);
        let set1 = ValidatorSet::new(vec![ValidatorInfo::new(v1, 1)]).unwrap();
        assert_eq!(set1.quorum_threshold(), 1);

        // 3 validator: masing-masing bobot 1 (total 3) -> (3*2)/3 + 1 = 3
        let (_, v2) = create_validator_keypair(2);
        let (_, v3) = create_validator_keypair(3);
        let set3 = ValidatorSet::new(vec![
            ValidatorInfo::new(v1, 1),
            ValidatorInfo::new(v2, 1),
            ValidatorInfo::new(v3, 1),
        ])
        .unwrap();
        assert_eq!(set3.quorum_threshold(), 3);

        // 4 validator: masing-masing bobot 1 (total 4) -> (4*2)/3 + 1 = 3
        let (_, v4) = create_validator_keypair(4);
        let set4 = ValidatorSet::new(vec![
            ValidatorInfo::new(v1, 1),
            ValidatorInfo::new(v2, 1),
            ValidatorInfo::new(v3, 1),
            ValidatorInfo::new(v4, 1),
        ])
        .unwrap();
        assert_eq!(set4.quorum_threshold(), 3);

        // 10 validator berbobot 10 (total 100) -> (100*2)/3 + 1 = 67
        let mut validators_100 = Vec::new();
        for i in 1..=10 {
            let (_, acct) = create_validator_keypair(i);
            validators_100.push(ValidatorInfo::new(acct, 10));
        }
        let set_100 = ValidatorSet::new(validators_100).unwrap();
        assert_eq!(set_100.quorum_threshold(), 67);

        // Penolakan validator berbobot nol
        let (_, v_zero) = create_validator_keypair(99);
        let zero_res = ValidatorSet::new(vec![ValidatorInfo::new(v_zero, 0)]);
        assert_eq!(zero_res.unwrap_err(), ConsensusError::ZeroWeightValidator);

        // Penolakan validator duplikat
        let dup_res = ValidatorSet::new(vec![
            ValidatorInfo::new(v1, 10),
            ValidatorInfo::new(v1, 20),
        ]);
        assert_eq!(dup_res.unwrap_err(), ConsensusError::DuplicateVote);
    }

    #[test]
    fn test_vote_rejection_invalid_signature_and_unregistered() {
        let (_proposer_key, proposer) = create_validator_keypair(0x10);
        let (val1_key, val1) = create_validator_keypair(0x20);
        let (_val2_key, val2) = create_validator_keypair(0x30);
        let (eve_key, eve) = create_validator_keypair(0x99); // Validator tak terdaftar

        let val_set = ValidatorSet::new(vec![
            ValidatorInfo::new(proposer, 10),
            ValidatorInfo::new(val1, 40),
            ValidatorInfo::new(val2, 50),
        ])
        .unwrap();

        let proposal = SegmentProposal::new(
            1,
            0,
            Hash::new([0xab; 32]),
            proposer,
            1,
        );

        // 1. Suara sah dari val1
        let valid_vote = Vote::sign(&proposal, val1, &val1_key);
        assert!(valid_vote.verify(&proposal).is_ok());

        // 2. Penolakan jika tanda tangan rusak/palsu
        let mut forged_vote = valid_vote.clone();
        forged_vote.signature = Signature::new([0xff; 64]);
        assert_eq!(
            forged_vote.verify(&proposal).unwrap_err(),
            ConsensusError::InvalidSignature
        );

        // 3. Penolakan jika proposal digest tidak cocok
        let different_proposal = SegmentProposal::new(
            1,
            1, // segmen beda
            Hash::new([0xcd; 32]),
            proposer,
            1,
        );
        assert_eq!(
            valid_vote.verify(&different_proposal).unwrap_err(),
            ConsensusError::ProposalMismatch
        );

        // 4. Penolakan saat menambahkan suara dari validator yang tidak terdaftar (Eve)
        let eve_vote = Vote::sign(&proposal, eve, &eve_key);
        assert!(eve_vote.verify(&proposal).is_ok()); // Kriptografis sah, tapi bukan anggota validator set

        let mut qc = QuorumCertificate::new(proposal);
        let add_res = qc.add_vote(&eve_vote, &val_set);
        assert_eq!(add_res.unwrap_err(), ConsensusError::ValidatorNotRegistered);
    }

    #[test]
    fn test_quorum_certificate_success_supermajority() {
        let (proposer_key, proposer) = create_validator_keypair(0xa1);
        let (val1_key, val1) = create_validator_keypair(0xa2);
        let (val2_key, val2) = create_validator_keypair(0xa3);
        let (_val3_key, val3) = create_validator_keypair(0xa4);

        // 4 validator, masing-masing bobot 25 (total 100). Ambang kuorum = 67
        let val_set = ValidatorSet::new(vec![
            ValidatorInfo::new(proposer, 25),
            ValidatorInfo::new(val1, 25),
            ValidatorInfo::new(val2, 25),
            ValidatorInfo::new(val3, 25),
        ])
        .unwrap();
        assert_eq!(val_set.quorum_threshold(), 67);

        let proposal = SegmentProposal::new(
            2,
            0,
            Hash::new([0x77; 32]),
            proposer,
            1,
        );

        let mut qc = QuorumCertificate::new(proposal);

        // Suara 1 (Proposer): bobot 25 < 67
        let vote_p = Vote::sign(&proposal, proposer, &proposer_key);
        let w1 = qc.add_vote(&vote_p, &val_set).unwrap();
        assert_eq!(w1, 25);
        assert!(!qc.is_final(&val_set));

        // Suara 2 (Val 1): bobot akumulasi 50 < 67
        let vote_1 = Vote::sign(&proposal, val1, &val1_key);
        let w2 = qc.add_vote(&vote_1, &val_set).unwrap();
        assert_eq!(w2, 50);
        assert!(!qc.is_final(&val_set));

        // Suara duplikat dari Val 1 harus ditolak
        let dup_vote = qc.add_vote(&vote_1, &val_set);
        assert_eq!(dup_vote.unwrap_err(), ConsensusError::DuplicateVote);

        // Suara 3 (Val 2): bobot akumulasi 75 >= 67 -> Kuorum Tercapai!
        let vote_2 = Vote::sign(&proposal, val2, &val2_key);
        let w3 = qc.add_vote(&vote_2, &val_set).unwrap();
        assert_eq!(w3, 75);
        assert!(qc.is_final(&val_set));

        // Verifikasi menyeluruh sertifikat kuorum
        assert!(qc.verify(&val_set).is_ok());
    }

    #[test]
    fn test_quorum_certificate_rejection_insufficient_quorum() {
        let (proposer_key, proposer) = create_validator_keypair(0xb1);
        let (val1_key, val1) = create_validator_keypair(0xb2);
        let (_, val2) = create_validator_keypair(0xb3);

        // 3 validator bobot masing-masing 10 (total 30). Ambang kuorum = (30*2)/3 + 1 = 21
        let val_set = ValidatorSet::new(vec![
            ValidatorInfo::new(proposer, 10),
            ValidatorInfo::new(val1, 10),
            ValidatorInfo::new(val2, 10),
        ])
        .unwrap();
        assert_eq!(val_set.quorum_threshold(), 21);

        let proposal = SegmentProposal::new(
            3,
            0,
            Hash::new([0x33; 32]),
            proposer,
            1,
        );

        let mut qc = QuorumCertificate::new(proposal);

        // Hanya 2 validator yang memberikan suara (bobot 20 < 21)
        let vote_p = Vote::sign(&proposal, proposer, &proposer_key);
        let vote_1 = Vote::sign(&proposal, val1, &val1_key);

        qc.add_vote(&vote_p, &val_set).unwrap();
        qc.add_vote(&vote_1, &val_set).unwrap();

        assert_eq!(qc.accumulated_weight, 20);
        assert!(!qc.is_final(&val_set));

        // Verifikasi sertifikat harus gagal karena kuorum tidak cukup
        let verify_result = qc.verify(&val_set);
        assert_eq!(verify_result.unwrap_err(), ConsensusError::InsufficientQuorum);
    }
}
