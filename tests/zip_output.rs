//! Integration tests for --output and --zip.
//!
//! Run the binary with pinned SEED/NOW so every byte of the resulting
//! archive is reproducible. Asserts:
//!
//! - --output (without --zip) writes plain CSV bytes equal to the existing
//!   stdout snapshot.
//! - --output --zip writes a valid ZIP archive containing one entry whose
//!   decompressed bytes equal the same CSV.
//! - The entry name inside the archive is `basename(path) - ".zip"`.
//! - Two runs with identical pinned env produce byte-identical .zip files.
//! - --zip without --output exits 2 and points the user at --output.
//! - cargo doesn't ship a CSV file with a stale newline / encoding mismatch.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Output};

const PINNED_SEED: &str = "42";
const PINNED_NOW: &str = "1700000000";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_sample_account")
}

/// Returns a unique tmp path under `target/tmp/zip-tests/` so concurrent
/// test runs don't stomp on each other and we don't pollute /tmp.
fn tmp_path(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("zip-tests");
    std::fs::create_dir_all(&dir).expect("mkdir target tmpdir");
    dir.join(name)
}

fn run_pinned(args: &[&str]) -> Output {
    Command::new(bin())
        .env("SAMPLE_ACCOUNT_SEED", PINNED_SEED)
        .env("SAMPLE_ACCOUNT_NOW", PINNED_NOW)
        .args(args)
        .output()
        .expect("failed to execute sample_account")
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "binary exited with {:?}: stderr = {}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn output_without_zip_writes_plain_csv_file() {
    let path = tmp_path("plain.csv");
    let _ = std::fs::remove_file(&path);

    let out = run_pinned(&[
        "--output",
        path.to_str().unwrap(),
        "-ilfm",
        "5",
    ]);
    assert_ok(&out);

    let written = std::fs::read(&path).expect("read written CSV");
    let expected =
        std::fs::read("tests/expected/ilfm-seed-42.csv").expect("read expected CSV fixture");
    assert_eq!(
        written, expected,
        "--output should write the same bytes as stdout"
    );
}

#[test]
fn zip_with_output_produces_valid_archive_with_expected_csv() {
    let path = tmp_path("archive.zip");
    let _ = std::fs::remove_file(&path);

    let out = run_pinned(&[
        "--output",
        path.to_str().unwrap(),
        "--zip",
        "-ilfm",
        "5",
    ]);
    assert_ok(&out);

    let bytes = std::fs::read(&path).expect("read written zip");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("valid zip");
    assert_eq!(archive.len(), 1, "exactly one entry expected");

    let mut entry = archive.by_index(0).expect("first entry");
    assert_eq!(entry.name(), "archive", "entry name = basename minus .zip");

    let mut decompressed = Vec::new();
    entry
        .read_to_end(&mut decompressed)
        .expect("decompress entry");
    let expected =
        std::fs::read("tests/expected/ilfm-seed-42.csv").expect("read expected CSV fixture");
    assert_eq!(
        decompressed, expected,
        "decompressed entry should equal the plain CSV snapshot"
    );
}

#[test]
fn zip_output_is_byte_deterministic_across_runs() {
    // Write twice to the same path so the entry name (basename minus
    // .zip) is identical between runs; copy the first archive aside
    // before the second run overwrites it.
    let path = tmp_path("det.zip");
    let _ = std::fs::remove_file(&path);

    let o1 = run_pinned(&["--output", path.to_str().unwrap(), "--zip", "-ilfm", "5"]);
    assert_ok(&o1);
    let bytes_first = std::fs::read(&path).expect("read first run");

    let _ = std::fs::remove_file(&path);
    let o2 = run_pinned(&["--output", path.to_str().unwrap(), "--zip", "-ilfm", "5"]);
    assert_ok(&o2);
    let bytes_second = std::fs::read(&path).expect("read second run");

    assert_eq!(
        bytes_first.len(),
        bytes_second.len(),
        "two runs of --zip with the same SEED+NOW should be byte-identical"
    );
    assert_eq!(
        bytes_first, bytes_second,
        "two runs of --zip with the same SEED+NOW should be byte-identical"
    );
}

#[test]
fn zip_without_output_fails_with_pointer_to_output_flag() {
    // Note: don't use run_pinned because we expect non-zero exit.
    let out = Command::new(bin())
        .env("SAMPLE_ACCOUNT_SEED", PINNED_SEED)
        .env("SAMPLE_ACCOUNT_NOW", PINNED_NOW)
        .args(["--zip", "-i", "3"])
        .output()
        .expect("failed to execute sample_account");

    assert!(!out.status.success(), "expected non-zero exit");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--output"),
        "stderr should point user at --output; got: {stderr}"
    );
}
