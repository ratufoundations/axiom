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
- **Execution Latency**: 0.04s (`test_torn_write_recovery_at_tail` + `test_sub_header_truncation_recovery`)
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

## Upcoming Tickets

### Ticket OPT-STORAGE-02: Zero-Fragmentation Pre-allocation
- **Target Subsystem**: `crates/axiom-storage`
- **Status**: Queued (Next Assignment)
- **Specification**: Implement zero-fragmentation fallocate / sparse file reservation for 128 MB segment allocation, eliminating file system fragmentation on sequential HDD/NVMe storage while maintaining strict 128 MB maximum segment boundaries.
