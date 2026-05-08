//! Verifies that the binary runs without `data/` next to its working
//! directory — i.e. the CSV data is embedded into the executable.
//!
//! Closes issue #1.

use std::process::Command;

/// Spawns the binary from a directory that does NOT contain `data/`.
/// Before issue #1 fix this would fail with "I/O error for
/// data/sample_account.csv: No such file or directory".
#[test]
fn runs_from_directory_without_data() {
    // `std::env::temp_dir()` is guaranteed not to contain our `data/`.
    let tmp = std::env::temp_dir();
    let bin = env!("CARGO_BIN_EXE_sample_account");

    let out = Command::new(bin)
        .current_dir(&tmp)
        .env("SAMPLE_ACCOUNT_SEED", "42")
        .env("SAMPLE_ACCOUNT_NOW", "1700000000")
        .args(["-ilfm", "3"])
        .output()
        .expect("failed to spawn sample_account");

    assert!(
        out.status.success(),
        "binary exited non-zero from {}: stderr = {}",
        tmp.display(),
        String::from_utf8_lossy(&out.stderr),
    );

    let stdout = String::from_utf8(out.stdout).expect("stdout was not UTF-8");
    assert_eq!(
        stdout.lines().count(),
        3,
        "expected 3 rows of output, got: {stdout:?}",
    );

    // Spot-check: row 1 must contain a comma-separated kanji+kana lastname
    // pair (4 commas total: id, last_kanji, last_kana, first_kanji,
    // first_kana, mail = 5 fields = 4 commas). Sanity that embedded data
    // parsed correctly.
    let first_row = stdout.lines().next().unwrap();
    assert_eq!(
        first_row.matches(',').count(),
        5,
        "first row malformed: {first_row}",
    );
}

/// Output from the repo-root cwd MUST equal output from a foreign cwd
/// for the same seed/now. Proves the embedded data is byte-identical to
/// the on-disk data.
#[test]
fn embedded_output_matches_repo_root() {
    let bin = env!("CARGO_BIN_EXE_sample_account");
    let args = ["-ilfmatpwcgbdorynq", "20"];

    // Run 1: from repo root (cargo test default cwd is CARGO_MANIFEST_DIR).
    let from_repo = Command::new(bin)
        .env("SAMPLE_ACCOUNT_SEED", "42")
        .env("SAMPLE_ACCOUNT_NOW", "1700000000")
        .args(args)
        .output()
        .expect("failed to spawn from repo root");
    assert!(
        from_repo.status.success(),
        "repo-root run failed: {}",
        String::from_utf8_lossy(&from_repo.stderr),
    );

    // Run 2: from temp dir.
    let tmp = std::env::temp_dir();
    let from_tmp = Command::new(bin)
        .current_dir(&tmp)
        .env("SAMPLE_ACCOUNT_SEED", "42")
        .env("SAMPLE_ACCOUNT_NOW", "1700000000")
        .args(args)
        .output()
        .expect("failed to spawn from temp dir");
    assert!(
        from_tmp.status.success(),
        "temp-dir run failed: {}",
        String::from_utf8_lossy(&from_tmp.stderr),
    );

    assert_eq!(
        from_repo.stdout, from_tmp.stdout,
        "output diverged between repo root and temp dir; embedded data must match file data",
    );
}
