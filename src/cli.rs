//! Hand-written argv parser. Mirrors C++ getopt_long behavior with one
//! extra guarantee: flag occurrence order = output column order.
//!
//! Supports:
//! - `-h` / `--help`
//! - short flag bundles like `-ilfm` (each char is a separate field flag)
//! - long flags `--<name>` resolved against `registry::FIELDS`
//! - `--telehpne` legacy alias for `--telephone` (preserves typo)
//! - `-j N` / `-jN` / `--jobs N` / `--jobs=N` for thread count
//!   (0 = auto-detect cores, 1 = single-threaded, N>=2 = N threads)
//! - `--output PATH` / `--output=PATH` writes CSV to a file instead of stdout.
//!   Long-only — `'o'` is taken by the `agegroup` field flag.
//! - `--zip` wraps the output in a single-entry ZIP archive. Requires
//!   `--output` (ZIP requires a seekable sink; stdout is not seekable).
//! - one positional integer = COUNT (default 100, last one wins)
//! - unknown flags → `error` set, callers print to stderr and exit 2

use std::io::{self, Write};

use crate::registry::{self, Field};

pub const DEFAULT_ROW_COUNT: u32 = 100;
pub const DEFAULT_JOBS: u32 = 1;

#[derive(Default)]
pub struct CliArgs {
    pub selected_fields: Vec<&'static Field>,
    pub count: u32,
    /// Thread count requested by the user.
    /// `0` means auto-detect (use available CPU cores).
    /// `1` means single-threaded sequential.
    /// `N >= 2` means `N` worker threads via rayon.
    pub jobs: u32,
    /// Optional output file path. `None` = stdout.
    pub output: Option<String>,
    /// Wrap output in a single-entry ZIP archive. Requires `output`.
    pub zip: bool,
    pub help: bool,
    pub error: Option<String>,
}

impl CliArgs {
    /// Resolve `jobs` into an actual thread count.
    /// `0 → available_parallelism()`, otherwise the configured value.
    pub fn effective_jobs(&self) -> usize {
        match self.jobs {
            0 => std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            n => n as usize,
        }
    }
}

pub fn parse_args<I, S>(args: I) -> CliArgs
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    // Materialize argv into a Vec so we can advance the cursor when a flag
    // consumes the next token (e.g. `-j 4`).
    let argv: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();

    let mut out = CliArgs {
        count: DEFAULT_ROW_COUNT,
        jobs: DEFAULT_JOBS,
        ..CliArgs::default()
    };

    let mut i = 0usize;
    while i < argv.len() {
        let token = argv[i].as_str();
        if token.is_empty() {
            i += 1;
            continue;
        }

        if token == "-h" || token == "--help" {
            out.help = true;
            return out;
        }

        // Jobs: -j N / -jN / --jobs N / --jobs=N
        if let Some(jobs_value) = parse_jobs_token(token, &argv, &mut i) {
            match jobs_value {
                Ok(n) => out.jobs = n,
                Err(e) => {
                    out.error = Some(e);
                    return out;
                }
            }
            i += 1;
            continue;
        }

        // Output path: --output PATH / --output=PATH (long-only because
        // 'o' is taken by the `agegroup` field flag).
        if let Some(output_value) = parse_output_token(token, &argv, &mut i) {
            match output_value {
                Ok(p) => out.output = Some(p),
                Err(e) => {
                    out.error = Some(e);
                    return out;
                }
            }
            i += 1;
            continue;
        }

        // ZIP toggle.
        if token == "--zip" {
            out.zip = true;
            i += 1;
            continue;
        }

        if let Some(name) = token.strip_prefix("--") {
            // Long flag.
            if name == "telehpne" {
                if let Some(f) = registry::find_by_long("telephone") {
                    out.selected_fields.push(f);
                    i += 1;
                    continue;
                }
            }
            match registry::find_by_long(name) {
                Some(f) => out.selected_fields.push(f),
                None => {
                    out.error = Some(format!("unrecognized option: --{name}"));
                    return out;
                }
            }
        } else if let Some(rest) = token.strip_prefix('-') {
            if rest.is_empty() {
                out.error = Some("empty option flag '-'".into());
                return out;
            }
            // Short bundle: each char is a separate flag.
            for c in rest.chars() {
                match registry::find_by_short(c) {
                    Some(f) => out.selected_fields.push(f),
                    None => {
                        out.error = Some(format!("unrecognized option: -{c}"));
                        return out;
                    }
                }
            }
        } else {
            // Positional. C++ takes the first non-flag argument as COUNT
            // (atoi, > 0). We match that.
            if let Ok(n) = token.parse::<u32>() {
                if n > 0 {
                    out.count = n;
                }
            }
        }
        i += 1;
    }

    if out.selected_fields.is_empty() {
        if let Some(id) = registry::find_by_short('i') {
            out.selected_fields.push(id);
        }
    }

    // Cross-flag validation. ZIP needs a seekable sink (central directory
    // is appended at the end and local headers are patched in place), and
    // stdout has no seek — so refuse `--zip` without `--output`.
    if out.zip && out.output.is_none() {
        out.error = Some("--zip requires --output <PATH> (stdout is not seekable)".into());
    }

    out
}

/// Returns `Some(Ok(n))` if the token was a `-j` / `--jobs` form (and may
/// advance `i` to consume the value token). Returns `Some(Err)` if the
/// token *was* a jobs flag but the value was missing or unparseable.
/// Returns `None` if the token is not a jobs flag at all.
fn parse_jobs_token(token: &str, argv: &[String], i: &mut usize) -> Option<Result<u32, String>> {
    // Form: --jobs=N
    if let Some(v) = token.strip_prefix("--jobs=") {
        return Some(parse_jobs_value(v));
    }
    // Form: --jobs N
    if token == "--jobs" {
        let next = argv.get(*i + 1);
        return Some(match next {
            Some(v) => {
                *i += 1; // consume value
                parse_jobs_value(v)
            }
            None => Err("missing argument for --jobs".into()),
        });
    }
    // Form: -j N
    if token == "-j" {
        let next = argv.get(*i + 1);
        return Some(match next {
            Some(v) => {
                *i += 1;
                parse_jobs_value(v)
            }
            None => Err("missing argument for -j".into()),
        });
    }
    // Form: -jN where N is digits. Make sure not to swallow an unrelated
    // bundle that happens to start with j (no field uses 'j' as short flag).
    if let Some(rest) = token.strip_prefix("-j") {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(parse_jobs_value(rest));
        }
    }
    None
}

fn parse_jobs_value(v: &str) -> Result<u32, String> {
    v.parse::<u32>()
        .map_err(|e| format!("invalid jobs value '{v}': {e}"))
}

/// Returns `Some(Ok(path))` if the token was a `--output` form (and may
/// advance `i` to consume the value token). Returns `Some(Err)` if the
/// token *was* the flag but the value was missing or empty. Returns
/// `None` if not an --output flag.
fn parse_output_token(
    token: &str,
    argv: &[String],
    i: &mut usize,
) -> Option<Result<String, String>> {
    if let Some(v) = token.strip_prefix("--output=") {
        if v.is_empty() {
            return Some(Err("missing argument for --output".into()));
        }
        return Some(Ok(v.to_string()));
    }
    if token == "--output" {
        return Some(match argv.get(*i + 1) {
            Some(v) if !v.is_empty() => {
                *i += 1;
                Ok(v.clone())
            }
            _ => Err("missing argument for --output".into()),
        });
    }
    None
}

pub fn print_help<W: Write>(out: &mut W, prog: &str) -> io::Result<()> {
    writeln!(
        out,
        "Usage: {prog} [OPTIONS] [COUNT]\n\n\
         Generate synthetic Japanese sample-account records as CSV on stdout.\n\n\
         Output columns are emitted in the order their flags appear on the\n\
         command line. With no flags, a single id column is emitted.\n\
         COUNT defaults to {DEFAULT_ROW_COUNT}.\n\n\
         Options:\n  \
         -h, --help            show this help and exit\n  \
         -j, --jobs <N>        thread count (0 = auto-detect cores, 1 = single, N = threads; default 1)\n  \
             --output <PATH>   write CSV to PATH instead of stdout\n  \
             --zip             wrap output in a single-entry ZIP archive (requires --output)"
    )?;
    for f in registry::FIELDS {
        writeln!(out, "  -{}, --{:<13} {}", f.short, f.long, f.desc)?;
    }
    writeln!(
        out,
        "\nAliases:\n  --telehpne            legacy alias for --telephone\n\n\
         Environment variables (mainly for testing):\n  \
         SAMPLE_ACCOUNT_SEED   pin RNG seed for reproducible output\n  \
         SAMPLE_ACCOUNT_NOW    pin \"current time\" (Unix epoch seconds; also pins the\n                         \
                       ZIP entry's last-modified timestamp for reproducible archives)\n\n\
         Examples:\n  \
         {prog} -ilfm 10                       # id, last/first name (kanji,kana), email\n  \
         {prog} --age --prefecture 5           # age and prefecture for 5 rows\n  \
         {prog} -ilfmpwc 100 > out.csv         # full record with address columns\n  \
         {prog} -j 0 -ilfm 100000              # all cores, 100k rows\n  \
         {prog} -ilfm 100 --output out.csv     # write CSV to a file\n  \
         {prog} -ilfm 100 --output out.zip --zip  # write a ZIP archive containing out.csv"
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> CliArgs {
        parse_args(args.iter().copied())
    }

    fn shorts(args: &CliArgs) -> Vec<char> {
        args.selected_fields.iter().map(|f| f.short).collect()
    }

    #[test]
    fn no_args_defaults_to_id_field_and_count_100() {
        let a = parse(&[]);
        assert_eq!(shorts(&a), vec!['i']);
        assert_eq!(a.count, DEFAULT_ROW_COUNT);
        assert_eq!(a.jobs, DEFAULT_JOBS);
        assert!(!a.help);
        assert!(a.error.is_none());
    }

    #[test]
    fn short_bundle_is_expanded_in_order() {
        let a = parse(&["-ilfm"]);
        assert_eq!(shorts(&a), vec!['i', 'l', 'f', 'm']);
    }

    #[test]
    fn separate_long_flags_preserve_order() {
        let a = parse(&["--age", "--prefecture", "5"]);
        assert_eq!(shorts(&a), vec!['a', 'p']);
        assert_eq!(a.count, 5);
    }

    #[test]
    fn telehpne_typo_alias_maps_to_telephone() {
        let a = parse(&["--telehpne"]);
        assert_eq!(shorts(&a), vec!['t']);
    }

    #[test]
    fn help_flag_short_circuits() {
        let a = parse(&["-ilfm", "--help", "--prefecture"]);
        assert!(a.help);
        assert_eq!(shorts(&a), vec!['i', 'l', 'f', 'm']);
    }

    #[test]
    fn unknown_short_flag_sets_error() {
        let a = parse(&["-iZ"]);
        assert!(a.error.is_some(), "expected error for unknown -Z");
    }

    #[test]
    fn unknown_long_flag_sets_error() {
        let a = parse(&["--nope"]);
        assert!(a.error.is_some());
    }

    #[test]
    fn count_zero_is_ignored_keeping_default() {
        let a = parse(&["-i", "0"]);
        assert_eq!(a.count, DEFAULT_ROW_COUNT);
    }

    #[test]
    fn last_count_wins() {
        let a = parse(&["-i", "5", "10"]);
        assert_eq!(a.count, 10);
    }

    #[test]
    fn duplicate_flags_emit_column_twice() {
        let a = parse(&["-ii"]);
        assert_eq!(shorts(&a), vec!['i', 'i']);
    }

    // -------- jobs / -j parsing --------

    #[test]
    fn dash_j_with_space_consumes_next_token() {
        let a = parse(&["-j", "4", "-i", "10"]);
        assert_eq!(a.jobs, 4);
        assert_eq!(shorts(&a), vec!['i']);
        assert_eq!(a.count, 10);
    }

    #[test]
    fn dash_j_attached_form() {
        let a = parse(&["-j4"]);
        assert_eq!(a.jobs, 4);
    }

    #[test]
    fn long_jobs_with_space() {
        let a = parse(&["--jobs", "8"]);
        assert_eq!(a.jobs, 8);
    }

    #[test]
    fn long_jobs_with_equals() {
        let a = parse(&["--jobs=2"]);
        assert_eq!(a.jobs, 2);
    }

    #[test]
    fn jobs_zero_means_auto() {
        let a = parse(&["-j", "0"]);
        assert_eq!(a.jobs, 0);
        // effective_jobs returns >= 1 (auto-detected cores or 1 fallback).
        assert!(a.effective_jobs() >= 1);
    }

    #[test]
    fn jobs_one_means_single() {
        let a = parse(&["-j", "1"]);
        assert_eq!(a.jobs, 1);
        assert_eq!(a.effective_jobs(), 1);
    }

    #[test]
    fn jobs_missing_value_sets_error() {
        let a = parse(&["-j"]);
        assert!(a.error.is_some());
    }

    #[test]
    fn jobs_unparseable_value_sets_error() {
        let a = parse(&["-j", "abc"]);
        assert!(a.error.is_some());
    }

    #[test]
    fn jobs_does_not_consume_field_letters() {
        // -i is a field flag, NOT confused with -j parsing.
        let a = parse(&["-i", "5"]);
        assert_eq!(shorts(&a), vec!['i']);
        assert_eq!(a.jobs, DEFAULT_JOBS);
    }

    // -------- output / zip parsing --------

    #[test]
    fn output_default_is_none() {
        let a = parse(&["-i", "5"]);
        assert_eq!(a.output, None);
        assert!(!a.zip);
        assert!(a.error.is_none());
    }

    #[test]
    fn output_long_with_space_consumes_next_token() {
        let a = parse(&["--output", "out.csv", "-ilfm", "5"]);
        assert_eq!(a.output.as_deref(), Some("out.csv"));
        assert_eq!(shorts(&a), vec!['i', 'l', 'f', 'm']);
        assert_eq!(a.count, 5);
    }

    #[test]
    fn output_long_with_equals() {
        let a = parse(&["--output=out.csv", "-i", "5"]);
        assert_eq!(a.output.as_deref(), Some("out.csv"));
    }

    #[test]
    fn output_missing_value_sets_error() {
        let a = parse(&["--output"]);
        assert!(a.error.is_some());
    }

    #[test]
    fn output_empty_value_sets_error() {
        let a = parse(&["--output="]);
        assert!(a.error.is_some());
    }

    #[test]
    fn zip_alone_requires_output() {
        let a = parse(&["--zip", "-i", "5"]);
        assert!(a.zip);
        assert!(
            a.error.as_ref().is_some_and(|s| s.contains("--output")),
            "expected error mentioning --output, got {:?}",
            a.error
        );
    }

    #[test]
    fn zip_with_output_parses_cleanly() {
        let a = parse(&["--zip", "--output", "out.zip", "-ilfm", "5"]);
        assert!(a.zip);
        assert_eq!(a.output.as_deref(), Some("out.zip"));
        assert!(a.error.is_none());
        assert_eq!(shorts(&a), vec!['i', 'l', 'f', 'm']);
    }

    #[test]
    fn output_does_not_swallow_field_letters() {
        // -i should still be a field flag even if --output is later in argv.
        let a = parse(&["-i", "--output", "x.csv", "5"]);
        assert_eq!(shorts(&a), vec!['i']);
        assert_eq!(a.output.as_deref(), Some("x.csv"));
        assert_eq!(a.count, 5);
    }
}
