# Technical Decisions — sample_account (Rust)

実装中に判断が必要な技術項目をまとめ、根拠と代替案を記録する。

## 1. RNG: `rand` crate 0.9 系の `SmallRng`

**選定**: `rand = "0.9"` の `rand::rngs::SmallRng`、`SeedableRng::seed_from_u64(u64)`、`random_range(start..=end)`。

**根拠**:
- `SmallRng` は xoshiro 系で、決定性・ポータビリティ・速度のバランスが良い。本ツールは暗号強度不要。
- `random_range` は `gen_range` の rand 0.9 後継。
- C++ の `rand()` ABI 互換は要件外なので採用に問題なし。

**代替案と却下理由**:
- `StdRng` (ChaCha12) — overkill。CSPRNG は不要、状態が大きい。
- `rand_pcg::Pcg64` 直接 — `SmallRng` で十分、依存追加に値しない。

## 2. 日付計算: `time` crate 0.3

**選定**: `time::OffsetDateTime::from_unix_timestamp(i64) -> Result`、`year() / month() / day()`。

**根拠**:
- `localtime` 的な振る舞いが要らず（`SAMPLE_ACCOUNT_NOW` を UTC として固定で良い）、UTC 固定で十分。
- `chrono` より軽量、API がシンプル。

**注意**:
- `month()` は `time::Month` enum、`u8` キャストで `1..=12`。
- C++ 版は `localtime` を使っているが、`SAMPLE_ACCOUNT_NOW=1700000000` 程度ではタイムゾーン差での日付ズレを許容（スナップショット再生成で吸収）。

## 3. CSV パース: 手書き `split(',')`

**選定**: `BufReader::lines()` + `line.split(',')` でフィールド分割。

**根拠**:
- データファイルが quote / escape / 改行を含まない単純フォーマット。
- `csv` crate を入れる利得が薄い。

**注意点**:
- `data/ages.csv` は人口に千の位区切り (`4,987,706`) を含むので、最後のフィールドだけ「カンマ含む全部」として扱い、`parse_digits` で数字だけ抽出。
- 空行はスキップする（C++ 版は `getline` 結果を空でも push してしまうが、データに空行は無いので未対応で良い）。

## 4. CLI パーサ: 手書き

**選定**: `std::env::args` を `Vec<String>` に集めて 1パスループ。`getopt` crate も `clap` も使わない。

**根拠**:
- C++ `getopt_long` の挙動を **「フラグ出現順 = 出力列順」** という方針で再現する必要があるが、`clap` ではこれが自然に出ない。
- 手書きなら 100 行未満で済む。

**考慮事項**:
- 短フラグ束: `-ilfm` → `i`,`l`,`f`,`m` に展開。
- 短フラグの「値付き」は本ツールでは不要 (no_argument のみ)。
- `--telehpne` を `--telephone` のエイリアスに（C++ 版互換）。
- 不明フラグ → `error: Some("unrecognized option: <name>".into())`、main で stderr + exit 2。

## 5. エラー型

**選定**: モジュールごとにシンプルな `enum`。`thiserror` / `anyhow` は使わない。

**根拠**:
- 依存最小化方針。`enum + impl Display + impl Error` 程度なら手書きで十分（10行）。
- main では `Box<dyn std::error::Error>` を受けて `eprintln!` するだけ。

```rust
#[derive(Debug)]
pub enum RepoError {
    Io { path: String, source: std::io::Error },
    Parse { path: String, line: usize, msg: String },
}
```

## 6. 出力: `String` ベース + `BufWriter`

**選定**: 1 行を `String` バッファに書き溜め、`BufWriter<StdoutLock>` に `write_all`。

**根拠**:
- 列ごとに stdout に直接 `print!` すると行末カンマ判定が散らばる。バッファに集めて `,` join がシンプル。
- `BufWriter` で stdout flush コストを抑える。
- `String` は内容が必ず UTF-8 と仮定（CSV 内容も UTF-8）。

**emit 関数のシグネチャ**:
```rust
fn emit_xxx(out: &mut String, ctx: &RowContext, deps: &mut Deps) {
    write!(out, "...").unwrap(); // String への write は失敗しない
}
```
`write!` の `unwrap()` は安全 (`Vec<u8>` への push は失敗しない)。

## 7. Field の動的ディスパッチをやめる根拠

C++ 版は `unique_ptr<IField>` の vector で、virtual call 経由で `emit` を呼ぶ。Rust ではこれを `fn(&mut String, ...)` 関数ポインタ table に置き換える。

**理由**:
- 17 個の Field はすべて状態を持たない（C++ 版でも空 class）。
- trait object (`Box<dyn Field>`) を作る必要がない。
- `&'static Field` を `Vec` に集めるだけで「出現順保持」も自然に表現できる。
- `cargo run` の起動時にヒープ確保が一切ない（const slice）。

## 8. Nix flake 構造

**選定**: `flake.nix` + `rust-overlay` input、`devShell` に Rust toolchain を入れる。

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
        };
      in {
        devShells.default = pkgs.mkShell {
          packages = [ rustToolchain pkgs.pkg-config ];
          # Rust analyzer に rust-src を見つけさせる
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };
      });
}
```

**理由**:
- `rust-overlay` は upstream Rust リリースを直で取れる、nixpkgs 同期遅れに影響されない。
- `extensions` で rust-analyzer / clippy / rustfmt を一括導入。
- `flake-utils.eachDefaultSystem` で macOS / Linux 両対応。

## 9. テストカバレッジ計測

**選定**: `cargo llvm-cov` を将来的に導入。Phase 0-9 の範囲では `cargo test` のみで進める。

**理由**:
- Nix flake に `llvm-cov` を入れるのは可能だが、初回 PR には不要。
- 後続作業で `nix develop -c cargo llvm-cov --html` を回せば良い。

## 10. Cargo.lock のコミット方針

**bin プロジェクトのため Cargo.lock はコミット**。`.gitignore` に `Cargo.lock` を入れない。再現可能ビルドのため。

## 11. 既知の制約

- `data/` は cwd 相対パス。バイナリを別ディレクトリから呼ぶと `file not found`。これは C++ 版と同じ仕様。
- `--telehpne` typo を維持。スクリプト互換のため。
- スナップショット再生成のたびに `tests/expected/*.csv` の差分が PR に乗る。意図的な変更か誤りかをレビュアが判断する必要あり。
