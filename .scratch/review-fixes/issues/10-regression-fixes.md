# 10 — Regressions and misses from issues 01–08

Status: done

## What to build

A follow-up review of the branch found behaviour that got worse than `main`, plus leftovers.

- Issue 03 narrowed the scan skip filter to the exact database names, so the executable itself,
  `.part` temp files and `modland_hash.db-journal` are now scanned when running from the tool's
  own folder. `main` skipped every path containing "modland_hash"; the bug was only that this
  also applied to directory names. Skip any *file name* containing the tool name.
- `extract_database` relied on `Ok(false)` stopping `for_each_entries`; sevenz-rust only stops
  the current block, so with several blocks the database was rewritten once per block. Only the
  first file with data may be written.
- `decompress_archive` deleted the downloaded archive on every failure, including when the
  archive is fine and only writing the database failed (locked file, full disk). Keep the
  archive in that case; delete it only when the archive itself cannot be opened or decoded.
- `write_atomically` renamed without syncing, so a crash could leave an empty file under the
  final name. Sync before rename.
- The archive path was derived in two places; use one helper.
- Leftovers: dead `pattern_hash == 0` guard in `get_files_from_pattern_hash`; the commented-out
  search function that issue 08 claimed to remove.

## Acceptance criteria

- [x] Files named `modland_hash`, `modland_hash.exe`, `modland_hash.db`, `modland_hash.db.7z`, `modland_hash.db.7z.part`, `modland_hash.db.part`, `modland_hash.db-journal` are not scanned; files in a directory whose name contains "modland_hash" still are
- [x] A multi-file archive writes the database exactly once, from the first file with data
- [x] A destination-side failure keeps the archive and says so; a source-side failure removes it
- [x] `write_atomically` syncs file data before the rename and reports which side failed
- [x] One function derives the archive path
- [x] No commented-out code block and no redundant zero guard remain
- [x] Tests cover the skip list, the multi-file archive, and the kept-archive case
- [x] `cargo build`, `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass

## Comments

### 2026-11-22 — Implemented
- Commit: Fix regressions from the review fixes
- Reviewers: 2 (combined, rust)
- Rounds: 1, 0 blocking findings; multi-block test strengthened after review
