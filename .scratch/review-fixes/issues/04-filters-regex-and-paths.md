# 04 — Filters: case-insensitive regex and encoded path matching

Status: done

## What to build

`Filters` (src/main.rs):

- `--include-sample-name` and `--search-filename` lowercase the *pattern* before compiling, which
  flips `\S`, `\D`, `\W`, `\B` etc. (`ahh\Sox` matches nothing while `ahhvox` matches). Compile with
  `RegexBuilder::case_insensitive(true)` instead and stop lowercasing patterns. An invalid regex
  must be reported as a normal error from `main` (no panic). `Filters::new` returns `Result`.
- `print_sample_rows` builds its own copy of the sample-name regex; it must use the single
  authoritative construction.
- Path filters (`--include-paths`, `--exclude-paths`) are compared against stored URLs, which are
  percent-encoded by `get_stored_url` (space -> `%20`, `'` -> `%27`), so a filter containing a
  space never matches. Encode filter values the same way before comparing.
- Items in the comma-separated filter lists are trimmed; empty items are dropped
  (`"mod, xm"` must behave like `"mod,xm"`).

## Acceptance criteria

- [x] Regex patterns are not lowercased; matching is case-insensitive via the regex builder
- [x] `ahh\Sox` matches the sample name `st-91:AHHVOX`
- [x] An invalid regex yields an error exit with a readable message, not a panic
- [x] One construction site for the sample-name regex
- [x] `--include-paths "/pub/modules/Protracker/Richard Joseph"` matches `/pub/modules/Protracker/Richard%20Joseph/x.mod`; same for exclude
- [x] List items are trimmed and empty items ignored
- [x] Unit tests for each criterion above on `Filters` / `apply_filter`
- [x] `cargo build`, `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass

## Comments

### 2026-11-22 — Implemented
- Commit: Fix regex case handling and encoded path filters
- Reviewers: 2 (combined, rust)
- Rounds: 1, 0 findings across the loop
