use std::io::{self, BufWriter, Write};
use std::process::ExitCode;

use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use time::OffsetDateTime;

use sample_account::cli::{parse_args, print_help};
use sample_account::field::{Deps, RowContext};
use sample_account::generators::{AddressGenerator, AgeAndDateGenerator, PersonGenerator};
use sample_account::registry::Field;
use sample_account::repos::{default_ages, default_persons, default_prefectures};
use sample_account::rng::{current_time, derive_row_seed, master_seed_from_env, Rng};

/// Average bytes per emitted row. Used to pre-size per-worker chunk
/// buffers. Generous for our 17 fields with kanji + numeric fields.
const ROW_BYTE_HINT: usize = 256;

/// Output flush buffer (1 MiB). Large enough to amortize syscall overhead
/// at multi-million-row counts without ballooning memory.
const OUT_BUF_BYTES: usize = 1 << 20;

/// Rows per parallel batch. Bounds peak memory at roughly
/// `BATCH_ROWS * ROW_BYTE_HINT` bytes (32 MiB) regardless of total row
/// count. Each batch is generated in parallel, then drained to the
/// BufWriter before the next batch starts. This is what lets the runner
/// stream tens-of-billions-of-rows workloads without OOM.
///
/// Tuning notes:
/// - 128k rows × 256 B = 32 MiB peak = comfortable on any modern host.
/// - With 12 workers that's ~10k rows / worker / batch — each batch
///   takes ~1 ms, so per-batch sync overhead is amortized within ~30 ms
///   of work even at 1M total rows.
const BATCH_ROWS: usize = 128 * 1024;

/// Below this count, even the parallel mode falls back to the
/// single-threaded streaming path. Pool startup outweighs any speedup
/// for tiny generations.
const PARALLEL_MIN_ROWS: usize = 1024;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let prog = argv
        .first()
        .map(String::as_str)
        .unwrap_or("sample_account")
        .to_string();

    match try_main(&prog, argv.iter().skip(1).map(|s| s.as_str())) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{prog}: {e}");
            ExitCode::from(1)
        }
    }
}

fn try_main<'a, I>(prog: &str, args: I) -> Result<ExitCode, Box<dyn std::error::Error>>
where
    I: Iterator<Item = &'a str>,
{
    let parsed = parse_args(args);

    if let Some(msg) = &parsed.error {
        eprintln!("{prog}: {msg}");
        eprintln!("Try '{prog} --help' for usage.");
        return Ok(ExitCode::from(2));
    }
    if parsed.help {
        let stdout = io::stdout();
        let mut lock = stdout.lock();
        print_help(&mut lock, prog)?;
        return Ok(ExitCode::SUCCESS);
    }

    // Load all repositories once, up front. Data is embedded in the
    // binary at compile time (see src/repos.rs::EMBEDDED_*_CSV) so this
    // works regardless of the current working directory.
    let persons = default_persons()?;
    let pref_repo = default_prefectures()?;
    let age_repo = default_ages()?;

    // Cache "now" once. Workers reuse these so the hot loop never reads
    // env vars per row.
    let now_unix = current_time();
    let now_year = OffsetDateTime::from_unix_timestamp(now_unix)
        .map(|dt| dt.year())
        .unwrap_or(1970);
    let master_seed = master_seed_from_env();

    // Build generators ONCE outside the worker loop. They borrow into the
    // repositories and are read-only after construction; rayon workers
    // share them via &.
    let person = PersonGenerator::new(&persons);
    let address = AddressGenerator::new(&pref_repo);
    let age_date = AgeAndDateGenerator::with_year(&age_repo, now_year);

    let count = parsed.count as usize;
    let fields: Vec<&'static Field> = parsed.selected_fields.clone();
    let jobs = parsed.effective_jobs();

    let stdout = io::stdout();
    let mut out = BufWriter::with_capacity(OUT_BUF_BYTES, stdout.lock());

    let rc = RowCtx {
        master_seed,
        now_unix,
        person: &person,
        address: &address,
        age_date: &age_date,
        fields: &fields,
    };

    if jobs <= 1 || count < PARALLEL_MIN_ROWS {
        run_serial(&mut out, count, &rc)?;
    } else {
        run_parallel(&mut out, count, &rc, jobs)?;
    }

    out.flush()?;
    Ok(ExitCode::SUCCESS)
}

/// Single-threaded streaming. Reuses one row-sized scratch buffer and
/// writes to the BufWriter row by row. Memory: O(1) regardless of count.
fn run_serial(
    out: &mut dyn Write,
    count: usize,
    rc: &RowCtx<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut scratch = Vec::with_capacity(ROW_BYTE_HINT);
    for row in 0..count {
        scratch.clear();
        append_row(&mut scratch, row as u32, rc);
        out.write_all(&scratch)?;
    }
    Ok(())
}

/// Multi-threaded streaming. Generates `BATCH_ROWS` rows per batch in
/// parallel, drains them to the BufWriter in order, then advances to the
/// next batch. Memory: O(BATCH_ROWS * ROW_BYTE_HINT) regardless of count.
///
/// The rayon pool is built once and reused via `pool.install` for every
/// batch — pool startup is paid only once, not per batch.
fn run_parallel(
    out: &mut dyn Write,
    count: usize,
    rc: &RowCtx<'_>,
    jobs: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let workers = jobs.min(count);
    let pool = ThreadPoolBuilder::new().num_threads(workers).build()?;

    let mut batch_start = 0usize;
    while batch_start < count {
        let batch_end = (batch_start + BATCH_ROWS).min(count);
        let batch_rows = batch_end - batch_start;
        // Don't spawn more workers than we have rows in this batch.
        let workers_for_batch = workers.min(batch_rows);
        let chunk_size = batch_rows / workers_for_batch;

        let chunks: Vec<Vec<u8>> = pool.install(|| {
            (0..workers_for_batch)
                .into_par_iter()
                .map(|wi| {
                    let cstart = batch_start + wi * chunk_size;
                    let cend = if wi == workers_for_batch - 1 {
                        batch_end
                    } else {
                        cstart + chunk_size
                    };
                    let mut buf = Vec::with_capacity((cend - cstart) * ROW_BYTE_HINT);
                    for row in cstart..cend {
                        append_row(&mut buf, row as u32, rc);
                    }
                    buf
                })
                .collect()
        });
        for chunk in chunks {
            out.write_all(&chunk)?;
        }
        batch_start = batch_end;
    }
    Ok(())
}

/// Read-only context shared across all rows of a run. Bundled so workers
/// only capture one reference instead of half a dozen.
struct RowCtx<'a> {
    master_seed: u64,
    now_unix: i64,
    person: &'a PersonGenerator<'a>,
    address: &'a AddressGenerator<'a>,
    age_date: &'a AgeAndDateGenerator<'a>,
    fields: &'a [&'static Field],
}

/// Appends one CSV row to `out`. Each row constructs its own RNG from
/// (master_seed, row); single- and multi-threaded modes therefore produce
/// byte-identical output for the same master seed.
#[inline]
fn append_row(out: &mut Vec<u8>, row: u32, rc: &RowCtx<'_>) {
    let master_seed = rc.master_seed;
    let now_unix = rc.now_unix;
    let person = rc.person;
    let address = rc.address;
    let age_date = rc.age_date;
    let fields = rc.fields;
    let mut rng = Rng::from_seed_with_now(derive_row_seed(master_seed, row), now_unix);

    // Order MUST stay first → last → pref → ward → city → age → roll_date.
    let first = rng.next_i32();
    let last = rng.next_i32();
    let pref_seed = rng.next_i32();
    let pref = address.weighted_prefecture_index(pref_seed);
    let ward = rng.next_i32();
    let city = rng.next_i32();
    let age = rng.next_i32();
    rng.roll_date();

    let ctx = RowContext {
        row: row as i32,
        first,
        last,
        pref,
        ward,
        city,
        age,
    };

    let mut deps = Deps {
        person,
        address,
        age_date,
        rng: &mut rng,
    };
    for (j, field) in fields.iter().enumerate() {
        if j > 0 {
            out.push(b',');
        }
        (field.emit)(out, &ctx, &mut deps);
    }
    out.push(b'\n');
}
