#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};
use ratu_aurion_engine::error::EngineError;
use ratu_aurion_engine::pipeline::PipelineCoordinator;
use ratu_aurion_index::entry::AccountLocation;
use ratu_aurion_index::keydir::Keydir;
use ratu_aurion_primitives::{
    crypto::{AccountId, Signature},
    record::{
        MutationRecord, BASE_TX_FEE, BASE_TX_FEE_ATOMIC, BPS_DENOMINATOR,
        DEFAULT_BURN_RATIO_BPS, DEFAULT_TIP_RATIO_BPS, RECORD_KIND_BLOCK_REWARD,
        RECORD_KIND_FEE_BURN, RECORD_KIND_TRANSFER,
    },
    value::AurValue,
};
use ratu_aurion_storage::writer::SegmentWriter;

fn unique_test_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ratu_aurion_fee_test_{label}_{nanos}"));
    fs::create_dir_all(&path).expect("Create test dir");
    path
}

fn create_keypair(seed_byte: u8) -> (SigningKey, AccountId) {
    let mut seed = [0u8; 32];
    seed[0] = seed_byte;
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();
    let account = AccountId::new(verifying_key.to_bytes());
    (signing_key, account)
}

fn sign_mutation(
    signing_key: &SigningKey,
    epoch: u64,
    seq: u64,
    sender: AccountId,
    recipient: AccountId,
    amount: AurValue,
) -> MutationRecord {
    let mut record = MutationRecord {
        epoch,
        sequence_number: seq,
        record_kind: RECORD_KIND_TRANSFER,
        sender,
        recipient,
        amount,
        signature: Signature::ZERO,
    };
    let payload = record.signing_payload();
    let dalek_sig = signing_key.sign(&payload);
    record.signature = Signature::new(dalek_sig.to_bytes());
    record
}

#[test]
fn test_fee_deduction_and_burn_ratio() {
    let dir = unique_test_dir("fee_deduction");
    let seg_path = dir.join("epoch_1_seg_0.log");
    let writer = SegmentWriter::create(&seg_path, 1, 1, 0).expect("Create writer");
    let mut keydir = Keydir::new();

    let (alice_key, alice) = create_keypair(0x11);
    let (_, bob) = create_keypair(0x22);

    let initial_balance = AurValue::from_whole_aur(100).expect("100 AUR");
    keydir.seed_account(alice, initial_balance, AccountLocation::new(1, 0, 0, 0));

    let coordinator = PipelineCoordinator::spawn_with_fee(writer, keydir, 2, BASE_TX_FEE)
        .expect("Spawn pipeline with base fee");

    let transfer_amount = AurValue::from_whole_aur(10).expect("10 AUR");
    let tx = sign_mutation(&alice_key, 1, 1, alice, bob, transfer_amount);
    let receipt = coordinator.submit(tx).expect("Submit tx with fee");
    assert_eq!(receipt.sequence_number, 1);

    // Assert pengirim dipotong tepat 10.001 AUR (10 AUR transfer + 0.001 AUR fee)
    let alice_bal = coordinator.query_balance(&alice).expect("Query alice balance");
    let expected_alice = initial_balance
        .checked_sub(transfer_amount)
        .expect("Sub transfer")
        .checked_sub(BASE_TX_FEE)
        .expect("Sub fee");
    assert_eq!(alice_bal, expected_alice);
    assert_eq!(alice_bal.to_atomic(), 899_990_000_000); // 89.999 AUR (899.990.000.000 atomic)

    // Assert penerima menerima tepat 10 AUR
    let bob_bal = coordinator.query_balance(&bob).expect("Query bob balance");
    assert_eq!(bob_bal, transfer_amount);

    // Assert fee dibagi 70% burn dan 30% tip
    let (burned, tip) = coordinator
        .cumulative_fees()
        .expect("Query cumulative fees");
    let expected_burn = BASE_TX_FEE
        .checked_mul_ratio(DEFAULT_BURN_RATIO_BPS, BPS_DENOMINATOR)
        .expect("Burn calc");
    let expected_tip = BASE_TX_FEE
        .checked_mul_ratio(DEFAULT_TIP_RATIO_BPS, BPS_DENOMINATOR)
        .expect("Tip calc");

    assert_eq!(burned, expected_burn);
    assert_eq!(tip, expected_tip);
    assert_eq!(burned.to_atomic(), 7_000_000); // 70% of 0.001 AUR
    assert_eq!(tip.to_atomic(), 3_000_000); // 30% of 0.001 AUR
    assert_eq!(burned.checked_add(tip).expect("Sum fees"), BASE_TX_FEE);

    coordinator.shutdown().expect("Shutdown");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_block_reward_dynamic_inflation_and_supply_update() {
    let dir = unique_test_dir("block_reward");
    let seg_path = dir.join("epoch_1_seg_0.log");
    let writer = SegmentWriter::create(&seg_path, 1, 1, 0).expect("Create writer");
    let mut keydir = Keydir::new();

    let (alice_key, alice) = create_keypair(0x33);
    let (_, bob) = create_keypair(0x44);
    let (_, leader) = create_keypair(0x55);

    // Inisialisasi total_supply = 100_000_000 AUR
    let initial_supply = AurValue::from_whole_aur(100_000_000).expect("100M AUR");
    keydir.seed_account(alice, initial_supply, AccountLocation::new(1, 0, 0, 0));
    assert_eq!(keydir.total_supply(), initial_supply);

    // Hitung block reward; pastikan non-zero integer atomic units
    let calculated_reward = keydir.compute_block_reward();
    assert!(
        calculated_reward > AurValue::ZERO,
        "Block reward must be non-zero atomic units"
    );

    let coordinator = PipelineCoordinator::spawn_with_fee(writer, keydir, 2, BASE_TX_FEE)
        .expect("Spawn pipeline");

    // Lakukan 1 transaksi dengan fee agar ada tip dan burn
    let transfer_amount = AurValue::from_whole_aur(10).expect("10 AUR");
    let tx = sign_mutation(&alice_key, 1, 1, alice, bob, transfer_amount);
    coordinator.submit(tx).expect("Submit tx");

    // Eksekusi slot commit dengan block reward ke leader
    let slot_receipt = coordinator
        .commit_slot(&leader)
        .expect("Commit slot to leader");
    assert_eq!(slot_receipt.block_reward, calculated_reward);
    assert_eq!(slot_receipt.fee_burned.to_atomic(), 7_000_000);
    assert_eq!(slot_receipt.fee_tip.to_atomic(), 3_000_000);

    // Assert leader menerima BlockReward + Tip
    let leader_bal = coordinator.query_balance(&leader).expect("Query leader balance");
    let expected_leader = calculated_reward
        .checked_add(slot_receipt.fee_tip)
        .expect("Leader reward + tip");
    assert_eq!(leader_bal, expected_leader);

    // Assert Keydir.total_supply global bertambah sebesar BlockReward - FeeBurn
    let final_supply = coordinator
        .query_total_supply()
        .expect("Query total supply");
    let expected_supply = initial_supply
        .checked_add(calculated_reward)
        .expect("Add reward")
        .checked_sub(slot_receipt.fee_burned)
        .expect("Sub burn");
    assert_eq!(final_supply, expected_supply);

    // Assert jumlah seluruh saldo akun pada 256 bucket persis sama dengan Keydir.total_supply
    let sum_balances = coordinator
        .query_sum_balances()
        .expect("Query sum balances");
    assert_eq!(sum_balances, final_supply);

    coordinator.shutdown().expect("Shutdown");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_net_deflation_scenario_high_burn() {
    let dir = unique_test_dir("net_deflation");
    let snap_path = dir.join("deflation_checkpoint.snap");
    let mut keydir = Keydir::new();

    let (_, alice) = create_keypair(0x66);
    let (_, bob) = create_keypair(0x77);
    let (_, leader) = create_keypair(0x88);

    // Inisialisasi total_supply awal 100 AUR (skala kecil agar 50,000 tx mendominasi inflasi)
    let initial_supply = AurValue::from_whole_aur(100).expect("100 AUR");
    keydir.seed_account(alice, initial_supply, AccountLocation::new(1, 0, 0, 0));

    let block_reward = keydir.compute_block_reward();

    // Simulasikan 50,000 mutasi transfer dengan fee
    // 50,000 tx * 0.001 AUR = 50 AUR total fee
    // FeeBurn = 50 AUR * 70% = 35 AUR
    // FeeTip = 50 AUR * 30% = 15 AUR
    let tx_count = 50_000u128;
    let single_fee = AurValue::from_atomic(BASE_TX_FEE_ATOMIC);
    let single_burn = single_fee
        .checked_mul_ratio(DEFAULT_BURN_RATIO_BPS, BPS_DENOMINATOR)
        .expect("burn");
    let single_tip = single_fee
        .checked_mul_ratio(DEFAULT_TIP_RATIO_BPS, BPS_DENOMINATOR)
        .expect("tip");

    let aggregate_burned = AurValue::from_atomic(single_burn.to_atomic() * tx_count);
    let aggregate_tips = AurValue::from_atomic(single_tip.to_atomic() * tx_count);

    // Eksekusi simulasi 50,000 transaksi langsung pada Keydir via apply_mutation_with_fee
    for seq in 1..=50_000u64 {
        let tx = MutationRecord {
            epoch: 1,
            sequence_number: seq,
            record_kind: RECORD_KIND_TRANSFER,
            sender: alice,
            recipient: bob,
            amount: AurValue::from_atomic(1_000),
            signature: Signature::new([0xAA; 64]),
        };
        keydir
            .apply_mutation_with_fee(&tx, single_fee, 1, 0, 42)
            .expect("Apply mutation with fee");
    }

    // Assert aggregate FeeBurn > BlockReward (skenario deflasi bersih)
    assert!(
        aggregate_burned > block_reward,
        "Aggregate fee burn ({}) must exceed block reward ({})",
        aggregate_burned.to_atomic(),
        block_reward.to_atomic()
    );

    // Finalisasi reward ke leader
    keydir
        .finalize_block_rewards(
            &leader,
            block_reward,
            aggregate_tips,
            aggregate_burned,
            AccountLocation::new(1, 0, 0, 3),
        )
        .expect("Finalize rewards");

    // Assert total_supply global berkurang bersih tanpa integer underflow
    assert!(
        keydir.total_supply() < initial_supply,
        "Total supply must decrease under net deflation"
    );
    assert_eq!(keydir.total_supply(), keydir.sum_bucket_balances());

    // Assert snapshot creation dan reload mempertahankan penurunan total supply
    keydir
        .save_checkpoint(&snap_path, 1, 0)
        .expect("Save checkpoint");
    let (reloaded_keydir, header) =
        Keydir::load_checkpoint(&snap_path).expect("Load checkpoint");

    assert_eq!(reloaded_keydir.total_supply(), keydir.total_supply());
    assert_eq!(
        AurValue::from_atomic(header.total_supply),
        keydir.total_supply()
    );
    assert_eq!(
        reloaded_keydir.sum_bucket_balances(),
        keydir.total_supply()
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_fail_fast_insufficient_balance_for_transfer_plus_fee() {
    let dir = unique_test_dir("fail_fast_fee");
    let seg_path = dir.join("epoch_1_seg_0.log");
    let writer = SegmentWriter::create(&seg_path, 1, 1, 0).expect("Create writer");
    let mut keydir = Keydir::new();

    let (alice_key, alice) = create_keypair(0x99);
    let (_, bob) = create_keypair(0xAA);

    // Alice memiliki tepat 10 AUR
    let exact_10_aur = AurValue::from_whole_aur(10).expect("10 AUR");
    keydir.seed_account(alice, exact_10_aur, AccountLocation::new(1, 0, 0, 0));

    let coordinator = PipelineCoordinator::spawn_with_fee(writer, keydir, 2, BASE_TX_FEE)
        .expect("Spawn pipeline");

    // Alice mencoba mentransfer seluruh 10 AUR (tanpa menyisakan saldo untuk 0.001 AUR fee)
    let bad_tx = sign_mutation(&alice_key, 1, 1, alice, bob, exact_10_aur);
    let result = coordinator.submit(bad_tx);

    // Assert ditolak fail-fast dengan InsufficientBalance pada verifikator Stage 1
    match result {
        Err(EngineError::InsufficientBalance { requested, available }) => {
            let expected_req = exact_10_aur.checked_add(BASE_TX_FEE).expect("Add fee");
            assert_eq!(requested, expected_req);
            assert_eq!(available, exact_10_aur);
        }
        other => panic!("Harus ditolak dengan InsufficientBalance, didapat: {:?}", other),
    }

    // Pastikan transaksi berikutnya yang sah (misal 5 AUR) berhasil dikomit pada disk offset 42
    let valid_amount = AurValue::from_whole_aur(5).expect("5 AUR");
    let good_tx = sign_mutation(&alice_key, 1, 1, alice, bob, valid_amount);
    let receipt = coordinator.submit(good_tx).expect("Submit valid tx");
    assert_eq!(receipt.sequence_number, 1);
    assert_eq!(receipt.disk_offset, 42);

    coordinator.shutdown().expect("Shutdown");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_replay_segment_with_synthetic_burn_and_reward_records() {
    let dir = unique_test_dir("replay_synthetic");
    let seg_path = dir.join("epoch_1_seg_0.log");
    let mut writer = SegmentWriter::create(&seg_path, 1, 1, 0).expect("Create writer");
    let mut source_keydir = Keydir::new();

    let (alice_key, alice) = create_keypair(0xBB);
    let (_, bob) = create_keypair(0xCC);
    let (_, leader) = create_keypair(0xDD);

    let initial_balance = AurValue::from_whole_aur(100).expect("100 AUR");
    source_keydir.seed_account(alice, initial_balance, AccountLocation::new(1, 0, 0, 0));

    // Tulis segmen berisi 10 user transfers, 10 synthetic fee burns, dan 1 block reward record
    let transfer_amount = AurValue::from_whole_aur(1).expect("1 AUR");
    let fee_burn = BASE_TX_FEE
        .checked_mul_ratio(DEFAULT_BURN_RATIO_BPS, BPS_DENOMINATOR)
        .expect("burn");
    let fee_tip = BASE_TX_FEE
        .checked_mul_ratio(DEFAULT_TIP_RATIO_BPS, BPS_DENOMINATOR)
        .expect("tip");

    let mut current_seq = 0u64;

    for _i in 1..=10 {
        current_seq += 1;
        // 1. User transfer record
        let tx = sign_mutation(&alice_key, 1, current_seq, alice, bob, transfer_amount);
        let off1 = writer.append_record(&tx).expect("Append transfer");
        source_keydir
            .apply_mutation(&tx, 1, 0, off1)
            .expect("Apply transfer to source");

        current_seq += 1;
        // 2. Synthetic fee burn record (sender dipotong fee burn)
        let burn_rec = MutationRecord {
            epoch: 1,
            sequence_number: current_seq,
            record_kind: RECORD_KIND_FEE_BURN,
            sender: alice,
            recipient: AccountId::ZERO,
            amount: fee_burn,
            signature: Signature::new([0x02; 64]),
        };
        let off2 = writer.append_record(&burn_rec).expect("Append burn");
        source_keydir
            .apply_mutation(&burn_rec, 1, 0, off2)
            .expect("Apply burn to source");
    }

    current_seq += 1;
    // 3. Block reward record (1 reward record untuk leader)
    let block_reward = source_keydir.compute_block_reward();
    let total_tips = AurValue::from_atomic(fee_tip.to_atomic() * 10);
    let leader_total_reward = block_reward
        .checked_add(total_tips)
        .expect("Add tips to reward");

    let reward_rec = MutationRecord {
        epoch: 1,
        sequence_number: current_seq,
        record_kind: RECORD_KIND_BLOCK_REWARD,
        sender: AccountId::ZERO,
        recipient: leader,
        amount: leader_total_reward,
        signature: Signature::new([0x03; 64]),
    };
    let off3 = writer.append_record(&reward_rec).expect("Append reward");
    source_keydir
        .apply_mutation(&reward_rec, 1, 0, off3)
        .expect("Apply reward to source");

    writer.flush_and_sync().expect("Flush writer");
    writer
        .seal_segment(ratu_aurion_primitives::crypto::Hash::new([0x55; 32]), 123456)
        .expect("Seal writer");

    // Replay segmen ke dalam Keydir baru yang bersih
    let mut fresh_keydir = Keydir::new();
    fresh_keydir.seed_account(alice, initial_balance, AccountLocation::new(1, 0, 0, 0));

    let replayed_count = fresh_keydir
        .replay_segment(&seg_path)
        .expect("Replay segment into fresh keydir");
    assert_eq!(replayed_count, 21); // 10 transfers + 10 burns + 1 reward

    // Assert state hash, saldo leader, saldo alice, saldo bob, dan total_supply cocok bit-for-bit
    assert_eq!(fresh_keydir.state_digest(), source_keydir.state_digest());
    assert_eq!(
        fresh_keydir.get_balance(&leader),
        source_keydir.get_balance(&leader)
    );
    assert_eq!(
        fresh_keydir.get_balance(&alice),
        source_keydir.get_balance(&alice)
    );
    assert_eq!(
        fresh_keydir.get_balance(&bob),
        source_keydir.get_balance(&bob)
    );
    assert_eq!(fresh_keydir.total_supply(), source_keydir.total_supply());
    assert_eq!(
        fresh_keydir.total_supply(),
        fresh_keydir.sum_bucket_balances()
    );

    let _ = fs::remove_dir_all(&dir);
}
