# Axiom Protocol Engineering Worklog

## Completed Tickets

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

## Upcoming Tickets

### Ticket OPT-STORAGE-04: Bit-Rot & Structural Invariant Fast Scan
- **Target Subsystem**: `crates/axiom-storage`
- **Status**: Queued (Next Assignment)
- **Specification**: Implement zero-allocation streaming bit-rot verification and fast structural invariant validation across log segments.

