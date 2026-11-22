# 11 — Publish the database only after extraction has fully succeeded

Status: done

## What to build

A Codex review of the branch found that the unpack path can still replace a working database
with a bad one, and that the atomic-write change altered three things `main` did not do.
All of it lives in the download/unpack code in `src/main.rs` (`write_atomically`, `download_db`,
`extract_database`, `decompress_archive`).

Refactor first: split `write_atomically` into "write and sync `<final>.part`" (returns the number
of bytes written) and "publish `.part` under the final name", so a caller can do work between the
two. Then change behaviour:

- **Truncated stream published as success.** `write_atomically` treats `read() == 0` as the end
  and renames. sevenz-rust 0.5.4's `Crc32VerifyingReader` (reader.rs:127) only checks the CRC
  once its remaining count reaches zero, so a stream that ends early returns `Ok(0)` without an
  error and a short/empty file replaces the live database. Compare the bytes written with
  `entry.size()`; a mismatch is a source-side error (archive is broken → it is removed).
- **Published before the archive iteration returned.** The rename happens inside the
  `for_each_entries` callback, but sevenz-rust then builds the decoder of every later block
  (reader.rs:1361, 1443) and that can fail. `extract_database` then returns an error although the
  live database was already replaced. Publish only after `for_each_entries` returned `Ok` and the
  size check passed; on any error remove the `.part` file and leave the live database untouched.
- **Permissions.** On `main` `File::create` truncated the existing file, keeping its mode. The
  renamed `.part` gets the umask default instead. When the destination already exists, give the
  `.part` file the destination's permissions before publishing.
- **Archive removal error swallowed.** After a successful unpack `main` propagated a failing
  `remove_file(archive)`; the branch ignores it. Propagate it on the success path (on the failure
  path keep reporting the extraction error).
- **Progress line left behind.** `download_db` calls `pb.finish()`, which leaves the finished
  "Downloading Database" line on screen; on `main` the bar was cleared when dropped. Clear it.

Out of scope (decided, do not change): validating regexes of flags the selected mode ignores,
sample-length values above `i64::MAX`, the masked-zero pattern-hash lookup, direct-file input
bypassing the scan skip list, the unreadable-root message.

## Acceptance criteria

- [x] `.part` writing and publishing are separate functions; `download_db` and `extract_database` both use them; one place builds the `.part` path
- [x] An entry whose decoded byte count differs from its declared size fails extraction, removes the archive and the `.part` file, and leaves an existing database byte-identical
- [x] An archive whose first block is valid and whose later block fails to decode fails extraction and leaves an existing database byte-identical
- [x] A destination-side failure still keeps the archive and says so (issue 10 behaviour unchanged)
- [x] Replacing an existing database or archive keeps that file's permissions (unix test with a non-default mode)
- [x] A failing archive removal after a successful unpack is returned as an error
- [x] No finished download progress line stays on screen
- [x] Tests cover the size mismatch, the failing later block, and the kept permissions
- [x] Old-vs-new binary diff over the standard command set shows no differences
- [x] `cargo build`, `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass; builds with Rust 1.75 APIs only; `Cargo.lock` stays `version = 3`

## Comments

### 2026-11-22 — Implemented
- Commit: Publish the database after extraction succeeds
- Reviewers: 3 (combined, coverage, rust)
- Rounds: 2, 1 finding across the loop (missing test for the archive-removal error)
- Old-vs-new binary diff (24 checks): only difference is the unpacked database keeping mode 640
