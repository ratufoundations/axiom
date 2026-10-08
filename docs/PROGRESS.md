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

## Upcoming Tickets

### Ticket OPT-STORAGE-03: Group Commit & Durability Tuning
- **Target Subsystem**: `crates/axiom-storage`
- **Status**: Queued (Next Assignment)
- **Specification**: Implement batch flushing scheduler and group commit durability modes (`FsyncAlways`, `FsyncBatch(N)`, `FsyncInterval(Duration)`), maximizing IOPS throughput while guaranteeing WAL determinism.
