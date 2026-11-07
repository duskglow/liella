//! `remez`: fits minimax polynomials described by fit files and writes the generated Rust
//! coefficient files (D-10.3). See `liella_mathtools::remez` for the algorithm and the fit file
//! format.
//!
//! Usage: `cargo run --release -p liella-mathtools --bin remez -- [--check] <fit files...>`.
//! With `--check`, nothing is written: each output is regenerated in memory and compared with
//! the file on disk, and the exit status is 1 if any differs.
//!
//! The fit files are handled concurrently, at most `available_parallelism()` at once (the
//! affinity mask), each fit's output buffered and printed in command-line order; the exit status
//! is the failure of any fit (D-10.54). With `--check`, a verdict whose inputs are unchanged since
//! it last checked up to date is replayed from the verdict cache
//! (`liella_mathtools::check_cache`) without fitting; `LIELLA_REMEZ_NO_CACHE=1` turns that off.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use liella_mathtools::check_cache::{self, Cache, KeyInputs};
use liella_mathtools::remez::{generate, parse_fit};

/// The workspace root (this crate lives in `crates/mathtools`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// How the generated header names the fit file: relative to the workspace root when possible,
/// with `/` separators, so the output does not depend on the invoking directory.
fn display_path(fit: &Path, root: &Path) -> String {
    let rel = match (fit.canonicalize(), root.canonicalize()) {
        (Ok(f), Ok(r)) => f.strip_prefix(&r).map(Path::to_path_buf).unwrap_or(f),
        _ => fit.to_path_buf(),
    };
    rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// One fit's result: its buffered standard output and error, and whether it passed.
#[derive(Default)]
struct Outcome {
    out: String,
    err: String,
    ok: bool,
}

fn run(path: &str, check: bool, root: &Path, cache: Option<&Cache>, o: &mut Outcome) -> Result<bool, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let text = std::str::from_utf8(&bytes).map_err(|e| format!("{path}: {e}"))?;
    let spec = parse_fit(text).map_err(|e| format!("{path}: {e}"))?;
    let display = display_path(Path::new(path), root);
    let out = root.join(&spec.output);
    if !check {
        let g = generate(&spec, &display).map_err(|e| format!("{path}: {e}"))?;
        o.out.push_str(&g.report);
        let out = root.join(&g.output);
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&out, g.source.as_bytes()).map_err(|e| format!("{}: {e}", out.display()))?;
        let _ = writeln!(o.out, "  wrote {}", g.output);
        return Ok(true);
    }
    // The coefficient file is read once: the same bytes make the key and the comparison.
    let on_disk = std::fs::read(&out).ok();
    let key = cache.map(|c| {
        check_cache::check_key(&KeyInputs {
            image: &c.image,
            mode: "--check",
            fit_arg: path,
            fit_display: &display,
            fit: &bytes,
            coeff_path: &spec.output,
            coeff: on_disk.as_deref(),
        })
    });
    if let (Some(c), Some(k)) = (cache, key.as_deref()) {
        if let Some(report) = c.lookup(k) {
            o.out.push_str(&report);
            let _ = writeln!(o.out, "  {}: up to date (cached {k})", spec.output);
            return Ok(true);
        }
    }
    let g = generate(&spec, &display).map_err(|e| format!("{path}: {e}"))?;
    o.out.push_str(&g.report);
    let on_disk = on_disk.and_then(|b| String::from_utf8(b).ok()).unwrap_or_default();
    if g.output != spec.output || on_disk != g.source {
        let _ = writeln!(o.err, "{}: out of date (regenerate with `cargo run --release -p liella-mathtools --bin remez -- {path}`)", g.output);
        return Ok(false);
    }
    let _ = writeln!(o.out, "  {}: up to date", g.output);
    if let (Some(c), Some(k)) = (cache, key.as_deref()) {
        if let Err(e) = c.store(k, &g.report) {
            let _ = writeln!(o.err, "remez: verdict not cached: {e}");
        }
    }
    Ok(true)
}

fn run_one(path: &str, check: bool, root: &Path, cache: Option<&Cache>) -> Outcome {
    let mut o = Outcome::default();
    match run(path, check, root, cache, &mut o) {
        Ok(good) => o.ok = good,
        Err(e) => {
            let _ = writeln!(o.err, "error: {e}");
            o.ok = false;
        }
    }
    o
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let files: Vec<&String> = args.iter().filter(|a| *a != "--check").collect();
    if files.is_empty() {
        eprintln!("usage: remez [--check] <fit files...>");
        return ExitCode::from(2);
    }
    let root = workspace_root();
    // The generating mode never reads the cache.
    let cache = if check && !check_cache::disabled() {
        match Cache::open() {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("remez: verdict cache off: {e}");
                None
            }
        }
    } else {
        None
    };
    let n = files.len();
    let workers = std::thread::available_parallelism().map_or(1, |p| p.get()).clamp(1, n);
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel::<(usize, Outcome)>();
    let ok = std::thread::scope(|s| {
        for _ in 0..workers {
            let tx = tx.clone();
            let (next, files, root, cache) = (&next, &files, &root, cache.as_ref());
            s.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(f) = files.get(i) else { break };
                    if tx.send((i, run_one(f, check, root, cache))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        // Print each fit's output in command-line order as soon as every earlier fit is done.
        let mut pending: BTreeMap<usize, Outcome> = BTreeMap::new();
        let mut emitted = 0;
        let mut ok = true;
        for (i, o) in rx {
            pending.insert(i, o);
            while let Some(o) = pending.remove(&emitted) {
                print!("{}", o.out);
                eprint!("{}", o.err);
                ok &= o.ok;
                emitted += 1;
            }
        }
        // A worker that died early leaves fits unreported: that is a failure, never a pass.
        ok && emitted == n
    });
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
