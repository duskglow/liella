//! `cargo xtask plants <file> --dir <new-dir> [--timeout <secs>] [--prepare-timeout <secs>]`: run a story's plants (mutations
//! that its tests must catch) the one way the rules ask for, instead of a hand-written script per
//! story (XTASK-PLANTS; `docs/plants.md`).
//!
//! It removes four failure classes that repeated in hand-written plants scripts:
//! - a reused copy directory (rsync restores old mtimes, so cargo skipped the rebuild and tested a
//!   stale mutant): `--dir` must not exist, and nothing is ever deleted;
//! - a copy without the products (`cargo xtask so` is run in the copy before the baseline);
//! - a products test run against the baseline products, blind to the mutant (XTASK-PLANTS-PRODUCTS-REBUILD: the
//!   products are rebuilt with each plant whose command may use them, see [`cmd_uses_products`]);
//! - an unanchored pattern that mutated the wrong site (every plant must change exactly one site,
//!   and a plant on a `.S` file must name its `liella_machine_<fn>:` label);
//! - a script that always exits 0 (any SURVIVED exits 1; a failed baseline exits 2);
//! - a copy missing a source file a test needs: after the copy every source path (`git ls-files`, tracked
//!   and untracked-not-ignored, in a git work tree; every file and symlink outside `/target` and
//!   `/fuzz/target` in a synced role, which has no `.git`) must exist in it, else the run is refused.
//!
//! A `--dir` under `/dev/shm` prints WARN-DEV-SHM: a test that mounts its own `/dev` or `/dev/shm`
//! hides such a copy (XTASK-PLANTS-TRACKED-COPY: `nscache_mode.sh` binds the copy back; the
//! `nodev` phase of `posixipc_run.sh` cannot).
//!
//! The plants file has one plant per line (blank lines and `#` comments are skipped):
//! `id | path | pattern | cmd | msg:<regex>` or `id | path | pattern | cmd | equivalent:<reason>`,
//! fields separated by ` | ` (space, bar, space). `pattern` is `lit:<old> ==> <new>` (a literal
//! that must occur exactly once) or `perl:s/<re>/<replacement>/<flags>` (applied by `perl`, which
//! must match exactly once; no `g` or `r` flag). `cmd` runs with `sh -c` in the copy's root.
//!
//! Everything is written under `--dir`: the copy in `<dir>/src` and the logs in `<dir>/logs`.

use std::fs;
use std::io::Write as _;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Instant;

/// Exit status: every plant CAUGHT (or EQUIVALENT).
const EXIT_OK: u8 = 0;
/// Exit status: at least one plant SURVIVED, or a step failed after the baseline.
const EXIT_SURVIVED: u8 = 1;
/// Exit status: the product build or a baseline command failed on the unmutated copy.
const EXIT_BASELINE: u8 = 2;
/// Exit status: the input was refused (usage, plants file, an ambiguous plant, an existing `--dir`).
const EXIT_REFUSED: u8 = 3;

/// The default limit for one command, in seconds (a mutant may loop forever). The project's per-plant limit
/// (owner, 2026-11-05): `--timeout 600`.
const DEFAULT_TIMEOUT: u64 = 600;

/// The default limit for the product build (`cargo xtask so`, baseline and per plant), in seconds. It is
/// separate from [`DEFAULT_TIMEOUT`]: that one decides CAUGHT-by-hang for a mutant's tests, and a traced cold
/// build takes longer than it on a slow host. A build that runs into this limit says nothing about the mutant.
const DEFAULT_PREPARE_TIMEOUT: u64 = 3600;

/// The exit status of `timeout(1)` when it ended the command: a mutant that hangs is caught by the limit.
const TIMED_OUT: i32 = 124;

/// How a plant is expected to be judged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// `msg:<regex>`: a failing `cmd` whose log matches the extended regular expression.
    Msg(String),
    /// `equivalent:<reason>`: the mutant behaves like the original by design.
    Equivalent(String),
    /// Neither: caught by exit status alone (WARN-RC-ONLY).
    RcOnly,
}

/// How a plant changes its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// `lit:<old> ==> <new>`.
    Literal { old: String, new: String },
    /// `perl:<s/// expression>`.
    Perl(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plant {
    pub id: String,
    pub path: String,
    pub pattern: Pattern,
    pub cmd: String,
    pub expect: Expect,
    /// PLANTS-UNGROUPED-MUTANTS: the plant is marked `layout:traced` (its command measures the start-up layout).
    /// A command that names a layout test is traced without the mark ([`targets_layout`]).
    pub layout: bool,
}

/// The verdict on one plant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Caught,
    Survived,
    Equivalent,
    WarnRcOnly,
    WarnNoMsg,
    /// The product build for the mutant ran into the prepare timeout: the mutant was not tested.
    BuildTimeout,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Caught => "CAUGHT",
            Verdict::Survived => "SURVIVED",
            Verdict::Equivalent => "EQUIVALENT",
            Verdict::WarnRcOnly => "WARN-RC-ONLY",
            Verdict::WarnNoMsg => "WARN-NO-MSG",
            Verdict::BuildTimeout => "BUILD-TIMEOUT",
        }
    }
}

/// One run: the tree to copy, the plants, the new directory, the command that builds the
/// products in the copy (`cargo xtask so`; the unit tests use a toy tree without one), and which
/// plants' commands use the products (given the copy; [`plants_use_products`]).
pub struct Config {
    pub src: PathBuf,
    /// The plants file as read: `<dir>/results.txt` is this text with the results appended.
    pub text: String,
    pub plants: Vec<Plant>,
    pub dir: PathBuf,
    pub prepare: Vec<String>,
    pub timeout: u64,
    /// The limit for `prepare` (the product build), apart from `timeout`.
    pub prepare_timeout: u64,
    pub uses_products: fn(&Path, &[Plant]) -> Vec<bool>,
}

/// XTASK-PLANTS-PRODUCTS-REBUILD: per plant, whether its command may use the products, so that
/// they are rebuilt with the mutant first. The test targets are judged by the rule
/// `cargo xtask affected` uses to build the products (`Repo::test_uses_products`).
pub fn plants_use_products(copy: &Path, plants: &[Plant]) -> Vec<bool> {
    let repo = crate::affected::Repo::load(copy);
    plants.iter().map(|p| cmd_uses_products(&p.cmd, |krate, name| repo.test_uses_products(krate, name))).collect()
}

/// Whether `cmd` may use the products. It is known not to only when it is one plain `cargo test`
/// whose targets are all named (`--lib`, `--bin <name>`, `--test <name>`) and no named test target
/// uses them (`lookup(package, target) == Some(false)`). Anything else (shell syntax, another
/// program such as `cargo xtask affected`, an unknown option, all targets of a package, an unknown
/// target) may: when unsure, rebuild.
pub fn cmd_uses_products(cmd: &str, lookup: impl Fn(Option<&str>, &str) -> Option<bool>) -> bool {
    if cmd.contains(|c: char| ";&|$`()<>'\"\\*?\n".contains(c)) {
        return true;
    }
    // `--opt=value` is `--opt value`.
    let words: Vec<&str> = cmd.split_whitespace().flat_map(|w| match w.split_once('=') {
        Some((k, v)) if k.starts_with("--") => vec![k, v],
        _ => vec![w],
    }).collect();
    let mut it = words.into_iter();
    if it.next() != Some("cargo") || it.next() != Some("test") {
        return true;
    }
    let (mut package, mut unit, mut tests) = (None, false, Vec::new());
    while let Some(w) = it.next() {
        match w {
            // The test binaries' own arguments.
            "--" => break,
            "-p" | "--package" if package.is_none() => match it.next() {
                Some(p) => package = Some(p),
                None => return true,
            },
            "--lib" => unit = true,
            "--bin" => match it.next() {
                Some(_) => unit = true,
                None => return true,
            },
            "--test" => match it.next() {
                Some(t) => tests.push(t),
                None => return true,
            },
            "-q" | "--quiet" | "--release" | "--no-fail-fast" | "--locked" | "--offline" | "--frozen" => {}
            "--features" | "-F" | "-j" | "--jobs" => {
                it.next();
            }
            // `--tests`, `--all-targets`, `--workspace`, `--doc`, a second package, ...
            w if w.starts_with('-') => return true,
            // A test name filter.
            _ => {}
        }
    }
    if !unit && tests.is_empty() {
        return true;
    }
    tests.into_iter().any(|t| lookup(package, t) != Some(false))
}

/// PLANTS-UNGROUPED-MUTANTS: whether `cmd` measures the start-up layout, so its products must be built traced
/// (`LIELLA_LAYOUT_TRACE=1`) rather than ungrouped: it names a start-up layout test (`cargo xtask affected`'s
/// [`crate::affected::LAYOUT_TESTS`]) or `layout-check`, or it is a `cargo xtask affected ... --run` without
/// `--only` (a change that reaches `libc.a` selects the layout tests). Any other plant can mark itself `layout:traced`.
pub fn targets_layout(cmd: &str) -> bool {
    let names = |w: &str| w.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')).any(|t| t == "layout-check" || crate::affected::LAYOUT_TESTS.iter().any(|(_, name)| t == *name));
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let affected_run = words.windows(2).any(|w| w == ["xtask", "affected"]) && words.contains(&"--run") && !words.iter().any(|w| w.starts_with("--only"));
    affected_run || words.iter().any(|w| names(w))
}

/// The start-up layout of a plants run's product build (`LIELLA_LAYOUT_TRACE`, [`crate::hotlists::TRACE_ENV`]).
fn layout_env(layout: crate::hotlists::Layout) -> &'static str {
    match layout {
        crate::hotlists::Layout::Traced => "1",
        crate::hotlists::Layout::Ungrouped => "ungrouped",
    }
}

fn layout_name(layout: crate::hotlists::Layout) -> &'static str {
    match layout {
        crate::hotlists::Layout::Traced => "traced",
        crate::hotlists::Layout::Ungrouped => "ungrouped",
    }
}

/// The outcome of a run: the exit status and the verdict per plant (in file order).
#[derive(Debug)]
pub struct Outcome {
    pub code: u8,
    pub verdicts: Vec<(String, Verdict)>,
}

/// `cargo xtask plants`: parse the arguments and run.
pub fn main(root: &Path, args: &[String]) -> ExitCode {
    match parse_args(root, args) {
        Ok(cfg) => {
            let out = run(&cfg);
            // The rules ask for every WARN line in the hand-back: repeat them together at the end.
            let warns: Vec<String> = out.verdicts.iter().filter(|(_, v)| matches!(v, Verdict::WarnRcOnly | Verdict::WarnNoMsg)).map(|(id, v)| format!("{id} {}", v.label())).collect();
            if !warns.is_empty() {
                println!("plants: WARN lines for the hand-back: {}", warns.join(", "));
            }
            ExitCode::from(out.code)
        }
        Err(e) => {
            eprintln!("xtask plants: REFUSED: {e}");
            ExitCode::from(EXIT_REFUSED)
        }
    }
}

const USAGE: &str = "usage: cargo xtask plants <plants-file> --dir <new-directory> [--timeout <secs>] [--prepare-timeout <secs>]";

fn parse_args(root: &Path, args: &[String]) -> Result<Config, String> {
    let (mut file, mut dir, mut timeout, mut prepare_timeout) = (None, None, DEFAULT_TIMEOUT, DEFAULT_PREPARE_TIMEOUT);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dir" => dir = Some(PathBuf::from(it.next().ok_or("--dir needs a directory")?)),
            "--timeout" => timeout = it.next().and_then(|t| t.parse().ok()).filter(|t| *t > 0).ok_or("--timeout needs a positive number of seconds")?,
            "--prepare-timeout" => prepare_timeout = it.next().and_then(|t| t.parse().ok()).filter(|t| *t > 0).ok_or("--prepare-timeout needs a positive number of seconds")?,
            s if s.starts_with('-') => return Err(format!("unknown option {s} ({USAGE})")),
            s if file.is_none() => file = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument {s} ({USAGE})")),
        }
    }
    let file = file.ok_or(USAGE)?;
    let dir = dir.ok_or(format!("--dir is required: a NEW directory in RAM, e.g. /dev/shm/claude/<story>/plants-<n> ({USAGE})"))?;
    let text = fs::read_to_string(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let dir = if dir.is_absolute() { dir } else { std::env::current_dir().map_err(|e| e.to_string())?.join(dir) };
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    Ok(Config { src: root.to_path_buf(), plants: parse_plants(&text)?, text, dir, prepare: vec![cargo, "xtask".into(), "so".into()], timeout, prepare_timeout, uses_products: plants_use_products })
}

/// Parse a plants file (see the module comment). Every line is checked; the errors are joined.
pub fn parse_plants(text: &str) -> Result<Vec<Plant>, String> {
    let mut plants: Vec<Plant> = Vec::new();
    let mut errors = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match parse_line(line) {
            Ok(p) if plants.iter().any(|q| q.id == p.id) => errors.push(format!("line {}: duplicate plant id {}", n + 1, p.id)),
            Ok(p) => plants.push(p),
            Err(e) => errors.push(format!("line {}: {e}", n + 1)),
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    if plants.is_empty() {
        return Err("the plants file has no plants".into());
    }
    Ok(plants)
}

fn parse_line(line: &str) -> Result<Plant, String> {
    let mut fields: Vec<&str> = line.split(" | ").map(str::trim).collect();
    // PLANTS-UNGROUPED-MUTANTS: an optional last field `layout:traced`, after the expectation.
    let mut layout = false;
    if fields.len() >= 5 {
        if let Some(v) = fields[fields.len() - 1].strip_prefix("layout:") {
            if v.trim() != "traced" {
                return Err(format!("`layout:{v}`: the only layout mark is `layout:traced` (the plant's products are built with the traced start-up layout)"));
            }
            layout = true;
            fields.pop();
        }
    }
    if fields.len() < 4 {
        return Err("expected `id | path | pattern | cmd | msg:<regex>` (or `equivalent:<reason>` last)".into());
    }
    let (id, path, pattern) = (fields[0], fields[1], fields[2]);
    // The last field (from the fifth on) is the expectation; a `cmd` may itself contain ` | `.
    let last = if fields.len() >= 5 { fields[fields.len() - 1] } else { "" };
    let expect = if let Some(re) = last.strip_prefix("msg:") {
        if re.is_empty() { return Err("msg: needs a regex".into()) }
        Expect::Msg(re.to_string())
    } else if let Some(why) = last.strip_prefix("equivalent:") {
        if why.trim().is_empty() { return Err("equivalent: needs a reason".into()) }
        Expect::Equivalent(why.trim().to_string())
    } else {
        Expect::RcOnly
    };
    let cmd_end = if expect == Expect::RcOnly { fields.len() } else { fields.len() - 1 };
    let cmd = fields[3..cmd_end].join(" | ");
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return Err(format!("bad plant id `{id}` (letters, digits, - _ . only)"));
    }
    let rel = Path::new(path);
    if path.is_empty() || rel.is_absolute() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!("{id}: path `{path}` must be relative to the tree, without `..`"));
    }
    if cmd.trim().is_empty() {
        return Err(format!("{id}: empty cmd"));
    }
    let pattern = if let Some(lit) = pattern.strip_prefix("lit:") {
        // `<old> ==>` with nothing after it deletes `<old>` (the field is trimmed).
        let (old, new) = lit.split_once(" ==> ").or_else(|| lit.strip_suffix(" ==>").map(|o| (o, ""))).ok_or(format!("{id}: lit: needs `<old> ==> <new>`"))?;
        if old.is_empty() { return Err(format!("{id}: empty literal")) }
        Pattern::Literal { old: old.to_string(), new: new.to_string() }
    } else if let Some(e) = pattern.strip_prefix("perl:") {
        check_perl(e).map_err(|m| format!("{id}: {m}"))?;
        Pattern::Perl(e.to_string())
    } else {
        return Err(format!("{id}: pattern must start with lit: or perl:"));
    };
    if path.ends_with(".S") {
        let text = match &pattern { Pattern::Literal { old, .. } => old.as_str(), Pattern::Perl(e) => e.as_str() };
        if !names_machine_anchor(text) {
            return Err(format!("{id}: a plant on a .S file must name its `liella_machine_<fn>:` label (e.g. perl:s/(liella_machine_<fn>:.*?)<pattern>/$1<replacement>/s)"));
        }
    }
    Ok(Plant { id: id.to_string(), path: path.to_string(), pattern, cmd, expect, layout })
}

/// A `perl:` pattern is one `s` substitution changing one site: no `g` (every site) and no `r`
/// (which would leave the file unchanged).
fn check_perl(e: &str) -> Result<(), String> {
    let mut chars = e.chars();
    let delim_ok = chars.next() == Some('s') && chars.next().is_some_and(|d| !d.is_alphanumeric() && !d.is_whitespace());
    if !delim_ok {
        return Err(format!("perl: must be one substitution s/<re>/<replacement>/<flags>, got `{e}`"));
    }
    let flags: String = e.chars().rev().take_while(char::is_ascii_lowercase).collect();
    if flags.contains('g') || flags.contains('r') {
        return Err("perl: the /g and /r flags are refused (a plant changes exactly one site)".into());
    }
    Ok(())
}

fn names_machine_anchor(text: &str) -> bool {
    text.match_indices("liella_machine_").any(|(i, m)| {
        let rest = &text[i + m.len()..];
        let name_len = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').count();
        name_len > 0 && rest[name_len..].starts_with(':')
    })
}

/// The mutant of `src`, or why the plant is refused (no site, more than one site, no change).
pub fn mutate(src: &str, pattern: &Pattern) -> Result<String, String> {
    let (sites, out) = match pattern {
        Pattern::Literal { old, new } => (src.matches(old.as_str()).count(), src.replacen(old.as_str(), new, 1)),
        Pattern::Perl(e) => perl_substitute(src, e)?,
    };
    match sites {
        0 => Err("the pattern matches no site".into()),
        1 if out == src => Err("the pattern matches one site but the replacement changes nothing".into()),
        1 => Ok(out),
        n => Err(format!("the pattern matches {n} sites; a plant must change exactly one (anchor it)")),
    }
}

/// Apply a `s///` expression with perl: the number of sites it matches (the same expression with
/// `/g`) and the text after one substitution.
fn perl_substitute(src: &str, e: &str) -> Result<(usize, String), String> {
    const SCRIPT: &str = r#"binmode STDIN; binmode STDOUT; local $/; my $src = <STDIN>; my $e = $ENV{LIELLA_PLANT_EXPR};
my $all = $src; my $n = eval "\$all =~ ${e}g"; die "bad perl: expression: $@" if $@;
my $one = $src; eval "\$one =~ $e"; die "bad perl: expression: $@" if $@;
print(($n || 0), "\n", $one);"#;
    let mut child = Command::new("perl")
        .args(["-e", SCRIPT])
        .env("LIELLA_PLANT_EXPR", e)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot run perl: {err}"))?;
    let mut stdin = child.stdin.take().ok_or("perl: no stdin")?;
    let input = src.as_bytes().to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = child.wait_with_output().map_err(|err| format!("perl: {err}"))?;
    let _ = writer.join();
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8(out.stdout).map_err(|_| "perl: the result is not UTF-8")?;
    let (n, rest) = text.split_once('\n').ok_or("perl: no result")?;
    Ok((n.trim().parse().map_err(|_| format!("perl: bad site count {n}"))?, rest.to_string()))
}

/// Check every plant against the tree it will run on.
fn check_all(tree: &Path, plants: &[Plant]) -> Result<(), String> {
    let errors: Vec<String> = plants
        .iter()
        .filter_map(|p| {
            let src = fs::read_to_string(tree.join(&p.path)).map_err(|e| format!("cannot read {}: {e}", p.path));
            src.and_then(|s| mutate(&s, &p.pattern)).err().map(|e| format!("{} ({}): {e}", p.id, p.path))
        })
        .collect();
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

/// Whether `dir` lies on a RAM file system (tmpfs, or a zram device), from `/proc/mounts`.
fn on_ram(dir: &Path) -> Option<bool> {
    let mounts = fs::read_to_string("/proc/mounts").ok()?;
    let best = mounts
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() >= 3).then(|| (f[0], f[1].replace("\\040", " "), f[2]))
        })
        .filter(|(_, mnt, _)| dir.starts_with(mnt))
        .max_by_key(|(_, mnt, _)| mnt.len())?;
    Some(best.2 == "tmpfs" || best.2 == "ramfs" || best.0.starts_with("/dev/zram"))
}

/// Run `argv` (or `sh -c <cmd>`) in `cwd` under `timeout`, its output into `log`, with `LIELLA_LAYOUT_TRACE` set
/// for `layout` (a product build, and a command that builds the products itself, get the plant's layout); the exit code.
fn run_logged(cwd: &Path, argv: &[String], log: &Path, timeout: u64, layout: crate::hotlists::Layout) -> Result<i32, String> {
    let file = fs::File::create(log).map_err(|e| format!("cannot create {}: {e}", log.display()))?;
    let err = file.try_clone().map_err(|e| e.to_string())?;
    // The child leads its own process group (timeout(1) makes the same group), so one group signal reaches every
    // grandchild, such as the gdb and probes a build-time trace starts, however the step ended.
    let mut child = Command::new("timeout")
        .args(["-k", "10"])
        .arg(timeout.to_string())
        .args(argv)
        .current_dir(cwd)
        // A target directory from the environment would build (and test) outside the copy.
        .env_remove("CARGO_TARGET_DIR")
        .env(crate::hotlists::TRACE_ENV, layout_env(layout))
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(err)
        .process_group(0)
        .spawn()
        .map_err(|e| format!("cannot run {argv:?}: {e}"))?;
    let pgid = child.id();
    let status = child.wait().map_err(|e| format!("cannot wait for {argv:?}: {e}"))?;
    kill_group(pgid);
    Ok(status.code().unwrap_or(128))
}

/// End every process left in group `pgid`: TERM, then KILL when the group is still there after a grace period.
fn kill_group(pgid: u32) {
    let signal = |sig: &str| Command::new("kill").args([sig, "--"]).arg(format!("-{pgid}")).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    if signal("-TERM") {
        std::thread::sleep(std::time::Duration::from_millis(300));
        signal("-KILL");
    }
}

fn sh(cmd: &str) -> Vec<String> {
    vec!["sh".into(), "-c".into(), cmd.into()]
}

fn tail(log: &Path, n: usize) -> String {
    let text = fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Whether the log matches the extended regular expression (`grep -E`).
fn log_matches(log: &Path, re: &str) -> bool {
    Command::new("grep").args(["-q", "-E", "-e", re]).arg(log).stdin(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// XTASK-PLANTS-CATCHER: what caught a plant, read from its command's log: `<test binary> <test name>` of the
/// first failing test the log names, so a run's record says which test is the evidence for the plant.
///
/// It reads a `cargo test` log (`Running <src> (<path>/<target>-<hash>)` names the binary, `test <name> ... FAILED`
/// the test, `Doc-tests <crate>` the doctests) and a nextest one (`FAIL [ 0.1s] (1/9) <crate>::<target> <name>`, or
/// any other failing status). Without a failing test line: the first panic of a thread named after a test, then a
/// panic outside a named test (`thread 'main'`, `'<unnamed>'`) with the binary it happened in, then the binary
/// cargo asks to rerun, then a build that does not compile.
pub fn catcher(log: &str) -> String {
    let mut binary: Option<String> = None;
    let mut panic: Option<(Option<String>, String)> = None;
    let mut rerun: Option<String> = None;
    let mut compile = false;
    let named = |binary: &Option<String>, test: &str| match binary {
        Some(b) => format!("{b} {test}"),
        None => format!("{test} (the test binary is not named in the log)"),
    };
    for line in log.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Running ") {
            // `Running \`target/debug/xtask ...\`` is cargo running a program, not a test binary.
            if !rest.starts_with('`') {
                let path = rest.rsplit_once('(').map_or(rest, |(_, p)| p.trim_end_matches(')'));
                let file = path.rsplit('/').next().unwrap_or(path);
                let target = file.rsplit_once('-').filter(|(_, h)| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit())).map_or(file, |(s, _)| s);
                binary = Some(target.to_string());
            }
        } else if let Some(krate) = t.strip_prefix("Doc-tests ") {
            binary = Some(format!("doctests::{}", krate.trim()));
        } else if let Some(test) = t.strip_prefix("test ").and_then(|r| r.strip_suffix(" ... FAILED")) {
            return named(&binary, test);
        } else if let Some(id) = nextest_failure(t) {
            return id;
        } else if let Some((thread, after)) = t.strip_prefix("thread '").and_then(|r| r.split_once('\'')) {
            if panic.is_none() && after.contains("panicked at") {
                panic = Some((binary.clone(), thread.to_string()));
            }
        } else if let Some(args) = t.strip_prefix("error: test failed, to rerun pass `") {
            rerun = rerun.or_else(|| Some(args.split('`').next().unwrap_or("").to_string()));
        } else if t.starts_with("error: could not compile") {
            compile = true;
        }
    }
    let binary = binary.or(rerun.map(|args| format!("`{args}`")));
    match panic {
        Some((b, thread)) if thread != "main" && thread != "<unnamed>" => named(&b, &thread),
        Some((b, thread)) => format!("{} (a panic outside a named test, thread '{thread}')", b.or(binary).unwrap_or_else(|| "a binary the log does not name".into())),
        None => match binary {
            Some(b) => format!("{b} (the log names no failing test)"),
            None if compile => "the build (the mutant does not compile)".into(),
            None => "no test the log names".into(),
        },
    }
}

/// A nextest line reporting a test that did not pass: `<STATUS> [ <secs>s] [(<n>/<m>)] <binary> <test>`.
fn nextest_failure(t: &str) -> Option<String> {
    let (status, rest) = t.split_once(' ')?;
    if !crate::nextest::failing_status(status) {
        return None;
    }
    let (_, mut rest) = rest.trim_start().strip_prefix('[')?.split_once(']')?;
    rest = rest.trim_start();
    if rest.starts_with('(') {
        rest = rest.split_once(')')?.1.trim_start();
    }
    let id = rest.trim_end();
    (!id.is_empty()).then(|| id.to_string())
}

/// The first line of the results block `results.txt` appends to the plants file.
const RESULTS_HEADER: &str = "# cargo xtask plants results";

/// The plants file with the run's results appended as comments (the parser skips them), replacing the block an
/// earlier run left in it.
fn with_results(text: &str, summary: &str, results: &[String]) -> String {
    let kept = text.find(&format!("\n{RESULTS_HEADER}")).map_or(text, |i| &text[..=i]);
    let kept = if kept.starts_with(RESULTS_HEADER) { "" } else { kept };
    let mut out = kept.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("{RESULTS_HEADER} ({summary}):\n"));
    for r in results {
        out.push_str(r);
        out.push('\n');
    }
    out
}

/// Write a file anew (a fresh mtime, so cargo rebuilds what depends on it).
fn write_fresh(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn refused(msg: String) -> Outcome {
    eprintln!("plants: REFUSED: {msg}");
    Outcome { code: EXIT_REFUSED, verdicts: Vec::new() }
}

/// The source paths the copy must hold, sorted, and how they were found. In a git work tree (`.git` a directory or a
/// worktree's file): `git ls-files` of the tracked and the untracked-not-ignored files that exist (a tracked file
/// deleted in the work tree is not expected); a tracked file under `/target` is expected too (the copy then lacks it
/// and the run is refused). Otherwise (a synced role: no `.git`) every file and symlink below `src` outside `/target`
/// and `/fuzz/target`, symlinks not followed.
fn source_paths(src: &Path) -> Result<(Vec<String>, &'static str), String> {
    if src.join(".git").exists() {
        let out = Command::new("git").arg("-C").arg(src).args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"]).output().map_err(|e| format!("git ls-files: {e}"))?;
        if !out.status.success() {
            return Err(format!("git ls-files: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let mut paths: Vec<String> = String::from_utf8_lossy(&out.stdout).split('\0').filter(|p| !p.is_empty() && fs::symlink_metadata(src.join(p)).is_ok()).map(str::to_string).collect();
        paths.sort();
        paths.dedup();
        return Ok((paths, "git ls-files"));
    }
    fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) -> Result<(), String> {
        for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let e = e.map_err(|e| e.to_string())?;
            let name = e.file_name().to_string_lossy().into_owned();
            let path = if rel.is_empty() { name } else { format!("{rel}/{name}") };
            if path == "target" || path == "fuzz/target" {
                continue;
            }
            if e.file_type().map_err(|e| e.to_string())?.is_dir() {
                walk(&e.path(), &path, out)?;
            } else {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    walk(src, "", &mut paths)?;
    paths.sort();
    Ok((paths, "every file of a tree without .git"))
}

/// The paths of `paths` that `copy` lacks (as a file, symlink or anything else; a dangling symlink counts as present).
fn missing_in(copy: &Path, paths: &[String]) -> Vec<String> {
    paths.iter().filter(|p| fs::symlink_metadata(copy.join(p)).is_err()).cloned().collect()
}

/// The whole run: refuse what cannot be trusted, copy, build, baseline, then each plant.
pub fn run(cfg: &Config) -> Outcome {
    if cfg.dir.exists() {
        return refused(format!("{} exists: a plants run needs a NEW directory (no reuse, no delete)", cfg.dir.display()));
    }
    if cfg.dir.starts_with(&cfg.src) {
        return refused(format!("{} is inside the tree it copies", cfg.dir.display()));
    }
    if let Err(e) = check_all(&cfg.src, &cfg.plants) {
        return refused(e);
    }
    if on_ram(&cfg.dir) == Some(false) {
        println!("plants: WARN-NOT-RAM: {} is not on tmpfs or zram (plant copies belong in RAM: /dev/shm/claude/<story>/ or /mnt/zram/claude/<story>/)", cfg.dir.display());
    }
    if cfg.dir.starts_with("/dev/shm") {
        println!("plants: WARN-DEV-SHM: {} is under /dev/shm: a test that mounts its own /dev or /dev/shm cannot see the copy (posixipc_products; nscache_mode.sh binds it back)", cfg.dir.display());
    }
    if let Some(parent) = cfg.dir.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return refused(format!("cannot create {}: {e}", parent.display()));
        }
    }
    // `create_dir` (not `_all`) fails if the directory appeared meanwhile: still no reuse.
    if let Err(e) = fs::create_dir(&cfg.dir) {
        return refused(format!("cannot create {}: {e}", cfg.dir.display()));
    }
    let (copy, logs) = (cfg.dir.join("src"), cfg.dir.join("logs"));
    let step = |what: &str, r: Result<(), String>| r.map_err(|e| eprintln!("plants: {what} failed: {e}"));
    if step("mkdir", fs::create_dir(&logs).map_err(|e| e.to_string())).is_err() {
        return Outcome { code: EXIT_SURVIVED, verdicts: Vec::new() };
    }
    // The source only: `target/` holds deliberately unreadable test files and stale builds.
    let mut rsync = Command::new("rsync");
    rsync.args(["-a", "--exclude", "/target", "--exclude", "/fuzz/target"]).arg(format!("{}/", cfg.src.display())).arg(&copy);
    if step("copy", crate::run(&mut rsync)).is_err() {
        return Outcome { code: EXIT_SURVIVED, verdicts: Vec::new() };
    }
    println!("plants: copy {} -> {}", cfg.src.display(), copy.display());
    match source_paths(&cfg.src) {
        Ok((paths, how)) => {
            let missing = missing_in(&copy, &paths);
            if !missing.is_empty() {
                let shown: Vec<&str> = missing.iter().take(20).map(String::as_str).collect();
                return refused(format!("the copy lacks {} of {} source paths ({how}), so a test could miss a file it needs: {}{}", missing.len(), paths.len(), shown.join(", "), if missing.len() > 20 { ", ..." } else { "" }));
            }
            println!("plants: copy check: all {} source paths ({how}) are in the copy", paths.len());
        }
        Err(e) => return refused(format!("cannot list the source paths of {}: {e}", cfg.src.display())),
    }
    // PLANTS-UNGROUPED-MUTANTS: a plant's products are built ungrouped (a compile only: a mutant's behaviour does not
    // depend on the start-up layout) unless the plant measures the layout ([`targets_layout`] or `layout:traced`).
    use crate::hotlists::Layout;
    let layouts: Vec<Layout> = cfg.plants.iter().map(|p| if p.layout || targets_layout(&p.cmd) { Layout::Traced } else { Layout::Ungrouped }).collect();
    // The products, then each distinct command on the unmutated copy, once per layout the plants use (ungrouped
    // first): each command's baseline sees the products in the layout its plants are built with.
    let mut baseline: Vec<(String, Vec<String>, Layout)> = Vec::new();
    let mut n = 0;
    for layout in [Layout::Ungrouped, Layout::Traced] {
        if !layouts.contains(&layout) {
            continue;
        }
        let so = if layout == Layout::Traced && layouts.contains(&Layout::Ungrouped) { "so-traced" } else { "so" };
        baseline.push((so.into(), cfg.prepare.clone(), layout));
        let start = baseline.len();
        for (p, _) in cfg.plants.iter().zip(&layouts).filter(|(_, l)| **l == layout) {
            if !baseline[start..].iter().any(|(_, argv, _)| argv[2] == p.cmd) {
                n += 1;
                baseline.push((format!("baseline-{n}"), sh(&p.cmd), layout));
            }
        }
    }
    // The layout the products in the copy hold (the last build's).
    let mut held = Layout::Ungrouped;
    for (name, argv, layout) in &baseline {
        let log = logs.join(format!("{name}.log"));
        // A product build has its own limit.
        let build = name.starts_with("so");
        let limit = if build { cfg.prepare_timeout } else { cfg.timeout };
        let start = Instant::now();
        match run_logged(&copy, argv, &log, limit, *layout) {
            Ok(0) if build => {
                held = *layout;
                println!("plants: {name} ok ({}; layout {}, LIELLA_LAYOUT_TRACE={}, in {:.0}s)", argv.join(" "), layout_name(*layout), layout_env(*layout), start.elapsed().as_secs_f64());
            }
            Ok(0) => println!("plants: {name} ok ({}; layout {})", argv.join(" "), layout_name(*layout)),
            Ok(TIMED_OUT) if build => {
                println!("plants: BASELINE FAILED: {name} timed out after {limit} s (prepare timeout; --prepare-timeout <secs>); no plant was applied. Last lines of {}:\n{}", log.display(), tail(&log, 20));
                return Outcome { code: EXIT_BASELINE, verdicts: Vec::new() };
            }
            Ok(rc) => {
                let hint = if *layout == Layout::Ungrouped && !build { " (built ungrouped; a command that measures the start-up layout needs `layout:traced` on its plants, docs/plants.md)" } else { "" };
                println!("plants: BASELINE FAILED: {name} rc {rc} ({}; layout {}){hint}; no plant was applied. Last lines of {}:\n{}", argv.join(" "), layout_name(*layout), log.display(), tail(&log, 20));
                return Outcome { code: EXIT_BASELINE, verdicts: Vec::new() };
            }
            Err(e) => {
                println!("plants: BASELINE FAILED: {name}: {e}");
                return Outcome { code: EXIT_BASELINE, verdicts: Vec::new() };
            }
        }
    }
    // XTASK-PLANTS-PRODUCTS-REBUILD: a command that uses the products gets them rebuilt with the
    // mutant; the others reuse the baseline build, rebuilt unmutated first if a plant left a mutant in it.
    let uses = (cfg.uses_products)(&copy, &cfg.plants);
    let mut products_hold_mutant = false;
    let mut verdicts = Vec::new();
    let mut results: Vec<String> = Vec::new();
    let mut failed = false;
    for ((p, &uses_products), &layout) in cfg.plants.iter().zip(&uses).zip(&layouts) {
        // A plant that measures the layout reads the products by definition.
        let products = uses_products || layout == Layout::Traced;
        let mut build = format!("the baseline products (layout {}), not rebuilt", layout_name(held));
        if !products && products_hold_mutant {
            let log = logs.join(format!("{}.so-restore.log", p.id));
            let start = Instant::now();
            // Only commands that do not read the products follow: ungrouped is enough.
            match run_logged(&copy, &cfg.prepare, &log, cfg.prepare_timeout, Layout::Ungrouped) {
                Ok(0) => {
                    products_hold_mutant = false;
                    held = Layout::Ungrouped;
                    build = format!("the products, rebuilt unmutated after the last plant, layout ungrouped, in {:.0}s (log {})", start.elapsed().as_secs_f64(), log.display());
                }
                rc => {
                    println!("plants: {}: ERROR: rebuilding the unmutated products: {rc:?}; stopping (log {})", p.id, log.display());
                    return Outcome { code: EXIT_SURVIVED, verdicts };
                }
            }
        }
        let file = copy.join(&p.path);
        let original = match fs::read_to_string(&file) {
            Ok(s) => s,
            Err(e) => {
                println!("plants: {}: ERROR: cannot read {}: {e}", p.id, p.path);
                results.push(format!("# {}: ERROR: cannot read {}: {e}", p.id, p.path));
                failed = true;
                continue;
            }
        };
        let mutant = match mutate(&original, &p.pattern) {
            Ok(m) => m,
            Err(e) => {
                println!("plants: {}: ERROR: {e}", p.id);
                results.push(format!("# {}: ERROR: {e}", p.id));
                failed = true;
                continue;
            }
        };
        if let Err(e) = write_fresh(&file, &mutant) {
            println!("plants: {}: ERROR: {e}", p.id);
            results.push(format!("# {}: ERROR: {e}", p.id));
            failed = true;
            continue;
        }
        let mut log = logs.join(format!("{}.log", p.id));
        let mut built = Ok(0);
        if products {
            products_hold_mutant = true;
            let so_log = logs.join(format!("{}.so.log", p.id));
            let start = Instant::now();
            built = run_logged(&copy, &cfg.prepare, &so_log, cfg.prepare_timeout, layout);
            held = layout;
            build = format!("the products, rebuilt with the mutant, layout {}, in {:.0}s (log {})", layout_name(layout), start.elapsed().as_secs_f64(), so_log.display());
            if !matches!(built, Ok(0)) {
                // The mutant does not build: judged on the build's log, the command is not run.
                build = format!("the products build failed on the mutant: {build}");
                log = so_log;
            }
        }
        let build_timed_out = products && matches!(built, Ok(TIMED_OUT));
        let rc = if matches!(built, Ok(0)) { run_logged(&copy, &sh(&p.cmd), &log, cfg.timeout, layout) } else { built };
        // Restore by writing (a fresh mtime) before anything else can go wrong.
        if let Err(e) = write_fresh(&file, &original) {
            println!("plants: {}: ERROR: restore: {e}; stopping (the copy holds the mutant)", p.id);
            return Outcome { code: EXIT_SURVIVED, verdicts };
        }
        let rc = match rc {
            Ok(rc) => rc,
            Err(e) => {
                println!("plants: {}: ERROR: {e}", p.id);
                results.push(format!("# {}: ERROR: {e}", p.id));
                failed = true;
                continue;
            }
        };
        let (verdict, detail) = match (&p.expect, rc) {
            _ if build_timed_out => (Verdict::BuildTimeout, format!("the products build timed out after {} s (prepare timeout); the mutant was not tested", cfg.prepare_timeout)),
            (Expect::Equivalent(why), 0) => (Verdict::Equivalent, why.clone()),
            (_, 0) => (Verdict::Survived, format!("`{}` passed on the mutant", p.cmd)),
            (_, TIMED_OUT) => (Verdict::Caught, format!("caught by timeout ({}s)", cfg.timeout)),
            (Expect::Msg(re), rc) if log_matches(&log, re) => (Verdict::Caught, format!("rc {rc}, /{re}/ in the log")),
            (Expect::Msg(re), rc) => (Verdict::WarnNoMsg, format!("rc {rc}, but /{re}/ is not in {}", log.display())),
            (Expect::Equivalent(_), rc) => (Verdict::Caught, format!("rc {rc} (declared equivalent, but caught)")),
            (Expect::RcOnly, rc) => (Verdict::WarnRcOnly, format!("rc {rc}, no msg: given")),
        };
        println!("plants: {}: {} ({}; log {}; build: {build})", p.id, verdict.label(), detail, log.display());
        // XTASK-PLANTS-CATCHER: a failed command names what failed, kept beside the plants in results.txt.
        if rc == 0 || build_timed_out {
            results.push(format!("# {}: {}", p.id, verdict.label()));
        } else {
            let by = if rc == TIMED_OUT { format!("timeout ({}s)", cfg.timeout) } else { catcher(&String::from_utf8_lossy(&fs::read(&log).unwrap_or_default())) };
            println!("plants: {}: caught by {by}", p.id);
            results.push(format!("# {}: {}, caught by {by}", p.id, verdict.label()));
        }
        verdicts.push((p.id.clone(), verdict));
    }
    let count = |v: Verdict| verdicts.iter().filter(|(_, x)| *x == v).count();
    let survived = count(Verdict::Survived);
    let summary = format!(
        "{} plants: {} CAUGHT, {} SURVIVED, {} EQUIVALENT, {} WARN-RC-ONLY, {} WARN-NO-MSG, {} BUILD-TIMEOUT{}",
        cfg.plants.len(),
        count(Verdict::Caught),
        survived,
        count(Verdict::Equivalent),
        count(Verdict::WarnRcOnly),
        count(Verdict::WarnNoMsg),
        count(Verdict::BuildTimeout),
        if failed { ", and errors (see above)" } else { "" }
    );
    println!("plants: {summary}");
    let record = cfg.dir.join("results.txt");
    if let Err(e) = write_fresh(&record, &with_results(&cfg.text, &summary, &results)) {
        println!("plants: ERROR: cannot write {}: {e}", record.display());
        failed = true;
    } else {
        println!("plants: the plants file with these results: {} (commit it as handoff/<STORY>-plants.txt)", record.display());
    }
    let code = if survived > 0 || count(Verdict::BuildTimeout) > 0 || failed { EXIT_SURVIVED } else { EXIT_OK };
    Outcome { code, verdicts }
}

#[cfg(test)]
mod tests {
    // XTASK-PLANTS: no spec clause (developer tooling): the plants runner on a toy tree.
    use super::*;

    /// A toy tree in a fresh temporary directory, and a `--dir` beside it that does not exist yet.
    fn toy(name: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("xtask-plants-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let src = base.join("tree");
        fs::create_dir_all(src.join("src")).unwrap();
        fs::write(src.join("src/value.txt"), "answer=42\nother=7\nother=7\n").unwrap();
        fs::create_dir_all(src.join("target")).unwrap();
        fs::write(src.join("target/stale"), "not copied").unwrap();
        (src, base.join("run"))
    }

    const CHECK: &str = "grep -q answer=42 src/value.txt || { echo WRONG-ANSWER; exit 1; }";

    fn config(src: &Path, dir: &Path, plants: &str) -> Config {
        Config { src: src.to_path_buf(), text: plants.to_string(), plants: parse_plants(plants).unwrap(), dir: dir.to_path_buf(), prepare: sh("true"), timeout: 60, prepare_timeout: 60, uses_products: |_, ps| vec![false; ps.len()] }
    }

    /// The toy's products: `product.txt`, a copy of `src/value.txt`; every build appends a line to `builds.txt`.
    const BUILD: &str = "cp src/value.txt product.txt && echo built >> builds.txt";
    const PRODUCT_CHECK: &str = "grep -q answer=42 product.txt || { echo WRONG-PRODUCT; exit 1; }";

    // no spec clause (XTASK-PLANTS-PRODUCTS-REBUILD): a plant seen only through the products is CAUGHT (the baseline
    // products alone would let it survive); a command that does not use them reuses the build, which is first rebuilt
    // unmutated when the plant before left its mutant in it
    #[test]
    fn a_products_command_gets_the_products_rebuilt_with_the_mutant() {
        let (src, dir) = toy("products");
        let plants = format!(
            "u1 | src/value.txt | perl:s/(answer=42\\n)other=7/$1other=8/ | {CHECK} | equivalent:other= is not read\n\
             pr | src/value.txt | lit:answer=42 ==> answer=43 | {PRODUCT_CHECK} | msg:WRONG-PRODUCT\n\
             u2 | src/value.txt | perl:s/answer=42/answer=44/ | {CHECK} | msg:WRONG-ANSWER\n"
        );
        let mut cfg = config(&src, &dir, &plants);
        cfg.prepare = sh(BUILD);
        cfg.uses_products = |_, ps| ps.iter().map(|p| p.cmd.contains("product.txt")).collect();
        let out = run(&cfg);
        assert_eq!(out.verdicts, vec![("u1".into(), Verdict::Equivalent), ("pr".into(), Verdict::Caught), ("u2".into(), Verdict::Caught)]);
        assert_eq!(out.code, EXIT_OK);
        // The baseline, `pr` with its mutant, the unmutated rebuild before `u2`; none for `u1`.
        assert_eq!(fs::read_to_string(dir.join("src/builds.txt")).unwrap().lines().count(), 3);
        assert!(dir.join("logs/pr.so.log").is_file() && dir.join("logs/u2.so-restore.log").is_file());
        assert!(!dir.join("logs/u1.so.log").exists() && !dir.join("logs/u1.so-restore.log").exists());
        assert_eq!(fs::read_to_string(dir.join("src/product.txt")).unwrap(), "answer=42\nother=7\nother=7\n");
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause (PLANTS-UNGROUPED-MUTANTS): an ordinary plant's products (and its baseline) are built ungrouped, a
    // plant that measures the layout (marked `layout:traced`, or naming a layout test) is built traced, and each command
    // sees its plant's layout in LIELLA_LAYOUT_TRACE
    #[test]
    fn ordinary_plants_build_ungrouped_and_layout_plants_traced() {
        let (src, dir) = toy("layout");
        let product = "grep -q answer=42 product.txt || { echo WRONG-PRODUCT; exit 1; }";
        let plants = format!(
            "or | src/value.txt | lit:answer=42 ==> answer=43 | echo cmd-or=$LIELLA_LAYOUT_TRACE >> cmds.txt; {product} | msg:WRONG-PRODUCT\n\
             mk | src/value.txt | perl:s/answer=42/answer=44/ | echo cmd-mk=$LIELLA_LAYOUT_TRACE >> cmds.txt; {product} | msg:WRONG-PRODUCT | layout:traced\n\
             nm | src/value.txt | lit:answer=42 ==> answer=45 | echo hot_text_products cmd-nm=$LIELLA_LAYOUT_TRACE >> cmds.txt; {product} | msg:WRONG-PRODUCT\n"
        );
        let mut cfg = config(&src, &dir, &plants);
        cfg.prepare = sh("cp src/value.txt product.txt && echo \"$(head -1 src/value.txt) layout=$LIELLA_LAYOUT_TRACE\" >> builds.txt");
        // Only the ordinary plant's command is known to read the products: the layout plants rebuild regardless.
        cfg.uses_products = |_, ps| ps.iter().map(|p| p.id == "or").collect();
        let out = run(&cfg);
        assert_eq!(out.verdicts, vec![("or".into(), Verdict::Caught), ("mk".into(), Verdict::Caught), ("nm".into(), Verdict::Caught)]);
        assert_eq!(out.code, EXIT_OK);
        // Baseline ungrouped, then traced; then each plant's mutant in its own layout.
        let builds = fs::read_to_string(dir.join("src/builds.txt")).unwrap();
        assert_eq!(builds, "answer=42 layout=ungrouped\nanswer=42 layout=1\nanswer=43 layout=ungrouped\nanswer=44 layout=1\nanswer=45 layout=1\n");
        assert!(dir.join("logs/so.log").is_file() && dir.join("logs/so-traced.log").is_file());
        // The baseline of each command, then the plant's run, each in the plant's layout.
        let cmds = fs::read_to_string(dir.join("src/cmds.txt")).unwrap();
        assert_eq!(cmds, "cmd-or=ungrouped\ncmd-mk=1\nhot_text_products cmd-nm=1\ncmd-or=ungrouped\ncmd-mk=1\nhot_text_products cmd-nm=1\n");
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause (PLANTS-UNGROUPED-MUTANTS): a file without layout plants never builds traced
    #[test]
    fn a_file_without_layout_plants_builds_ungrouped_only() {
        let (src, dir) = toy("ungrouped");
        let plants = format!("pr | src/value.txt | lit:answer=42 ==> answer=43 | {PRODUCT_CHECK} | msg:WRONG-PRODUCT\n");
        let mut cfg = config(&src, &dir, &plants);
        cfg.prepare = sh("cp src/value.txt product.txt && echo layout=$LIELLA_LAYOUT_TRACE >> builds.txt");
        cfg.uses_products = |_, ps| vec![true; ps.len()];
        let out = run(&cfg);
        assert_eq!(out.verdicts, vec![("pr".into(), Verdict::Caught)]);
        assert_eq!(fs::read_to_string(dir.join("src/builds.txt")).unwrap(), "layout=ungrouped\nlayout=ungrouped\n");
        assert!(!dir.join("logs/so-traced.log").exists());
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause (PLANTS-UNGROUPED-MUTANTS): the commands that measure the start-up layout
    #[test]
    fn which_commands_target_the_layout() {
        for cmd in [
            "cargo test -p liella --test hot_text_products",
            "cargo test -p liella --test=heap_meta_products",
            "cargo test -p liella --test cold_bss_products startup",
            "cargo xtask layout-check",
            "cargo xtask affected --only test:liella:hot_text_products --run",
            "cargo xtask affected --files csys/src/x.c --run",
        ] {
            assert!(targets_layout(cmd), "{cmd}");
        }
        for cmd in [
            "cargo test -p liella --test stdio_short_io",
            "cargo xtask affected --only conformance:s27_dladdr --with-exports --run",
            "cargo xtask affected --files csys/src/x.c --list",
            "cargo test -p xtask --bin xtask affected::tests --run",
            "cargo test -p xtask --bin xtask layout::tests",
            "cargo test -p liella --test hot_text_products_helper",
        ] {
            assert!(!targets_layout(cmd), "{cmd}");
        }
    }

    // no spec clause (XTASK-PLANTS-PRODUCTS-REBUILD): a mutant whose products do not build is judged on the build's log
    #[test]
    fn a_mutant_that_breaks_the_products_build_is_judged_on_its_log() {
        let (src, dir) = toy("nobuild");
        let plants = format!("nb | src/value.txt | lit:answer=42 ==> answer=99 | {PRODUCT_CHECK} | msg:BUILD-BROKE\n");
        let mut cfg = config(&src, &dir, &plants);
        cfg.prepare = sh("grep -q answer=99 src/value.txt && { echo BUILD-BROKE; exit 1; }; cp src/value.txt product.txt");
        cfg.uses_products = |_, ps| vec![true; ps.len()];
        let out = run(&cfg);
        assert_eq!(out.verdicts, vec![("nb".into(), Verdict::Caught)]);
        assert!(!dir.join("logs/nb.log").exists(), "the command must not run on a mutant that does not build");
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause (XTASK-PLANTS-PRODUCTS-REBUILD): only a plain `cargo test` of named targets none of which uses the
    // products skips the rebuild
    #[test]
    fn which_commands_use_the_products() {
        let lookup = |k: Option<&str>, t: &str| match (k, t) {
            (Some("liella") | None, "catalogs_products") => Some(true),
            (Some("liella") | None, "stdio_short_io") => Some(false),
            _ => None,
        };
        for (cmd, want) in [
            ("cargo test -p liella --test stdio_short_io", false),
            ("cargo test -p liella --test=stdio_short_io short_write -- --test-threads 1", false),
            ("cargo test -p xtask --bin xtask plants::", false),
            ("cargo test -p liella --lib diag", false),
            ("cargo test -p liella --test catalogs_products", true),
            ("cargo test -p liella --test stdio_short_io --test catalogs_products", true),
            ("cargo test -p liella --test unknown_target", true),
            ("cargo test -p liella", true),
            ("cargo test -p liella --tests", true),
            ("cargo test -p liella --test stdio_short_io && true", true),
            ("cargo xtask affected --only conformance:printf --with-exports --run", true),
            ("sh tests/oracle/run.sh", true),
        ] {
            assert_eq!(cmd_uses_products(cmd, lookup), want, "{cmd}");
        }
    }

    // no spec clause: a plant its command catches is CAUGHT, a declared one with no msg is WARN-RC-ONLY, the copy has no target/
    #[test]
    fn a_caught_plant_exits_zero_and_is_restored() {
        let (src, dir) = toy("caught");
        let plants = format!("p1 | src/value.txt | lit:answer=42 ==> answer=43 | {CHECK} | msg:WRONG-ANSWER\np2 | src/value.txt | perl:s/answer=4\\d/answer=0/ | {CHECK}\n");
        let out = run(&config(&src, &dir, &plants));
        assert_eq!(out.verdicts, vec![("p1".to_string(), Verdict::Caught), ("p2".to_string(), Verdict::WarnRcOnly)]);
        assert_eq!(out.code, EXIT_OK);
        assert_eq!(fs::read_to_string(dir.join("src/src/value.txt")).unwrap(), "answer=42\nother=7\nother=7\n");
        assert!(!dir.join("src/target").exists(), "target/ must not be copied");
        assert!(dir.join("logs/p1.log").is_file());
        let results = fs::read_to_string(dir.join("results.txt")).unwrap();
        assert!(results.starts_with(&plants), "results.txt keeps the plants file: {results}");
        assert!(results.contains("\n# p1: CAUGHT, caught by no test the log names\n# p2: WARN-RC-ONLY, caught by "), "{results}");
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause (XTASK-PLANTS-CATCHER): one failing test in a recorded `cargo test` log names its binary and test
    #[test]
    fn catcher_names_the_failing_test_of_a_cargo_test_log() {
        let log = "   Compiling liella-ld v0.1.0\n     Running `target/debug/xtask affected --run`\n     Running unittests src/lib.rs (target/debug/deps/liella_ld-0f3a9c2b1d4e5f60)\n\nrunning 3 tests\ntest elf::ok ... ok\n\n     Running tests/elf_tables.rs (target/debug/deps/elf_tables-a1b2c3d4e5f60718)\n\nrunning 2 tests\ntest tables_read_back ... ok\ntest sysv_chain_loop ... FAILED\n\nfailures:\n\n---- sysv_chain_loop stdout ----\nthread 'sysv_chain_loop' panicked at crates/ld/tests/elf_tables.rs:9:5:\nloop not found\n\nerror: test failed, to rerun pass `-p liella-ld --test elf_tables`\n";
        assert_eq!(catcher(log), "elf_tables sysv_chain_loop");
        assert_eq!(catcher("     Running unittests src/lib.rs (target/debug/deps/liella_ld-0f3a9c2b1d4e5f60)\ntest elf::tests::bad ... FAILED\n"), "liella_ld elf::tests::bad");
        assert_eq!(catcher("   Doc-tests liella_ld\ntest crates/ld/src/elf.rs - Elf (line 3) ... FAILED\n"), "doctests::liella_ld crates/ld/src/elf.rs - Elf (line 3)");
    }

    // no spec clause (XTASK-PLANTS-CATCHER): with several failing tests the first one in the log is the catcher
    #[test]
    fn catcher_names_the_first_of_several_failing_tests() {
        let cargo = "     Running tests/stdio.rs (target/debug/deps/stdio-00112233aabbccdd)\ntest b_second ... ok\ntest c_third ... FAILED\ntest a_first ... FAILED\n";
        assert_eq!(catcher(cargo), "stdio c_third");
        let nextest = "    Starting 9 tests across 2 binaries\n        PASS [   0.010s] (1/9) liella-core::stdio ok_one\n        FAIL [   0.004s] (2/9) liella-core::stdio short_write\n     SIGSEGV [   0.020s] (3/9) liella-core::stdio crash\n------------\n     Summary [   0.100s] 9 tests run: 7 passed, 2 failed\n        FAIL [   0.004s] (2/9) liella-core::stdio short_write\n";
        assert_eq!(catcher(nextest), "liella-core::stdio short_write");
        assert_eq!(catcher("     SIGABRT [   0.020s] liella-core crash_test\n        FAIL [   0.004s] liella-core other\n"), "liella-core crash_test");
    }

    // no spec clause (XTASK-PLANTS-CATCHER): a panic with no test name, a binary that aborted, a build that fails, nothing
    #[test]
    fn catcher_without_a_failing_test_line() {
        let main = "     Running tests/posixipc_run.rs (target/debug/deps/posixipc_run-0123456789abcdef)\nthread 'main' panicked at crates/core/tests/posixipc_run.rs:40:9:\nphase nodev failed\nerror: test failed, to rerun pass `-p liella-core --test posixipc_run`\n";
        assert_eq!(catcher(main), "posixipc_run (a panic outside a named test, thread 'main')");
        assert_eq!(catcher("thread '<unnamed>' panicked at src/x.rs:1:1:\nboom\n"), "a binary the log does not name (a panic outside a named test, thread '<unnamed>')");
        let named = "     Running tests/t.rs (target/debug/deps/t-0123456789abcdef)\nthread 'mmap_guard' panicked at tests/t.rs:3:3:\nbad\n";
        assert_eq!(catcher(named), "t mmap_guard");
        let aborted = "     Running tests/t.rs (target/debug/deps/t-0123456789abcdef)\nrunning 1 test\nerror: test failed, to rerun pass `--test t`\n\nCaused by:\n  process didn't exit successfully (signal: 6, SIGABRT)\n";
        assert_eq!(catcher(aborted), "t (the log names no failing test)");
        assert_eq!(catcher("error: test failed, to rerun pass `--test t`\n"), "`--test t` (the log names no failing test)");
        assert_eq!(catcher("error[E0308]: mismatched types\nerror: could not compile `liella-ld` (lib) due to 1 previous error\n"), "the build (the mutant does not compile)");
        assert_eq!(catcher("WRONG-ANSWER\n"), "no test the log names");
    }

    // no spec clause (XTASK-PLANTS-CATCHER): results.txt replaces the block an earlier run appended
    #[test]
    fn results_replace_an_earlier_block() {
        let first = with_results("# story\np | a | lit:x ==> y | true\n", "1 plants: 1 CAUGHT", &["# p: CAUGHT, caught by t x".into()]);
        assert_eq!(first, "# story\np | a | lit:x ==> y | true\n# cargo xtask plants results (1 plants: 1 CAUGHT):\n# p: CAUGHT, caught by t x\n");
        assert_eq!(with_results(&first, "1 plants: 1 SURVIVED", &["# p: SURVIVED".into()]), "# story\np | a | lit:x ==> y | true\n# cargo xtask plants results (1 plants: 1 SURVIVED):\n# p: SURVIVED\n");
        assert_eq!(parse_plants(&first).unwrap().len(), 1);
        assert_eq!(with_results("p | a | lit:x ==> y | true", "s", &[]), "p | a | lit:x ==> y | true\n# cargo xtask plants results (s):\n");
    }

    // no spec clause: a surviving plant exits non-zero; a wrong msg is WARN-NO-MSG; an equivalent one is EQUIVALENT
    #[test]
    fn a_surviving_plant_exits_non_zero() {
        let (src, dir) = toy("survived");
        let plants = format!("s1 | src/value.txt | perl:s/(answer=42\\n)other=7/$1other=8/ | {CHECK} | msg:WRONG-ANSWER\ns2 | src/value.txt | lit:answer=42 ==> answer=41 | {CHECK} | msg:NOT-PRINTED\ns3 | src/value.txt | perl:s/(answer=42\\n)other=7/$1other=9/ | {CHECK} | equivalent:other= is not read\n");
        let out = run(&config(&src, &dir, &plants));
        assert_eq!(out.verdicts, vec![("s1".into(), Verdict::Survived), ("s2".into(), Verdict::WarnNoMsg), ("s3".into(), Verdict::Equivalent)]);
        assert_eq!(out.code, EXIT_SURVIVED);
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause: a plant matching two sites is refused before anything is copied
    #[test]
    fn a_plant_matching_two_sites_is_refused() {
        let (src, dir) = toy("twosites");
        for pattern in ["lit:other=7 ==> other=8", "perl:s/other=7/other=8/"] {
            let out = run(&config(&src, &dir, &format!("t1 | src/value.txt | {pattern} | {CHECK} | msg:X\n")));
            assert_eq!(out.code, EXIT_REFUSED, "{pattern}");
            assert!(!dir.exists(), "a refused run creates nothing");
        }
        assert!(mutate("a\na\n", &Pattern::Perl("s/a/b/".into())).unwrap_err().contains("2 sites"));
        assert!(mutate("a\n", &Pattern::Perl("s/z/b/".into())).unwrap_err().contains("no site"));
        assert!(parse_plants("t | x.txt | perl:s/a/b/g | true | msg:X").unwrap_err().contains("/g"));
        assert!(parse_plants("t | csys/src/machine/x.S | perl:s/ret/nop/ | true | msg:X").unwrap_err().contains("liella_machine_"));
        assert!(parse_plants("t | csys/src/machine/x.S | perl:s/(liella_machine_toyfn:.*?)ret/$1nop/s | true | msg:X").is_ok());
        assert!(parse_plants("t | ../x.txt | lit:a ==> b | true | msg:X").is_err());
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause: an existing --dir is refused and left untouched
    #[test]
    fn a_reused_dir_is_refused() {
        let (src, dir) = toy("reused");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("keep"), "x").unwrap();
        let out = run(&config(&src, &dir, &format!("r1 | src/value.txt | lit:answer=42 ==> answer=43 | {CHECK} | msg:WRONG-ANSWER\n")));
        assert_eq!(out.code, EXIT_REFUSED);
        assert!(out.verdicts.is_empty());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "nothing written into a reused dir");
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause: a failing baseline (products or command) exits 2 before any plant is applied
    #[test]
    fn a_failed_baseline_exits_two() {
        let (src, dir) = toy("baseline");
        let plants = "b1 | src/value.txt | lit:answer=42 ==> answer=43 | grep -q answer=99 src/value.txt | msg:X\n";
        let out = run(&config(&src, &dir, plants));
        assert_eq!(out.code, EXIT_BASELINE);
        assert!(out.verdicts.is_empty());
        assert_eq!(fs::read_to_string(dir.join("src/src/value.txt")).unwrap(), "answer=42\nother=7\nother=7\n");
        let dir2 = dir.with_file_name("run2");
        let mut cfg = config(&src, &dir2, &format!("b1 | src/value.txt | lit:answer=42 ==> answer=43 | {CHECK} | msg:X\n"));
        cfg.prepare = sh("echo the products are missing; exit 1");
        assert_eq!(run(&cfg).code, EXIT_BASELINE);
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause: the plants file format. The labels here are made up: naming a real machine export in xtask makes
    // `affected` count this file as a user of that `.S` file and escalate its changes to FULL.
    #[test]
    fn plants_lines_parse() {
        let p = parse_plants("# comment\n\nx-1 | a/b.rs | lit:a || b ==> a && b | cargo test -p q --test t | msg:t1.*FAILED\ny | a/b.rs | perl:s{a}{b} | true | equivalent: same result\n").unwrap();
        assert_eq!(p[0].pattern, Pattern::Literal { old: "a || b".into(), new: "a && b".into() });
        assert_eq!(p[0].expect, Expect::Msg("t1.*FAILED".into()));
        assert_eq!(p[1].expect, Expect::Equivalent("same result".into()));
        assert!(parse_plants("x | a | lit:a ==> b | true | msg:X\nx | a | lit:a ==> c | true | msg:X").unwrap_err().contains("duplicate"));
        assert!(parse_plants("x | a | a ==> b | true | msg:X").is_err());
        assert_eq!(parse_plants("d | a | lit:drop me ==> | true").unwrap()[0].pattern, Pattern::Literal { old: "drop me".into(), new: String::new() });
        assert!(names_machine_anchor("(liella_machine_toyfn:.*?)x") && !names_machine_anchor("liella_machine_:") && !names_machine_anchor("liella_machine_x"));
        // PLANTS-UNGROUPED-MUTANTS: `layout:traced` after the expectation (or alone after the command).
        let l = parse_plants("a | f | lit:x ==> y | c1 | c2 | msg:BAD | layout:traced\nb | f | lit:x ==> y | c3 | layout:traced\nc | f | lit:x ==> y | c4 | msg:Z\n").unwrap();
        assert_eq!((l[0].cmd.as_str(), &l[0].expect, l[0].layout), ("c1 | c2", &Expect::Msg("BAD".into()), true));
        assert_eq!((l[1].cmd.as_str(), &l[1].expect, l[1].layout), ("c3", &Expect::RcOnly, true));
        assert!(!l[2].layout);
        assert!(parse_plants("a | f | lit:x ==> y | c | msg:X | layout:yes").unwrap_err().contains("layout:traced"));
    }

    // no spec clause: the default per-plant limit is 600 s (the project rule), and --timeout overrides it
    #[test]
    fn default_timeout_is_600_seconds() {
        assert_eq!(DEFAULT_PREPARE_TIMEOUT, 3600);
        let (src, dir) = toy("defaulttimeout");
        let file = src.join("plants.txt");
        fs::write(&file, format!("d1 | src/value.txt | lit:answer=42 ==> answer=43 | {CHECK} | msg:WRONG-ANSWER\n")).unwrap();
        let (f, d) = (file.display().to_string(), dir.display().to_string());
        let cfg = parse_args(&src, &[f.clone(), "--dir".into(), d.clone()]).unwrap();
        assert_eq!(cfg.timeout, 600);
        assert_eq!(DEFAULT_TIMEOUT, 600);
        let cfg = parse_args(&src, &[f, "--dir".into(), d, "--timeout".into(), "7".into()]).unwrap();
        assert_eq!(cfg.timeout, 7);
        assert_eq!(cfg.prepare_timeout, 3600);
        let cfg = parse_args(&src, &[file.display().to_string(), "--dir".into(), dir.display().to_string(), "--prepare-timeout".into(), "9".into()]).unwrap();
        assert_eq!((cfg.timeout, cfg.prepare_timeout), (600, 9));
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git").arg("-C").arg(dir).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    }

    // XTASK-PLANTS-TRACKED-COPY, no spec clause: every source path of a git work tree reaches the copy, including
    // unusual kinds (a symlink, a dangling symlink, an executable script, a dot directory, a name with a space and a
    // newline, a `target` directory below the top), and the copy check lists them all; without git, every file but /target.
    #[test]
    fn every_tracked_path_of_an_unusual_kind_is_copied() {
        let (src, dir) = toy("tracked");
        if !git(&src, &["init", "-q"]) {
            println!("skipped: no git (the tracked-path case needs `git init`)");
            return;
        }
        fs::create_dir_all(src.join(".config")).unwrap();
        fs::write(src.join(".config/x.toml"), "x = 1\n").unwrap();
        fs::create_dir_all(src.join("crates/x/target")).unwrap();
        fs::write(src.join("crates/x/target/keep.txt"), "source, not a build\n").unwrap();
        fs::write(src.join("src/a b\nc.txt"), "odd name\n").unwrap();
        fs::write(src.join("src/run.sh"), "#!/bin/sh\nexit 0\n").unwrap();
        let mut mode = fs::metadata(src.join("src/run.sh")).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
        fs::set_permissions(src.join("src/run.sh"), mode).unwrap();
        std::os::unix::fs::symlink("value.txt", src.join("src/link")).unwrap();
        std::os::unix::fs::symlink("nowhere", src.join("src/dangling")).unwrap();
        fs::write(src.join(".gitignore"), "/target\n").unwrap();
        assert!(git(&src, &["add", "-A"]), "git add");
        fs::write(src.join("src/untracked.txt"), "new, not added yet\n").unwrap();
        let (paths, how) = source_paths(&src).unwrap();
        assert_eq!(how, "git ls-files");
        for p in [".config/x.toml", "crates/x/target/keep.txt", "src/a b\nc.txt", "src/run.sh", "src/link", "src/dangling", "src/untracked.txt"] {
            assert!(paths.iter().any(|x| x == p), "{p} listed: {paths:?}");
        }
        assert!(!paths.iter().any(|x| x.starts_with("target/")), "ignored target/ is not a source path");
        let out = run(&config(&src, &dir, &format!("c1 | src/value.txt | lit:answer=42 ==> answer=43 | {CHECK} && test -x src/run.sh && test -L src/dangling | msg:WRONG-ANSWER\n")));
        assert_eq!(out.code, EXIT_OK, "{:?}", out.verdicts);
        assert!(missing_in(&dir.join("src"), &paths).is_empty());
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // XTASK-PLANTS-TRACKED-COPY, no spec clause: a tracked file the copy lacks (here one force-added under the excluded
    // /target) refuses the run before the build or any plant, naming the path
    #[test]
    fn a_tracked_path_missing_from_the_copy_refuses_the_run() {
        let (src, dir) = toy("missing");
        if !git(&src, &["init", "-q"]) {
            println!("skipped: no git (the tracked-path case needs `git init`)");
            return;
        }
        fs::write(src.join("target/needed.sh"), "echo a test needs me\n").unwrap();
        fs::write(src.join(".gitignore"), "/target\n").unwrap();
        assert!(git(&src, &["add", "src/value.txt", ".gitignore"]) && git(&src, &["add", "-f", "target/needed.sh"]), "git add");
        let mut cfg = config(&src, &dir, &format!("m1 | src/value.txt | lit:answer=42 ==> answer=43 | {CHECK} | msg:WRONG-ANSWER\n"));
        cfg.prepare = sh("echo BUILT > built.txt");
        let out = run(&cfg);
        assert_eq!(out.code, EXIT_REFUSED);
        assert!(out.verdicts.is_empty() && !dir.join("src/built.txt").exists(), "refused before the build");
        assert_eq!(missing_in(&dir.join("src"), &source_paths(&src).unwrap().0), vec!["target/needed.sh".to_string()]);
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause: the build step has its own limit; a slow build is not cut by the test timeout
    #[test]
    fn slow_prepare_is_not_cut_by_the_test_timeout() {
        let (src, dir) = toy("slowprep");
        let mut cfg = config(&src, &dir, "q1 | src/value.txt | lit:answer=42 ==> answer=43 | grep -q answer=42 src/value.txt || { echo WRONG-ANSWER; exit 1; } | msg:WRONG-ANSWER\n");
        cfg.prepare = sh("sleep 2");
        cfg.timeout = 1;
        cfg.prepare_timeout = 30;
        let out = run(&cfg);
        assert_eq!(out.verdicts, vec![("q1".to_string(), Verdict::Caught)]);
        assert_eq!(out.code, EXIT_OK);
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause: a baseline build that runs into the prepare timeout is named as such
    #[test]
    fn baseline_prepare_timeout_is_reported() {
        let (src, dir) = toy("prepbase");
        let mut cfg = config(&src, &dir, "q1 | src/value.txt | lit:answer=42 ==> answer=43 | true | msg:x\n");
        cfg.prepare = sh("sleep 30");
        cfg.prepare_timeout = 1;
        let out = run(&cfg);
        assert_eq!(out.code, EXIT_BASELINE);
        assert!(out.verdicts.is_empty());
        assert!(dir.join("logs/so.log").exists());
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause: a mutant whose build times out is BUILD-TIMEOUT, never CAUGHT, and fails the run
    #[test]
    fn a_plant_build_that_times_out_is_not_caught() {
        let (src, dir) = toy("prepplant");
        let mut cfg = config(&src, &dir, "q1 | src/value.txt | lit:answer=42 ==> answer=43 | true | msg:x\n");
        cfg.prepare = sh("if grep -q answer=43 src/value.txt; then sleep 30; fi");
        cfg.prepare_timeout = 1;
        cfg.uses_products = |_, ps| vec![true; ps.len()];
        let out = run(&cfg);
        assert_eq!(out.verdicts, vec![("q1".to_string(), Verdict::BuildTimeout)]);
        assert_ne!(out.code, EXIT_OK);
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause: a plant whose command hangs is CAUGHT by the timeout, even with a msg: its log cannot match
    #[test]
    fn a_plant_that_times_out_is_caught() {
        let (src, dir) = toy("timeout");
        let mut cfg = config(&src, &dir, "h1 | src/value.txt | lit:answer=42 ==> answer=43 | if grep -q answer=43 src/value.txt; then sleep 30; fi | msg:WRONG-ANSWER\n");
        cfg.timeout = 1;
        let out = run(&cfg);
        assert_eq!(out.verdicts, vec![("h1".to_string(), Verdict::Caught)]);
        assert_eq!(out.code, EXIT_OK);
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }

    // no spec clause: a step that ends at its limit leaves no process of its group alive (a grandchild that ignores TERM, as the step's own leader does not)
    #[test]
    fn a_timed_out_step_leaves_no_process_of_its_group() {
        let (src, dir) = toy("killtree");
        fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("grandchild.pid");
        let log = dir.join("step.log");
        let cmd = format!("(trap '' TERM; exec sleep 300) & echo $! > {}; wait", pidfile.display());
        let rc = run_logged(&dir, &sh(&cmd), &log, 1, crate::hotlists::Layout::Ungrouped).unwrap();
        assert_eq!(rc, TIMED_OUT);
        let pid = fs::read_to_string(&pidfile).unwrap().trim().to_string();
        let alive = |pid: &str| fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|st| !st.rsplit(')').next().unwrap_or("").trim_start().starts_with('Z'));
        // Reaping by init may lag the kill: poll a bounded number of times.
        for _ in 0..500 {
            if !alive(&pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!alive(&pid), "grandchild {pid} survived the step's timeout");
        let _ = fs::remove_dir_all(src.parent().unwrap());
    }
}
