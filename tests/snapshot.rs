//! Snapshot tests. Run the binary with pinned seed/now and diff against
//! checked-in expected outputs. Any unintended drift fails the test.
//!
//! Also verifies that single-threaded (`-j 1`) and multi-threaded
//! (`-j N>=2`) modes produce byte-identical output for the same seed —
//! the cornerstone of our deterministic-by-design parallelism.

use std::process::Command;

fn run(args: &[&str]) -> String {
    let bin = env!("CARGO_BIN_EXE_sample_account");
    let out = Command::new(bin)
        .env("SAMPLE_ACCOUNT_SEED", "42")
        .env("SAMPLE_ACCOUNT_NOW", "1700000000")
        .args(args)
        .output()
        .expect("failed to execute sample_account");
    assert!(
        out.status.success(),
        "binary exited with {:?}: stderr = {}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8(out.stdout).expect("stdout was not UTF-8")
}

fn assert_snapshot(scenario: &str, args: &[&str], expected_path: &str) {
    let actual = run(args);
    let expected = std::fs::read_to_string(expected_path)
        .unwrap_or_else(|e| panic!("read {expected_path}: {e}"));
    if actual != expected {
        let mut diff = String::new();
        for (i, (a, b)) in actual.lines().zip(expected.lines()).enumerate() {
            if a != b {
                diff.push_str(&format!(
                    "line {}:\n  actual  : {a}\n  expected: {b}\n",
                    i + 1
                ));
            }
        }
        if actual.lines().count() != expected.lines().count() {
            diff.push_str(&format!(
                "line count: actual {} vs expected {}\n",
                actual.lines().count(),
                expected.lines().count()
            ));
        }
        panic!("scenario '{scenario}' mismatch:\n{diff}");
    }
}

// ---- pinned-seed snapshot scenarios ----

#[test]
fn snapshot_all_flags() {
    assert_snapshot(
        "all flags (long output)",
        &["-ilfmatpwcgbdorynq", "5"],
        "tests/expected/all-flags-seed-42.csv",
    );
}

#[test]
fn snapshot_ilfm() {
    assert_snapshot(
        "id+name+mail (-ilfm)",
        &["-ilfm", "5"],
        "tests/expected/ilfm-seed-42.csv",
    );
}

#[test]
fn snapshot_default() {
    assert_snapshot(
        "default (no flags)",
        &["3"],
        "tests/expected/default-seed-42.csv",
    );
}

#[test]
fn snapshot_long_aliases() {
    assert_snapshot(
        "long aliases",
        &["--telephone", "--agegroup", "--birthyear", "4"],
        "tests/expected/long-aliases-seed-42.csv",
    );
}

// ---- single vs multi parity ----

/// 4 worker threads should produce the same bytes as 1 worker. Use a
/// reasonably-large row count so we exercise per-row seed derivation
/// across many indices.
#[test]
fn parallel_matches_single_for_full_columns() {
    let single = run(&["-j", "1", "-ilfmatpwcgbdorynq", "200"]);
    let multi = run(&["-j", "4", "-ilfmatpwcgbdorynq", "200"]);
    assert_eq!(single, multi, "single and multi modes diverged");
}

/// Auto mode (`-j 0`) picks core count at runtime; output must still
/// match `-j 1`.
#[test]
fn auto_mode_matches_single() {
    let single = run(&["-j", "1", "-ilfm", "100"]);
    let auto = run(&["-j", "0", "-ilfm", "100"]);
    assert_eq!(single, auto, "auto mode diverged from single");
}

/// Existing pinned snapshots were generated with default `-j 1`. Using
/// `-j 4` against the same expected file must still match.
#[test]
fn parallel_matches_pinned_snapshot() {
    let bin = env!("CARGO_BIN_EXE_sample_account");
    let out = Command::new(bin)
        .env("SAMPLE_ACCOUNT_SEED", "42")
        .env("SAMPLE_ACCOUNT_NOW", "1700000000")
        .args(["-j", "4", "-ilfmatpwcgbdorynq", "5"])
        .output()
        .expect("failed to execute");
    let actual = String::from_utf8(out.stdout).unwrap();
    let expected = std::fs::read_to_string("tests/expected/all-flags-seed-42.csv").unwrap();
    assert_eq!(actual, expected);
}

/// Forces the runner past at least 2 BATCH_ROWS boundaries (BATCH_ROWS is
/// 128k in main.rs). With 300k rows we hit 3 batches in parallel mode,
/// proving the streaming path stitches batches together correctly and
/// preserves row order across batch boundaries.
#[test]
fn streaming_multi_batch_matches_single() {
    let single = run(&["-j", "1", "-ilfm", "300000"]);
    let multi = run(&["-j", "4", "-ilfm", "300000"]);
    assert_eq!(single.len(), multi.len(), "streaming output sizes differed");
    assert_eq!(single, multi, "streaming multi-batch diverged from single");
    // Sanity: row count matches.
    assert_eq!(
        single.lines().count(),
        300_000,
        "expected 300k rows in single output"
    );
}
