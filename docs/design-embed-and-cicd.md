# Design — CSV embed + CI/CD pipeline

> Issue: [#1 Make data loading independent of current working directory](https://github.com/sho7650/sample_account_rust/issues/1)
> Companion: [`plan-embed-and-cicd.md`](./plan-embed-and-cicd.md) (rollout / file list / validation)

This document records the **design decisions**, the **alternatives considered**, and the **primary sources** that justified each pick. Every external claim links to an upstream doc (action README, RFC, or vendor reference) so future readers can reproduce the reasoning.

---

## 1. Goals

| # | Goal | Measurable acceptance |
|---|---|---|
| G1 | The binary runs from any working directory (`/tmp`, `~`, etc.) without `data/` next to it | `cd /tmp && /abs/path/sample_account -ilfm 3` produces 3 valid rows |
| G2 | `cargo install --path .` followed by `sample_account` from a fresh shell works | Single command install succeeds, no `cd` needed |
| G3 | CI runs `build` / `test` / `clippy` / `fmt` on Linux + macOS + Windows for every PR and main push | 3 green jobs on PR within 5 min |
| G4 | `release-please` automates version bumps + CHANGELOG + GitHub Release based on conventional commits | Merging a `feat:` PR creates a release PR; merging that PR creates a tag + Release |
| G5 | Each new tag publishes prebuilt binaries for 3 native targets (Linux x86_64-gnu, Windows x86_64-MSVC, macOS aarch64) | All 3 archives appear under "Assets" of the GitHub Release |
| G6 | Output remains byte-identical to the current implementation given the same `SAMPLE_ACCOUNT_SEED` / `SAMPLE_ACCOUNT_NOW` | Existing `tests/expected/*.csv` snapshots pass unchanged |

---

## 2. Phase 1 — CSV embedding

### 2.1 Decision: `include_str!` baked into the binary

The four CSV files in `data/` (~4.8 MB total: 4.2 MB address.csv + 552 kB sample_account.csv + 4 kB each for prefectures/ages) are inlined as `&'static str` constants at compile time using the `include_str!` macro.

**Source**: [Rust Reference — `include_str` macro](https://doc.rust-lang.org/std/macro.include_str.html) — "Includes a UTF-8 encoded file as a string. The file is located relative to the current file… utf8-checked at compile time."

### 2.2 Why not the alternatives?

| Alternative | Why rejected |
|---|---|
| **Multi-path search** (env var → exe-relative → cwd) | More code, install-layout fragile, still file I/O at startup. Doesn't satisfy G2 cleanly because user has to copy data files into the install dir or set an env var. |
| **Hybrid (embed + env override)** | Adds ~30 lines for a use case with no concrete demand. We already have `SAMPLE_ACCOUNT_SEED` / `SAMPLE_ACCOUNT_NOW` as the existing escape hatches; adding a third would dilute clarity. Can be added later if a real need arises. |
| **`rust-embed` crate** | Adds a dependency (`rust-embed`) for a feature that std `include_str!` already gives us. The crate's value is iterating multiple files via a derive macro — overkill for 4 named files. |
| **Re-distribute data alongside binary** (e.g. `~/.local/share/sample_account/`) | XDG-correct but mismatches "single self-contained binary" goal. Installer scripts would have to ship CSVs too. |

### 2.3 API shape

`src/repos.rs` keeps two parallel APIs:

```rust
// Existing — kept for tests/repos.rs (file-format coverage) and future
// callers who want to swap data sources.
pub fn load_persons<P: AsRef<Path>>(path: P) -> Result<Vec<PersonRecord>, RepoError>;
pub fn load_prefectures<P: AsRef<Path>, Q: AsRef<Path>>(p: P, a: Q) -> Result<PrefectureRepo, RepoError>;
pub fn load_ages<P: AsRef<Path>>(path: P) -> Result<AgeRepo, RepoError>;

// New — what main() uses. No file I/O, no path resolution.
pub fn default_persons()     -> Result<Vec<PersonRecord>, RepoError>;
pub fn default_prefectures() -> Result<PrefectureRepo, RepoError>;
pub fn default_ages()        -> Result<AgeRepo, RepoError>;
```

Internally each `load_*` and `default_*` shares a private `parse_*<R: BufRead>(reader, source_label)` helper. `default_*` wraps `include_str!`'d bytes in `io::Cursor` and passes a label like `"<embedded:sample_account.csv>"` for error reporting.

**Source**: This mirrors the Go reference implementation at `/Volumes/dev/src/golang/work/sample_account/internal/repo/`, which exposes both `Default*()` (embedded) and `Load*FromFile()`.

### 2.4 Binary size impact

| Configuration | `release` binary size (estimated) |
|---|---|
| Current (no embed) | ~1.8 MB |
| With embed (4.8 MB CSV) | ~6.6 MB |

The address.csv (4.2 MB) dominates. UTF-8 strings compress well in MachO/ELF rodata sections so actual file growth is close to raw CSV size. Acceptable trade-off; comparable to many CLI tools (`gh` is ~30 MB, `rg` is ~6 MB).

### 2.5 Test impact

- `tests/repos.rs` — keeps using `load_*("data/...")` to exercise the file-parser path. **Unchanged**.
- `tests/snapshot.rs` — runs the binary, which now uses `default_*`. Snapshot bytes are identical because the parser is the same and the CSV bytes are identical. **Unchanged**.
- New `tests/embedded.rs` — spawns the binary from a working directory **without `data/`** to prove G1/G2. Uses `tempfile::TempDir` or just `std::env::set_current_dir` in the test process.

### 2.6 Version bump

`feat:` commit per Conventional Commits → minor bump per [SemVer](https://semver.org/#spec-item-7) for 0.x.y.

`Cargo.toml: version = "0.4.7"` → `0.5.0`. release-please will then track from this baseline.

---

## 3. Phase 2 — GitHub Actions CI

### 3.1 Workflow shape

Single workflow `.github/workflows/ci.yml`. Triggers:
- `push` on `main` (post-merge sanity)
- `pull_request` (pre-merge gate)

Job matrix: `os × {ubuntu-latest, macos-latest, windows-latest}` × stable Rust toolchain.

**Source**: [GitHub Actions — using a matrix](https://docs.github.com/en/actions/using-jobs/using-a-matrix-for-your-jobs).

### 3.2 Steps per job

```yaml
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

**Sources**:
- `dtolnay/rust-toolchain@stable` — current best-practice replacement for the deprecated `actions-rs/toolchain`. ([dtolnay/rust-toolchain README](https://github.com/dtolnay/rust-toolchain))
- `Swatinem/rust-cache@v2` — caches cargo registry and target dir; `save-if` limits cache writes to `main` per their official guidance ([context7 `/swatinem/rust-cache`](https://context7.com/swatinem/rust-cache/llms.txt) → "Conditional Cache Saving Strategy"). Reduces cache pollution from PR branches.

### 3.3 Why not nix-based CI?

The repo has `flake.nix` for local dev, but adding `cachix/install-nix-action` to CI doubles setup time (~2 min per job × 3 jobs) and obscures error messages with a Nix layer. Direct `dtolnay/rust-toolchain@stable` is faster and provides clearer signal for the OS-specific bugs CI is meant to catch.

### 3.4 Windows line-ending guard

Per [Git documentation on `core.autocrlf`](https://git-scm.com/book/en/v2/Customizing-Git-Git-Configuration#_core_autocrlf), Windows checkouts may default to CRLF, which would corrupt:
- `tests/expected/*.csv` (snapshot diff fails)
- `data/*.csv` (CSV parser sees `\r` at line ends)
- `*.rs` (occasional rustfmt false positives)

**Mitigation**: Add `.gitattributes`:
```
* text=auto eol=lf
*.rs text eol=lf
*.csv text eol=lf
*.toml text eol=lf
*.md text eol=lf
*.yml text eol=lf
*.nix text eol=lf
*.sh text eol=lf
```

**Source**: [Git Pro — `.gitattributes`](https://git-scm.com/docs/gitattributes#_end_of_line_conversion).

---

## 4. Phase 3 — release-please

### 4.1 Why release-please over alternatives

| Tool | Source | Why preferred / rejected |
|---|---|---|
| **release-please-action v4** | [googleapis/release-please-action](https://github.com/googleapis/release-please-action) | **Picked.** GitHub-native, supports `release-type: rust`, manifest mode for future multi-crate, only requires `GITHUB_TOKEN`. Per context7 `/googleapis/release-please-action/llms.txt`, `release-type: rust` updates `Cargo.toml + CHANGELOG.md`. |
| `release-plz` | [release-plz/release-plz](https://github.com/release-plz/release-plz) | Rust-specific, supports cargo registry publish. **Rejected for v1** because it requires a `cargo` registry workflow we don't currently target (we publish binaries, not crates). Can revisit if/when we publish to crates.io. |
| `cargo-release` | [crate-ci/cargo-release](https://github.com/crate-ci/cargo-release) | Local CLI tool; doesn't run in CI by itself. **Rejected** — needs a human to invoke. |
| `semantic-release` | [semantic-release/semantic-release](https://github.com/semantic-release/semantic-release) | Node.js-based, JS ecosystem-first. **Rejected** — adds Node toolchain dependency to CI. |

### 4.2 Configuration files

`.release-please-manifest.json`:
```json
{ ".": "0.5.0" }
```
The single `.` key means root-package mode. Initial value matches the Phase 1 bump.

`release-please-config.json`:
```json
{
  "release-type": "rust",
  "packages": {
    ".": {
      "release-type": "rust",
      "package-name": "sample_account",
      "include-component-in-tag": false,
      "draft": false,
      "prerelease": false
    }
  },
  "bump-minor-pre-major": true,
  "bump-patch-for-minor-pre-major": false,
  "$schema": "https://raw.githubusercontent.com/googleapis/release-please/main/schemas/config.json"
}
```

**Notes**:
- `bump-minor-pre-major: true` ensures `feat:` triggers minor (0.5.0 → 0.6.0) instead of major (0 → 1.0.0) while we're still on `0.x`. Per [release-please bumping section](https://github.com/googleapis/release-please/blob/main/docs/customizing.md#bump-minor-pre-major), this is the recommended setting for libraries pre-1.0.
- `include-component-in-tag: false` keeps tags as `v0.6.0` instead of `sample_account-v0.6.0`.

### 4.3 Workflow

`.github/workflows/release-please.yml`:
```yaml
name: release-please
on:
  push:
    branches: [main]
permissions:
  contents:      write
  issues:        write
  pull-requests: write
jobs:
  release-please:
    runs-on: ubuntu-latest
    steps:
      - uses: googleapis/release-please-action@v4
        with:
          config-file:   release-please-config.json
          manifest-file: .release-please-manifest.json
```

**Source**: Permissions block exactly matches [release-please-action README "Configure GitHub Actions Workflow Permissions"](https://github.com/googleapis/release-please-action#permissions).

### 4.4 Token strategy

Default `${{ github.token }}` (= `secrets.GITHUB_TOKEN`) is used. **No PAT** required because:
- The action only needs `contents/issues/pull-requests` write — all granted by `permissions:` block.
- The asset-upload job runs in the **same workflow** (chained via `needs:` against release-please's `release_created` output), so the
  GITHUB_TOKEN-cannot-trigger-downstream-workflows restriction is sidestepped — see §5.1.

> **Correction (post-v0.6.0).** An earlier version of this document
> claimed that a separate `on: release: types: [created]` workflow would
> fire when release-please-action creates a release via GITHUB_TOKEN.
> **That was wrong.** Per the release-please-action README:
> _"When you use the repository's GITHUB_TOKEN to perform tasks, events
> triggered by the GITHUB_TOKEN will not create a new workflow run."_
> Neither `release: created` NOR `push: tags` fires under those
> conditions. Tag v0.6.0 was published with no assets because of this
> bug; fixed in PR by combining the two workflows into one (§5.1).

### 4.5 What release-please modifies

Per `release-type: rust`:
- `Cargo.toml` — bumps `version = "..."`
- `Cargo.lock` — auto-regenerated by cargo and committed (`Cargo.toml` change forces lockfile bump)
- `CHANGELOG.md` — created on first run, appended on subsequent releases. Format per [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) / Conventional Commits parser.

### 4.6 Conventional Commits cheat sheet

| Commit prefix | Bump (pre-1.0) | Bump (post-1.0) |
|---|---|---|
| `fix:` | patch | patch |
| `feat:` | **minor** (with `bump-minor-pre-major`) | minor |
| `feat!:` or `BREAKING CHANGE:` footer | minor (per `bump-minor-pre-major`) | major |
| `docs:` / `chore:` / `ci:` / `test:` | none | none |

**Source**: [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/) — "fix" → patch, "feat" → minor, "BREAKING CHANGE" → major.

---

## 5. Phase 4 — Cross-platform release binaries

### 5.1 Trigger — combined into one workflow (was: separate)

The asset upload job lives in the **same** `.github/workflows/release.yml` file as the release-please job. Two jobs, one workflow run:

```yaml
on:
  push:    { branches: [main] }     # normal release-please flow
  workflow_dispatch:                # manual backfill (see below)
    inputs:
      tag: { required: false, default: "" }

jobs:
  release-please: { ... outputs release_created, tag_name ... }

  upload-assets:
    needs: release-please
    if: |
      always() &&
      (
        (github.event_name == 'push' && needs.release-please.outputs.release_created == 'true') ||
        (github.event_name == 'workflow_dispatch' && inputs.tag != '')
      )
    strategy:
      matrix: ...3 native targets...
```

**Why combined?** Per the release-please-action README:
> "When you use the repository's GITHUB_TOKEN to perform tasks, events triggered by the GITHUB_TOKEN will not create a new workflow run."

A standalone `on: release: types: [created]` workflow would never fire from a release-please-created release (which uses GITHUB_TOKEN by default). The fix is either:
1. **(picked)** Chain via `needs:` in the same workflow run — no PAT needed, no recursion concern.
2. (rejected) Issue a Personal Access Token, store as a secret, pass to release-please as `token`. Adds secret-management overhead.

**Manual backfill via workflow_dispatch.** When a release was already created without assets (e.g. v0.6.0 cut before this fix), `gh workflow run release.yml -f tag=v0.6.0` re-runs the asset upload against an existing tag. The release-please job is skipped (`if: github.event_name == 'push'`), upload-assets runs in dispatch mode using `inputs.tag`.

### 5.2 Target matrix

**3 native targets, no cross-compilation.** Per user decision, the matrix is intentionally minimal: one runner per OS, only natively-compiled targets. This keeps the release pipeline trivial and removes the `cross` toolchain dependency entirely.

| # | Target | Runner | Tool | Rationale |
|---|---|---|---|---|
| 1 | `x86_64-unknown-linux-gnu` | `ubuntu-latest` | cargo (native) | Standard Linux distros (Debian/Ubuntu/Fedora/RHEL/etc.) — covers the vast majority of Linux users |
| 5 | `x86_64-pc-windows-msvc` | `windows-latest` | cargo (native) | Standard Windows binary, MSVC toolchain (default Rust on Windows) |
| 8 | `aarch64-apple-darwin` | `macos-latest` | cargo (native) | Apple Silicon — `macos-latest` is arm64 since macos-14 per [taiki-e README note](https://github.com/taiki-e/upload-rust-binary-action#inputs) |

(Numbering preserved from the v2 design draft for traceability. Targets 2/3/4/6/7 from that draft were dropped per user decision.)

### 5.3 Why the minimal matrix

| Concern | Decision rationale |
|---|---|
| Coverage of the user base | The 3 picked targets cover ~95% of likely consumers: Linux (x86_64 server / WSL), modern Mac (Apple Silicon), Windows (MSVC). Users on niche platforms (Alpine/musl, ARM Linux, Intel Mac, MinGW) can build from source — the embedded data + pure-Rust deps make that trivial. |
| Avoid `cross` complexity | `cross` requires Docker on the runner and adds 3-5 min per cross target. Skipping it removes a toolchain failure mode. |
| GitHub Actions minutes | 3 jobs × ~3 min = ~10 min/release. Negligible. |
| Future expansion | Adding more targets later is one matrix entry per target. The architecture (taiki-e action) supports it without restructuring. |

**Trade-off accepted**: Intel Mac users (`x86_64-apple-darwin`) and ARM Linux users (`aarch64-unknown-linux-gnu`) must `cargo install --git ...` from source. Documented in README.

### 5.4 Workflow shape — `.github/workflows/release.yml`

The combined workflow (see §5.1 for trigger logic). Both jobs are in
this single file; the `release-binaries.yml` from the v1 design has
been removed.

```yaml
name: release
on:
  push:    { branches: [main] }
  workflow_dispatch:
    inputs:
      tag: { description: "...", required: false, default: "" }

permissions:
  contents:      write
  issues:        write
  pull-requests: write

jobs:
  release-please:
    if: github.event_name == 'push'
    runs-on: ubuntu-latest
    outputs:
      release_created: ${{ steps.release.outputs.release_created }}
      tag_name:        ${{ steps.release.outputs.tag_name }}
    steps:
      - uses: googleapis/release-please-action@v4
        id: release
        with:
          config-file:   release-please-config.json
          manifest-file: .release-please-manifest.json

  upload-assets:
    needs: release-please
    if: |
      always() &&
      (
        (github.event_name == 'push' && needs.release-please.outputs.release_created == 'true') ||
        (github.event_name == 'workflow_dispatch' && inputs.tag != '')
      )
    permissions:
      contents: write
    strategy:
      fail-fast: false
      matrix:
        include:
          - { target: x86_64-unknown-linux-gnu, os: ubuntu-latest  }
          - { target: x86_64-pc-windows-msvc,   os: windows-latest }
          - { target: aarch64-apple-darwin,     os: macos-latest   }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
        with:
          ref: ${{ inputs.tag != '' && inputs.tag || needs.release-please.outputs.tag_name }}
      - uses: taiki-e/upload-rust-binary-action@v1
        with:
          bin:      sample_account
          target:   ${{ matrix.target }}
          archive:  $bin-$tag-$target
          tar:      unix
          zip:      windows
          include:  LICENSE,README.md
          checksum: sha256
          ref:      refs/tags/${{ inputs.tag != '' && inputs.tag || needs.release-please.outputs.tag_name }}
          token:    ${{ secrets.GITHUB_TOKEN }}
```

**Sources**: Inputs verified against [taiki-e action.yml](https://github.com/taiki-e/upload-rust-binary-action/blob/main/action.yml). The `ref: refs/tags/...` input uploads to a specific tag — see [taiki-e README "Supported events"](https://github.com/taiki-e/upload-rust-binary-action#supported-events) ("You can upload binaries from arbitrary event to arbitrary tag by specifying the `ref` input option"). All 3 targets are native to their respective runners → no cross-compilation, no extra toolchain setup.

### 5.5 Asset naming

Per taiki-e action README, `archive: $bin-$tag-$target` with `tar/zip` produces:

```
sample_account-v0.6.0-x86_64-unknown-linux-gnu.tar.gz
sample_account-v0.6.0-x86_64-unknown-linux-gnu.tar.gz.sha256
sample_account-v0.6.0-x86_64-pc-windows-msvc.zip
sample_account-v0.6.0-x86_64-pc-windows-msvc.zip.sha256
sample_account-v0.6.0-aarch64-apple-darwin.tar.gz
sample_account-v0.6.0-aarch64-apple-darwin.tar.gz.sha256
```

3 archives + 3 checksums = **6 release assets total**.

### 5.6 Out-of-scope (deferred)

| Topic | Reason for deferral |
|---|---|
| macOS code signing | Requires Apple Developer Program ($99/year) + signing identity in repo secrets. User explicitly accepted in plan questions. |
| Windows code signing | Requires EV certificate (~$300/year). Deferred. |
| Additional targets (Linux musl, Linux aarch64, Intel macOS, Windows MinGW) | Per user decision in re-plan: keep matrix to native-only for v1. Niche-platform users build from source via `cargo install --git`. Adding more targets later is a one-line matrix entry per target. |
| `universal-apple-darwin` (fat binary x86_64+arm64) | Apple Silicon users covered by `aarch64-apple-darwin`; Intel Mac users build from source. |
| Reproducible builds (`SOURCE_DATE_EPOCH`) | Out of scope for v1 |
| SBOM / cargo-auditable / supply-chain attestation | Could add `actions/attest-build-provenance` later. Deferred. |

---

## 6. Cross-cutting concerns

### 6.1 Security

- **Pin actions by major version (`@v4`, `@v2`, `@v1`)** following the pattern documented in [taiki-e/upload-rust-binary-action — Security](https://github.com/taiki-e/upload-rust-binary-action#security): "The `@v<major>` tags are updated with each release. If you want to enhance workflow stability and security against supply chain attacks, consider using the `@v<major>.<minor>.<patch>` tag or their hash to pin the version."
- For v1 we accept major-version pinning as the trade-off between security and update friction. Can tighten to SHA pinning in a follow-up if needed.

### 6.2 Concurrency

Add to each workflow:
```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
```
Cancels stale PR runs when new commits land; lets push-on-main complete. **Source**: [GitHub docs — Using concurrency](https://docs.github.com/en/actions/using-jobs/using-concurrency).

### 6.3 Cargo.lock policy

- **Committed** (already is). Required for reproducible binary builds in Phase 4.
- release-please updates it automatically per `release-type: rust` semantics.

### 6.4 Cargo.toml metadata for SemVer + future crates.io publish

Add to `[package]`:
```toml
repository    = "https://github.com/sho7650/sample_account_rust"
homepage      = "https://github.com/sho7650/sample_account_rust"
keywords      = ["csv", "synthetic-data", "japan", "data-generator"]
categories    = ["command-line-utilities"]
rust-version  = "1.75"   # MSRV; conservatively older than dev-shell's 1.95
```

`rust-version` enables [`cargo` MSRV-aware resolver](https://doc.rust-lang.org/cargo/reference/manifest.html#the-rust-version-field). 1.75 covers all features we use (`partition_point`, edition 2021).

---

## 7. Summary of source inventory

| Source | Used for |
|---|---|
| [release-please-action README](https://github.com/googleapis/release-please-action) + context7 `/googleapis/release-please-action` | Phase 3 workflow + permissions |
| [release-please customizing](https://github.com/googleapis/release-please/blob/main/docs/customizing.md) | `bump-minor-pre-major` decision |
| [Swatinem/rust-cache README](https://github.com/Swatinem/rust-cache) + context7 `/swatinem/rust-cache` | Phase 2 cache config + `save-if` strategy |
| [taiki-e/upload-rust-binary-action README](https://github.com/taiki-e/upload-rust-binary-action) | Phase 4 workflow + asset naming |
| [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/) | Commit message conventions |
| [SemVer 2.0.0](https://semver.org/) | Version bump rules |
| [Rust Reference — `include_str`](https://doc.rust-lang.org/std/macro.include_str.html) | Phase 1 mechanism |
| [Git Pro — `.gitattributes`](https://git-scm.com/docs/gitattributes) | Line-ending normalization |
| [GitHub docs — concurrency](https://docs.github.com/en/actions/using-jobs/using-concurrency) | Workflow concurrency |
| [GitHub docs — `GITHUB_TOKEN` event behavior](https://docs.github.com/en/actions/security-guides/automatic-token-authentication#using-the-github_token-in-a-workflow) | Why `release: created` works but `push: tags` doesn't |
