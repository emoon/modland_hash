# 02 — FFI soundness: null slices and file length

Status: done

## What to build

1. `CData::get_samples` (src/main.rs) calls `slice::from_raw_parts` with a null pointer when the
   file is not a module or has no samples. This is UB; a debug build aborts with
   "unsafe precondition(s) violated" on any non-module file. Return an empty slice for null/zero.
   Same guard for `get_instrument_names`. `get_track_info` dereferences `song_data` before its
   null check — check first.
2. `hash_file` takes `int len` in C++ (`external/libopenmpt/interface.cpp`) and `u32` in Rust;
   Rust passes `file_data.len() as _`. Files >= 2 GiB become negative/truncated and are then cast
   to `size_t` -> out-of-bounds read. Make the length `size_t` / `usize` on both sides, and make
   the Rust-side counts in `CData` match the C `int` fields exactly (`c_int`), rejecting negatives.
3. In `hash_file`, the `samples` / `instruments` allocations leak if an exception is thrown before
   they are stored in `data`. Store them in `data` as soon as they are allocated.
4. Add `// SAFETY:` comments to every `unsafe` block stating the actual argument.

Do not change the pattern hash, sample hashing or any stored value.

## Acceptance criteria

- [x] `get_samples` / `get_instrument_names` never build a slice or read through a null pointer
- [x] `song_data` is null-checked before any dereference in `get_track_info`
- [x] `hash_file` length is `size_t` in C++ and `usize` in Rust; no narrowing cast at the call site
- [x] `CData` count fields have the same signedness/width on both sides and negative counts are treated as 0
- [x] No allocation in `hash_file` leaks on the exception path
- [x] Every `unsafe` block has a `// SAFETY:` comment that argues soundness
- [x] Test (runs in debug via `cargo test`): `get_track_info` on a temp file containing non-module bytes returns a `TrackInfo` with no samples and `pattern_hash == 0` without aborting
- [x] Test: `get_track_info` on a minimal valid 4-channel ProTracker MOD generated in the test returns a non-zero `pattern_hash` and 31 samples
- [x] `cargo build`, `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass

## Comments

### 2026-11-22 — Implemented
- Commit: Make the libopenmpt FFI boundary sound
- Reviewers: 2 (combined, rust)
- Rounds: 1, 0 findings across the loop
