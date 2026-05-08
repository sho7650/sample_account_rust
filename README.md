# sample_account (Rust port)

[![CI](https://github.com/sho7650/sample_account_rust/actions/workflows/ci.yml/badge.svg)](https://github.com/sho7650/sample_account_rust/actions/workflows/ci.yml)

Synthetic Japanese personal-account record generator (name, address, age,
mail, phone, etc.). Outputs CSV on stdout. Columns are selected via
short/long option flags — flag occurrence order determines column order.

A Rust port of the C++17 implementation at
`/Volumes/dev/src/cpp/work/sample_account/`.

## Quick start

```sh
nix develop                              # enter dev shell with rustc + cargo + clippy + rustfmt
cargo run --release -- --help            # show options
cargo run --release -- -ilfm 10          # 10 rows: id, last/first name (kanji,kana), email
cargo run --release -- -j 0 -ilfm 100000 # 100k rows on all CPU cores
cargo test                               # 60+ tests across unit, integration, snapshot
```

The CSV data files are **embedded into the binary at compile time**, so
the executable runs from any working directory:

```sh
cargo install --path .         # install to ~/.cargo/bin
cd /tmp                        # any directory works
sample_account -ilfm 3         # data is baked in, no data/ needed next to binary
```

(Closes [issue #1](https://github.com/sho7650/sample_account_rust/issues/1).)

## Build & development

### With Nix (recommended)

```sh
nix develop                       # one-shot
direnv allow                      # if you want auto-activation via direnv
cargo build                       # debug
cargo build --release             # optimized
cargo test                        # all tests
cargo test --test snapshot --release  # snapshot suite only
```

The `flake.nix` pins `rust-overlay`'s stable channel and bundles
`rust-analyzer`, `clippy`, and `rustfmt` into the dev shell.

### Without Nix

```sh
rustup toolchain install stable
cargo build --release
```

## Output columns

| short | long          | description                                     |
|-------|---------------|-------------------------------------------------|
| `-i`  | `--id`        | sequential row id (1-based)                     |
| `-l`  | `--lastname`  | last name (kanji,kana — TWO CSV fields)         |
| `-f`  | `--firstname` | first name (kanji,kana — TWO CSV fields)        |
| `-m`  | `--mail`      | `firstname_lastname@example.com`                |
| `-t`  | `--telephone` | `090-XXXX-XXXX`                                 |
| `-p`  | `--prefecture`| prefecture name (population-weighted)           |
| `-w`  | `--ward`      | ward / municipality                             |
| `-c`  | `--city`      | city / district                                 |
| `-g`  | `--gender`    | 男 / 女                                         |
| `-b`  | `--blood`     | ABO blood type                                  |
| `-a`  | `--age`       | age in years (population-weighted)              |
| `-o`  | `--agegroup`  | age group rounded down to the decade            |
| `-y`  | `--birthyear` | birth year derived from age                     |
| `-r`  | `--reward`    | annual income-like figure                       |
| `-d`  | `--date`      | random valid date `YYYY/M/D`                    |
| `-n`  | `--random`    | random signed integer in ±10,000,000            |
| `-q`  | `--quotient`  | random fraction in `[0.00, 0.99]`               |

Legacy aliases:
- `--telehpne` — preserved typo of `--telephone`, kept for downstream scripts.

## Memory bounds

Both modes stream output: memory is **independent of row count**.

| mode | peak RSS | how |
|---|---|---|
| `-j 1` (single) | ~25 MB | one row-sized scratch buffer reused; rows flush directly to a 1 MiB `BufWriter` |
| `-j 0/N` (parallel) | ~70 MB | rows are processed in 128 k-row **batches**; each batch parallel-generated, drained to BufWriter, next batch starts |

Measured (1M and 10M rows, all 17 columns):

| count | `-j 1` RSS | `-j 0` RSS |
|---|---|---|
| 1M | 23 MB | 57 MB |
| 10M | 23 MB | 71 MB |

The same memory profile holds for billions of rows — only the wall time
grows (linearly). `BATCH_ROWS = 128 * 1024` and `OUT_BUF_BYTES = 1 MiB`
in `src/main.rs` are the tunables.

## Parallelism

The generator can run on multiple CPU cores via [rayon](https://docs.rs/rayon/).
Each row is generated independently from a deterministically-derived
sub-seed, so **single- and multi-threaded modes produce byte-identical
output for the same `SAMPLE_ACCOUNT_SEED`.**

```
-j, --jobs <N>
  0   auto-detect (use all CPU cores)
  1   single-threaded sequential (default)
  N>=2  N worker threads
```

Examples:

```sh
sample_account -j 1 -ilfm 10000        # explicit single (default)
sample_account -j 4 -ilfm 100000       # 4 workers
sample_account -j 0 -ilfm 1000000      # use all cores
sample_account --jobs 8 -ilfm 100000   # equivalent long form
sample_account --jobs=8 -ilfm 100000   # equivalent with =
```

When to use which:
- **Single mode (default):** small generations (< ~50k rows) where rayon
  pool startup and per-row allocation overhead can outweigh speedups.
- **Multi mode:** large generations (≥ ~100k rows) or when running on
  spare cores. Output is order-preserving — row 1 always comes before
  row 2 even when 12 workers are emitting concurrently.

## Determinism

Two environment variables make output reproducible — used by snapshot
tests in `tests/expected/`.

- `SAMPLE_ACCOUNT_SEED` — pins the RNG seed (replaces wall-clock seeding).
- `SAMPLE_ACCOUNT_NOW`  — pins "current time" (Unix epoch seconds) used by
  `Rng::roll_date()` and `AgeAndDateGenerator::birth_year()`.

```sh
SAMPLE_ACCOUNT_SEED=42 SAMPLE_ACCOUNT_NOW=1700000000 \
  ./target/release/sample_account -ilfm 5
```

## Architecture

Three layers, dependencies flow downward only:

```
┌─────────────────────────────────────────────────────┐
│ CLI Layer        src/cli.rs, src/main.rs            │
└──────────────────────┬──────────────────────────────┘
                       ▼
┌─────────────────────────────────────────────────────┐
│ Generation Layer src/registry.rs (Field table)      │
│                  src/field.rs    (RowContext, Deps) │
│                  src/generators.rs                  │
│                  src/rng.rs                         │
└──────────────────────┬──────────────────────────────┘
                       ▼
┌─────────────────────────────────────────────────────┐
│ Data Layer       src/repos.rs  (CSV loaders)        │
└─────────────────────────────────────────────────────┘
```

Design notes are in `docs/`:
- [`docs/PRD.md`](docs/PRD.md) — product requirements
- [`docs/architecture.md`](docs/architecture.md) — module layout
- [`docs/system_design.md`](docs/system_design.md) — types & signatures
- [`docs/tech_doc.md`](docs/tech_doc.md) — technical decisions

## Adding a new column

1. Add an `emit_<name>` free function in `src/registry.rs`.
2. Append a `Field { short, long, desc, emit: emit_<name> }` to the
   `FIELDS` slice.

That's it. The CLI parser, `--help` text, and short-option lookup all
derive from `FIELDS` — no other edits needed. Mirrors the C++ structure
where adding a column required one new `IField` subclass and one
registration line.

## Per-row state

`RowContext` (in `src/field.rs`) holds the random integers drawn once
per row: `first`, `last`, `pref`, `ward`, `city`, `age`, plus the row
index. Multiple fields can read the same context value to stay
consistent (e.g. the same `first` index drives both first-name and
email's local-part). `Rng::roll_date()` is also called once per row so
the `--date` field's year/month/day are self-consistent.

## Repository contract

Repositories are loaded once at startup. They expose `Vec` fields that
generators borrow into and index by `n.rem_euclid(len)`. To swap data
sources (JSON, in-memory, etc.), implement equivalent loader functions
returning the same record types.

## Data files

CSVs are committed; do not regenerate casually.

- `data/sample_account.csv` — `last_kanji,last_kana,last_romaji,first_kanji,first_kana,first_romaji,gender,blood`
- `data/prefectures.csv` — `code,name,population` (47 rows; `zips` is
  computed at load time from `address.csv`)
- `data/address.csv` — `pref_code,prefecture,ward,city` (sorted by
  `pref_code`; the loader assumes contiguous grouping)
- `data/ages.csv` — `generation,population` (population uses
  thousand-separators; `parse_digits` strips non-digits before parsing)

## Tests

```sh
cargo test                                 # run everything
cargo test --lib                           # unit tests only
cargo test --test repos                    # repository integration tests
cargo test --release --test snapshot       # snapshot diff vs tests/expected/
```

Snapshot tests run the binary with `SAMPLE_ACCOUNT_SEED=42
SAMPLE_ACCOUNT_NOW=1700000000` and diff against `tests/expected/*.csv`.

When intentionally changing output (new column, fix, etc.), regenerate
the expected files:

```sh
SAMPLE_ACCOUNT_SEED=42 SAMPLE_ACCOUNT_NOW=1700000000 \
  cargo run --release -- -ilfmatpwcgbdorynq 5 \
  > tests/expected/all-flags-seed-42.csv
# … repeat for the other 3 scenarios.
```

## Differences from the C++ version

- **No byte-for-byte snapshot compatibility.** The C++ version uses libc
  `rand()`; this port uses `rand::rngs::SmallRng`. Snapshots were
  regenerated.
- **CSV data is embedded in the binary** via `include_str!` so the
  executable runs from any working directory. The C++ version required
  `cd repo_root` because it loaded `./data/*.csv` at runtime.
- **Field dispatch is via `fn` pointer table instead of `unique_ptr<IField>`
  vector.** Same observable behavior, no heap allocation.
- **CSV parsing is hand-rolled** (no `csv` crate). Data files are simple
  enough; matches C++ approach.
- **Errors via `Result` + `enum RepoError`** instead of
  `std::runtime_error`.
- **Per-row independent RNG.** C++ used a single shared `rand()` state
  across all rows; this port derives each row's seed from
  `master_seed + row_index * 0x9E37…` (golden ratio) so multi-threaded
  generation is deterministic. Side effect: snapshots are not directly
  comparable across the two ports.
- **Multi-core CSV generation via rayon (`-j N`).** Not present in the
  C++ version. Output is order-preserving and bit-identical to single
  mode for any given seed.
- **No `tool/convert-address.sh`** — data files are copied from the C++
  repo and not regenerated here.

## License

MIT — see [LICENSE](LICENSE).
