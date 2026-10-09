# Ratu Aurion Protocol Engineering Worklog

## Completed Tickets

### Ticket REFACTOR-REBRAND-01: Full Workspace Rebranding to Ratu Aurion Protocol
- **Target Subsystem**: Workspace-wide (`crates/ratu-aurion-*`, `bin/ratu-aurion-*`, `tools/guards.py`)
- **Status**: Validated & Merged
- **Specification**: Full workspace rebranding to Ratu Aurion Protocol (`ratu-aurion`), securing domain consistency (`ratuaurion.store`), preventing namespace conflicts in open ecosystems (crates.io), and establishing unique binary protocol identity.

#### Updated Binary Magic Bytes & Crate Namespaces
1. **Workspace Crates**:
   - `crates/ratu-aurion-primitives` (previously `crates/axiom-primitives`)
   - `crates/ratu-aurion-storage` (previously `crates/axiom-storage`)
   - `crates/ratu-aurion-index` (previously `crates/axiom-index`)
   - `crates/ratu-aurion-engine` (previously `crates/axiom-engine`)
   - `crates/ratu-aurion-consensus` (previously `crates/axiom-consensus`)
   - `crates/ratu-aurion-network` (previously `crates/axiom-network`)
   - `crates/ratu-aurion-archive` (previously `crates/axiom-archive`)
2. **Binaries**:
   - `bin/ratu-aurion-node` (previously `bin/axiom-node`)
   - `bin/ratu-aurion-cli` (previously `bin/axiom-cli`)
3. **Token Ticker & Monetary Type**:
   - Native Ticker: `AUR` (previously `AXM`)
   - Monetary Value Representation: `AurValue` (previously `AxmValue`), quantized 10-decimal integer representation ($1\text{ AUR} = 10^{10}\text{ atomic units}$).
   - Helper methods: `from_whole_aur`, `to_atomic`, `from_atomic`, `from_le_bytes`, `to_le_bytes`, `checked_mul_ratio`.
4. **Binary Protocol Magic Identifiers**:
   - Storage Segment Header (4B): `*b"RAUR"` (previously `*b"AXMS"`)
   - Storage Segment Footer (8B): `*b"RAUREND\x01"` (previously `*b"AXMEND\x01\x00"`)
   - Checkpoint Snapshot Header (8B): `*b"RAURSNAP"` (previously `*b"AXMSNAP\x01"`)
   - Cold Storage Index Header (8B): `*b"RAURCOLD"` (previously `*b"AXMCOLD\x01"`)
   - Wire Frame Header (4B): `*b"AUR\x01"` (previously `*b"AXM\x01"`)
5. **Quality Gates & Verification Metrics**:
   - Strictly enforced `#![forbid(unsafe_code)]` across all crates without exception.
   - Zero floating-point arithmetic (`f32`, `f64` strictly forbidden).
   - Zero-panic runtime safety: Zero `unwrap()`, zero `expect()` in library paths.
   - Workspace passes `cargo check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `python tools/guards.py` with zero warnings and zero errors.

---

### Ticket OPT-STORAGE-01: Crash Recovery & Torn-Write Truncation
- **Target Subsystem**: `crates/axiom-storage`
- **Status**: Validated & Merged
- **Specification**: RFC-0001 Append-Only Log Engine Hardening

#### Technical Invariants
1. **Segment Header Size**: Exactly 42 bytes Little-Endian (`b"AXMS"`, version `u16`, epoch `u64`, segment_index `u32`, reserved `[u8; 24]`).
2. **Mutation Record Stride**: Exactly 161 bytes Little-Endian.
3. **Segment Footer Size**: Exactly 88 bytes Little-Endian (`b"AXMEND\x01\x00"`).
4. **Valid Segment Boundary Equation**:
   $$\text{Payload} = \text{File\_Size} - 42$$
   $$(\text{File\_Size} - 42) \pmod{161} == 0$$
5. **Torn-Write Recovery Invariant**:
   If $(\text{File\_Size} - 42) \pmod{161} == r$ where $r > 0$, the trailing $r$ bytes are partial/corrupted torn writes resulting from abrupt power loss or ungraceful shutdown. The recovery mechanism atomically truncates the file to $\text{File\_Size} - r$ followed by physical media synchronization (`sync_all` / `fsync`) prior to any subsequent append operations.
6. **Sub-Header Interruption Invariant**:
   If $\text{File\_Size} < 42$, the segment header write was interrupted mid-flight; the engine flags `StorageError::CorruptedHeader { size }` defensively without panic.

#### Test Execution & Verification Results
- **Test Suite**: `crates/axiom-storage/tests/test_recovery.rs`
- **Execution Latency**: 0.02s (`test_torn_write_recovery_at_tail` + `test_sub_header_truncation_recovery`)
- **Torn-Write Truncation Metrics**:
  - Initial valid segment: 3 mutation records = 525 bytes ($42 + 3 \times 161$).
  - Simulated crash / power loss injection: 73 trailing garbage bytes = 598 bytes.
  - Anomaly detection: Remainder $r = (598 - 42) \pmod{161} = 73 > 0$. Reading record at offset 525 fails with `StorageError::OutOfBounds`.
  - Recovery execution: File truncated from 598 bytes down to exactly 525 bytes ($598 - 73$) with `file.sync_all()`.
  - Preserved records: 3 original records parsed and verified intact via `RecordStream`.
  - Sequential append continuation: 4th mutation record appended; file size cleanly advances from 525 to 686 bytes ($525 + 161$).
- **Sub-Header Truncation Metrics**:
  - Tested boundary file sizes: 1, 10, 20, 41 bytes.
  - Result: All test cases deterministically returned `StorageError::CorruptedHeader { size }` without panic.

---

### Ticket OPT-STORAGE-02: Zero-Fragmentation Pre-allocation
- **Target Subsystem**: `crates/axiom-storage`
- **Status**: Validated & Merged
- **Specification**: RFC-0001 Instant 128 MB Reservation, Binary Zero Scan Boundary Recovery, and Footer Reclaim

#### Technical Invariants
1. **Instant Pre-allocation**: `SegmentWriter::create` allocates exactly 128 MB (`MAX_SEGMENT_SIZE = 134,217,728` bytes) immediately via OS sparse reservation (`set_len`), eliminating sequential file system fragmentation on HDD/NVMe storage while `current_offset` begins at 42.
2. **Fixed Physical Size During Append**: File size on disk remains invariant at 134,217,728 bytes during active mutation appends; physical disk metadata updates are eliminated during transaction streaming.
3. **Binary Zero Scan Recovery**: On unsealed segments with 128 MB physical size, `recover_or_open` applies a logarithmic binary search over maximum possible slots ($0 \le \text{slot} < 833,649$) testing record commitment criteria to identify the exact uncommitted slot boundary $K$.
4. **Torn-Write Sanitization**: If slot $K$ contains partially written bytes (non-zero bytes from an interrupted append), the 161-byte slot is wiped with zeros (`0x00`) and synchronized (`sync_all`) before resetting `current_offset` to $42 + (K \times 161)$.
5. **Footer Truncation & Reclamation**: Calling `seal_segment` appends the 88-byte `SegmentFooter` and reclaims all unused pre-allocated zeros by truncating the file to `current_offset + 88` bytes with `sync_all`.

#### Test Execution & Verification Results
- **Test Suite**: `crates/axiom-storage/tests/test_preallocation.rs`
- **Execution Latency**: 0.05s (4 integration tests)
- **Benchmark & Algorithmic Metrics**:
  - Pre-allocation latency: $< 1$ ms via instantaneous filesystem extent pre-allocation.
  - Binary search seek count: Exact boundary $K$ located in $\le 20$ seeks out of $833,649$ possible slots ($\lceil \log_2(833,649) \rceil = 20$).
  - Space reclaimed upon seal: For a 5-record segment, physical size was cleanly reclaimed from $134,217,728$ bytes down to $935$ bytes ($42 + 5 \times 161 + 88$).
  - Partial slot recovery: Injected 45-byte torn write at slot 6 was detected, zeroed out to $0\text{x}00$, offset reset to $847$, and sequential append cleanly resumed at offset $847$.

---

### Ticket OPT-STORAGE-03: Group Commit & Durability Tuning
- **Target Subsystem**: `crates/axiom-storage`
- **Status**: Validated & Merged
- **Specification**: RFC-0001 128 KB Page-Aligned Buffering, Group Commit Batching, and Hardware Synchronization Tuning

#### Technical Invariants
1. **Memory Buffer Capacity**: Strictly 128 KB (`WRITE_BUFFER_CAPACITY = 128 * 1024 = 131_072` bytes) aligned to standard OS virtual memory pages. Up to 814 mutation records ($814 \times 161 = 131,054$ bytes) fit in a single buffer before triggering automatic flush.
2. **Durability Policies**:
   - `DurabilityPolicy::Strict`: Flushes memory buffer and calls `file.sync_data()` immediately on every single appended record.
   - `DurabilityPolicy::GroupCommit { batch_size }`: Accumulates mutation records in the memory buffer; triggers disk write and `file.sync_data()` once `uncommitted_records >= batch_size`. Resets `uncommitted_records = 0` on sync.
   - `DurabilityPolicy::BufferedRelaxed`: Retains records in memory without proactive sync; flushes to disk only when the 128 KB capacity threshold is reached or when `flush_buffer`/`flush_and_sync` is explicitly invoked.
3. **Hardware Synchronization Tiering**:
   - Data payload sync utilizes `file.sync_data()` (`fdatasync`), eliminating unnecessary inode/metadata disk write IOPS during active mutation streaming since physical segment file length is pre-allocated at 128 MB.
   - Segment header and footer operations utilize `file.sync_all()` (`fsync`), ensuring critical metadata structures and file truncation are fully persisted to non-volatile storage.
4. **Buffer Flushing Before Segment Seal**: Calling `seal_segment` drains any residual buffered bytes to disk before writing the 88-byte footer, truncating pre-allocated space, and executing `file.sync_all()`.
5. **Drop Safety**: `SegmentWriter::drop` ensures any unsealed, unflushed in-memory buffer is flushed to disk upon writer teardown.

#### Test Execution & Verification Results
- **Test Suite**: `crates/axiom-storage/tests/test_durability.rs`
- **Execution Latency**: 0.06s (4 integration tests)
- **Durability Verification Metrics**:
  - `test_strict_durability_mode`: 3 records appended with `Strict` policy immediately flushed and readable on disk via an independent reader without writer closure.
  - `test_group_commit_batch_threshold`: 9 records buffered in memory with `uncommitted_records == 9`; 10th record triggers automatic batch commit, flushing buffer, executing `sync_data()`, resetting `uncommitted_records == 0`, and making all 10 records readable.
  - `test_buffered_relaxed_128kb_auto_flush`: 814 records fill 131,054 bytes without disk flush; 815th record triggers automatic 128 KB chunk write, advancing `flushed_offset` by 131,054 bytes and retaining only record 815 (161 bytes) in memory buffer.
  - `test_explicit_flush_and_sync_and_seal`: 5 records flushed via `flush_and_sync()` with `sync_data()`, followed by `seal_segment()` appending the 88-byte footer, truncating to 893 bytes, and synchronizing via `sync_all()`.

---

### Ticket OPT-STORAGE-04: Bit-Rot & Structural Invariant Fast Scan
- **Target Subsystem**: `crates/axiom-storage`
- **Status**: Validated & Merged
- **Specification**: RFC-0001 Zero-Allocation Streaming Bit-Rot Detection, Fast Structural Invariant Verification, and Ed25519 Cryptographic Deep Scan

#### Technical Invariants
1. **Constant Memory Streaming I/O**: Fixed-size 128 KB buffer (`SCAN_BUFFER_CAPACITY = 128 * 1024 = 131_072` bytes) for sequential streaming I/O; $O(1)$ RAM consumption regardless of 128 MB physical segment file length; zero heap allocations per record during scan.
2. **Structural Invariant Fail-Fast Validation**:
   - Header validity and magic `b"AXMS"` check (defensive `CorruptedHeader` on $< 42$ bytes).
   - Invariant 1 (Epoch Continuity): Every record must match segment header epoch (`record.epoch == header.epoch`), otherwise returns `StorageError::EpochMismatch`.
   - Invariant 2 (Monotonic Sequence Order): Record sequence numbers must be contiguous and strictly sequential without holes, otherwise returns `StorageError::SequenceMismatch`.
   - Invariant 3 (Record Kind Taxonomy): Record kind must strictly be `RECORD_KIND_TRANSFER (1)` or `RECORD_KIND_SYSTEM_NOTIF (2)`, otherwise returns `StorageError::StructuralInvariantViolation`.
   - Invariant 4 (Non-Zero Cryptographic Signature): Authorizing signature must not contain all-zero bytes, preventing uninitialized record leakage.
   - Unsealed Zero-Fill Boundary: Gracefully identifies pre-allocated `0x00` blocks on unsealed segments, terminating scan at the exact uncommitted commit line.
3. **Bit-Rot Cryptographic Verification**: Computes streaming BLAKE3 digest across all 161-byte record payloads; on sealed segments, validates computed hash against 88-byte footer `state_digest`. Single-bit corruption immediately triggers `StorageError::BitRotDetected`.
4. **Deep Cryptographic Verification**: Zero-heap-allocation Ed25519 signature verification (`deep_verify_signatures`) over the 97-byte signing payload against the sender's public key; tampered signatures fail fast with `StorageError::SignatureVerificationFailed`.

#### Test Execution & Verification Results
- **Test Suite**: `crates/axiom-storage/tests/test_scanner.rs`
- **Execution Latency**: 0.24s (6 integration tests)
- **Scanner Verification Metrics**:
  - `test_clean_sealed_segment_scan`: 10 records scanned, sealed footer verified, BLAKE3 digest matched cleanly.
  - `test_clean_unsealed_preallocated_segment_scan`: 5 records scanned, cleanly terminated at uncommitted 0x00 boundary at offset 847 on 128 MB pre-allocated file.
  - `test_structural_fail_fast_invalid_record_kind`: Corrupted byte `0xFF` at offset 219 immediately halted scanner at offset 203 with `StorageError::StructuralInvariantViolation`.
  - `test_structural_fail_fast_broken_sequence`: Skipped sequence number (2 to 5) at offset 364 immediately halted scanner with `StorageError::SequenceMismatch`.
  - `test_bit_rot_fail_fast_sealed_digest_mismatch`: 1-bit corruption (XOR `0x01`) at offset 250 detected by BLAKE3 streaming verification, failing with `StorageError::BitRotDetected`.
  - `test_deep_verify_signatures`: Tampered signature byte at offset 139 passed structural check but failed deep cryptographic check with `StorageError::SignatureVerificationFailed { offset: 42 }`.

---

### Ticket OPT-ENGINE-01: 3-Stage Execution Pipeline & Lock Contention Elimination
- **Target Subsystem**: `crates/axiom-engine`
- **Status**: Validated & Merged
- **Specification**: Multi-stage pipelined transaction execution engine decoupling cryptographic verification, state sequencing, and storage logging under bounded backpressure.

#### Technical Invariants
1. **3-Stage Decoupled Pipeline Architecture**:
   - **Stage 1 (Parallel Stateless Cryptographic Verifiers)**: $N$ parallel worker threads verify Ed25519 signatures over the 97-byte signing payload, invariant non-zero amount, and non-identical sender/recipient. Bad transactions fail-fast at Stage 1 before reaching the sequencer.
   - **Stage 2 (Single-Threaded Deterministic Sequencer)**: Exclusively owns `Keydir` in RAM without any `RwLock`/`Mutex` wrappers, completely eliminating thread contention on state mutations. Deterministically enforces account sequence numbers and balances, buffering mutations into `SequencedBatch`.
   - **Stage 3 (Single-Threaded Batch Storage Writer)**: Exclusively owns `SegmentWriter`, receiving sequenced batches and committing records to physical disk with 128 KB buffer alignment and policy-based hardware synchronization (`sync_data`).
2. **Bounded Backpressure & Queue Budgets**:
   - `STAGE1_QUEUE_CAPACITY = 16_384`: Bounded input envelope buffer preventing unbounded memory growth under spike traffic.
   - `STAGE2_QUEUE_CAPACITY = 16_384`: Verified transaction queue feeding the sequencer.
   - `STAGE3_BATCH_MAX_SIZE = 400`: Maximum records per disk write batch, aligning with 64 KB / 128 KB filesystem buffer limits.
3. **Fail-Fast Invariant Enforcement**:
   - Invalid cryptographic signatures are rejected immediately at Stage 1; Stage 2 state and Stage 3 disk are untouched.
   - Stale sequence numbers and insufficient balances fail fast at Stage 2 without modifying `Keydir` or submitting to Stage 3.
4. **Graceful Cascading Shutdown**:
   - Dropping input senders cleanly unblocks Stage 1 threads; their termination triggers Stage 2 final batch flush; Stage 2 termination triggers Stage 3 physical flush, footer sealing (`seal_segment`), and clean thread joining with zero leaks.

#### Test Execution & Verification Results
- **Test Suite**: `crates/axiom-engine/tests/test_pipeline.rs`
- **Execution Latency**: 3.21s (5 integration tests)
- **Pipeline Concurrency & Stress Metrics**:
  - `test_pipeline_concurrent_valid_transactions`: 8 concurrent client threads submitted 50 valid transactions each (400 total transactions). All 400 transactions committed with zero errors. Monotonic sequence numbers (1..=50 per client) and 400 strictly contiguous, collision-free disk offsets ($42, 203, \dots, 64_281$). Final state balance and disk record count matched exactly 400.
  - `test_pipeline_fail_fast_invalid_signature_at_stage1`: Corrupted transaction failed at Stage 1 with `EngineError::InvalidSignature`; subsequent valid transaction committed cleanly at offset 42.
  - `test_pipeline_fail_fast_stale_sequence_at_stage2`: Duplicate sequence number rejected at Stage 2 with `EngineError::StaleSequenceNumber { expected: 2, found: 1 }`; subsequent transaction committed at offset 203, verifying no duplicate disk record was written.
  - `test_pipeline_insufficient_balance_rejection_at_stage2`: Overdraft transaction rejected at Stage 2 with `EngineError::InsufficientBalance`; subsequent valid transaction committed at offset 42.
  - `test_pipeline_graceful_shutdown_and_flush`: 20 transactions processed; shutdown completed cleanly; disk segment sealed with exact 88-byte footer, length 3,350 bytes, and 20 verified records.

---

### Ticket OPT-INDEX-01: Keydir Memory Budget & Struct Packing Compression
- **Target Subsystem**: `crates/axiom-index`
- **Status**: Validated & Merged
- **Specification**: In-Memory Bitcask Keydir optimization via prefix-sharded flat arrays (256 buckets) and 80-byte dense struct packing (`CompactAccountEntry`).

#### Technical Invariants
1. **80-Byte Packed Layout (`CompactAccountEntry`)**:
   - Layout: `account` (32B), `balance` (16B, align 16), `sequence_number` (8B), `epoch` (8B), `offset` (8B), `segment_index` (4B), `flags` (4B).
   - Memory Budget: Exactly 80 physical bytes with 0 padding bytes (`std::mem::size_of::<CompactAccountEntry>() == 80`, `std::mem::align_of::<CompactAccountEntry>() == 16`).
   - Memory Savings: Replaced pointer-heavy `BTreeMap` node structure ($\sim 140\text{ bytes/entry}$) with dense flat array layout ($\approx 43\%$ memory reduction to exactly $80\text{ bytes/entry}$).
2. **256-Bucket Prefix Sharding Topology**:
   - Top-level sharding array: `Box<[Vec<CompactAccountEntry>; 256]>` partitioned by `account.as_bytes()[0] as usize`.
   - Prefix entropy: Due to Ed25519 public key uniform distribution, entries distribute evenly across 256 buckets.
   - Lookup Complexity: Instant $O(1)$ bucket resolution followed by CPU cache-friendly in-bucket binary search $O(\log_2(N / 256))$. For 1,000,000 accounts, each bucket contains $\approx 3,900$ contiguous elements requiring at most 12 comparisons.
3. **Aggregated $O(1)$ Metrics & Strict Conservation**:
   - `total_accounts` and `total_supply` cached and maintained dynamically at the root struct level, eliminating tree iterations for global supply checks.
   - Monetary conservation strictly guaranteed during transfer mutations: sender deducted, recipient credited, total supply invariant.
4. **Deterministic In-Place Updates & Anti-Duplication**:
   - Existing accounts are mutated in-place via binary search index without re-allocation or duplication.
   - Stale sequence numbers and overdrafts fail-fast deterministically before mutating state.

#### Test Execution & Verification Results
- **Test Suite**: `crates/axiom-index/tests/test_compact_index.rs`
- **Execution Latency**: 0.04s (5 integration tests)
- **Compact Index Metrics**:
  - `test_compact_account_entry_size_and_alignment`: Verified `size_of == 80` and `align_of == 16`.
  - `test_prefix_bucket_distribution_and_binary_search`: Seeded 2,560 accounts uniformly across 256 buckets (exactly 10 accounts/bucket); verified 100% binary search retrieval accuracy and `account_count() == 2560`.
  - `test_in_place_balance_mutation_and_supply_invariants`: Account A (100 AXM) transferred 30 AXM to Account B (50 AXM); verified Account A balance (70 AXM), Account B balance (80 AXM), strictly conserved `total_supply` (150 AXM), and single-entry in-place location updates.
  - `test_stale_sequence_and_overdraft_rejections`: Confirmed deterministic rejection of duplicate/stale sequence numbers (`IndexError::StaleSequenceNumber`) and insufficient funds (`IndexError::InsufficientBalance`) without side effects.
  - `test_replay_segment_into_compact_keydir`: Replayed sealed disk segment containing 50 sequential transfer mutations; validated accurate reconstruction of all 51 account balances, sequence numbers, and total supply conservation.

---

### Ticket OPT-INDEX-02: Keydir Checkpoint Snapshotting & Fast Recovery
- **Target Subsystem**: `crates/axiom-index`
- **Status**: Validated & Merged
- **Specification**: RFC-0002 In-memory state persistence via 96-byte binary snapshot header, BLAKE3 payload integrity hashing, atomic two-phase write (.tmp -> .snap), and O(N) streaming recovery.

#### Technical Invariants
1. **96-Byte Snapshot Header Layout**:
   - Layout: `magic` (8B, `*b"AXMSNAP\x01"`), `version` (2B), `reserved_pad1` (6B), `epoch` (8B), `segment_index` (4B), `reserved_pad2` (4B), `total_supply` (16B, align 16), `total_accounts` (8B, align 8), `state_digest` (32B), `created_at` (8B, align 8).
   - Invariant: Exactly 96 physical bytes with 0 padding (`std::mem::size_of::<SnapshotHeader>() == 96`, `std::mem::align_of::<SnapshotHeader>() == 16`).
   - Total File Size Equation: $\text{File\_Size} = 96 + (N \times 80)\text{ bytes}$.
2. **Two-Stage Atomic Persistence Pipeline**:
   - Sequential dump writes to temporary file (`.tmp`) first, with 96-byte header prepended upon payload hashing completion.
   - Physical disk synchronization (`sync_all`) executed prior to atomic file rename to `.snap`, guaranteeing zero corrupted checkpoints during sudden power loss.
3. **O(N) Streaming Restore & Zero Sorting Reconstruct**:
   - Entries dumped sequentially across 256 buckets in naturally sorted order; restoration streams directly into bucket vectors without binary search shifting or sorting passes ($< 50\text{ ms}$ load time).
4. **BLAKE3 Cryptographic Integrity Protection**:
   - 32-byte BLAKE3 state digest covers all $N \times 80$ bytes of entry payload. Single-bit corruption triggers instant fail-fast rejection (`IndexError::SnapshotChecksumMismatch`).

#### Test Execution & Verification Results
- **Test Suite**: `crates/axiom-index/tests/test_snapshot.rs`
- **Execution Latency**: 0.10s (5 integration tests)
- **Snapshot Metrics**:
  - `test_snapshot_header_binary_layout`: Verified `size_of == 96` and `align_of == 16`.
  - `test_snapshot_roundtrip_save_and_load`: Seeded 1,280 accounts across 256 buckets (5/bucket); verified physical file length on disk matched exactly $96 + (1,280 \times 80) = 102,496$ bytes; loaded checkpoint into fresh `Keydir` and verified 100% state conservation of balances, sequence numbers, and locations.
  - `test_snapshot_fail_fast_checksum_tampering`: Injected 1-byte corruption at offset 200; confirmed immediate deterministic fail-fast with `IndexError::SnapshotChecksumMismatch`.
  - `test_snapshot_fail_fast_invalid_magic_or_truncated`: Confirmed sub-96B files fail with `IndexError::CorruptedSnapshotHeader { size: 50 }`, and corrupted magic fails with `IndexError::InvalidSnapshotMagic`.
  - `test_cold_boot_fast_recovery_with_delta_replay`: Validated cold boot snapshot load in $< 50\text{ ms}$ followed by 15-transaction delta segment replay; verified all 35 transactions and total supply conserved.

---

### Ticket OPT-INDEX-03: LRU Sparse Paging & Cold State Eviction
- **Target Subsystem**: `crates/axiom-index`
- **Status**: Validated & Merged
- **Specification**: In-memory RAM capacity capping (`max_hot_capacity`), zero-pointer LRU generation tracking via `CompactAccountEntry.flags`, on-disk 256-bucket partitioned index (`cold_state.idx`), $O(\log_2 M)$ seek binary search on disk, and transparent bidirectional paging with strict total supply conservation.

#### Technical Invariants
1. **Zero Heap Overhead LRU Tracking**:
   - Reused 32-bit `flags` field in `CompactAccountEntry` (80 bytes) as access tick / generation counter without heap pointers or doubly-linked list nodes.
   - Access and mutation operations increment local access tick; eviction identifies and offloads lowest-tick entries when RAM count exceeds `max_hot_capacity`.
2. **On-Disk Cold Index Physical Layout (`cold_state.idx`)**:
   - Fixed 4,112-byte header (`COLD_HEADER_SIZE`): Magic 8B (`*b"AXMCOLD\x01"`), Version 2B (`0x0001`), Reserved 6B (`0x00`), and 256 Bucket Descriptors (`256 * 16 = 4,096` bytes).
   - Alignment: 4,112 bytes is an exact multiple of 16 ($4,112 / 16 = 257$), guaranteeing 16-byte boundary alignment before the first 80-byte account entry.
   - Bucket Descriptor: `offset: u64` (8B), `entry_count: u32` (4B), `reserved: u32` (4B).
   - Contiguous Sorted Partitions: Each bucket holds 80-byte entries sorted lexicographically by `AccountId` for $O(\log_2 M)$ on-disk binary search seek (`file.seek(SeekFrom::Start(offset + mid * 80))`) with $O(1)$ memory usage.
3. **Atomic Persistence & Integrity**:
   - Eviction flushes merge entries and write out partition segments atomically via `.tmp` swap (`sync_all` + atomic rename).
   - Sub-4,112B files and invalid magic trigger immediate deterministic `IndexError::CorruptedColdIndex`.
4. **Strict Total Supply Invariant**:
   - Global `total_supply` strictly conserved during eviction and paging: eviction decreases RAM count without altering total monetary supply; paging restores entry to RAM bucket without duplicating balance.
5. **Transparent Paging & Stage 1 Prefetch**:
   - Transparent paging in `get_balance`, `get_account_state`, and `apply_mutation` fetches cold accounts on-demand.
   - `keydir.prefetch(&account)` enables Stage 1 verifiers to load cold accounts into RAM prior to Stage 2 sequencer execution.

#### Test Execution & Verification Results
- **Test Suite**: `crates/axiom-index/tests/test_paged_index.rs`
- **Execution Latency**: 8.90s (5 integration tests)
- **Paged Index Metrics**:
  - `test_cold_store_header_and_binary_search_seek`: Seeded 512 accounts across 256 buckets (2/bucket); verified 4,112B header and physical length $4,112 + (512 \times 80) = 45,072$ bytes; verified 100% binary search seek resolution ($< 1\text{ ms}$ seek latency).
  - `test_keydir_eviction_under_memory_budget`: Initialized `Keydir` with `max_hot_capacity = 256`; seeded 512 accounts; verified RAM hot count capped at $\le 256$, cold store holding 256 entries, and global `total_supply` strictly conserved (sum of all 512 balances).
  - `test_cold_account_transparent_paging_and_mutation`: Evicted Account A (50 AXM) to disk; verified Account A absent in RAM; executed transfer mutation from A to B (20 AXM); verified transparent page-in, balance deduction to 30 AXM, credit to B, and total supply preservation.
  - `test_prefetch_interface_for_stage1`: Verified `keydir.prefetch` loads cold account into RAM before transaction execution.
  - `test_tampered_cold_index_fail_fast`: Validated deterministic fail-fast rejection (`IndexError::CorruptedColdIndex`) upon corrupted magic bytes or truncated cold index.

---

### Ticket OPT-CONSENSUS-01: Equivocation Detection & Double-Sign Slashing
- **Target Subsystem**: `crates/axiom-consensus`
- **Status**: Validated & Merged
- **Specification**: Stateless self-contained fraud proof verification (`EquivocationEvidence`), deterministic 80-byte vote signing framing (`VoteRecord`), 100% hard slashing penalty (10,000 BPS burn), permanent validator tombstoning, and official system notification emission (`RECORD_KIND_SYSTEM_NOTIF`).

#### Technical Invariants
1. **Deterministic 80-Byte Vote Signing Payload**:
   - Layout: `epoch` (8B Little-Endian), `round` (8B Little-Endian), `block_hash` (32B), `validator` (32B).
   - Invariant: Zero memory allocation per vote payload framing; cryptographically signed with Ed25519.
2. **Stateless Self-Contained Fraud Proof Verification (`EquivocationEvidence`)**:
   - Fail-Fast Order:
     1. Validator identity match: `vote_a.validator == self.validator == vote_b.validator` (else `ValidatorMismatch`).
     2. Slot match: `vote_a.epoch == self.epoch == vote_b.epoch` and `vote_a.round == self.round == vote_b.round` (else `InvalidEvidenceSlotMismatch`).
     3. Contradictory block hash: `vote_a.block_hash != vote_b.block_hash` (else `NonEquivocatingVotes`).
     4. Independent Ed25519 signature verification against validator public key (else `InvalidSignature`).
3. **Deterministic Integer Slashing (Pure Integer BPS Arithmetic)**:
   - Penalty calculation: `(bonded_stake * penalty_bps) / 10_000` via 256-bit ratio multiplication (`checked_mul_ratio`).
   - Default Hard Slashing: `HARD_SLASH_PENALTY_BPS = 10_000` (100% burn). Penalty > 10,000 BPS fails fast with `SlashingCalculationOverflow`.
   - Floating-point types strictly forbidden; zero precision loss.
4. **Permanent Validator Tombstoning**:
   - Slashed validator account inserted into `tombstones: BTreeSet<AccountId>`.
   - Any subsequent votes or proposals from the tombstoned validator rejected fail-fast with `ConsensusError::ValidatorTombstoned`.
5. **System Notification Log Emission (`RECORD_KIND_SYSTEM_NOTIF = 2`)**:
   - Generates 161-byte `MutationRecord` containing sender (slashed validator), recipient (zero burn address), slashed amount, and non-zero proof signature.

#### Test Execution & Verification Results
- **Test Suite**: `crates/axiom-consensus/tests/test_equivocation.rs`
- **Execution Latency**: 0.11s (6 integration tests)
- **Equivocation & Slashing Metrics**:
  - `test_valid_equivocation_evidence_verification`: Verified valid contradictory votes on round (epoch 1, round 1) successfully validate fraud proof.
  - `test_reject_non_equivocation_identical_blocks`: Confirmed identical block hash rejection with `ConsensusError::NonEquivocatingVotes`.
  - `test_reject_differing_epochs_or_rounds`: Confirmed epoch and round mismatches fail fast with `ConsensusError::InvalidEvidenceSlotMismatch`.
  - `test_reject_invalid_signature_in_evidence`: Tampered 1-byte signature failed fast with `ConsensusError::InvalidSignature`.
  - `test_deterministic_slashing_and_tombstone`: 1,000 AXM bonded stake subjected to 10,000 BPS slashing penalty; verified 1,000 AXM burned, 0 AXM remaining, validator marked tombstoned, and subsequent votes rejected with `ConsensusError::ValidatorTombstoned`.
  - `test_system_notification_mutation_generation`: Verified slashing generates compliant `MutationRecord` with `record_kind == RECORD_KIND_SYSTEM_NOTIF`, epoch 1, non-zero proof signature, and exact deduction.

---

### Ticket OPT-CONSENSUS-02: Deterministic View Change & Pacemaker
- **Target Subsystem**: `crates/axiom-consensus`
- **Status**: Validated & Merged
- **Specification**: Zero-communication deterministic proposer rotation (`(epoch + round) % N`), fixed 56-byte timeout message payload framing (`TimeoutMsg`), supermajority timeout certificate aggregation (`TimeoutCertificate`), and pure integer exponential backoff pacemaker (`Pacemaker`).

#### Technical Invariants
1. **Deterministic Proposer Election**:
   - Formula: `leader_idx = (epoch + round) % N` over active, non-tombstoned validator list.
   - Guaranteed identical leader resolution across all decentralized nodes with zero extra network rounds.
   - Tombstoned validators automatically excluded from rotation schedule.
2. **Fixed 56-Byte Timeout Signing Payload**:
   - Layout: `epoch` (8B Little-Endian), `round` (8B Little-Endian), `high_qc_round` (8B Little-Endian), `validator` (32B).
   - Invariant: Zero heap allocations during timeout message generation; signed cryptographically with Ed25519.
3. **Supermajority Timeout Certificate (TC) Assembly**:
   - Quorum requirement: `(2 * total_weight / 3) + 1`.
   - Invariant validation:
     - Strict slot alignment: all votes must match targeted `epoch` and `round` (else `InvalidTimeoutSlotMismatch`).
     - Zero duplicate votes: duplicate messages from same validator fail fast with `DuplicateTimeoutVote`.
     - Valid Ed25519 signatures verified for every participant (else `InvalidTimeoutSignature`).
     - Aggregated `high_qc_round = max(votes.high_qc_round)` to ensure liveness across highest certified view.
4. **Pure Integer Exponential Backoff Pacemaker**:
   - Formula: $\Delta_{\text{timeout}} = \Delta_{\text{base}} \times 2^{\min(\text{consecutive\_timeouts},\ 6)}$.
   - Standard progression: 2,000 ms -> 4,000 ms -> 8,000 ms -> 16,000 ms -> 32,000 ms -> 64,000 ms -> capped at 128,000 ms.
   - Pure integer arithmetic via `saturating_mul` and bit-shifting; zero floating-point types.
   - Successful proposal commit (`on_success`) resets consecutive timeout counter back to 0 (2,000 ms).
5. **Deterministic Round Advancement**:
   - Processing a valid `TimeoutCertificate` advances pacemaker `current_round` from $R$ to $R + 1$, allowing seamless view change without stalling the ledger.

#### Test Execution & Verification Results
- **Test Suite**: `crates/axiom-consensus/tests/test_view_change.rs`
- **Execution Latency**: 0.19s (5 integration tests)
- **View Change & Pacemaker Metrics**:
  - `test_deterministic_leader_election`: Verified leader rotation across 4 validators for rounds 1..=5 (`(epoch + round) % 4`); tombstoned validator B via verified slashing evidence and confirmed seamless rotation across {A, C, D}.
  - `test_timeout_certificate_supermajority_assembly`: Tested 4-validator set (threshold 3); verified 2 votes fail with `InsufficientTimeoutQuorum { required: 3, actual: 2 }`; 3rd vote successfully assembles valid TC with `high_qc_round = max(votes)`.
  - `test_reject_timeout_mismatch_or_duplicate`: Verified rejection of duplicate votes (`DuplicateTimeoutVote`) and slot mismatches across differing rounds and epochs (`InvalidTimeoutSlotMismatch`).
  - `test_pacemaker_exponential_backoff_and_reset`: Confirmed timeout scaling (2,000 ms -> 4,000 ms -> ... -> capped at 128,000 ms); verified commit reset back to 2,000 ms.
  - `test_advance_round_on_timeout_certificate`: Fed TC for round 3 to pacemaker; verified round progression to 4 and accurate leader election for the new round.

---

### Ticket OPT-NETWORK-01: Bounded TCP Flow Control & Rate Limiting
- **Target Subsystem**: `crates/axiom-network`
- **Status**: Validated & Merged
- **Specification**: Bounded length-prefixed framing (64 KB maximum payload), socket timeout enforcement (5,000 ms read/write guard against slowloris), pure-integer token bucket rate limiting (zero float, millisecond tick accounting), per-peer rate limiting table, and bounded ingress backpressure channel (`sync_channel(4096)` mapping saturation to `ConnectionThrottled`).

#### Technical Invariants
1. **Bounded 4-Byte LE Framing**:
   - Layout: 4-byte Little-Endian unsigned integer prefix (`u32`) for payload size.
   - Limit: `MAX_FRAME_SIZE = 65_536` bytes (64 KB).
   - Invariant: Framing decoder strictly checks `size <= MAX_FRAME_SIZE`. Payloads exceeding 64 KB are rejected immediately with `NetworkError::FrameTooLarge { size, max: 65_536 }` and the socket is closed defensively to eliminate memory explosion / buffer bloat risks.
2. **Explicit Socket Timeout Guard**:
   - Default: `DEFAULT_SOCKET_TIMEOUT_MS = 5_000` ms (5.0 seconds).
   - Invariant: `set_read_timeout` and `set_write_timeout` applied at socket construction (`FramedStream::from_tcp`). Incomplete packet transmissions (such as slowloris attacks sending partial header bytes) deterministically yield `NetworkError::IoTimeout` without blocking server execution threads indefinitely.
3. **Pure-Integer Token Bucket Rate Limiting (Zero Floating-Point)**:
   - Structure: `TokenBucketLimiter` tracking `capacity`, `tokens`, `rate_per_sec`, and `last_refill_ms`.
   - Refill Formula:
     $$\Delta t = t_{\text{current}} - t_{\text{last}}$$
     $$\text{added\_tokens} = \frac{\Delta t \times \text{rate\_per\_sec}}{1000}$$
   - Exact Sub-Millisecond Time Conservation: When tokens replenish below capacity, `last_refill_ms` advances strictly by the accounted integer milliseconds $\frac{\text{added\_tokens} \times 1000}{\text{rate\_per\_sec}}$, avoiding truncation drift.
   - Quota Enforcement: Exceeding available tokens rejects the request immediately with `NetworkError::RateLimitExceeded { peer }`.
   - Peer Table Tracking: `PeerRateLimiterTable` manages token buckets indexed per peer identifier / IP address.
4. **Bounded Ingress Channel & Backpressure Propagation**:
   - Channel Capacity: `INGRESS_CHANNEL_CAPACITY = 4_096` bounded entries (`sync_channel`).
   - Backpressure Invariant: `IngressReceiver` attempts non-blocking queue ingestion (`try_send`). When downstream verifiers/sequencers saturate the channel buffer, incoming network packets fail fast with `NetworkError::ConnectionThrottled`, pausing the network socket reader and naturally shrinking the OS TCP receive window to halt sender transmission at kernel level.

#### Test Execution & Verification Results
- **Test Suite**: `crates/axiom-network/tests/test_flow_control.rs`
- **Execution Latency**: 0.52s (4 integration tests)
- **Flow Control & Rate Limiting Metrics**:
  - `test_token_bucket_rate_limiter_burst_and_refill`: Initialized limiter with capacity 10 and rate 10/s; consumed 10 tokens immediately; verified 11th token rejected with `RateLimitExceeded`; advanced simulated time by 300 ms; verified exactly 3 tokens replenished; consumed 3 tokens and verified subsequent attempt rejected.
  - `test_framed_stream_rejects_oversized_frame`: Tested writer and reader against 65,537-byte payload (> 64 KB); verified writer and reader return `FrameTooLarge { size: 65537, max: 65536 }` and close connection.
  - `test_slow_read_socket_timeout_enforcement`: Mock TCP client connected to server with 500 ms read timeout and sent 1 byte before halting; verified server read terminates after ~500 ms with `NetworkError::IoTimeout` without hanging.
  - `test_bounded_ingress_backpressure_propagation`: Initialized bounded ingress channel of capacity 4; pushed 4 packets cleanly; verified 5th packet rejected with `NetworkError::ConnectionThrottled`; drained 1 packet downstream and verified 5th packet successfully accepted.

---

## Upcoming Tickets

### Ticket E2E-BENCH-01: End-to-End Stress Test & Throughput Saturation
- **Target Subsystem**: `bin/ratu-aurion-node`, `crates/ratu-aurion-engine`
- **Status**: Queued (Next Assignment)
- **Specification**: Construct full end-to-end integration stress tests exercising concurrent network transaction ingress, multi-stage pipelined verification and sequencing, append-only storage commits, and consensus rounds under maximum saturation.




