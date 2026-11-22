# 03 — File scanning: wrong skip filter and panics on unreadable entries

Status: done

## What to build

`get_files` (src/main.rs):

- `!filename.contains("modland_hash")` is tested against the full path, so every file under a
  directory whose path contains "modland_hash" is silently skipped (no output, exit 0). Only the
  database file itself and its `.7z` should be skipped: compare the entry's file name against
  `DB_FILENAME` and `DB_FILENAME + ".7z"`. Keep skipping `*.listing`.
- `e.unwrap()` / `file.metadata().unwrap()` / `std::fs::metadata(path).unwrap()` panic on a
  permission-denied directory or a file that vanishes mid-scan. Print a warning to stderr and skip.

`get_track_info`: `File::open(..).unwrap()` / `read_to_end(..).unwrap()` abort the whole run for one
unreadable file. Return `Result<TrackInfo>`; callers (`match_dir_against_db`, `match_samples`,
`build_database`) print a warning to stderr and continue with the next file.

## Acceptance criteria

- [x] Files in a directory whose path contains "modland_hash" are returned by `get_files`
- [x] A file named `modland_hash.db` or `modland_hash.db.7z` is skipped; `*.listing` is still skipped
- [x] No `unwrap`/`expect` on filesystem results remains in `get_files` or `get_track_info`
- [x] An unreadable file or directory produces a warning on stderr and the scan continues
- [x] In `build_database` a skipped file produces no `files` row
- [x] Tests cover: the modland_hash-in-path case, DB file skipping, `.listing` skipping, non-recursive vs recursive depth, and `get_track_info` returning `Err` for a missing file
- [x] `cargo build`, `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass

## Comments

### 2026-11-22 — Implemented
- Commit: Scan files by name and survive unreadable ones
- Reviewers: 2 (combined, rust)
- Rounds: 1, 0 findings across the loop
