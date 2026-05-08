# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working
with code in this repository.

## Project overview

`sample_account` is a Rust 2021 command-line generator for synthetic
Japanese personal-account records. It is a port of the C++17
implementation at `/Volumes/dev/src/cpp/work/sample_account/`. Output
is CSV on stdout, columns selected via short/long option flags. Data
is sourced at runtime from CSV files in `data/`.

## Dev environment

The dev shell is provided by Nix flake. Always work inside `nix develop`:

```sh
nix develop                        # opens a shell with the pinned rustc/cargo
nix develop -c cargo build         # one-shot
nix develop -c cargo test          # one-shot tests
```

If `direnv` is installed and `direnv allow` was run, `cd` into the repo
auto-loads the dev shell.

## Build & test

```sh
cargo build                              # debug
cargo build --release                    # optimized
cargo test                               # 50+ tests
cargo test --lib                         # unit tests in src/
cargo test --test repos                  # repository integration tests
cargo test --release --test snapshot     # snapshot suite (runs the binary)

cargo run --release -- --help
cargo run --release -- -ilfm 10
cargo run --release -- -j 0 -ilfm 100000  # multi-core generation
```

The CSV data files are embedded into the binary at compile time via
`include_str!` (see `src/repos.rs::EMBEDDED_*_CSV`), so the binary works
from any working directory. The `data/*.csv` files are still on disk for
the file-based loader (`load_*(path)`) used by `tests/repos.rs`.

## Architecture

Three layers, dependencies flow downward only. Mirrors the C++ source
file-for-file.

```
CLI Layer        src/cli.rs, src/main.rs
                 argv parser, --help, row loop, stdout writes
                     │
Generation Layer src/registry.rs   (Field table, 17 emit fns, FIELDS slice)
                 src/field.rs      (RowContext + Deps borrow bundle)
                 src/generators.rs (PersonGenerator / AddressGenerator /
                                    AgeAndDateGenerator)
                 src/rng.rs        (SmallRng wrapper + env vars)
                     │
Data Layer       src/repos.rs      (record structs + RepoError +
                                    parse_*<R: BufRead> helpers +
                                    load_*(path) for file I/O +
                                    default_*() using include_str!
                                    embedded const &str)
```

### Adding a new column

1. Define `fn emit_<name>(out: &mut String, ctx: &RowContext, deps: &mut Deps)`
   in `src/registry.rs`.
2. Append a `Field { short: 'X', long: "<name>", desc: "...", emit: emit_<name> }`
   to the `FIELDS` const slice.

That's it. The CLI parser, `--help` text, and short-option lookup all
derive from `FIELDS` — no other edits needed.

### Per-row state

`RowContext` (in `src/field.rs`) holds the random integers drawn once
per row: `first`, `last`, `pref`, `ward`, `city`, `age`, plus the row
index. Multiple fields can read the same context value to stay
consistent (e.g. the same `first` index drives both first-name and
email's local-part). `Rng::roll_date()` is called once per row so the
`--date` field's year/month/day are self-consistent.

The order of `rng.next_i32()` calls in `build_row_into` MUST stay
`first → last → pref → ward → city → age → roll_date`. Snapshot tests
rely on this ordering.

### Per-row deterministic RNG (multi-core)

Each row builds its own `Rng` from a sub-seed derived as
`master_seed.wrapping_add((row_idx as u64).wrapping_mul(GOLDEN))` (see
`src/rng.rs::derive_row_seed`). This means:

- `-j 1` (single) and `-j 0/N` (multi) produce **byte-identical output**
  for the same master seed.
- Adding/removing rows in the middle does NOT shift the RNG stream of
  later rows (each row is independent).
- Adding a new field that consumes RNG calls inside the row WILL change
  the bytes for all rows. Regenerate snapshots after such changes.

### Streaming I/O (memory bounds)

Both modes stream output and use **constant memory regardless of row
count** (verified at 1M and 10M; same model holds for billions):

- **Single mode** (`run_serial`): one row-sized scratch `Vec<u8>` is
  reused across rows. Each row appends, flushes to the 1 MiB `BufWriter`,
  then truncates. Peak RSS ~25 MB.
- **Parallel mode** (`run_parallel`): rows are processed in batches of
  `BATCH_ROWS` (128k). Each batch is generated in parallel into per-
  worker `Vec<u8>` buffers (~10 MiB total per batch), drained to the
  BufWriter in order, then the next batch starts. The rayon pool is
  built once and reused via `pool.install` for every batch — pool
  startup is paid once, not per batch. Peak RSS ~70 MB.

When tuning:
- Lower `BATCH_ROWS` → less peak memory, more sync overhead.
- Higher `BATCH_ROWS` → less sync overhead, more memory per batch.
- 128k × 256 B = 32 MiB peak is comfortable. Don't go below ~10k unless
  you need very-low-memory environments — at very small batches the
  per-batch join overhead dominates the actual work.

### Repository contract

Repositories are read-only after construction. They are loaded once in
`try_main` and borrowed by Generators (`'a` lifetime tied to `main`'s
stack). Loaders return `Result<_, RepoError>`. To swap data sources,
write equivalent loader functions returning the same record types.

`src/repos.rs` exposes two parallel APIs:
- `default_*()` — parses CSV bytes baked in via `include_str!`. Used by
  `main`; works from any working directory.
- `load_*(path)` — opens a file and parses it. Kept so `tests/repos.rs`
  can verify the on-disk CSV format. Both share the same private
  `parse_*<R: BufRead>(reader, source)` body.

When `data/*.csv` files are updated, the binary needs a rebuild
(`include_str!` is compile-time). Both APIs read identical bytes after
rebuild — verified by `default_*_matches_load_*` unit tests.

## Determinism / test hooks

Two environment variables make output reproducible — used by
`tests/snapshot.rs` and useful for any regression test:

- `SAMPLE_ACCOUNT_SEED` — pins the RNG seed (replaces wall-clock
  seeding).
- `SAMPLE_ACCOUNT_NOW`  — pins "current time" (Unix epoch seconds) used
  by `Rng::roll_date()` and `AgeAndDateGenerator::birth_year()`.

When regenerating expected snapshots, set both:

```sh
SAMPLE_ACCOUNT_SEED=42 SAMPLE_ACCOUNT_NOW=1700000000 \
  cargo run --release -- -ilfmatpwcgbdorynq 5 \
  > tests/expected/all-flags-seed-42.csv
```

## Data files

CSVs are committed; do not regenerate casually. They are copied verbatim
from the C++ repo.

- `data/sample_account.csv` — `last_kanji,last_kana,last_romaji,first_kanji,first_kana,first_romaji,gender,blood`
- `data/prefectures.csv` — `code,name,population` (47 rows; `zips`
  computed at load time from `address.csv`)
- `data/address.csv` — `pref_code,prefecture,ward,city` (sorted by
  `pref_code`; loader assumes contiguous grouping). Some rows have an
  empty `city` column.
- `data/ages.csv` — `generation,population` (population uses
  thousand-separators; `parse_digits` strips non-digits before parsing)

## Notable constraints / gotchas

- The legacy `--telehpne` long-option typo is preserved as an alias for
  `--telephone` in `cli.rs`. Do NOT "fix" it without understanding which
  downstream scripts use it.
- The version field lives in `Cargo.toml` (`version = "0.4.7"`) — bump
  it on release.
- `tests/snapshot.rs` uses `env!("CARGO_BIN_EXE_sample_account")` to
  locate the built binary. Cargo handles this; no manual path needed.
- `Field`'s `emit` is `fn(...)` (not `Fn` closure). All emit functions
  must be plain `fn`s — no captured state. This is intentional, mirrors
  C++'s stateless `IField` subclasses.
- Snapshot tests are NOT byte-for-byte compatible with the C++
  version's output. Different RNG (libc rand vs `SmallRng`).
- `-j` short flag is special-cased in the parser — it consumes the next
  token (`-j 4`) or its own digits (`-j4`). Make sure no field uses `'j'`
  as its short flag (none currently do).
- In multi-core mode, the row generation closure is executed on rayon
  worker threads. The closure captures `&persons`, `&pref_repo`,
  `&age_repo`, `&fields` — all read-only borrows — so no `Mutex` or
  `Arc` needed. Cost: each row constructs three throwaway `Generator`s,
  but they're empty borrows so this is essentially free.

## Test layout

| Tests | Where |
|---|---|
| Unit tests for repos / rng / generators / registry / cli | `src/<mod>.rs` `#[cfg(test)] mod tests` |
| Integration tests for repository loaders against real CSVs | `tests/repos.rs` |
| Snapshot tests (run binary, diff stdout) | `tests/snapshot.rs` |

## Recent commit style — Conventional Commits required

[release-please](https://github.com/googleapis/release-please) automates
versioning and CHANGELOG generation by parsing commit messages on `main`.
Every commit message MUST follow [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/):

| Prefix | Effect (pre-1.0, with `bump-minor-pre-major`) | Example |
|---|---|---|
| `feat:` | minor bump (0.5.0 → 0.6.0) | `feat: add CSV column for credit limit` |
| `fix:` | patch bump (0.5.0 → 0.5.1) | `fix: handle empty city in address.csv` |
| `feat!:` or `BREAKING CHANGE:` footer | also minor pre-1.0 (per `bump-patch-for-minor-pre-major: false`) | |
| `docs:` / `chore:` / `ci:` / `test:` / `refactor:` / `perf:` | **no bump** | `docs: clarify per-row RNG derivation` |

Non-conformant messages are silently ignored by release-please (no bump,
no CHANGELOG entry). Reviewers should reject PRs with non-conventional
commit messages on the squash-merge title.

## Release flow

1. Land conventional `feat:` / `fix:` PRs on `main`.
2. The [`release-please` workflow](.github/workflows/release-please.yml)
   opens or updates a "release PR" titled `chore(main): release X.Y.Z`.
3. Merge the release PR — it tags `vX.Y.Z` and creates a GitHub Release.
4. The [`release-binaries` workflow](.github/workflows/release-binaries.yml)
   fires on `release: created`, builds 3 native targets (Linux x86_64,
   Windows MSVC, macOS Apple Silicon) via
   [taiki-e/upload-rust-binary-action](https://github.com/taiki-e/upload-rust-binary-action),
   and uploads 6 assets (3 archives + 3 `.sha256`).

## Planning docs

If you need to make a substantial change, the original migration plan
and design docs live in:

- `tasks/todo.md` — phased TODO checklist
- `docs/PRD.md` — requirements
- `docs/architecture.md` — module layout
- `docs/system_design.md` — types and signatures
- `docs/tech_doc.md` — technical decisions and their rationale
