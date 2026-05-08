# System Design — sample_account (Rust)

具体的な型定義、関数シグネチャ、モジュール内 API を確定する。実装はこの設計に従う。

## 1. `src/repos.rs`

```rust
use std::path::Path;

#[derive(Debug, Clone)]
pub struct PersonRecord {
    pub last_kanji:  String,
    pub last_kana:   String,
    pub last_name:   String,  // romaji
    pub first_kanji: String,
    pub first_kana:  String,
    pub first_name:  String,  // romaji
    pub gender:      String,
    pub blood_type:  String,
}

#[derive(Debug, Clone)]
pub struct PrefectureRecord {
    pub number:     i32,
    pub name:       String,
    pub population: i32,
    pub zips:       i32,   // 集計時に上書き
}

#[derive(Debug, Clone)]
pub struct AddressRecord {
    pub number:     i32,
    pub prefecture: String,
    pub ward:       String,
    pub city:       String,
}

#[derive(Debug, Clone)]
pub struct AgeBucket {
    pub generation: i32,
    pub population: i32,
    pub start:      i32,
}

pub struct PrefectureRepo {
    pub prefectures:      Vec<PrefectureRecord>,
    pub addresses:        Vec<AddressRecord>,
    pub total_population: i32,
}

pub struct AgeRepo {
    pub buckets:   Vec<AgeBucket>,
    pub total_age: i32,
}

#[derive(Debug)]
pub enum RepoError {
    Io { path: String, source: std::io::Error },
    Parse { path: String, line: usize, msg: String },
}

impl std::fmt::Display for RepoError { /* ... */ }
impl std::error::Error for RepoError { /* ... */ }

pub fn load_persons(path: impl AsRef<Path>) -> Result<Vec<PersonRecord>, RepoError>;
pub fn load_prefectures(
    pref_path: impl AsRef<Path>,
    addr_path: impl AsRef<Path>,
) -> Result<PrefectureRepo, RepoError>;
pub fn load_ages(path: impl AsRef<Path>) -> Result<AgeRepo, RepoError>;

// 千の位区切りを除去して i32 に
pub(crate) fn parse_digits(s: &str) -> i32;
```

### `load_prefectures` の zip 集計ロジック

```rust
// 1) prefectures.csv を line ごとに読む → Vec<PrefectureRecord>
// 2) address.csv を line ごとに読む → Vec<AddressRecord>
// 3) addresses は number 昇順に grouping されている前提
//    現在のグループ番号を current_pref に保持
//    境界が変わるたびに pref[current-1].zips = count を確定
```

## 2. `src/rng.rs`

```rust
use rand::{SeedableRng, RngExt};
use rand::rngs::SmallRng;

pub struct Rng {
    inner: SmallRng,
    now:   i64,    // per-row date snapshot (unix timestamp)
}

impl Rng {
    pub fn new() -> Self {
        let seed = match std::env::var("SAMPLE_ACCOUNT_SEED") {
            Ok(s) => s.parse::<u64>().unwrap_or(0),
            Err(_) => {
                use std::time::{SystemTime, UNIX_EPOCH};
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
            }
        };
        Self { inner: SmallRng::seed_from_u64(seed), now: 0 }
    }

    /// C++ rand() と等価な「非負 i32」を返す。
    pub fn next_i32(&mut self) -> i32 {
        self.inner.random_range(0..=i32::MAX)
    }

    pub fn roll_date(&mut self) {
        let reference = current_time();
        // 0..reference 内の unix timestamp を引く
        if reference > 0 {
            self.now = self.inner.random_range(0..reference);
        } else {
            self.now = 0;
        }
    }

    pub fn year(&self)  -> i32 { /* time::OffsetDateTime::from_unix_timestamp(self.now)?.year() */ }
    pub fn month(&self) -> i32 { /* .month() as u8 as i32 */ }
    pub fn day(&self)   -> i32 { /* .day() as i32 */ }
}

pub fn current_time() -> i64 {
    if let Ok(s) = std::env::var("SAMPLE_ACCOUNT_NOW") {
        if let Ok(v) = s.parse::<i64>() { return v; }
    }
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}
```

## 3. `src/generators.rs`

```rust
use crate::repos::{PersonRecord, PrefectureRepo, AgeRepo};
use crate::rng::Rng;

pub struct PersonGenerator<'a> {
    records: &'a [PersonRecord],
}

impl<'a> PersonGenerator<'a> {
    pub fn new(records: &'a [PersonRecord]) -> Self { Self { records } }
    pub fn last_name(&self, n: i32) -> String;
    pub fn first_name(&self, n: i32) -> String;
    pub fn mail_address(&self, first: i32, last: i32) -> String;
    pub fn gender(&self, n: i32) -> &str;
    pub fn blood_type(&self, n: i32) -> &str;
}

pub struct AddressGenerator<'a> {
    repo: &'a PrefectureRepo,
}

impl<'a> AddressGenerator<'a> {
    pub fn new(repo: &'a PrefectureRepo) -> Self { Self { repo } }
    pub fn weighted_prefecture_index(&self, n: i32) -> usize;
    pub fn prefecture_name(&self, idx: usize) -> &str;
    pub fn ward(&self, pref: usize, n: i32) -> &str;
    pub fn city(&self, pref: usize, n: i32) -> &str;
    fn address_index(&self, pref: usize, n: i32) -> usize;
}

pub struct AgeAndDateGenerator<'a> {
    repo: &'a AgeRepo,
}

impl<'a> AgeAndDateGenerator<'a> {
    pub fn new(repo: &'a AgeRepo) -> Self { Self { repo } }
    pub fn age(&self, n: i32) -> i32;
    pub fn age_group(&self, n: i32) -> i32;
    pub fn birth_year(&self, n: i32) -> i32;          // current_year() - age(n)
    pub fn reward(&self, n: i32, rng: &mut Rng) -> i32;
}
```

## 4. `src/field.rs`

```rust
use crate::generators::{PersonGenerator, AddressGenerator, AgeAndDateGenerator};
use crate::rng::Rng;

#[derive(Clone, Copy)]
pub struct RowContext {
    pub row:   i32,
    pub first: i32,
    pub last:  i32,
    pub pref:  usize,
    pub ward:  i32,
    pub city:  i32,
    pub age:   i32,
}

pub struct Deps<'a> {
    pub person:   &'a PersonGenerator<'a>,
    pub address:  &'a AddressGenerator<'a>,
    pub age_date: &'a AgeAndDateGenerator<'a>,
    pub rng:      &'a mut Rng,
}
```

## 5. `src/registry.rs`

```rust
use std::fmt::Write;
use crate::field::{RowContext, Deps};

pub struct Field {
    pub short: char,
    pub long:  &'static str,
    pub desc:  &'static str,
    pub emit:  fn(&mut String, &RowContext, &mut Deps),
}

fn emit_id(out: &mut String, ctx: &RowContext, _: &mut Deps) {
    write!(out, "{}", ctx.row + 1).unwrap();
}
fn emit_lastname(out: &mut String, ctx: &RowContext, d: &mut Deps) {
    write!(out, "{}", d.person.last_name(ctx.last)).unwrap();
}
// ... 17 free fns

pub const FIELDS: &[Field] = &[
    Field { short: 'i', long: "id",         desc: "sequential row id (1-based)",
            emit: emit_id },
    Field { short: 'l', long: "lastname",   desc: "last name (kanji,kana — two CSV fields)",
            emit: emit_lastname },
    // … 15 more, in C++ buildDefaultRegistry order
];

pub fn find_by_short(c: char) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.short == c)
}

pub fn short_optstring() -> String {
    FIELDS.iter().map(|f| f.short).collect()
}
```

## 6. `src/cli.rs`

```rust
pub const DEFAULT_ROW_COUNT: u32 = 100;

pub struct CliArgs {
    pub selected_fields: Vec<&'static crate::registry::Field>,
    pub count: u32,
    pub help:  bool,
    pub error: Option<String>,
}

pub fn parse_args<I, S>(args: I) -> CliArgs
where I: IntoIterator<Item = S>, S: AsRef<str>;

pub fn print_help(out: &mut impl std::io::Write, prog: &str) -> std::io::Result<()>;
```

### パーサ動作

```
1) prog 名はスキップ
2) 各 token を順に判定:
   - "--help" / "-h" → help = true、ループ終了
   - "--telehpne"    → telephone Field を push
   - "--<longname>"  → registry から検索、見つからなければ error
   - "-xxxx"         → 各文字を short flag として展開、未知文字は error
   - 数字のみ        → count にパース（既存値は上書き）
   - その他          → error("unrecognized option")
3) selected_fields が空なら IdField 1個を default
```

## 7. `src/main.rs`

```rust
use std::io::{self, BufWriter, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match try_main() {
        Ok(code) => code,
        Err(e) => { eprintln!("sample_account: {e}"); ExitCode::from(1) }
    }
}

fn try_main() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let prog = args.get(0).map(|s| s.as_str()).unwrap_or("sample_account");
    let parsed = cli::parse_args(args.iter().skip(1).map(|s| s.as_str()));

    if let Some(msg) = parsed.error {
        eprintln!("{prog}: {msg}");
        eprintln!("Try '{prog} --help' for usage.");
        return Ok(ExitCode::from(2));
    }
    if parsed.help {
        cli::print_help(&mut io::stdout(), prog)?;
        return Ok(ExitCode::SUCCESS);
    }

    let persons   = repos::load_persons("data/sample_account.csv")?;
    let pref_repo = repos::load_prefectures("data/prefectures.csv", "data/address.csv")?;
    let age_repo  = repos::load_ages("data/ages.csv")?;

    let person   = generators::PersonGenerator::new(&persons);
    let address  = generators::AddressGenerator::new(&pref_repo);
    let age_date = generators::AgeAndDateGenerator::new(&age_repo);
    let mut rng  = rng::Rng::new();

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let mut row_buf = String::with_capacity(256);

    for i in 0..parsed.count {
        let mut ctx = RowContext {
            row:   i as i32,
            first: rng.next_i32(),
            last:  rng.next_i32(),
            pref:  0,
            ward:  rng.next_i32(),
            city:  rng.next_i32(),
            age:   rng.next_i32(),
        };
        // pref は weighted index を別 next で
        let pref_n = rng.next_i32();
        ctx.pref = address.weighted_prefecture_index(pref_n);
        rng.roll_date();

        row_buf.clear();
        let mut deps = Deps { person: &person, address: &address, age_date: &age_date, rng: &mut rng };
        for (j, field) in parsed.selected_fields.iter().enumerate() {
            if j > 0 { row_buf.push(','); }
            (field.emit)(&mut row_buf, &ctx, &mut deps);
        }
        row_buf.push('\n');
        out.write_all(row_buf.as_bytes())?;
    }
    Ok(ExitCode::SUCCESS)
}
```

**重要**: C++ 版の `rng.next()` 呼び出し順を正確に一致させる必要がある（first→last→pref→ward→city→age→rollDate）。これによって seed 固定での出力が安定する。

## 8. テスト

### `tests/repos.rs`

C++ の `test_repos.cpp` の5アサーションをそのまま移植 + `parse_digits` の単体テスト。

### `tests/snapshot.rs`

```rust
use std::process::Command;

fn run(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_sample_account"))
        .env("SAMPLE_ACCOUNT_SEED", "42")
        .env("SAMPLE_ACCOUNT_NOW", "1700000000")
        .args(args)
        .output()
        .expect("binary failed");
    assert!(out.status.success(), "non-zero exit: {:?}", out);
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn snapshot_all_flags() {
    let actual = run(&["-ilfmatpwcgbdorynq", "5"]);
    let expected = include_str!("expected/all-flags-seed-42.csv");
    assert_eq!(actual, expected);
}
// + 3 more
```

## 9. 設計上の決定ポイントと根拠

| 決定 | 根拠 |
|---|---|
| `Field` を関数ポインタ table | C++ の virtual ディスパッチを zero-cost に置換、状態を持たないため trait object 不要 |
| `&[PersonRecord]` 借用 | `Arc` 不要、`main` が単一所有、ライフタイム明示 |
| `time::OffsetDateTime` UTC | `localtime` のロケール依存を避け、`SAMPLE_ACCOUNT_NOW` 固定で完全決定的 |
| `Rng::next_i32` の値域を `0..=i32::MAX` | C++ の `rand()` が返す非負整数領域と一致、modulo 演算が等価 |
| `String::push_str` ベースの emit | `BufWriter` への一括書き込みで `fprintf` 連発を避ける |
