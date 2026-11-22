# 06 — build-database: strip root prefix only, validate before deleting

Status: done

## What to build

- `build_database` (src/main.rs) computes the stored name with
  `input_path.replace(database_path, "")`, which removes *every* occurrence of the root string
  (`-b .` strips all dots; `-b pub` mangles `/x/pub/y`). Strip the root as a path prefix only.
  The stored URL must keep its current shape for normal use: for root `/data/modland` and file
  `/data/modland/pub/modules/a b.mod` the stored url is `/pub/modules/a%20b.mod` (leading `/`,
  forward slashes), also when the root is given with a trailing slash or as `.`.
- `main` deletes the existing database before checking that the build path exists, so a typo
  leaves an empty database. Validate the path first and return an error without touching the
  existing database.

## Acceptance criteria

- [x] Only the leading root is stripped; later occurrences of the root string in the path are preserved
- [x] Stored url starts with `/` and uses `/` separators for roots given as `dir`, `dir/` and `.`
- [x] `--build-database <missing path>` exits with an error and leaves an existing database untouched
- [x] Unit tests for the path-to-stored-url function covering the cases above
- [x] `cargo build`, `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass

## Comments

### 2026-11-22 — Implemented
- Commit: Strip only the root prefix from stored names
- Reviewers: 2 (combined, rust)
- Rounds: 1, 0 findings across the loop
