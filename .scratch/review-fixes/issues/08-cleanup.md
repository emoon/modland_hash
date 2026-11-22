# 08 — Small cleanups from the review

Status: done

## What to build

- `get_dupes`: `Vec::with_capacity(700_0000)` reserves 7 million entries (~168 MB). Drop the
  preallocation.
- Remove the unused instrument-name path: `CData::get_instrument_names`, `TrackInfo.instrument_names`
  and the C++ side that fills `instrument_names` (it reads index `0..count` of a 1-based array and
  nothing consumes it). Keep the `CData` field layout identical on both sides (leave the pointer
  field, always null) and keep the `instruments` table creation so the schema is unchanged.
- Cargo.toml: `Build = "build.rs"` is an unrecognised key (remove; build.rs is auto-detected);
  remove the unused `filetime` dependency.
- Text: "modland_has executable" -> "modland_hash executable"; the `--exclude-file-extensions`
  help text says `--skip-file-extensions`; readme lists `-b, --build-database` twice.
- Remove dead commented-out code blocks and unused `_spinner_style` values in src/main.rs.

Do not change the pattern hash or any stored value.

## Acceptance criteria

- [x] No 7M-entry preallocation
- [x] Instrument-name code removed on both sides; `CData` layout still matches; schema unchanged
- [x] Cargo.toml has no `Build` key and no `filetime`; Cargo.lock updated accordingly
- [x] Text fixes applied in src/main.rs help strings and readme.md
- [x] Dead commented-out code removed from src/main.rs
- [x] `cargo build`, `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass

## Comments

### 2026-11-22 — Implemented
- Commit: Remove dead code and unused dependency
- Reviewers: 2 (combined, rust)
- Rounds: 1, 0 findings across the loop
