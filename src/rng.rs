//! Deterministic-friendly RNG facade.
//!
//! Reads `SAMPLE_ACCOUNT_SEED` and `SAMPLE_ACCOUNT_NOW` env vars so
//! snapshot tests get stable output. Each row in `try_main` constructs
//! its own `Rng` from a deterministically-derived sub-seed so single-
//! and multi-threaded modes produce identical output.

use rand::rngs::SmallRng;
use rand::{Rng as _, SeedableRng};
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;

/// SplitMix-style golden-ratio constant. Mixing the row index against the
/// master seed via this multiplier gives well-decorrelated sub-seeds even
/// when row indices are sequential 0, 1, 2, ….
const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

pub struct Rng {
    inner: SmallRng,
    /// Reference "now" captured at construction time. Used as the upper
    /// bound when `roll_date` picks a random instant. Caching this avoids
    /// re-reading the `SAMPLE_ACCOUNT_NOW` env var on every row in the
    /// hot loop.
    ref_now: i64,
    /// Per-row date snapshot (Unix epoch seconds). Set by `roll_date`.
    now: i64,
}

impl Rng {
    /// Build an `Rng` from a seed and a pre-captured "now". The hot path
    /// uses this so workers don't read env vars per row.
    pub fn from_seed_with_now(seed: u64, ref_now: i64) -> Self {
        Self {
            inner: SmallRng::seed_from_u64(seed),
            ref_now,
            now: 0,
        }
    }

    /// Build an `Rng` from a seed; reads `SAMPLE_ACCOUNT_NOW` once at
    /// construction. Convenience for tests and callers that don't pre-cache.
    pub fn from_seed(seed: u64) -> Self {
        Self::from_seed_with_now(seed, current_time())
    }

    /// Build an `Rng` whose master seed comes from `SAMPLE_ACCOUNT_SEED`
    /// (or wall-clock seconds if unset). Mostly used in tests; production
    /// callers use `master_seed_from_env` + `from_seed_with_now` directly.
    pub fn new() -> Self {
        Self::from_seed(master_seed_from_env())
    }

    /// Returns a non-negative i32. Mirrors the value range of C++ `rand()`.
    pub fn next_i32(&mut self) -> i32 {
        self.inner.random_range(0..=i32::MAX)
    }

    /// Picks a random Unix timestamp in `[0, ref_now)` and stores it for
    /// `year/month/day` to read. Idempotent within a row. No env reads.
    pub fn roll_date(&mut self) {
        self.now = if self.ref_now > 0 {
            self.inner.random_range(0..self.ref_now)
        } else {
            0
        };
    }

    pub fn year(&self) -> i32 {
        offset_dt(self.now).year()
    }

    pub fn month(&self) -> i32 {
        offset_dt(self.now).month() as u8 as i32
    }

    pub fn day(&self) -> i32 {
        offset_dt(self.now).day() as i32
    }
}

impl Default for Rng {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads `SAMPLE_ACCOUNT_SEED` if present, otherwise falls back to
/// wall-clock seconds. The result is the *master* seed; per-row sub-seeds
/// derive from it via `derive_row_seed`.
pub fn master_seed_from_env() -> u64 {
    match env::var("SAMPLE_ACCOUNT_SEED") {
        Ok(s) => s.parse::<u64>().unwrap_or(0),
        Err(_) => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    }
}

/// Deterministically derive a per-row sub-seed from the master seed. Both
/// single- and multi-threaded modes use this so output is identical across
/// modes given the same master seed.
pub fn derive_row_seed(master: u64, row: u32) -> u64 {
    master.wrapping_add((row as u64).wrapping_mul(GOLDEN))
}

/// Honors `SAMPLE_ACCOUNT_NOW`; falls back to wall-clock seconds.
pub fn current_time() -> i64 {
    if let Ok(s) = env::var("SAMPLE_ACCOUNT_NOW") {
        if let Ok(v) = s.parse::<i64>() {
            return v;
        }
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Returns the current calendar year. Used by `birth_year` math.
pub fn current_year() -> i32 {
    offset_dt(current_time()).year()
}

fn offset_dt(unix_seconds: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(unix_seconds).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_produces_same_sequence() {
        let mut a = Rng::from_seed(42);
        let mut b = Rng::from_seed(42);
        for _ in 0..100 {
            assert_eq!(a.next_i32(), b.next_i32());
        }
    }

    #[test]
    fn different_seed_diverges_quickly() {
        let mut a = Rng::from_seed(42);
        let mut b = Rng::from_seed(43);
        // After a handful of draws the streams should disagree at least once.
        let mut any_diff = false;
        for _ in 0..16 {
            if a.next_i32() != b.next_i32() {
                any_diff = true;
                break;
            }
        }
        assert!(
            any_diff,
            "neighboring seeds produced identical 16-draw prefix"
        );
    }

    #[test]
    fn next_i32_is_non_negative() {
        let mut rng = Rng::from_seed(1);
        for _ in 0..1000 {
            assert!(rng.next_i32() >= 0);
        }
    }

    #[test]
    fn derive_row_seed_decorrelates_neighbors() {
        // Row 0 and row 1 produce different sub-seeds.
        assert_ne!(derive_row_seed(42, 0), derive_row_seed(42, 1));
        // Same row, different masters → different sub-seeds.
        assert_ne!(derive_row_seed(42, 7), derive_row_seed(43, 7));
        // Determinism: same inputs → same output.
        assert_eq!(derive_row_seed(42, 100), derive_row_seed(42, 100));
    }

    #[test]
    fn current_time_honors_env_var() {
        // SAFETY: env::set_var is unsafe in edition 2024+; we're on 2021.
        env::set_var("SAMPLE_ACCOUNT_NOW", "1700000000");
        let v = current_time();
        env::remove_var("SAMPLE_ACCOUNT_NOW");
        assert_eq!(v, 1_700_000_000);
    }

    #[test]
    fn roll_date_produces_valid_ymd() {
        let mut rng = Rng::from_seed(7);
        env::set_var("SAMPLE_ACCOUNT_NOW", "1700000000");
        rng.roll_date();
        env::remove_var("SAMPLE_ACCOUNT_NOW");

        let y = rng.year();
        let m = rng.month();
        let d = rng.day();
        assert!((1970..=2023).contains(&y), "year {y} out of range");
        assert!((1..=12).contains(&m), "month {m} out of range");
        assert!((1..=31).contains(&d), "day {d} out of range");
    }
}
