# Implementation Checklist — r200_uhf Rewrite

Starting from: `feat/reliable-write-error-handling` branch.

---

## Phase 0 — Clean Slate

- [x] Create branch `rewrite/sans-io` from current HEAD
- [x] Delete `src/`, `examples/`, `Cargo.toml`, `Cargo.lock`
- [x] Create new `Cargo.toml` from scratch
- [x] Create directory skeleton

---

## Phase 1 — Core Error Types

- [x] `src/core/mod.rs` — module declarations + re-exports
- [x] `src/core/error.rs` — `CoreError`, `FrameError`, `CommandError`

---

## Phase 2 — Frame Layer

- [x] `src/core/frame.rs` — `Frame`, `FrameType`, `checksum()`, `encode_command()`, `decode()`

---

## Phase 3 — Command Trait + Types

- [x] `src/core/command.rs` — `Command` trait definition
- [x] `src/core/command.rs` — `GetModuleInfo`, `ModuleInfoParam`, `ModuleInfoResponse`
- [x] `src/core/command.rs` — `SinglePollingInstruction`, `MultiplePollingInstruction`, `StopMultiplePolling`
- [x] `src/core/command.rs` — `SetSelect`, `SetSendSelect`
- [x] `src/core/command.rs` — `ReadLabel`, `WriteLabel`, `MemBank`
- [x] `src/core/command.rs` — `KillTag`, `LockTag`
- [x] `src/core/command.rs` — `GetWorkingArea`, `SetWorkingArea`
- [x] `src/core/command.rs` — `GetWorkingChannel`, `ChannelInfo`
- [x] `src/core/command.rs` — `GetTransmitPower`, `SetTransmitPower`

---

## Phase 4 — Tag & Region

- [x] `src/core/tag.rs` — `Tag` struct, `parse()`, `Display`, manual `Hash`/`Eq` (EPC only)
- [x] `src/core/region.rs` — `Region` enum, `from_byte()`, `channel_frequency()`

---

## Phase 5 — lib.rs

- [x] `src/lib.rs` — module declarations, feature gates, re-exports, doc comments

---

## Phase 6 — Sync Transport

- [x] `src/sync/mod.rs` — `SyncReader<W>` struct (generic over port type)
- [x] `src/sync/mod.rs` — `send()`, rolling-buffer `read_frame()` with persistent buffer

---

## Phase 7 — Async Transport

- [x] `src/async_transport/mod.rs` — `AsyncReader<W>` struct (generic over port type)
- [x] `src/async_transport/mod.rs` — `send()`, `read_frame()` with `tokio::time::timeout`

---

## Phase 8 — Tests (Core)

- [x] `src/core/frame.rs` — frame encode/decode round-trips, checksum, error variants (10 tests)
- [x] `src/core/command.rs` — encode + decode for each command type (22 tests)
- [x] `src/core/tag.rs` — parse valid/truncated/empty, `epc_hex()`, Hash/Eq (7 tests)
- [x] `src/core/region.rs` — `from_byte`, `channel_frequency` per region (8 tests)
- [x] `src/core/error.rs` — `Display` formatting, `is_success` (implicit in other tests)

---

## Phase 9 — Tests (Transport)

- [x] `src/sync/mod.rs` — mock stream tests for key commands (8 tests)
- [x] `src/async_transport/mod.rs` — tokio mock tests for key commands (4 tests)

---

## Phase 10 — CLI

- [x] `src/cli/mod.rs` — `run()`, clap arg parsing, command dispatch (stub)
- [x] `src/cli/display.rs` — tag display, hex helpers
- [x] `src/cli/port.rs` — placeholder
- [x] `src/main.rs` — binary entry point

---

## Phase 11 — Examples

- [x] `examples/sync_poll.rs`
- [x] `examples/async_poll.rs`

---

## Phase 12 — Finalize

- [ ] `cargo fmt`
- [ ] `cargo clippy --all-features` — no warnings
- [ ] Update `README.md`
- [ ] Update `features.md` — mark newly implemented commands
- [ ] Merge to main

---

## Test Results

```
running 62 tests — all passed ✓
```

| Module | Tests | Status |
|--------|-------|--------|
| `core::frame` | 10 | ✓ |
| `core::command` | 22 | ✓ |
| `core::tag` | 7 | ✓ |
| `core::region` | 8 | ✓ |
| `sync` | 8 | ✓ |
| `async_transport` | 4 | ✓ |
| **Total** | **62** | **✓** |
