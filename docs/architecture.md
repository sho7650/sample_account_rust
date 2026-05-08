# Architecture — sample_account (Rust)

## 1. 高レベル構造

C++ 版と同じ3層構造を維持。依存は上から下に一方向。

```
┌───────────────────────────────────────────────────────────┐
│  CLI Layer                  src/cli.rs, src/main.rs       │
│  - argv パース、--help、行ループ、stdout 書き出し          │
└──────────────────────────┬────────────────────────────────┘
                           │ 依存
                           ▼
┌───────────────────────────────────────────────────────────┐
│  Generation Layer                                          │
│  - src/registry.rs (Field 関数ポインタ table、17 emit fn) │
│  - src/field.rs    (RowContext, Deps)                     │
│  - src/generators.rs (Person/Address/AgeAndDate)          │
│  - src/rng.rs      (SmallRng + env vars + 日付スナップ)   │
└──────────────────────────┬────────────────────────────────┘
                           │ 依存
                           ▼
┌───────────────────────────────────────────────────────────┐
│  Data Layer                 src/repos.rs                  │
│  - PersonRecord / PrefectureRecord / AddressRecord /      │
│    AgeBucket struct + CSV ローダ                          │
└───────────────────────────────────────────────────────────┘
```

## 2. C++ → Rust マッピング

| C++ | Rust | 備考 |
|---|---|---|
| `IField` (abstract class) | `struct Field { emit: fn(...) }` | 動的ディスパッチを関数ポインタ table に置換 |
| `unique_ptr<IField>` の vector | `pub const FIELDS: &[Field]` | static const slice、heap 不要 |
| `IPersonRepo` etc (interface) | 直接 struct + `&[PersonRecord]` | trait は不要、所有を `Vec` で管理 |
| `CsvPersonRepo` constructor が `throw` | `fn load_persons(path) -> Result<Vec<…>, RepoError>` | エラーは `Result` |
| `RowContext` POD | `struct RowContext` Copy | 同名・同フィールド |
| `Rng` ラッパ (std::rand) | `Rng { SmallRng, now: i64 }` | `rand` crate 0.9 採用 |
| `getopt_long` | 手書きパーサ (`std::env::args` ループ) | 出現順保持を簡単に |
| `fprintf(out, …)` | `write!(out, …)` または `String::push_str` | `out: &mut String` を渡す |
| `std::time(nullptr)` / `localtime` | `time::OffsetDateTime::now_utc()` / `from_unix_timestamp` | `time` crate 採用 |
| `throw std::runtime_error` | `Result<T, RepoError>` + `?` 演算子 | |
| ヘッダ + 実装の対 | `mod` 単位で1ファイル | 通常の Rust 流儀 |

## 3. モジュール一覧と責務

### `main.rs`
- `fn main() -> ExitCode`
- 例外的なエラーは `eprintln!` + `ExitCode::from(N)` で返す
- リポジトリ load → generators 構築 → RNG → 行ループ
- 出力先は `stdout().lock()` を確保して `BufWriter` でラップ

### `cli.rs`
- `CliArgs` struct
- `parse_args(args, registry) -> CliArgs`
- `print_help(out, prog)` (registry から自動生成)
- フラグ束展開、typo alias、不明フラグ検出

### `registry.rs`
- `Field` struct
- 17 個の自由関数 (`emit_id`, `emit_lastname`, ...)
- `FIELDS: &[Field]` const slice
- `find_by_short(c) -> Option<&'static Field>`

### `field.rs`
- `RowContext` struct (`Copy`)
- `Deps<'a>` struct (Generators + Rng への借用バンドル)

### `generators.rs`
- 3 generator struct (Person/Address/AgeAndDate)
- リポジトリ struct への借用 (`'a` lifetime)

### `rng.rs`
- `Rng` 構造体: `SmallRng` を内包
- env var 読み込み、now() override
- `roll_date()` で per-row 日付スナップショット

### `repos.rs`
- レコード struct 群 (`PersonRecord`, etc.)
- ローダ関数群 (`load_persons` など)
- `RepoError` enum (`thiserror` 不使用、手書き `Display` + `Error` impl)
- `parse_digits()` ヘルパー

## 4. データ所有モデル

```rust
// main.rs ローカルに以下を持つ
let persons     : Vec<PersonRecord>     = load_persons(...)?;
let pref_repo   : PrefectureRepo        = load_prefectures(...)?;
let age_repo    : AgeRepo               = load_ages(...)?;

let person_gen  = PersonGenerator::new(&persons);
let address_gen = AddressGenerator::new(&pref_repo);
let age_gen     = AgeAndDateGenerator::new(&age_repo);
let mut rng     = Rng::new();
```

- リポジトリは `main` がオーナー、Generator は `&` 借用
- Generator のライフタイムはリポジトリと同じか短い
- 行ループでは `Deps { person, address, age_date, rng: &mut rng }` を毎周再構築（`rng` だけ可変借用）

## 5. エラー伝搬

```
load_*  → Result<_, RepoError>          (ファイルI/Oとパース失敗)
parse_args → CliArgs.error: Option<…>   (構造化エラー、main で分岐)
main → ExitCode (1: ロード失敗, 2: CLI エラー, 0: 正常)
```

## 6. テスト戦略

| 層 | テスト種別 | 場所 |
|---|---|---|
| repos | 統合テスト (実 CSV ファイル) | `tests/repos.rs` |
| rng | 単体テスト (env var pin で決定性確認) | `src/rng.rs` 内 `#[cfg(test)]` |
| generators | 単体テスト (リポジトリの fixture 渡し) | `src/generators.rs` 内 |
| cli | 単体テスト (argv ベクトルでパース検証) | `src/cli.rs` 内 |
| 全体 | スナップショット (Command 実行) | `tests/snapshot.rs` |

C++ には無かった generator の単体テストを Rust 側で追加するのが価値。

## 7. ビルドプロファイル

- `dev`: デフォルト (debug, opt-level=0)
- `release`: `cargo build --release` (LTO は不要、最適化 default)
- `nix develop`: rust-overlay の stable toolchain (clippy/rustfmt/rust-analyzer 込み)
