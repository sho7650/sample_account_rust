//! Output sink factory.
//!
//! `run_serial` and `run_parallel` write CSV bytes through `&mut dyn Write`.
//! This module owns the construction and lifecycle of the underlying sink so
//! the hot loops in `main.rs` stay sink-agnostic.
//!
//! Three destinations:
//!
//! 1. **Stdout** — default when `--output` is absent. Buffered through a 1 MiB
//!    `BufWriter` to amortize syscalls; same behavior as the original code.
//! 2. **File** — used when `--output PATH` is present without `--zip`. Plain
//!    CSV bytes streamed to disk through the same 1 MiB `BufWriter`.
//! 3. **Zip** — used when `--output PATH --zip` is present. Wraps a
//!    `BufWriter<File>` in `zip::ZipWriter`. A single Deflate-compressed entry
//!    is opened up front and CSV bytes flow row-by-row into the deflate
//!    stream. Constant memory regardless of row count, same as the plain
//!    paths. The ZIP variant is gated behind the `zip` Cargo feature.
//!
//! Determinism: the ZIP variant pins the entry's last-modified time
//! (derived from `now_unix`, falling back to the `DETERMINISTIC_FALLBACK`
//! epoch), the compression level, and unix permissions. Combined with the
//! exact-version pin on the `zip` crate in `Cargo.toml`, the resulting
//! archive bytes are reproducible for a given `SAMPLE_ACCOUNT_SEED` +
//! `SAMPLE_ACCOUNT_NOW`.

use std::fs::File;
use std::io::{self, BufWriter, Write};

/// Output buffer in front of the destination. Matches the buffer size used
/// historically for stdout. 1 MiB amortizes syscall overhead at multi-million-
/// row counts without significantly inflating peak RSS.
const OUT_BUF_BYTES: usize = 1 << 20;

/// Options derived from CLI args. Built by `try_main` and consumed once by
/// [`OutputSink::open`].
pub struct OutputOptions<'a> {
    /// `None` -> stdout. `Some(path)` -> regular file (or zip archive if `zip`).
    pub path: Option<&'a str>,
    /// Wrap the file in a `ZipWriter`. Requires `path.is_some()` (validated
    /// in `cli::parse_args`).
    pub zip: bool,
    /// Pinned "current time" in Unix epoch seconds. Used as the entry's
    /// last-modified time in zip mode so the archive bytes are reproducible.
    pub now_unix: i64,
}

/// Owns the chosen sink for the lifetime of the run. Drop the sink only
/// AFTER calling [`OutputSink::finalize`] — `Drop` itself does not flush
/// the zip central directory.
pub enum OutputSink {
    Stdout(BufWriter<io::Stdout>),
    File(BufWriter<File>),
    // Boxed to keep the enum size balanced — `ZipWriter` carries a
    // larger internal state than `BufWriter<File>`, and we don't want
    // every plain-CSV run to pay for that footprint.
    #[cfg(feature = "zip")]
    Zip(Box<zip_sink::ZipSink>),
}

impl OutputSink {
    pub fn open(opts: OutputOptions<'_>) -> io::Result<Self> {
        match (opts.path, opts.zip) {
            (None, false) => Ok(Self::Stdout(BufWriter::with_capacity(
                OUT_BUF_BYTES,
                io::stdout(),
            ))),
            (None, true) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--zip requires --output <PATH> (stdout is not seekable)",
            )),
            (Some(path), false) => {
                let file = File::create(path)?;
                Ok(Self::File(BufWriter::with_capacity(OUT_BUF_BYTES, file)))
            }
            (Some(path), true) => Self::open_zip(path, opts.now_unix),
        }
    }

    #[cfg(feature = "zip")]
    fn open_zip(path: &str, now_unix: i64) -> io::Result<Self> {
        zip_sink::ZipSink::open(path, now_unix).map(|z| Self::Zip(Box::new(z)))
    }

    #[cfg(not(feature = "zip"))]
    fn open_zip(_path: &str, _now_unix: i64) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "--zip support not compiled into this binary (rebuild with the `zip` feature)",
        ))
    }

    /// Borrow the sink as a generic `Write`. Cheap; returns the same trait
    /// object the row generators expect.
    pub fn as_write(&mut self) -> &mut dyn Write {
        match self {
            Self::Stdout(w) => w,
            Self::File(w) => w,
            #[cfg(feature = "zip")]
            Self::Zip(z) => z,
        }
    }

    /// Flush buffers and finalize the archive. ALWAYS call before dropping;
    /// dropping a `ZipWriter` without `finish()` produces a truncated, invalid
    /// archive (no central directory).
    pub fn finalize(self) -> io::Result<()> {
        match self {
            Self::Stdout(mut w) => w.flush(),
            Self::File(mut w) => {
                w.flush()?;
                // BufWriter::Drop would do this anyway, but flush errors are
                // silently swallowed in Drop — surface them here instead.
                Ok(())
            }
            #[cfg(feature = "zip")]
            Self::Zip(z) => z.finalize(),
        }
    }
}

/// Default last-modified date for ZIP entries when `SAMPLE_ACCOUNT_NOW` is
/// not in a representable range (or when zip's `DateTime` rejects it). Picked
/// to be comfortably representable in the DOS time format the ZIP standard
/// uses (year >= 1980).
#[cfg(feature = "zip")]
const DETERMINISTIC_FALLBACK_YEAR: u16 = 2020;

#[cfg(feature = "zip")]
mod zip_sink {
    use super::{BufWriter, File, Write, DETERMINISTIC_FALLBACK_YEAR, OUT_BUF_BYTES};
    use std::io;
    use std::path::Path;

    use time::OffsetDateTime;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, DateTime, ZipWriter};

    /// Pinned compression level. 6 matches zlib's "default compression" and
    /// produces deterministic bytes for our flate2/zlib-rs backend. Do not
    /// change without regenerating any byte-equality fixtures.
    const COMPRESSION_LEVEL: i64 = 6;

    /// Streaming ZIP writer with a single file entry opened up front.
    pub struct ZipSink {
        inner: ZipWriter<BufWriter<File>>,
    }

    impl ZipSink {
        pub fn open(path: &str, now_unix: i64) -> io::Result<Self> {
            let file = File::create(path)?;
            let buf = BufWriter::with_capacity(OUT_BUF_BYTES, file);
            let mut zip = ZipWriter::new(buf);

            let entry_name = derive_entry_name(path);
            let mtime = derive_mtime(now_unix);
            let opts = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .compression_level(Some(COMPRESSION_LEVEL))
                .last_modified_time(mtime)
                .unix_permissions(0o644);

            zip.start_file(entry_name, opts)
                .map_err(|e| io::Error::other(format!("zip start_file: {e}")))?;

            Ok(Self { inner: zip })
        }

        pub fn finalize(self) -> io::Result<()> {
            // ZipWriter::finish writes the central directory and returns the
            // inner writer. We then explicitly flush the BufWriter to surface
            // any deferred File write errors.
            let mut buf = self
                .inner
                .finish()
                .map_err(|e| io::Error::other(format!("zip finish: {e}")))?;
            buf.flush()?;
            Ok(())
        }
    }

    impl Write for ZipSink {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.inner.write(b)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    /// Entry name = basename(path) with a trailing `.zip` stripped, falling
    /// back to `output.csv` when the result is empty.
    fn derive_entry_name(path: &str) -> String {
        let base = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let stripped = base.strip_suffix(".zip").unwrap_or(base);
        if stripped.is_empty() {
            "output.csv".to_string()
        } else {
            stripped.to_string()
        }
    }

    /// Derive a stable `zip::DateTime` from a Unix timestamp. ZIP's DOS time
    /// has 2-second resolution and a 1980-2107 year range; values outside
    /// that range fall back to a fixed deterministic value.
    fn derive_mtime(now_unix: i64) -> DateTime {
        if let Ok(dt) = OffsetDateTime::from_unix_timestamp(now_unix) {
            // round seconds down to 2-second resolution to match DOS time.
            let secs = (dt.second() / 2) * 2;
            if let Ok(z) = DateTime::from_date_and_time(
                dt.year() as u16,
                dt.month() as u8,
                dt.day(),
                dt.hour(),
                dt.minute(),
                secs,
            ) {
                return z;
            }
        }
        // Safe constants — these cannot fail.
        DateTime::from_date_and_time(DETERMINISTIC_FALLBACK_YEAR, 1, 1, 0, 0, 0)
            .expect("fallback DateTime is always valid")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn entry_name_strips_zip_suffix() {
            assert_eq!(derive_entry_name("out.zip"), "out");
            assert_eq!(derive_entry_name("data.csv.zip"), "data.csv");
            assert_eq!(derive_entry_name("/tmp/sub/dir/bundle.zip"), "bundle");
        }

        #[test]
        fn entry_name_falls_back_when_path_is_dot_zip() {
            assert_eq!(derive_entry_name(".zip"), "output.csv");
        }

        #[test]
        fn entry_name_keeps_base_when_no_zip_suffix() {
            assert_eq!(derive_entry_name("out.csv"), "out.csv");
            assert_eq!(derive_entry_name("/tmp/foo"), "foo");
        }

        #[test]
        fn mtime_deterministic_for_pinned_epoch() {
            let a = derive_mtime(1_700_000_000);
            let b = derive_mtime(1_700_000_000);
            // DateTime is Copy/Eq via its packed representation.
            assert_eq!(a.datepart(), b.datepart());
            assert_eq!(a.timepart(), b.timepart());
        }
    }
}
