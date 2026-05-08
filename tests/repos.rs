// Integration tests for the repository layer. Mirrors tests/test_repos.cpp
// from the C++ source, plus a few Rust-specific assertions.

use sample_account::repos::{load_ages, load_persons, load_prefectures, RepoError};

const PERSON_CSV: &str = "data/sample_account.csv";
const PREF_CSV: &str = "data/prefectures.csv";
const ADDR_CSV: &str = "data/address.csv";
const AGE_CSV: &str = "data/ages.csv";

#[test]
fn person_repo_loads_records() {
    let recs = load_persons(PERSON_CSV).expect("load_persons should succeed");
    assert!(!recs.is_empty(), "person records must not be empty");
    for r in &recs {
        assert!(
            !r.last_kanji.is_empty(),
            "every record needs a kanji last name"
        );
    }
}

#[test]
fn prefecture_repo_loads_47_prefectures() {
    let repo = load_prefectures(PREF_CSV, ADDR_CSV).expect("load_prefectures should succeed");
    assert_eq!(repo.prefectures.len(), 47, "Japan has 47 prefectures");
    assert!(
        repo.total_population > 100_000_000,
        "total population should exceed 100M (got {})",
        repo.total_population
    );
    assert!(!repo.addresses.is_empty(), "addresses must not be empty");
}

#[test]
fn prefecture_repo_assigns_zips_to_each_prefecture() {
    let repo = load_prefectures(PREF_CSV, ADDR_CSV).unwrap();
    let mut total_zips: i64 = 0;
    for p in &repo.prefectures {
        assert!(p.zips >= 0, "{} has negative zips", p.name);
        total_zips += p.zips as i64;
    }
    assert_eq!(
        total_zips as usize,
        repo.addresses.len(),
        "sum of per-prefecture zip counts must equal total addresses"
    );
}

#[test]
fn age_repo_strips_thousand_separators() {
    let repo = load_ages(AGE_CSV).expect("load_ages should succeed");
    assert!(!repo.buckets.is_empty(), "age buckets must not be empty");
    // First bucket in data/ages.csv is "0,4,987,706" => population must be
    // millions, not the single digit 4.
    assert!(
        repo.buckets[0].population > 1_000_000,
        "first bucket population should exceed 1M (got {})",
        repo.buckets[0].population
    );
    assert!(
        repo.total_age > 100_000_000,
        "total population should exceed 100M (got {})",
        repo.total_age
    );
}

#[test]
fn person_repo_returns_error_on_missing_file() {
    let result = load_persons("data/does-not-exist.csv");
    assert!(result.is_err(), "expected an error for missing file");
    matches!(result.unwrap_err(), RepoError::Io { .. });
}
