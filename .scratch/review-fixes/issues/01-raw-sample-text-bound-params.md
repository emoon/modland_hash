# 01 — Raw sample text + bound SQL parameters

Status: done

## What to build

`get_string_cstr` (src/main.rs) returns an SQL literal (`'name'`, apostrophes doubled) and that
string is stored in `SampleInfo.text` / `sha256_hash` (`'hex'` or `NULL`). The SQL quoting leaks
into output: `-p` prints the local file's sample names wrapped in quotes, trailing blank names are
not trimmed (they are `''`, never empty), and `match_samples` slices `[1..len-1]` to undo it.

Separate persistence from the domain model:

- `SampleInfo.text` holds the raw sample name; `SampleInfo.sha256_hash` becomes `Option<String>` (raw hex).
- All inserts in `build_database` and the sample lookup in `match_samples` use bound parameters
  instead of `format!`-built SQL. `DbCommand::Insert` carries typed data, not an SQL string.
- `find-samples-with-length*` queries bind the length too.
- Remove the quote-stripping slice in `match_samples`.

The database contents produced by `--build-database` must stay byte-for-byte equivalent in meaning:
same tables, same column values (text without quotes, hash hex or NULL, pattern_hash NULL when 0).
Behaviour for non-UTF-8 names (stored as empty) is unchanged in this issue.

## Acceptance criteria

- [x] No SQL statement in src/main.rs is built by interpolating data values with `format!`
- [x] `SampleInfo.text` is the raw name; `sha256_hash` is `Option<String>`; `get_string_cstr` no longer SQL-escapes
- [x] `-p` output for the local file shows names without quotes and trims trailing blank rows like DB entries
- [x] `--match-samples` prints names without the `[1..len-1]` slice and without quotes in the detail header
- [x] Unit test: building a DB into a temp file from a temp dir, then reading rows back, yields the expected `files`/`samples` values, including a sample name containing an apostrophe (insert path may be tested at the DB-writer level without real module files)
- [x] `cargo build`, `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass

## Comments

### 2026-11-22 — Implemented
- Commit: Store raw sample text, bind SQL parameters
- Reviewers: 3 (correctness+quality, test coverage, rust)
- Rounds: 1, 0 findings across the loop
