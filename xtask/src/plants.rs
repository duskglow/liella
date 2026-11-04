//! `cargo xtask plants <file> --dir <new-dir> [--timeout <secs>]`: run a story's plants (mutations
//! that its tests must catch) the one way the rules ask for, instead of a hand-written script per
//! story (XTASK-PLANTS; `docs/plants.md`).
//!
//! It removes four failure classes that repeated in hand-written plants scripts:
//! - a reused copy directory (rsync restores old mtimes, so cargo skipped the rebuild and tested a
//!   stale mutant): `--dir` must not exist, and nothing is ever deleted;
//! - a copy without the products (`cargo xtask so` is run in the copy before the baseline);
//! - an unanchored pattern that mutated the wrong site (every plant must change exactly one site,
//!   and a plant on a `.S` file must name its `liella_machine_<fn>:` label);
//! - a script that always exits 0 (any SURVIVED exits 1; a failed baseline exits 2).
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
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

/// Exit status: every plant CAUGHT (or EQUIVALENT).
const EXIT_OK: u8 = 0;
/// Exit status: at least one plant SURVIVED, or a step failed after the baseline.
const EXIT_SURVIVED: u8 = 1;
/// Exit status: the product build or a baseline command failed on the unmutated copy.
const EXIT_BASELINE: u8 = 2;
/// Exit status: the input was refused (usage, plants file, an ambiguous plant, an existing `--dir`).
const EXIT_REFUSED: u8 = 3;

/// The default limit for one command, in seconds (a mutant may loop forever).
const DEFAULT_TIMEOUT: u64 = 1800;

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
}

/// The verdict on one plant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Caught,
    Survived,
    Equivalent,
    WarnRcOnly,
    WarnNoMsg,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Caught => "CAUGHT",
            Verdict::Survived => "SURVIVED",
            Verdict::Equivalent => "EQUIVALENT",
            Verdict::WarnRcOnly => "WARN-RC-ONLY",
            Verdict::WarnNoMsg => "WARN-NO-MSG",
        }
    }
}

/// One run: the tree to copy, the plants, the new directory and the command that builds the
/// products in the copy (`cargo xtask so`; the unit tests use a toy tree without one).
pub struct Config {
    pub src: PathBuf,
    pub plants: Vec<Plant>,
    pub dir: PathBuf,
    pub prepare: Vec<String>,
    pub timeout: u64,
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

const USAGE: &str = "usage: cargo xtask plants <plants-file> --dir <new-directory> [--timeout <secs>]";

fn parse_args(root: &Path, args: &[String]) -> Result<Config, String> {
    let (mut file, mut dir, mut timeout) = (None, None, DEFAULT_TIMEOUT);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dir" => dir = Some(PathBuf::from(it.next().ok_or("--dir needs a directory")?)),
            "--timeout" => timeout = it.next().and_then(|t| t.parse().ok()).filter(|t| *t > 0).ok_or("--timeout needs a positive number of seconds")?,
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
    Ok(Config { src: root.to_path_buf(), plants: parse_plants(&text)?, dir, prepare: vec![cargo, "xtask".into(), "so".into()], timeout })
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
    let fields: Vec<&str> = line.split(" | ").map(str::trim).collect();
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
    Ok(Plant { id: id.to_string(), path: path.to_string(), pattern, cmd, expect })
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

/// Run `argv` (or `sh -c <cmd>`) in `cwd` under `timeout`, its output into `log`; the exit code.
fn run_logged(cwd: &Path, argv: &[String], log: &Path, timeout: u64) -> Result<i32, String> {
    let file = fs::File::create(log).map_err(|e| format!("cannot create {}: {e}", log.display()))?;
    let err = file.try_clone().map_err(|e| e.to_string())?;
    let status = Command::new("timeout")
        .arg(timeout.to_string())
        .args(argv)
        .current_dir(cwd)
        // A target directory from the environment would build (and test) outside the copy.
        .env_remove("CARGO_TARGET_DIR")
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(err)
        .status()
        .map_err(|e| format!("cannot run {argv:?}: {e}"))?;
    Ok(status.code().unwrap_or(128))
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

/// Write a file anew (a fresh mtime, so cargo rebuilds what depends on it).
fn write_fresh(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn refused(msg: String) -> Outcome {
    eprintln!("plants: REFUSED: {msg}");
    Outcome { code: EXIT_REFUSED, verdicts: Vec::new() }
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
    // The products, then each distinct command on the unmutated copy.
    let mut baseline: Vec<(String, Vec<String>)> = vec![("so".into(), cfg.prepare.clone())];
    for p in &cfg.plants {
        if !baseline.iter().skip(1).any(|(_, argv)| argv[2] == p.cmd) {
            baseline.push((format!("baseline-{}", baseline.len()), sh(&p.cmd)));
        }
    }
    for (name, argv) in &baseline {
        let log = logs.join(format!("{name}.log"));
        match run_logged(&copy, argv, &log, cfg.timeout) {
            Ok(0) => println!("plants: {name} ok ({})", argv.join(" ")),
            Ok(rc) => {
                println!("plants: BASELINE FAILED: {name} rc {rc} ({}); no plant was applied. Last lines of {}:\n{}", argv.join(" "), log.display(), tail(&log, 20));
                return Outcome { code: EXIT_BASELINE, verdicts: Vec::new() };
            }
            Err(e) => {
                println!("plants: BASELINE FAILED: {name}: {e}");
                return Outcome { code: EXIT_BASELINE, verdicts: Vec::new() };
            }
        }
    }
    let mut verdicts = Vec::new();
    let mut failed = false;
    for p in &cfg.plants {
        let file = copy.join(&p.path);
        let original = match fs::read_to_string(&file) {
            Ok(s) => s,
            Err(e) => {
                println!("plants: {}: ERROR: cannot read {}: {e}", p.id, p.path);
                failed = true;
                continue;
            }
        };
        let mutant = match mutate(&original, &p.pattern) {
            Ok(m) => m,
            Err(e) => {
                println!("plants: {}: ERROR: {e}", p.id);
                failed = true;
                continue;
            }
        };
        if let Err(e) = write_fresh(&file, &mutant) {
            println!("plants: {}: ERROR: {e}", p.id);
            failed = true;
            continue;
        }
        let log = logs.join(format!("{}.log", p.id));
        let rc = run_logged(&copy, &sh(&p.cmd), &log, cfg.timeout);
        // Restore by writing (a fresh mtime) before anything else can go wrong.
        if let Err(e) = write_fresh(&file, &original) {
            println!("plants: {}: ERROR: restore: {e}; stopping (the copy holds the mutant)", p.id);
            return Outcome { code: EXIT_SURVIVED, verdicts };
        }
        let rc = match rc {
            Ok(rc) => rc,
            Err(e) => {
                println!("plants: {}: ERROR: {e}", p.id);
                failed = true;
                continue;
            }
        };
        let (verdict, detail) = match (&p.expect, rc) {
            (Expect::Equivalent(why), 0) => (Verdict::Equivalent, why.clone()),
            (_, 0) => (Verdict::Survived, format!("`{}` passed on the mutant", p.cmd)),
            (Expect::Msg(re), rc) if log_matches(&log, re) => (Verdict::Caught, format!("rc {rc}, /{re}/ in the log")),
            (Expect::Msg(re), rc) => (Verdict::WarnNoMsg, format!("rc {rc}, but /{re}/ is not in {}", log.display())),
            (Expect::Equivalent(_), rc) => (Verdict::Caught, format!("rc {rc} (declared equivalent, but caught)")),
            (Expect::RcOnly, rc) => (Verdict::WarnRcOnly, format!("rc {rc}, no msg: given")),
        };
        println!("plants: {}: {} ({}; log {})", p.id, verdict.label(), detail, log.display());
        verdicts.push((p.id.clone(), verdict));
    }
    let count = |v: Verdict| verdicts.iter().filter(|(_, x)| *x == v).count();
    let survived = count(Verdict::Survived);
    println!(
        "plants: {} plants: {} CAUGHT, {} SURVIVED, {} EQUIVALENT, {} WARN-RC-ONLY, {} WARN-NO-MSG{}",
        cfg.plants.len(),
        count(Verdict::Caught),
        survived,
        count(Verdict::Equivalent),
        count(Verdict::WarnRcOnly),
        count(Verdict::WarnNoMsg),
        if failed { ", and errors (see above)" } else { "" }
    );
    let code = if survived > 0 || failed { EXIT_SURVIVED } else { EXIT_OK };
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
        Config { src: src.to_path_buf(), plants: parse_plants(plants).unwrap(), dir: dir.to_path_buf(), prepare: sh("true"), timeout: 60 }
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
        let _ = fs::remove_dir_all(src.parent().unwrap());
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
    }
}
