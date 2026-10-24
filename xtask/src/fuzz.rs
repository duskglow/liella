//! `cargo xtask fuzz`: the fuzz harnesses of `fuzz/` (implementation-plan.md §6, "Fuzzing
//! infrastructure"). libFuzzer through `cargo fuzz`, with the nightly toolchain named in
//! `fuzz/rust-toolchain.toml` (installed beside the pinned stable one, which the products keep).
//!
//! ```text
//! cargo xtask fuzz --list
//! cargo xtask fuzz <target>|--all --time <minutes> [--chunk <minutes>] [--jobs <n>] [--asan]
//! cargo xtask fuzz <target> --repro <file>    run one input (a crash artifact)
//! cargo xtask fuzz <target> --tmin <file>     minimise a crashing input
//! cargo xtask fuzz <target> --cmin            minimise the corpus (keeps coverage)
//! ```
//!
//! The corpus lives outside the repository, in `$LIELLA_FUZZ_CORPUS/<target>/corpus` (default
//! `~/liella-fuzz-corpus`), with crash artifacts beside it in `artifacts/`; the repository's
//! seeds (`fuzz/seeds/<target>`) are read as a second corpus and never written. A run is a
//! series of chunks, each a fresh libFuzzer process resuming from the corpus; the first crash,
//! time-out or memory-limit report ends the run. On the shared machines the whole command runs
//! under the queue (`~/bin/shared`), and one invocation must stay well under its 150-minute cap:
//! more than [`MAX_MINUTES`] in total is refused.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The longest run one invocation accepts (the queue caps a job at 150 minutes, the build
/// included).
const MAX_MINUTES: u64 = 140;

/// Per-target libFuzzer settings: the largest input, the time-out per input (seconds), the
/// memory limit (MiB), and the longest chunk (minutes; short for the targets that leak their
/// `'static` input, `liella_fuzz::LEAK_BUDGET`).
struct Settings {
    max_len: usize,
    timeout: u32,
    rss_mb: u32,
    chunk: u64,
}

fn settings(target: &str) -> Settings {
    match target {
        "locale" => Settings { max_len: 1 << 20, timeout: 20, rss_mb: 4096, chunk: 10 },
        "charmap" => Settings { max_len: 1 << 16, timeout: 20, rss_mb: 4096, chunk: 20 },
        "nscache" => Settings { max_len: 1 << 16, timeout: 10, rss_mb: 2048, chunk: 30 },
        "regex" | "regex_dfa" | "glob" | "wordexp" | "printf" | "scanf" => Settings { max_len: 4096, timeout: 10, rss_mb: 2048, chunk: 30 },
        _ => Settings { max_len: 16384, timeout: 10, rss_mb: 2048, chunk: 30 },
    }
}

/// The targets: the `[[bin]]` names of `fuzz/Cargo.toml`.
fn targets(root: &Path) -> Result<Vec<String>, String> {
    let manifest = fs::read_to_string(root.join("fuzz/Cargo.toml")).map_err(|e| format!("fuzz/Cargo.toml: {e}"))?;
    let mut out = Vec::new();
    let mut in_bin = false;
    for line in manifest.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_bin = l == "[[bin]]";
        } else if in_bin {
            if let Some(v) = l.strip_prefix("name = ") {
                out.push(v.trim_matches('"').to_string());
            }
        }
    }
    Ok(out)
}

/// The nightly toolchain of `fuzz/rust-toolchain.toml`.
fn toolchain(root: &Path) -> Result<String, String> {
    let t = fs::read_to_string(root.join("fuzz/rust-toolchain.toml")).map_err(|e| format!("fuzz/rust-toolchain.toml: {e}"))?;
    t.lines()
        .filter_map(|l| l.trim().strip_prefix("channel"))
        .filter_map(|v| v.split('"').nth(1))
        .next()
        .map(str::to_string)
        .ok_or_else(|| "fuzz/rust-toolchain.toml names no channel".into())
}

fn corpus_root() -> PathBuf {
    if let Some(d) = std::env::var_os("LIELLA_FUZZ_CORPUS") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join("liella-fuzz-corpus")
}

/// `rustup run <nightly> cargo fuzz <args>` in `fuzz/`, with the environment of this `cargo`
/// invocation removed so that the nightly toolchain builds into a target directory of its own.
fn cargo_fuzz(root: &Path, nightly: &str) -> Command {
    let mut c = nightly_cargo(root, nightly);
    c.arg("fuzz");
    c
}

/// `rustup run <nightly> cargo` in `fuzz/` (environment of this `cargo` removed, own target dir).
fn nightly_cargo(root: &Path, nightly: &str) -> Command {
    let rustup = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .map(|h| h.join("bin/rustup"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("rustup"));
    let mut c = Command::new(rustup);
    c.current_dir(root.join("fuzz")).args(["run", nightly, "cargo"]);
    for v in ["RUSTUP_TOOLCHAIN", "CARGO", "RUSTC", "RUSTDOC", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_MANIFEST_DIR", "CARGO_PKG_NAME"] {
        c.env_remove(v);
    }
    if let Some(t) = std::env::var_os("CARGO_TARGET_DIR") {
        c.env("CARGO_TARGET_DIR", PathBuf::from(t).join("fuzz"));
    }
    c
}

/// The build options every subcommand shares: release with debug assertions (so integer
/// overflow panics, as in the test builds), no sanitizer unless `--asan` (the library is safe
/// Rust; findings are panics, time-outs and memory blow-ups).
fn build_opts(asan: bool) -> Vec<&'static str> {
    vec!["--release", "--debug-assertions", "--sanitizer", if asan { "address" } else { "none" }]
}

fn status(c: &mut Command) -> Result<bool, String> {
    c.status().map(|s| s.success()).map_err(|e| format!("cannot run {c:?}: {e} (is cargo-fuzz installed? `cargo install cargo-fuzz`)"))
}

/// `cargo xtask fuzz-check`: type-check every fuzz target (HYGIENE-1). The harnesses are a workspace of
/// their own on a pinned nightly, so a signature change in a library crate breaks them without any gate
/// noticing (wordexp after WORDEXP-3, regex after RX-T2-1: every fuzz binary failed to build for days).
/// `cargo check --bins` needs no fuzzer runtime or sanitizer, and is incremental. The gate runs it
/// (`xtask test`, and `affected` whenever `crates/` or `fuzz/` changed).
pub fn check(root: &Path) -> Result<(), String> {
    let nightly = toolchain(root)?;
    let mut c = nightly_cargo(root, &nightly);
    c.args(["check", "--bins"]);
    if status(&mut c)? {
        println!("xtask fuzz-check: all {} fuzz targets type-check", targets(root)?.len());
        Ok(())
    } else {
        Err("the fuzz targets do not build (fuzz/ is a workspace of its own: a library signature changed? see `cargo xtask fuzz-check`)".into())
    }
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let all = targets(root)?;
    let mut target: Option<String> = None;
    let mut minutes: Option<u64> = None;
    let mut chunk: Option<u64> = None;
    let mut jobs = 1u32;
    let mut asan = false;
    let mut repro: Option<String> = None;
    let mut tmin: Option<String> = None;
    let mut cmin = false;
    let mut every = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--list" => {
                for t in &all {
                    println!("{t}");
                }
                return Ok(());
            }
            "--time" => minutes = Some(value("--time")?.parse().map_err(|e| format!("--time: {e}"))?),
            "--chunk" => chunk = Some(value("--chunk")?.parse().map_err(|e| format!("--chunk: {e}"))?),
            "--jobs" => jobs = value("--jobs")?.parse().map_err(|e| format!("--jobs: {e}"))?,
            "--asan" => asan = true,
            "--repro" => repro = Some(value("--repro")?),
            "--tmin" => tmin = Some(value("--tmin")?),
            "--cmin" => cmin = true,
            "--all" => every = true,
            t if !t.starts_with('-') && target.is_none() => target = Some(t.to_string()),
            other => return Err(format!("unknown argument {other}; see `cargo xtask fuzz --list` and xtask/src/fuzz.rs")),
        }
    }
    let chosen: Vec<String> = match (&target, every) {
        (Some(t), false) if all.contains(t) => vec![t.clone()],
        (Some(t), false) => return Err(format!("no fuzz target {t}; targets: {}", all.join(" "))),
        (None, true) => all.clone(),
        _ => return Err("name one target, or --all".into()),
    };
    let nightly = toolchain(root)?;
    let opts = build_opts(asan);
    if std::env::var_os("SLOT_HELD").is_none() && Path::new(&format!("{}/bin/shared", std::env::var("HOME").unwrap_or_default())).exists() {
        eprintln!("xtask fuzz: warning: not running under the machine queue (~/bin/shared)");
    }

    if let Some(file) = repro.or(tmin.clone()) {
        let [t] = chosen.as_slice() else { return Err("--repro and --tmin take one target".into()) };
        let file = std::fs::canonicalize(&file).map_err(|e| format!("{file}: {e}"))?;
        let mut c = cargo_fuzz(root, &nightly);
        if tmin.is_some() {
            c.arg("tmin").args(&opts).arg(t).arg(&file).args(["--", &format!("-timeout={}", settings(t).timeout)]);
        } else {
            c.arg("run").args(&opts).arg(t).arg(&file).args(["--", &format!("-timeout={}", settings(t).timeout), &format!("-rss_limit_mb={}", settings(t).rss_mb)]);
        }
        return if status(&mut c)? { Ok(()) } else { Err(format!("{t} fails on {}", file.display())) };
    }

    if cmin {
        for t in &chosen {
            let corpus = corpus_root().join(t).join("corpus");
            fs::create_dir_all(&corpus).map_err(|e| format!("{}: {e}", corpus.display()))?;
            if !status(cargo_fuzz(root, &nightly).arg("cmin").args(&opts).arg(t).arg(&corpus))? {
                return Err(format!("cmin of {t} failed"));
            }
        }
        return Ok(());
    }

    let minutes = minutes.ok_or("--time <minutes> is required")?;
    let total = minutes.saturating_mul(u64::try_from(chosen.len()).unwrap_or(u64::MAX));
    if minutes == 0 || total > MAX_MINUTES {
        return Err(format!("{total} minutes in one invocation: keep it between 1 and {MAX_MINUTES} (the queue caps a job at 150); run longer campaigns as repeated invocations, which resume from the corpus"));
    }
    if !status(cargo_fuzz(root, &nightly).arg("build").args(&opts))? {
        return Err("the fuzz targets do not build".into());
    }
    for t in &chosen {
        let s = settings(t);
        let dir = corpus_root().join(t);
        let corpus = dir.join("corpus");
        let artifacts = dir.join("artifacts");
        fs::create_dir_all(&corpus).map_err(|e| format!("{}: {e}", corpus.display()))?;
        fs::create_dir_all(&artifacts).map_err(|e| format!("{}: {e}", artifacts.display()))?;
        let seeds = root.join("fuzz/seeds").join(t);
        let per_chunk = chunk.unwrap_or(s.chunk).clamp(1, minutes);
        let mut left = minutes;
        let mut n = 0u32;
        while left > 0 {
            let this = per_chunk.min(left);
            left -= this;
            n += 1;
            println!("xtask fuzz: {t}: chunk {n}, {this} min (corpus {}, artifacts {})", corpus.display(), artifacts.display());
            let mut c = cargo_fuzz(root, &nightly);
            c.arg("run").args(&opts).arg(t).arg(&corpus);
            if seeds.is_dir() {
                c.arg(&seeds);
            }
            c.arg("--").args([
                format!("-max_total_time={}", this * 60),
                format!("-max_len={}", s.max_len),
                format!("-timeout={}", s.timeout),
                format!("-rss_limit_mb={}", s.rss_mb),
                format!("-artifact_prefix={}/", artifacts.display()),
                "-print_final_stats=1".to_string(),
            ]);
            if jobs > 1 {
                c.arg(format!("-fork={jobs}"));
            }
            if !status(&mut c)? {
                return Err(format!(
                    "{t}: libFuzzer reported a failure; the input is in {} (triage: implementation-plan.md §4.5)",
                    artifacts.display()
                ));
            }
        }
        let count = fs::read_dir(&corpus).map(|d| d.count()).unwrap_or(0);
        println!("xtask fuzz: {t}: done, {count} corpus inputs");
    }
    Ok(())
}
