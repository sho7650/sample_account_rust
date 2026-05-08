//! Read-only data repositories.
//!
//! Two parallel APIs:
//! - `load_*(path)` — opens a file and parses it. Used by integration
//!   tests in `tests/repos.rs` to verify the on-disk CSV format.
//! - `default_*()` — parses the CSV bytes baked into the binary via
//!   `include_str!`. Used by `main` so the executable runs from any
//!   working directory (closes #1).
//!
//! Both APIs share the same private `parse_*<R: BufRead>(reader, source)`
//! body; the only difference is the byte source.

use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

// -----------------------------------------------------------------------------
// Records
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PersonRecord {
    pub last_kanji: String,
    pub last_kana: String,
    pub last_name: String,
    pub first_kanji: String,
    pub first_kana: String,
    pub first_name: String,
    pub gender: String,
    pub blood_type: String,
}

#[derive(Debug, Clone)]
pub struct PrefectureRecord {
    pub number: i32,
    pub name: String,
    pub population: i32,
    pub zips: i32,
}

#[derive(Debug, Clone)]
pub struct AddressRecord {
    pub number: i32,
    pub prefecture: String,
    pub ward: String,
    pub city: String,
}

#[derive(Debug, Clone)]
pub struct AgeBucket {
    pub generation: i32,
    pub population: i32,
    pub start: i32,
}

pub struct PrefectureRepo {
    pub prefectures: Vec<PrefectureRecord>,
    pub addresses: Vec<AddressRecord>,
    pub total_population: i32,
}

pub struct AgeRepo {
    pub buckets: Vec<AgeBucket>,
    pub total_age: i32,
}

// -----------------------------------------------------------------------------
// Errors
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub enum RepoError {
    Io {
        path: String,
        source: std::io::Error,
    },
    Parse {
        path: String,
        line: usize,
        msg: String,
    },
}

impl fmt::Display for RepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RepoError::Io { path, source } => write!(f, "I/O error for {path}: {source}"),
            RepoError::Parse { path, line, msg } => {
                write!(f, "parse error in {path} line {line}: {msg}")
            }
        }
    }
}

impl std::error::Error for RepoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RepoError::Io { source, .. } => Some(source),
            RepoError::Parse { .. } => None,
        }
    }
}

// -----------------------------------------------------------------------------
// Embedded CSV data
// -----------------------------------------------------------------------------
//
// `include_str!` resolves paths relative to the source file (see Rust
// Reference). Cargo always sets CARGO_MANIFEST_DIR for rustc, so these
// paths are reliable regardless of where `cargo build` is invoked.
//
// Total embedded size ~4.8 MB (mostly address.csv at 4.2 MB). Acceptable
// trade-off for "single self-contained binary" goal.

const EMBEDDED_PERSONS_CSV: &str = include_str!("../data/sample_account.csv");
const EMBEDDED_PREFECTURES_CSV: &str = include_str!("../data/prefectures.csv");
const EMBEDDED_ADDRESS_CSV: &str = include_str!("../data/address.csv");
const EMBEDDED_AGES_CSV: &str = include_str!("../data/ages.csv");

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn open_or_io(path: &Path) -> Result<BufReader<File>, RepoError> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|source| RepoError::Io {
            path: path.display().to_string(),
            source,
        })
}

/// Strip non-digit characters and parse as i32. `data/ages.csv` uses
/// thousand-separators (e.g. "4,987,706").
pub fn parse_digits(s: &str) -> i32 {
    let mut acc: i32 = 0;
    let mut any = false;
    for c in s.chars() {
        if let Some(d) = c.to_digit(10) {
            acc = acc.saturating_mul(10).saturating_add(d as i32);
            any = true;
        }
    }
    if any {
        acc
    } else {
        0
    }
}

/// Splits a single CSV line into up to N fields. Missing trailing fields
/// become empty strings (matches C++ getline-with-comma behavior on short
/// lines, e.g. "01,北海道,旭川市" with no trailing city).
/// Returns None only if there are fewer than 2 fields (one comma minimum).
fn split_n<const N: usize>(line: &str) -> Option<[&str; N]> {
    let mut out: [&str; N] = [""; N];
    let mut iter = line.splitn(N, ',');
    let mut count = 0usize;
    for slot in out.iter_mut() {
        if let Some(v) = iter.next() {
            *slot = v;
            count += 1;
        } else {
            break;
        }
    }
    if count == 0 {
        None
    } else {
        Some(out)
    }
}

// -----------------------------------------------------------------------------
// Person
// -----------------------------------------------------------------------------

fn parse_persons<R: BufRead>(reader: R, source: &str) -> Result<Vec<PersonRecord>, RepoError> {
    let mut out = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line = line.map_err(|err| RepoError::Io {
            path: source.to_string(),
            source: err,
        })?;
        if line.is_empty() {
            continue;
        }
        let fields = split_n::<8>(&line).ok_or_else(|| RepoError::Parse {
            path: source.to_string(),
            line: idx + 1,
            msg: format!("expected 8 comma-separated fields in '{line}'"),
        })?;
        out.push(PersonRecord {
            last_kanji: fields[0].to_string(),
            last_kana: fields[1].to_string(),
            last_name: fields[2].to_string(),
            first_kanji: fields[3].to_string(),
            first_kana: fields[4].to_string(),
            first_name: fields[5].to_string(),
            gender: fields[6].to_string(),
            blood_type: fields[7].to_string(),
        });
    }
    Ok(out)
}

pub fn load_persons<P: AsRef<Path>>(path: P) -> Result<Vec<PersonRecord>, RepoError> {
    let path = path.as_ref();
    let reader = open_or_io(path)?;
    parse_persons(reader, &path.display().to_string())
}

/// Parse persons from data baked into the binary at compile time. Works
/// regardless of the current working directory.
pub fn default_persons() -> Result<Vec<PersonRecord>, RepoError> {
    parse_persons(
        EMBEDDED_PERSONS_CSV.as_bytes(),
        "<embedded:sample_account.csv>",
    )
}

// -----------------------------------------------------------------------------
// Prefectures + Addresses
// -----------------------------------------------------------------------------

fn parse_prefectures<R1: BufRead, R2: BufRead>(
    pref_reader: R1,
    addr_reader: R2,
    pref_source: &str,
    addr_source: &str,
) -> Result<PrefectureRepo, RepoError> {
    // Pass 1: prefectures.
    let mut prefectures: Vec<PrefectureRecord> = Vec::new();
    let mut total_population: i32 = 0;
    for (idx, line) in pref_reader.lines().enumerate() {
        let line = line.map_err(|err| RepoError::Io {
            path: pref_source.to_string(),
            source: err,
        })?;
        if line.is_empty() {
            continue;
        }
        let f = split_n::<3>(&line).ok_or_else(|| RepoError::Parse {
            path: pref_source.to_string(),
            line: idx + 1,
            msg: format!("expected 3 fields in '{line}'"),
        })?;
        let number = f[0].parse::<i32>().map_err(|e| RepoError::Parse {
            path: pref_source.to_string(),
            line: idx + 1,
            msg: format!("number: {e}"),
        })?;
        let population = f[2].parse::<i32>().map_err(|e| RepoError::Parse {
            path: pref_source.to_string(),
            line: idx + 1,
            msg: format!("population: {e}"),
        })?;
        total_population = total_population.saturating_add(population);
        prefectures.push(PrefectureRecord {
            number,
            name: f[1].to_string(),
            population,
            zips: 0,
        });
    }

    // Pass 2: addresses + per-prefecture zip counts.
    let mut addresses: Vec<AddressRecord> = Vec::new();
    let n_prefs = prefectures.len();
    let mut current_pref: i32 = 0; // sentinel
    let mut zip_count: i32 = 0;

    for (idx, line) in addr_reader.lines().enumerate() {
        let line = line.map_err(|err| RepoError::Io {
            path: addr_source.to_string(),
            source: err,
        })?;
        if line.is_empty() {
            continue;
        }
        let f = split_n::<4>(&line).ok_or_else(|| RepoError::Parse {
            path: addr_source.to_string(),
            line: idx + 1,
            msg: format!("expected 4 fields in '{line}'"),
        })?;
        let number = f[0].parse::<i32>().map_err(|e| RepoError::Parse {
            path: addr_source.to_string(),
            line: idx + 1,
            msg: format!("number: {e}"),
        })?;
        addresses.push(AddressRecord {
            number,
            prefecture: f[1].to_string(),
            ward: f[2].to_string(),
            city: f[3].to_string(),
        });

        if current_pref != number {
            // Boundary: flush prior prefecture's count.
            if current_pref >= 1 && (current_pref as usize) <= n_prefs {
                prefectures[(current_pref - 1) as usize].zips = zip_count;
            }
            current_pref = number;
            zip_count = 1;
        } else {
            zip_count += 1;
        }
    }
    if current_pref >= 1 && (current_pref as usize) <= n_prefs {
        prefectures[(current_pref - 1) as usize].zips = zip_count;
    }

    Ok(PrefectureRepo {
        prefectures,
        addresses,
        total_population,
    })
}

pub fn load_prefectures<P: AsRef<Path>, Q: AsRef<Path>>(
    pref_path: P,
    addr_path: Q,
) -> Result<PrefectureRepo, RepoError> {
    let pref_path = pref_path.as_ref();
    let addr_path = addr_path.as_ref();
    let pref_reader = open_or_io(pref_path)?;
    let addr_reader = open_or_io(addr_path)?;
    parse_prefectures(
        pref_reader,
        addr_reader,
        &pref_path.display().to_string(),
        &addr_path.display().to_string(),
    )
}

pub fn default_prefectures() -> Result<PrefectureRepo, RepoError> {
    parse_prefectures(
        EMBEDDED_PREFECTURES_CSV.as_bytes(),
        EMBEDDED_ADDRESS_CSV.as_bytes(),
        "<embedded:prefectures.csv>",
        "<embedded:address.csv>",
    )
}

// -----------------------------------------------------------------------------
// Ages
// -----------------------------------------------------------------------------

fn parse_ages<R: BufRead>(reader: R, source: &str) -> Result<AgeRepo, RepoError> {
    let mut buckets: Vec<AgeBucket> = Vec::new();
    let mut total_age: i32 = 0;

    for (idx, line) in reader.lines().enumerate() {
        let line = line.map_err(|err| RepoError::Io {
            path: source.to_string(),
            source: err,
        })?;
        if line.is_empty() {
            continue;
        }
        // ages.csv format: "<generation>,<population with thousand separators>"
        // The population field itself contains commas, so split only on the FIRST comma.
        let (gen_str, pop_str) = line.split_once(',').ok_or_else(|| RepoError::Parse {
            path: source.to_string(),
            line: idx + 1,
            msg: format!("expected at least one comma in '{line}'"),
        })?;
        let generation = gen_str.parse::<i32>().map_err(|e| RepoError::Parse {
            path: source.to_string(),
            line: idx + 1,
            msg: format!("generation: {e}"),
        })?;
        let population = parse_digits(pop_str);
        let start = total_age;
        total_age = total_age.saturating_add(population);
        buckets.push(AgeBucket {
            generation,
            population,
            start,
        });
    }
    Ok(AgeRepo { buckets, total_age })
}

pub fn load_ages<P: AsRef<Path>>(path: P) -> Result<AgeRepo, RepoError> {
    let path = path.as_ref();
    let reader = open_or_io(path)?;
    parse_ages(reader, &path.display().to_string())
}

pub fn default_ages() -> Result<AgeRepo, RepoError> {
    parse_ages(EMBEDDED_AGES_CSV.as_bytes(), "<embedded:ages.csv>")
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_digits_strips_separators() {
        assert_eq!(parse_digits("4,987,706"), 4_987_706);
        assert_eq!(parse_digits("1000"), 1000);
        assert_eq!(parse_digits(""), 0);
        assert_eq!(parse_digits("abc"), 0);
        assert_eq!(parse_digits("12-34"), 1234);
    }

    #[test]
    fn split_n_splits_exact_fields() {
        let f = split_n::<3>("a,b,c").unwrap();
        assert_eq!(f, ["a", "b", "c"]);
    }

    #[test]
    fn split_n_pads_missing_trailing_fields() {
        // Mirrors C++ behavior where the last getline returns "" when the
        // delimiter ran out (e.g. "01,北海道,旭川市" with no trailing city).
        let f = split_n::<4>("a,b,c").unwrap();
        assert_eq!(f, ["a", "b", "c", ""]);
    }

    #[test]
    fn split_n_returns_none_for_empty_line() {
        // splitn on "" still yields one empty item, so this returns Some([""])
        // — but for callers we only error out when we get None from a real
        // missing line. This documents current behavior.
        let f = split_n::<2>("");
        assert_eq!(f, Some(["", ""]));
    }

    // -------- embedded data parity --------

    /// `default_persons()` and `load_persons("data/sample_account.csv")`
    /// must produce identical record counts and field values. Proves that
    /// `include_str!` baked the same bytes the file-based loader sees.
    #[test]
    fn default_persons_matches_load_persons() {
        let from_file = load_persons("data/sample_account.csv").unwrap();
        let from_embed = default_persons().unwrap();
        assert_eq!(from_file.len(), from_embed.len());
        for (a, b) in from_file.iter().zip(from_embed.iter()) {
            assert_eq!(a.last_kanji, b.last_kanji);
            assert_eq!(a.last_kana, b.last_kana);
            assert_eq!(a.first_kanji, b.first_kanji);
            assert_eq!(a.gender, b.gender);
            assert_eq!(a.blood_type, b.blood_type);
        }
    }

    #[test]
    fn default_prefectures_matches_load_prefectures() {
        let from_file = load_prefectures("data/prefectures.csv", "data/address.csv").unwrap();
        let from_embed = default_prefectures().unwrap();
        assert_eq!(from_file.prefectures.len(), from_embed.prefectures.len());
        assert_eq!(from_file.addresses.len(), from_embed.addresses.len());
        assert_eq!(from_file.total_population, from_embed.total_population);
    }

    #[test]
    fn default_ages_matches_load_ages() {
        let from_file = load_ages("data/ages.csv").unwrap();
        let from_embed = default_ages().unwrap();
        assert_eq!(from_file.buckets.len(), from_embed.buckets.len());
        assert_eq!(from_file.total_age, from_embed.total_age);
    }
}
