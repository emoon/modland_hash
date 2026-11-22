# 07 — Atomic database download and decompress

Status: done

## What to build

`download_db` writes straight to `modland_hash.db.7z` and `decompress_db` writes straight over the
live `modland_hash.db` (src/main.rs). If either is interrupted, the next run finds a partial `.7z`
and panics in `SevenZReader::open(..).unwrap()` on every start (possibly after truncating the
working database) until the user deletes the file by hand.

- Download to `modland_hash.db.7z.part` and rename to `.7z` only after the body is fully written.
- Decompress to `modland_hash.db.part` and rename over `modland_hash.db` only on success; remove
  the `.7z` afterwards. On failure, remove the partial output and the bad archive, keep any
  existing database, and return an error (no panic).
- A leftover `.part` file from an earlier run is overwritten, never read.
- `Content-Length` is optional: without it, show progress without a total instead of panicking.
- No `unwrap` on I/O, network or archive results in these two functions.
- Structure the code so the "write stream to temp, then rename" and decompress steps are testable
  without network access.

## Acceptance criteria

- [x] A truncated/corrupt `modland_hash.db.7z` yields an error, is removed, and an existing database is byte-identical afterwards
- [x] The live database is only ever replaced by rename of a completely written file
- [x] The `.7z` only appears under its final name when completely downloaded
- [x] Missing `Content-Length` does not panic
- [x] No `unwrap`/`expect` on fallible I/O in `download_db` / `decompress_db`
- [x] Tests (no network): corrupt archive keeps existing db and cleans up; valid archive (created in the test with sevenz-rust) replaces the db and removes the archive; stream-to-file helper leaves no final file when the reader errors
- [x] `cargo build`, `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass

## Comments

### 2026-11-22 — Implemented
- Commit: Download and unpack the database atomically
- Reviewers: 3 (correctness+quality, test coverage, rust)
- Rounds: 1, 0 findings across the loop
