# 05 — Correct match label when printing sample names

Status: done

## What to build

`print_found_entries` (src/main.rs) prints `(pattern_hash)` for every match when
`--print-sample-names` is set, even for exact hash matches. The label must be the same with and
without `-p`: `(hash) (pattern_hash)`, `(hash)` or `(pattern_hash)`.

Replace the `(bool, bool)` tuple with a small type that makes the two flags named, and derive the
label from it in one place so the label logic can be unit tested. Remove the redundant
`&& args.print_sample_names`.

## Acceptance criteria

- [x] Label is identical with and without `-p`
- [x] The match kind is a named type, not `(bool, bool)`; one function produces the label
- [x] Unit test for the three label variants
- [x] `cargo build`, `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass

## Comments

### 2026-11-22 — Implemented
- Commit: Label hash matches correctly with -p
- Reviewers: 2 (combined, rust)
- Rounds: 1, 0 findings across the loop
