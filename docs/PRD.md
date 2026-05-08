# Product Requirements Document — sample_account (Rust port)

## 1. 概要

`sample_account` は日本語の合成個人アカウントデータ（氏名・住所・年齢・メール・電話番号など）を CSV 形式で標準出力に出力する CLI ツール。既存 C++17 実装を Rust に移植する。

## 2. 背景

C++ 版が以下の問題を抱えており、Rust 移植によって解決を図る:

- 元コードはオブジェクト指向の理解が浅いまま `switch case` 中心で書かれ、最近のリファクタで OOP 化されたばかり。所有権モデルと型安全性をより厳密に持つ Rust に乗り換えることで:
  - 文字列・vector の所有権をコンパイル時に検査
  - エラー処理を `Result` で明示
  - `unique_ptr<IField>` 動的ディスパッチを関数ポインタ table に置換し、シンプル化
- Nix で再現可能な開発環境を提供し、誰がチェックアウトしても同じ Rust toolchain で作業できる

## 3. ユーザストーリー

- **データエンジニアとして**、本番ライクなテストデータが欲しい。`./sample_account -ilfmpwc 1000 > test.csv` で1000行の合成データが出ること。
- **QA エンジニアとして**、回帰テストで決定的な出力を再現したい。`SAMPLE_ACCOUNT_SEED=42 SAMPLE_ACCOUNT_NOW=1700000000` を指定すれば常に同じ CSV が得られること。
- **新規開発者として**、`git clone` 後に `nix develop` だけで開発開始したい。Rust toolchain を別途インストールしなくて良いこと。
- **メンテナとして**、新規 CSV 列の追加が「1ファイル編集 + 1行登録」で済むこと（C++ 版と同じ Field strategy 構造）。

## 4. 機能要件

### F1. CLI

- `./sample_account [OPTIONS] [COUNT]` 形式
- COUNT 省略時 100 行
- `-h` / `--help` で usage を stdout に出力、exit 0
- 短フラグ束 (`-ilfm` → id, lastname, firstname, mail) を展開
- 長フラグ (`--lastname` 等)
- `--telehpne` を `--telephone` のエイリアスとして残す（既存スクリプト互換）
- 不明フラグは stderr に `unrecognized option` を出力し exit 2
- フラグの **出現順が CSV 列順** を決める（出力順序保証）

### F2. 列 (Field)

C++ 版と同一の17列をサポート:

| short | long | 内容 |
|---|---|---|
| i | id | 1始まり連番 |
| l | lastname | 姓 (kanji,kana の2 CSVフィールド) |
| f | firstname | 名 (kanji,kana の2 CSVフィールド) |
| m | mail | `firstname_lastname@example.com` |
| t | telephone | `090-XXXX-XXXX` |
| p | prefecture | 人口加重で都道府県 |
| w | ward | 区/市町村 |
| c | city | 町域 |
| g | gender | 男/女 |
| b | blood | A/B/O/AB |
| a | age | 年齢 (人口分布加重) |
| o | agegroup | 年代 (10刻み) |
| y | birthyear | 生年 |
| r | reward | 想定年収 |
| d | date | YYYY/M/D |
| n | random | ±10,000,000 |
| q | quotient | 0.00〜0.99 の小数 |

### F3. データソース

- `data/sample_account.csv` (姓名・性別・血液型)
- `data/prefectures.csv` (47都道府県の人口)
- `data/address.csv` (郵便番号→住所)
- `data/ages.csv` (年代別人口、千の位区切り含む)
- 起動時にバイナリと同じ cwd の `data/` を読む

### F4. 決定性

`SAMPLE_ACCOUNT_SEED` (RNG seed) と `SAMPLE_ACCOUNT_NOW` (Unix epoch sec) の両方を指定すれば、stdout は完全に決定的でなければならない。テストはこの性質に依存する。

### F5. テスト

- 単体テスト: リポジトリ層、Generator、CLI パース、RNG 決定性
- スナップショットテスト: 4シナリオで `tests/expected/*.csv` と完全一致
- カバレッジ目標: 80%+ (line coverage)

## 5. 非機能要件

- **再現可能ビルド**: `nix develop` 内で `cargo build` が通ること
- **ポータビリティ**: macOS (Apple Silicon, Intel) と Linux でテストが通ること
- **依存最小化**: 必要 crate は `rand`, `time` の2本のみ
- **起動時間**: 100行生成で 100ms 未満（C++版と同等）

## 6. 範囲外 (Out of scope)

- `tool/convert-address.sh` (raw Japan Post → address.csv) は移植しない。データファイルは C++ リポからコピーで運用
- バージョン番号 `0.4.7` は据え置き、リリースタグは Rust 側で別管理する想定
- バイト一致のスナップショット互換性 (C++ libc rand を真似ない)

## 7. 受け入れ基準

1. `nix develop -c cargo build` が成功
2. `nix develop -c cargo test` が PASS（`repos`, `cli`, `generators`, `rng`, `snapshot`）
3. `cargo run -- --help` が C++ 版と等価な usage を出力
4. `SAMPLE_ACCOUNT_SEED=42 SAMPLE_ACCOUNT_NOW=1700000000 ./sample_account -ilfmatpwcgbdorynq 5` を 2 回走らせて diff が空
5. tarpaulin / llvm-cov でカバレッジ 80%+
