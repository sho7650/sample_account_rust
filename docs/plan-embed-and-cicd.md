# Plan — CSV embed + CI/CD pipeline

> Companion: [`design-embed-and-cicd.md`](./design-embed-and-cicd.md) (decisions / sources / alternatives)
> Issue: [#1](https://github.com/sho7650/sample_account_rust/issues/1)

This document is the **operational plan** — file-by-file changes, PR split,
order of execution, and the verification gates between phases. The design
doc covers *why*; this doc covers *what* and *when*.

---

## 1. PR split (rollout order)

The four phases must land in **two PRs**, in this order:

```
PR #A — feat: embed data + CI            (Phase 1 + Phase 2)
   │
   │  merge → main
   │
   ▼
PR #B — ci: release-please + binaries    (Phase 3 + Phase 4)
   │
   │  merge → main
   │
   ▼
release-please opens release PR  (auto)
   │
   │  merge → tag v0.6.0  (or whichever bumps from current)
   │
   ▼
release-binaries fires           (auto, builds 8 archives)
```

**Why two PRs and not one or four?**
- **PR #A** before #B: Phase 3 needs the version `0.5.0` baseline that Phase 1 establishes. Otherwise the first release-please PR would propose a bump from a stale version.
- **Phase 1 + 2 in one PR**: The CI Phase 2 introduces is what verifies Phase 1 builds and tests cleanly across OS. Splitting them means Phase 1 lands without OS coverage.
- **Phase 3 + 4 in one PR**: Phase 4's workflow file must already exist on `main` when the first release tag is created — otherwise the binary build never fires. Including both in #B guarantees that.
- Splitting Phase 4 into a third PR risks a window where release-please could create a tag with no asset workflow watching.

---

## 2. PR #A — feat: embed data + CI

### 2.1 File list

#### Modified

| File | Change |
|---|---|
| `Cargo.toml` | bump `version = "0.4.7"` → `"0.5.0"`; add `repository`, `homepage`, `keywords`, `categories`, `rust-version` metadata |
| `src/repos.rs` | extract `parse_persons<R: BufRead>` / `parse_prefectures` / `parse_ages`; add `default_persons()` / `default_prefectures()` / `default_ages()` using `include_str!`; existing `load_*` becomes thin wrapper calling `parse_*` |
| `src/main.rs` | replace `load_persons("data/sample_account.csv")?` etc. with `default_persons()?` etc. |
| `README.md` | remove "must be run from the repo root" caveat from Quick start; add note about embedded data; update Differences-from-C++ section |
| `CLAUDE.md` | reflect embed strategy in Architecture and Notable constraints sections |
| `tasks/todo.md` | add Phase 11/12 review entries on completion |

#### New

| File | Purpose |
|---|---|
| `tests/embedded.rs` | Integration test: spawn binary from `tempfile::TempDir` (no `data/` dir present); assert output is byte-identical to baseline run from repo root |
| `.gitattributes` | Force `eol=lf` for `*.rs *.csv *.toml *.md *.yml *.nix *.sh` to prevent Windows CI failures |
| `.github/workflows/ci.yml` | CI workflow per design §3 |

#### Touched indirectly

- `Cargo.lock` — auto-updates from `version` bump

### 2.2 Code skeleton (preview only — not yet implemented)

```rust
// src/repos.rs (additions)

pub fn default_persons() -> Result<Vec<PersonRecord>, RepoError> {
    parse_persons(io::Cursor::new(EMBEDDED_PERSONS_CSV.as_bytes()),
                  "<embedded:sample_account.csv>")
}
pub fn default_prefectures() -> Result<PrefectureRepo, RepoError> {
    parse_prefectures(
        io::Cursor::new(EMBEDDED_PREFECTURES_CSV.as_bytes()),
        io::Cursor::new(EMBEDDED_ADDRESS_CSV.as_bytes()),
        "<embedded:prefectures.csv>",
        "<embedded:address.csv>",
    )
}
pub fn default_ages() -> Result<AgeRepo, RepoError> {
    parse_ages(io::Cursor::new(EMBEDDED_AGES_CSV.as_bytes()),
               "<embedded:ages.csv>")
}

const EMBEDDED_PERSONS_CSV:     &str = include_str!("../data/sample_account.csv");
const EMBEDDED_PREFECTURES_CSV: &str = include_str!("../data/prefectures.csv");
const EMBEDDED_ADDRESS_CSV:     &str = include_str!("../data/address.csv");
const EMBEDDED_AGES_CSV:        &str = include_str!("../data/ages.csv");
```

```rust
// src/main.rs change
let persons   = repos::default_persons()?;
let pref_repo = repos::default_prefectures()?;
let age_repo  = repos::default_ages()?;
```

### 2.3 CI workflow skeleton

```yaml
name: CI
on:
  push:         { branches: [main] }
  pull_request:

permissions:
  contents: read

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}

jobs:
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: clippy, rustfmt }
      - uses: Swatinem/rust-cache@v2
        with:
          save-if: ${{ github.ref == 'refs/heads/main' }}
      - run: cargo fmt --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo build --release
      - run: cargo test --release
```

### 2.4 Validation gates for PR #A

Before merge, all of the following must hold:

- [ ] `cargo build` succeeds locally and binary size is between 5 MB and 10 MB
- [ ] `cd /tmp && /abs/path/sample_account -ilfm 3` outputs 3 valid rows
- [ ] `cargo install --path .` then `cd /tmp && sample_account -ilfm 3` succeeds
- [ ] `cargo test --release` — 50+ tests pass (existing 54 + new `tests/embedded.rs`)
- [ ] All 4 existing snapshot CSVs match (`cargo test --release --test snapshot`)
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] `cargo fmt --check` clean
- [ ] CI green on all 3 OS (Linux / macOS / Windows)
- [ ] Conventional commit message: `feat: embed CSV data and add CI workflow`

### 2.5 Commit message

```
feat: embed CSV data and add CI workflow

Closes #1.
- Bake data/*.csv into the binary via include_str! so the executable
  runs from any working directory.
- Keep load_*(path) APIs intact for tests/repos.rs to verify the
  parser; main now uses default_*() (embedded path).
- Add tests/embedded.rs that runs the binary from a tempdir without
  data/ next to it, verifying byte-identical output.
- Add .github/workflows/ci.yml: build/test/clippy/fmt on Linux,
  macOS, Windows.
- Add .gitattributes to enforce LF line endings (prevents CRLF
  corrupting CSV/snapshot/test files on Windows).
- Bump version 0.4.7 → 0.5.0 (minor; feat with bump-minor-pre-major).
- Cargo.toml gains repository/keywords/categories/rust-version
  metadata for future crates.io publication and SemVer tooling.
```

---

## 3. PR #B — ci: release-please + release-binaries (3 native targets)

### 3.1 File list

#### New

| File | Purpose |
|---|---|
| `release-please-config.json` | Release-please rust release-type config (per design §4.2) |
| `.release-please-manifest.json` | `{ ".": "0.5.0" }` baseline |
| `.github/workflows/release-please.yml` | Workflow per design §4.3 |
| `.github/workflows/release.yml` | Combined workflow per design §5.1 / §5.4 (release-please + 3-target binaries chained via `needs:`). Replaces the v1 split into release-please.yml + release-binaries.yml, which was broken because GITHUB_TOKEN-created releases don't trigger downstream workflows. |

#### Modified

| File | Change |
|---|---|
| `README.md` | add "Installation" section pointing at GitHub Releases; show how to grab pre-built binary by target |
| `CLAUDE.md` | brief note on conventional-commits → release-please flow; instruct future Claude to use `feat:` / `fix:` prefixes |

### 3.2 Validation gates for PR #B

Pre-merge:
- [ ] YAML lint passes (`actionlint` if installed; or visual review)
- [ ] CI on PR branch is green (CI workflow from PR #A still passes — release-please / release-binaries don't run on PRs, only on push to main / release events, so they won't gate the PR)
- [ ] Conventional commit: `ci: add release-please and release-binaries workflows`

Post-merge (validates the workflows actually fire):
- [ ] Within 1-2 min of merging PR #B, `release-please` workflow appears in Actions tab and succeeds
- [ ] A "release PR" titled like `chore(main): release 0.5.1` (or similar) is opened by `github-actions[bot]`
- [ ] That PR contains: `Cargo.toml` version bump, `Cargo.lock` regeneration, `CHANGELOG.md` (newly created)

End-to-end (validates the full release pipeline):
1. **Merge the release PR** that release-please opened
2. Within ~30s, a tag `vX.Y.Z` and a GitHub Release appear
3. Within ~30s, `release-binaries` workflow fires with 8 matrix jobs
4. After ~5-10 min, all 8 jobs complete and the Release page lists 16 assets (8 archives + 8 .sha256)
5. **Manual smoke test**: download one Linux GNU + one Windows MSVC archive, extract, run `./sample_account -ilfm 3` — must succeed

### 3.3 Commit message

```
ci: add release-please and release-binaries workflows

- Add release-please-action v4 workflow on push:main, release-type rust.
  Manifest mode with bump-minor-pre-major for 0.x.y SemVer.
- Add release-binaries workflow triggered by release: types: [created].
  Builds and uploads archives for 3 native targets via taiki-e/upload-
  rust-binary-action@v1.
- Targets: x86_64-unknown-linux-gnu (ubuntu-latest),
  x86_64-pc-windows-msvc (windows-latest), aarch64-apple-darwin
  (macos-latest). All native, no cross. Archives include LICENSE +
  README; sha256 checksums published alongside.
- Update README with Installation section linking to Releases.
```

---

## 4. Risk register and mitigations

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R1 | `include_str!` paths are relative to the source file; running `cargo build` from anywhere other than the crate root might break | LOW | HIGH | Cargo always invokes rustc with `CARGO_MANIFEST_DIR` set; `include_str!("../data/x.csv")` is reliably relative to `src/repos.rs`. Verified by [Rust Reference — include_str](https://doc.rust-lang.org/std/macro.include_str.html). |
| R2 | Binary size jumps from ~1.8 MB to ~6.6 MB and surprises users | LOW | LOW | Documented in README + design doc. ~6 MB is small relative to comparable tools. |
| R3 | Windows CI fails due to CRLF in checked-in CSVs | MEDIUM | MEDIUM | `.gitattributes` forces `eol=lf` for `*.csv` and other text files. PR #A includes this file. |
| R4 | First release-please PR doesn't bump from `0.5.0` (because no `feat:`/`fix:` commits between PR #A and PR #B if they happen back-to-back without intermediate commits) | MEDIUM | LOW | Acceptable: PR #B itself is a `ci:` (no bump). The first bump happens whenever the next `feat:` or `fix:` lands. If we want a release immediately, we can add `release-as: 0.5.0` once to release-please-config to force-create v0.5.0. |
| R5 | `release-please` doesn't update `Cargo.lock` and CI fails on lock-out-of-sync | LOW | MEDIUM | Per release-please docs, `release-type: rust` regenerates Cargo.lock. If it doesn't, add `extra-files: ["Cargo.lock"]` to config. |
| R6 | macOS Gatekeeper blocks unsigned binaries | LOW (downloaders are aware) | LOW | Document in README: `xattr -d com.apple.quarantine sample_account` after download. Code signing deferred per design §5.6. |
| R7 | Workflow concurrency races (two release-please runs) | LOW | LOW | `concurrency: cancel-in-progress: false` on release-please ensures sequential processing. |
| R8 | Conventional-commits enforcement (a non-conventional commit lands on main and release-please skips it silently) | MEDIUM | LOW | Acceptable for v1: release-please simply doesn't bump. Could add `commitlint` later if drift becomes a problem. |
| R9 | Cargo.toml `rust-version = "1.75"` becomes false because we add a feature requiring newer Rust | LOW | LOW | CI runs on stable, so any Rust 1.75+ usage that fails compile would surface immediately. |

---

## 5. Validation checklist (final, end-to-end)

After both PRs are merged and the first release pipeline has run:

| Check | Expected | How to verify |
|---|---|---|
| Issue #1 closed | `gh issue view 1` shows status closed via PR #A reference | Manual |
| Binary works from any cwd | `cd /tmp && /path/to/sample_account -ilfm 3` succeeds | Manual / `tests/embedded.rs` |
| `cargo install --path .` works | Fresh shell: `sample_account -ilfm 3` succeeds | Manual |
| CI green on Linux/macOS/Windows for every PR | Actions tab shows 3 successful matrix jobs | Manual |
| release-please opens a release PR for `feat:`/`fix:` commits | Actions tab + PR list | Manual |
| Tags are created with format `v0.6.0` (no component prefix) | `git fetch --tags && git tag -l` | Manual |
| Each Release has 6 assets (3 archives + 3 checksums) | `gh release view <tag>` | Manual |
| Downloaded Linux gnu binary runs on a clean Ubuntu | Optional smoke test in a Docker `ubuntu:22.04` | Manual |
| Snapshot bytes unchanged across both PRs | `cargo test --release --test snapshot` | Automated by CI |

---

## 6. Estimated timing

| Step | Wall time |
|---|---|
| PR #A: code + tests + CI yaml | ~45 min implementation |
| PR #A: review + merge | depends on reviewer |
| PR #A: post-merge CI run | ~5 min |
| PR #B: yaml only (3 files) | ~20 min implementation |
| PR #B: review + merge | depends on reviewer |
| First release-please PR appearance | ~1-2 min after merge |
| Merging release PR + tag creation + 8 binary builds | ~5-10 min |
| **Total for first end-to-end release** | **~1.5-2 hours** including review |

---

## 7. Out-of-scope (will not do in this work)

- crates.io publication via `cargo publish` (requires registry token; deferred until we want library users)
- Code signing for macOS / Windows binaries (cost + key management; deferred per design §5.6)
- Docker / container image publishing
- Homebrew / scoop / winget packaging
- `cargo dist` (alternative to taiki-e for binary distribution; could revisit if taiki-e setup grows)
- SBOM / build provenance attestation
- Reproducible builds with pinned `SOURCE_DATE_EPOCH`
- crates.io badge or download counter
- Automated dependency updates (Dependabot / Renovate) — could add later
