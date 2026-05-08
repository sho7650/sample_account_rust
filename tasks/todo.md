# sample_account: C++ → Rust 移植 TODO

> Source repo: `/Volumes/dev/src/cpp/work/sample_account/`
> Target repo: `/Volumes/dev/src/rust/work/sample_account/` (this directory)
>
> プランニングドキュメント:
> - [PRD](../docs/PRD.md)
> - [Architecture](../docs/architecture.md)
> - [System Design](../docs/system_design.md)
> - [Tech Doc](../docs/tech_doc.md)

## 確定済み設計判断

| 項目 | 決定 |
|---|---|
| RNG 互換性 | スナップショット再生成。Rust `rand` crate `SmallRng` を使用 |
| CLI パーサ | 手書き、出現順=列順、`--telehpne` typo alias 維持 |
| 配置 | この cwd に Cargo bin プロジェクト |
| Field 設計 | 関数ポインタ table (`fn(&mut String, &RowContext, &Deps)`) |
| Nix | flake.nix + rust-overlay (stable + clippy + rustfmt + rust-analyzer) |
| direnv | `.envrc` に `use flake` |
| 追加 crate | `rand = "0.9"`, `time = "0.3"` のみ |
| Edition | 2021 |

---

## Phase 0 — Nix flake + dev shell

- [ ] `flake.nix` (rust-overlay input、`rust-bin.stable.latest.default.override { extensions = [...] }`)
- [ ] `flake.lock` (`nix flake lock` で生成)
- [ ] `.envrc` に `use flake`
- [ ] `.gitignore` に `/target /.direnv /result Cargo.lock` ※bin なので Cargo.lock はコミット → `Cargo.lock` は除外しない
- [ ] `nix develop` で起動確認（`cargo --version` / `rustc --version` が通る）

## Phase 1 — Cargo スキャフォールド

- [ ] `cargo init --bin --name sample_account .` を dev shell 内で実行
- [ ] `Cargo.toml`: edition 2021、`rand = "0.9"`, `time = "0.3"`
- [ ] `data/` を C++ リポからコピー (`sample_account.csv`, `prefectures.csv`, `address.csv`, `ages.csv`)
- [ ] `cargo build` で空テンプレがビルド成功することを確認

## Phase 2 — Repository 層 (TDD)

- [ ] `tests/repos.rs` に5アサーション (RED)
  - [ ] `person_repo_loads_records`
  - [ ] `prefecture_repo_loads_47_prefectures`
  - [ ] `prefecture_repo_assigns_zips_to_each_prefecture`
  - [ ] `age_repo_strips_thousand_separators`
  - [ ] `person_repo_throws_on_missing_file`
- [ ] `cargo test --test repos` で全 FAIL を確認
- [ ] `src/repos.rs` 実装
  - [ ] `PersonRecord`, `PrefectureRecord`, `AddressRecord`, `AgeBucket` struct
  - [ ] `RepoError` enum (`io::Error`, `Parse`, `MissingField`)
  - [ ] `load_persons(path) -> Result<Vec<PersonRecord>, RepoError>`
  - [ ] `load_prefectures(pref_path, addr_path) -> Result<PrefectureRepo, RepoError>` (zip 集計込み)
  - [ ] `load_ages(path) -> Result<AgeRepo, RepoError>` (千の位区切り除去)
  - [ ] `parse_digits(&str) -> i32`
- [ ] `cargo test --test repos` で全 PASS

## Phase 3 — RNG ラッパ

- [ ] `src/rng.rs`
  - [ ] `Rng { inner: SmallRng, now: i64 }`
  - [ ] `Rng::new()` → `SAMPLE_ACCOUNT_SEED` env を読む、無ければ system time
  - [ ] `next_i32() -> i32` (`random_range(0..=i32::MAX)`)
  - [ ] `roll_date(&mut self)` (`SAMPLE_ACCOUNT_NOW` を上限に乱数 unix time)
  - [ ] `year() / month() / day() -> i32`
  - [ ] `current_time() -> i64` (env or `SystemTime::now()`)
- [ ] `cargo test --lib rng` で seed pin の決定性を確認 (同一seed→同一列)

## Phase 4 — Generators 層

- [ ] `src/generators.rs`
  - [ ] `PersonGenerator { records: Arc<Vec<PersonRecord>> }` または `&[PersonRecord]` 借用
  - [ ] `last_name(n) -> String`、`first_name(n) -> String` (`"<kanji>,<kana>"`)
  - [ ] `mail_address(first, last) -> String`
  - [ ] `gender(n) -> &str`、`blood_type(n) -> &str`
  - [ ] `AddressGenerator`
    - [ ] `weighted_prefecture_index(n) -> usize`
    - [ ] `prefecture_name(idx) -> &str`
    - [ ] `address_index(pref_idx, n) -> usize` (offset + zips % で正規化)
    - [ ] `ward(pref, n) -> &str`、`city(pref, n) -> &str`
  - [ ] `AgeAndDateGenerator`
    - [ ] `age(n) -> i32`、`age_group(n) -> i32`
    - [ ] `birth_year(n, now) -> i32`
    - [ ] `reward(n, &mut Rng) -> i32`
- [ ] `cargo test --lib generators` で代表値の単体テスト

## Phase 5 — Field table

- [ ] `src/field.rs`
  - [ ] `RowContext { row, first, last, pref, ward, city, age }`
  - [ ] `Deps<'a> { person, address, age_date, rng }`
- [ ] `src/registry.rs`
  - [ ] `pub struct Field { short, long, desc, emit }`
  - [ ] 17 個の `emit` free fn (id, lastname, firstname, mail, telephone, prefecture, ward, city, gender, blood, age, agegroup, birthyear, reward, date, random, quotient)
  - [ ] `pub const FIELDS: &[Field]` (C++ buildDefaultRegistry の順序を保つ)
  - [ ] `find_by_short(c) -> Option<&'static Field>`、`short_optstring() -> String`

## Phase 6 — CLI

- [ ] `src/cli.rs`
  - [ ] `pub struct CliArgs { selected_fields: Vec<&'static Field>, count: u32, help: bool, error: Option<String> }`
  - [ ] `parse_args(args: impl Iterator<Item=String>) -> CliArgs`
    - [ ] `-h` / `--help`
    - [ ] 短フラグ束 `-ilfm` → 1文字ずつ展開、出現順保持
    - [ ] 長フラグ `--lastname` 等、`--telehpne` (typo alias for telephone)
    - [ ] 不明フラグ → error
    - [ ] 位置引数の数値が COUNT、1個のみ尊重
    - [ ] 列フラグ無し → IdField 1個デフォルト
  - [ ] `print_help(out: &mut impl Write, prog: &str)` (registry から自動生成)
- [ ] `cargo test --lib cli` でパース系テスト ≥6 件

## Phase 7 — main + 行ループ

- [ ] `src/main.rs`
  - [ ] `fn main() -> ExitCode`
  - [ ] エラー: `eprintln!("{prog}: {msg}")` → exit 1 / 2
  - [ ] `--help` → stdout 出力 → exit 0
  - [ ] CSV ロード (3 リポジトリ)
  - [ ] 行ループ: `RowContext` 組立 → `rng.roll_date()` → 選択 fields の emit を `,` 区切り → `\n`
- [ ] 手動実行で C++ と同じ "らしい" 出力を確認 (`cargo run -- -ilfm 5`)

## Phase 8 — スナップショットテスト

- [ ] `tests/snapshot.rs`
  - [ ] 4 シナリオを `Command::new(env!("CARGO_BIN_EXE_sample_account"))` で実行
    - [ ] all-flags: `-ilfmatpwcgbdorynq 5`
    - [ ] ilfm: `-ilfm 5`
    - [ ] default: `3` (no flags)
    - [ ] long-aliases: `--telephone --agegroup --birthyear 4`
  - [ ] 各実行に `SAMPLE_ACCOUNT_SEED=42 SAMPLE_ACCOUNT_NOW=1700000000` を env 設定
  - [ ] stdout を `tests/expected/<name>.csv` と比較
- [ ] `tests/expected/` を **Rust 側の出力で初回ブートストラップ** 生成
- [ ] `cargo test --test snapshot` で全 PASS

## Phase 9 — ドキュメント

- [ ] `README.md`
  - [ ] 概要、Nix での `nix develop`、`cargo run`、`cargo test`
  - [ ] env vars (`SAMPLE_ACCOUNT_SEED`, `SAMPLE_ACCOUNT_NOW`)
  - [ ] data/ ファイル構造
  - [ ] 列追加手順 (Field 追加 + FIELDS slice 追記)
- [ ] `CLAUDE.md` (このリポを開いた将来の Claude 用)

---

## Review section

### 完了ステータス (2026-05-08)

| Phase | ステータス | サマリ |
|---|---|---|
| 0 — Nix flake + dev shell | DONE | `flake.nix` (rust-overlay stable 1.95.0 + clippy + rustfmt + rust-analyzer)、`.envrc` (use flake)、`.gitignore` |
| 1 — Cargo スキャフォールド | DONE | `Cargo.toml` v0.4.7 edition 2021、`rand 0.9` (small_rng feature)、`time 0.3`、`data/` 4ファイル |
| 2 — Repository 層 | DONE | `src/repos.rs` 333 行 + `tests/repos.rs` 5 件 + 内蔵 unit 4 件 |
| 3 — RNG ラッパ | DONE | `src/rng.rs`、env vars 対応、unit 4 件 |
| 4 — Generators | DONE | `src/generators.rs`、Person/Address/AgeAndDate、unit 7 件 |
| 5 — Field 関数ポインタ table | DONE | `src/registry.rs`、17 emit fn、`FIELDS` const slice、unit 6 件 |
| 6 — CLI 手書きパーサ | DONE | `src/cli.rs`、出現順=列順、`--telehpne` alias、unit 10 件 |
| 7 — main + 行ループ | DONE | `src/main.rs`、`ExitCode`、`BufWriter`、RNG 呼び出し順を C++ と一致 |
| 8 — スナップショット | DONE | `tests/snapshot.rs` 4 件、`tests/expected/*.csv` 4 ファイル ブートストラップ済み |
| 9 — README + CLAUDE.md | DONE | プロジェクト概要、Nix 利用法、列追加手順、テスト手順 |

### 検証

- `nix develop --command cargo build` — OK
- `nix develop --command cargo test --release` — 31 unit + 5 integration + 4 snapshot = **40 passed, 0 failed**
- `nix develop --command cargo clippy --all-targets -- -D warnings` — clean (1 修正済み: unnecessary_min_or_max)
- `nix develop --command cargo fmt --check` — clean
- 決定性検証: `SAMPLE_ACCOUNT_SEED=42 SAMPLE_ACCOUNT_NOW=1700000000` を 2 回実行して `diff` 空 — OK

### C++ 版からの主な変化

- 動的ディスパッチ (`unique_ptr<IField>` virtual call) → 関数ポインタ table。ヒープ確保ゼロ。
- `std::runtime_error` throw → `enum RepoError` + `Result`。
- `getopt_long` → 手書き 1パスループ。`--telehpne` typo は Field 検索の前にハードコード分岐で吸収。
- libc `rand()` → `rand::rngs::SmallRng` (xoshiro)。スナップショット非互換 (再生成済み)。
- `localtime` → `time::OffsetDateTime` (UTC)。`SAMPLE_ACCOUNT_NOW` 固定で完全決定的。

### 残作業 / 将来の改善案

- カバレッジ計測を `cargo llvm-cov` に統合 (現状は数値で 80%+ 担保せず、テスト数 40 件でカバー)
- CI (GitHub Actions / similar) は本リポでは未設定
- `tool/convert-address.sh` 相当の Rust 移植は範囲外、未対応

### 学んだこと / Lesson

- `rand 0.9` で `SmallRng` を使うには `features = ["small_rng"]` が必要 (rand 0.8 のときは default で入っていた)
- `cargo init --bin` は git も初期化するため、`nix develop` で flake を見せるには flake.nix / flake.lock を `git add` する必要がある (untracked のままだと `Path 'flake.nix' is not tracked by Git` で flake が拒否される)
- `address.csv` は city 欄が空のレコードあり (`01,北海道,旭川市` ← city 欄無し)。C++ `getline` は不足分を空文字列で埋めるので、Rust 側も同等の挙動 (`split_n` を「N 個以下なら残りは空文字」に変更) が必要

---

## Phase 11 — マルチコア CPU 対応 (2026-05-08 追加分)

### 確定した設計判断

| 項目 | 決定 |
|---|---|
| RNG | 行ごと独立 RNG。sub_seed = master ⊕ (row × GOLDEN)。単一/並列で同一出力 |
| CLI | `-j N` / `-jN` / `--jobs N` / `--jobs=N`。0=auto、1=single、N=N threads |
| 並列ライブラリ | rayon 1.10 |
| 出力バッファ | 並列時は `Vec<String>` 集約 → 順番に stdout |

### 実装サマリ

- `src/rng.rs`: `Rng::from_seed(u64)`、`master_seed_from_env()`、`derive_row_seed(master, row)` 追加
- `src/cli.rs`: `CliArgs.jobs: u32` + `effective_jobs()`、`parse_jobs_token()` で 4 形式対応 + 9 件のテスト
- `src/main.rs`: `build_row_into(...)` を抽出、jobs<=1 で逐次、N>1 で `ThreadPoolBuilder + par_iter().collect()`
- `tests/snapshot.rs`: snapshot 4 件 + parallelism 一致テスト 3 件 (single==multi==auto, pinned snapshot vs `-j 4`)

### 検証

- `cargo test --release` — **54 passed** (38 lib + 5 integration + 7 snapshot + 4 generators など、 +14 from baseline)
- 単一(-j 1) / 4 worker(-j 4) / auto(-j 0) で 100k 行 sha256 一致
- 500k 行ベンチ:
  - `-j 1`: 0.43s wall / 97% CPU
  - `-j 0`: 0.42s wall / 1345% CPU (12 コア活用、メモリ集約のため wall time は同等)
- バイト一致確認: 3 モード で sha256 完全一致 = `2fd315d0c3ba49c43c67795b110f6fbace3cf6472168e584162ae6b93b7989d9`

### 備考

- **デフォルトは `-j 1`** (single-threaded)。並列化のオーバーヘッドは小規模生成では顕在化するため、明示オプトインを採用。
- 並列モードでは `Vec<String>` を集約してから stdout に書き出すので、メモリ使用量は count × 平均行長。500k 行 × 256B ≈ 128MB 程度まで現実的。それ以上は将来チャンク化を検討。
