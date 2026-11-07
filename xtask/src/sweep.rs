//! `sweep` (SWEEP-RUN-TOOL): the periodic timing sweep's runner and its analyzer.
//!
//! `sweep run --out <dir> [--only <bench>,...]`, in a staged tree (`bench --stage`) on the timing host, runs
//! `bench --no-build --staged --counters --only <groups>` with its output in `<dir>/bench.log`. The default groups
//! are every `cbench_*` group of `bench/budgets.tsv`, `cbench_libm` included (SWEEP-COMPARE-SELECTED: the libm bench
//! takes about two minutes per host). It then decides from the tables the
//! bench wrote, not from its exit status alone, whether the MEASUREMENT finished:
//! - finished (exit 0): every selected `ratio_host` row has a median in a `ratio_host.tsv` written by this run,
//!   `rounds.tsv` and a non-empty `counters.tsv` were written, and a nonzero bench exit is explained entirely by
//!   limit flags (every line of the bench's `budget regressions` report says that a row exceeds a limit). It
//!   writes the per-host table `<dir>/sweep.tsv`, copies the bench's tables into `<dir>`, and prints one summary
//!   line (also in `<dir>/summary.txt`) naming the flagged rows. A flagged row is what a sweep looks for, so it
//!   never fails the run. `sweep.tsv`'s `selected` column is `yes` for the selected `ratio_host` rows and `no` for
//!   counted rows outside the selection (the libm bench's calibration rows). With libm rows selected it also writes
//!   and prints the per-family table of the libm time ratios (`<dir>/libm-families.md`).
//!   It also writes and prints the distribution of the selected rows' time ratios (`<dir>/distribution.md`,
//!   SWEEP-DISTRIBUTION): rows per bucket (<= 1.05, 1.05-2, 2-10, 10-100, > 100), the five slowest rows, and the
//!   median per family (libm: family and precision; other benches: the bench), `RETHINK?` above 10x.
//! - not finished (nonzero): the bench died on a signal, a table is missing (the tables of an earlier run are
//!   deleted before the bench starts, so they never count), a selected row has no median, or the bench reported a
//!   failure that is not a limit flag (a stage refused, a row not measured, a counted value missing). No
//!   `sweep.tsv` is written and `summary.txt` says `INCOMPLETE`.
//!
//! `sweep compare --new <class>=<sweep.tsv>... (--old <class>=<sweep.tsv>... | --old-table <results.md>)
//! [--label <name>]` compares the new per-host tables (one per CPU class) with the previous sweep's and prints the
//! report sections to stdout: verdict counts per class, the time-ratio distribution and family medians per class, (a) regressions, (b) reopen, (c) improved, rows new or no
//! longer measured, run-to-run spread, the median change of the time ratio and the libm per-family table. It judges
//! only the rows whose `selected` column is `yes`; a table without the column (written before
//! SWEEP-COMPARE-SELECTED) has every row judged, as before, and the report says so. `--old-table` reads the combined
//! markdown table of a results file written before this tool (the rows after `## Table`, columns `instr <CLASS>`,
//! `time <CLASS>`, `spread <CLASS>`). The output depends only on the input tables (rows sorted by name).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The bench tables a sweep reads and keeps (`bench::out_dir`).
const OUT_TABLES: [&str; 5] = ["ratio_host.tsv", "rounds.tsv", "counters.tsv", "decision.tsv", "bench_times.tsv"];
/// Rows timed but not judged (owner deferral of the wordexp rows, kept from the 1106 sweeps).
const DEFERRED: [&str; 1] = ["wordexp"];
/// Verdict bounds on the median Liella/glibc time ratio.
const FASTER: f64 = 0.95;
const SLOWER: f64 = 1.05;
const USAGE: &str = "usage: xtask sweep run --out <dir> [--only <bench>,...]\n       xtask sweep compare --new <class>=<sweep.tsv>... (--old <class>=<sweep.tsv>... | --old-table <results.md>) [--label <name>]";
/// The columns of `sweep.tsv`.
const SWEEP_HEADER: &str = "bench\trow\tinstr\tcycles\ttime\tspread\trounds\tverdict\tlimit\tstatus\tflagged\tselected\n";

pub(crate) fn run(root: &Path, args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("run") => {
            let exe = std::env::current_exe().map_err(|e| format!("sweep run: the xtask binary: {e}"))?;
            run_sweep(root, &[exe.into_os_string()], &args[1..])
        }
        Some("compare") => compare(&args[1..]).map(|r| print!("{r}")),
        _ => Err(USAGE.into()),
    }
}

/// `--flag value` pairs; every flag must be in `allowed`.
fn parse_args<'a>(args: &'a [String], allowed: &[&str]) -> Result<BTreeMap<&'a str, Vec<&'a str>>, String> {
    let mut m: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if !allowed.contains(&a.as_str()) {
            return Err(format!("sweep: unknown argument `{a}`\n{USAGE}"));
        }
        let v = it.next().ok_or(format!("sweep: {a} needs a value\n{USAGE}"))?;
        m.entry(a.as_str()).or_default().push(v.as_str());
    }
    Ok(m)
}

fn last<'a>(m: &BTreeMap<&str, Vec<&'a str>>, flag: &str) -> Option<&'a str> {
    m.get(flag).and_then(|v| v.last()).copied()
}

/// The selected groups and their `ratio_host` rows `(bench, row)`, in `bench/budgets.tsv` order.
type Selection = (Vec<String>, Vec<(String, String)>);

/// The groups of `--only` (default: every `cbench_*` group, `cbench_libm` included) and their `ratio_host` rows.
fn selection(budgets: &str, only: Option<&str>) -> Result<Selection, String> {
    let rows: Vec<Vec<&str>> = budgets.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).map(|l| l.split('\t').collect()).collect();
    let known: BTreeSet<&str> = rows.iter().filter_map(|r| r.first().copied()).collect();
    let groups: Vec<String> = match only {
        Some(o) => o.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect(),
        None => known.iter().filter(|b| b.starts_with("cbench_")).map(|b| b.to_string()).collect(),
    };
    if groups.is_empty() {
        return Err("sweep run: no bench group selected".into());
    }
    if let Some(g) = groups.iter().find(|g| !known.contains(g.as_str())) {
        return Err(format!("sweep run: `{g}` has no rows in bench/budgets.tsv"));
    }
    let sel: Vec<(String, String)> = rows.iter().filter(|r| r.len() > 3 && r[2] == "ratio_host" && groups.iter().any(|g| g == r[0])).map(|r| (r[0].to_string(), r[1].to_string())).collect();
    if sel.is_empty() {
        return Err(format!("sweep run: the groups {} have no ratio_host row: nothing to time against glibc", groups.join(",")));
    }
    Ok((groups, sel))
}

/// `bench`: the command that runs xtask (the xtask binary itself; a stand-in script in the tests).
fn run_sweep(root: &Path, bench: &[OsString], args: &[String]) -> Result<(), String> {
    let a = parse_args(args, &["--out", "--only"])?;
    let out = PathBuf::from(last(&a, "--out").ok_or(format!("sweep run: --out <dir> is required\n{USAGE}"))?);
    let budgets = fs::read_to_string(root.join("bench/budgets.tsv")).map_err(|e| format!("sweep run: bench/budgets.tsv: {e}"))?;
    let (groups, selected) = selection(&budgets, last(&a, "--only"))?;
    fs::create_dir_all(&out).map_err(|e| format!("sweep run: {}: {e}", out.display()))?;
    let bench_dir = crate::bench::out_dir(root)?;
    // Tables of an earlier run (here or in the bench's directory) must never pass for this run's.
    let stale = OUT_TABLES.iter().map(|t| bench_dir.join(t)).chain(["sweep.tsv", "summary.txt", "libm-families.md", "distribution.md"].iter().map(|t| out.join(t)));
    for p in stale {
        match fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("sweep run: {}: {e}", p.display())),
        }
    }
    let log_path = out.join("bench.log");
    let log = fs::File::create(&log_path).map_err(|e| format!("sweep run: {}: {e}", log_path.display()))?;
    let err = log.try_clone().map_err(|e| format!("sweep run: {}: {e}", log_path.display()))?;
    let only = groups.join(",");
    println!("sweep: {} ratio_host rows of the groups {only}; bench output in {}", selected.len(), log_path.display());
    let (prog, pre) = bench.split_first().ok_or("sweep run: no bench command")?;
    let status = Command::new(prog)
        .args(pre)
        .args(["bench", "--no-build", "--staged", "--counters", "--only", only.as_str()])
        .current_dir(root)
        .stdout(log)
        .stderr(err)
        .status()
        .map_err(|e| format!("sweep run: cannot start the bench: {e}"))?;
    let log_text = String::from_utf8_lossy(&fs::read(&log_path).unwrap_or_default()).into_owned();
    let read = |n: &str| fs::read_to_string(bench_dir.join(n)).ok();
    let tables = Tables { ratio_host: read("ratio_host.tsv"), rounds: read("rounds.tsv"), counters: read("counters.tsv"), decision: read("decision.tsv") };
    let summary = out.join("summary.txt");
    match assess(&selected, status.code(), &log_text, &tables) {
        Ok(s) => {
            for t in OUT_TABLES {
                let p = bench_dir.join(t);
                if p.is_file() {
                    fs::copy(&p, out.join(t)).map_err(|e| format!("sweep run: {}: {e}", p.display()))?;
                }
            }
            let table = out.join("sweep.tsv");
            fs::write(&table, render(&s)).map_err(|e| format!("sweep run: {}: {e}", table.display()))?;
            let line = format!("{} table {}", summary_line(&s), table.display());
            fs::write(&summary, format!("{line}\n")).map_err(|e| format!("sweep run: {}: {e}", summary.display()))?;
            println!("{line}");
            let judged: ClassRows = s.rows.iter().filter(|r| r.selected).map(|r| (format!("{}/{}", r.bench, r.row), Rec { instr: r.instr, time: r.time, spread: r.spread })).collect();
            if let Some(f) = libm_families(&judged) {
                let p = out.join("libm-families.md");
                let text = format!("# libm median time ratio per family and precision (cpu {})\n\n{f}", s.cpu);
                fs::write(&p, &text).map_err(|e| format!("sweep run: {}: {e}", p.display()))?;
                print!("{text}");
            }
            if let Some(d) = distribution(&judged) {
                let p = out.join("distribution.md");
                let text = format!("# Time ratio distribution and family medians (cpu {})\n\n{d}", s.cpu);
                fs::write(&p, &text).map_err(|e| format!("sweep run: {}: {e}", p.display()))?;
                print!("{text}");
            }
            Ok(())
        }
        Err(e) => {
            let msg = format!("sweep: INCOMPLETE: {e} (bench {status}); the measurement did not finish, no sweep.tsv written");
            let _ = fs::write(&summary, format!("{msg}\n"));
            let lines: Vec<&str> = log_text.lines().collect();
            let tail = lines[lines.len().saturating_sub(5)..].join("\n");
            Err(format!("{msg}\nlast lines of {}:\n{tail}", log_path.display()))
        }
    }
}

/// The bench tables of one run (`None`: not written by it).
#[derive(Default, Clone)]
struct Tables {
    ratio_host: Option<String>,
    rounds: Option<String>,
    counters: Option<String>,
    decision: Option<String>,
}

/// A tab-separated table with a header line.
struct Tsv<'a> {
    cols: Vec<&'a str>,
    rows: Vec<Vec<&'a str>>,
}

impl<'a> Tsv<'a> {
    fn parse(name: &str, text: &'a str, need: &[&str]) -> Result<Self, String> {
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let cols: Vec<&str> = lines.next().ok_or(format!("{name} is empty"))?.split('\t').collect();
        if let Some(n) = need.iter().find(|n| !cols.contains(n)) {
            return Err(format!("{name}: no column `{n}` in its header"));
        }
        Ok(Tsv { cols, rows: lines.map(|l| l.split('\t').collect()).collect() })
    }

    fn get(&self, row: &[&'a str], col: &str) -> Option<&'a str> {
        self.cols.iter().position(|c| *c == col).and_then(|i| row.get(i).copied())
    }

    fn key(&self, row: &[&'a str]) -> (String, String) {
        (self.get(row, "bench").unwrap_or("").to_string(), self.get(row, "row").unwrap_or("").to_string())
    }
}

fn num(s: Option<&str>) -> Option<f64> {
    s.and_then(|s| s.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
}

/// One row of the per-host table.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    bench: String,
    row: String,
    instr: Option<f64>,
    cycles: Option<f64>,
    time: Option<f64>,
    spread: Option<f64>,
    rounds: usize,
    limit: String,
    status: String,
    flagged: bool,
    /// A `ratio_host` row of the selected groups (false: a counted row outside the selection, e.g. a calibration row).
    selected: bool,
}

/// A finished sweep on one host.
#[derive(Debug)]
struct Sweep {
    cpu: String,
    rc: i32,
    measured: usize,
    rows: Vec<Row>,
    flagged: Vec<String>,
}

/// The failure lines of the bench's last `budget regressions` report (`None`: no report in the log).
fn limit_report(log: &str) -> Option<Vec<&str>> {
    let lines: Vec<&str> = log.lines().collect();
    let at = lines.iter().rposition(|l| l.contains("bench: budget regressions:"))?;
    Some(lines[at + 1..].iter().take_while(|l| l.starts_with("  ")).map(|l| l.trim()).collect())
}

/// Whether the run measured every selected row (see the module comment); the table and the flagged rows if so.
fn assess(selected: &[(String, String)], code: Option<i32>, log: &str, t: &Tables) -> Result<Sweep, String> {
    let rc = code.ok_or("the bench was killed by a signal (a crash or a kill)")?;
    let rh_text = t.ratio_host.as_deref().ok_or("ratio_host.tsv was not written by this run (the bench stopped before every selected row was measured)")?;
    let rounds_text = t.rounds.as_deref().ok_or("rounds.tsv was not written by this run")?;
    let cnt_text = t.counters.as_deref().ok_or("counters.tsv was not written by this run (the counted run did not finish)")?;
    let rh = Tsv::parse("ratio_host.tsv", rh_text, &["cpu", "bench", "row", "median_ratio", "limit", "status"])?;
    let rd = Tsv::parse("rounds.tsv", rounds_text, &["bench", "row", "liella_ns", "host_ns"])?;
    let cn = Tsv::parse("counters.tsv", cnt_text, &["bench", "row", "instr_ratio", "cycles_ratio"])?;
    if cn.rows.is_empty() {
        return Err("counters.tsv has no counted row".into());
    }
    let dec = t.decision.as_deref().map(|d| Tsv::parse("decision.tsv", d, &["bench", "row", "verdict"])).transpose()?;
    let mut flagged: BTreeSet<String> = BTreeSet::new();
    let mut host: BTreeMap<(String, String), (Option<f64>, String, String)> = BTreeMap::new();
    let mut cpu = String::from("?");
    for r in &rh.rows {
        let k = rh.key(r);
        let status = rh.get(r, "status").unwrap_or("").to_string();
        if status.starts_with("FAIL") {
            flagged.insert(format!("{}/{}", k.0, k.1));
        }
        cpu = rh.get(r, "cpu").unwrap_or("?").to_string();
        host.insert(k, (num(rh.get(r, "median_ratio")), rh.get(r, "limit").unwrap_or("-").to_string(), status));
    }
    let missing: Vec<String> = selected.iter().filter(|k| !matches!(host.get(*k), Some((Some(_), _, _)))).map(|(b, r)| format!("{b}/{r}")).collect();
    if !missing.is_empty() {
        let first: Vec<&str> = missing.iter().take(5).map(String::as_str).collect();
        return Err(format!("{} of {} selected rows not measured (no median in ratio_host.tsv): {}", missing.len(), selected.len(), first.join(", ")));
    }
    let mut verdicts: BTreeMap<(String, String), String> = BTreeMap::new();
    if let Some(d) = &dec {
        for r in &d.rows {
            let (k, v) = (d.key(r), d.get(r, "verdict").unwrap_or("").to_string());
            if v.starts_with("FAIL") {
                flagged.insert(format!("{}/{}", k.0, k.1));
            }
            verdicts.insert(k, v);
        }
    }
    if rc != 0 {
        let report = limit_report(log).ok_or(format!("the bench exited with {rc} without its `bench: budget regressions:` report"))?;
        if report.is_empty() {
            return Err(format!("the bench exited with {rc} and an empty `budget regressions` report"));
        }
        for l in report {
            match l.split_once(": ") {
                Some((k, what)) if k.contains('/') && !k.contains(' ') && what.contains(" exceed") => {
                    flagged.insert(k.to_string());
                }
                _ => return Err(format!("the bench reported a failure that is not a limit flag: `{l}`")),
            }
        }
    }
    let mut ratios: BTreeMap<(String, String), Vec<f64>> = BTreeMap::new();
    for r in &rd.rows {
        if let (Some(l), Some(h)) = (num(rd.get(r, "liella_ns")), num(rd.get(r, "host_ns"))) {
            if h > 0.0 {
                ratios.entry(rd.key(r)).or_default().push(l / h);
            }
        }
    }
    let counted: BTreeMap<(String, String), &Vec<&str>> = cn.rows.iter().map(|r| (cn.key(r), r)).collect();
    let keys: BTreeSet<(String, String)> = selected.iter().cloned().chain(counted.keys().cloned()).collect();
    let rows = keys
        .into_iter()
        .map(|k| {
            let is_selected = selected.contains(&k);
            let rr = ratios.get(&k).map(Vec::as_slice).unwrap_or(&[]);
            let (h_med, limit, mut status) = host.get(&k).cloned().unwrap_or((None, "-".into(), "-".into()));
            let time = if rr.is_empty() { h_med } else { Some(crate::bench::stats(rr).0) };
            let spread = time.filter(|t| *t > 0.0 && !rr.is_empty()).map(|t| {
                let (_, lo, hi) = crate::bench::stats(rr);
                (hi - lo) / t
            });
            if status.starts_with("advisory") {
                if let Some(v) = verdicts.get(&k) {
                    status = v.clone();
                }
            }
            let c = counted.get(&k);
            let key = format!("{}/{}", k.0, k.1);
            Row {
                instr: c.and_then(|r| num(cn.get(r, "instr_ratio"))),
                cycles: c.and_then(|r| num(cn.get(r, "cycles_ratio"))),
                time,
                spread,
                rounds: rr.len(),
                limit,
                status,
                flagged: flagged.contains(&key),
                selected: is_selected,
                bench: k.0,
                row: k.1,
            }
        })
        .collect();
    Ok(Sweep { cpu, rc, measured: selected.len(), rows, flagged: flagged.into_iter().collect() })
}

fn verdict(t: Option<f64>) -> &'static str {
    match t {
        None => "-",
        Some(t) if t < FASTER => "faster",
        Some(t) if t > SLOWER => "slower",
        Some(_) => "parity",
    }
}

fn f3(x: Option<f64>) -> String {
    x.map_or("-".into(), |v| format!("{v:.3}"))
}

/// `sweep.tsv`: one line per selected row (and per counted row), sorted by bench and row.
fn render(s: &Sweep) -> String {
    let mut out = String::from(SWEEP_HEADER);
    for r in &s.rows {
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            r.bench,
            r.row,
            f3(r.instr),
            f3(r.cycles),
            f3(r.time),
            f3(r.spread),
            r.rounds,
            verdict(r.time),
            r.limit,
            r.status,
            if r.flagged { "yes" } else { "no" },
            if r.selected { "yes" } else { "no" }
        );
    }
    out
}

/// The verdict counts cover the selected rows only (TIMING-SWEEP-LIBM-1107: the libm bench also counts
/// `add_baseline` and two `mxcsr_*` rows, which are in `sweep.tsv` but are not `ratio_host` rows); the
/// other counted rows are named after them.
fn summary_line(s: &Sweep) -> String {
    let judged: Vec<&str> = s.rows.iter().filter(|r| r.selected && !deferred(&r.row)).map(|r| verdict(r.time)).filter(|v| *v != "-").collect();
    let n = |v: &str| judged.iter().filter(|x| **x == v).count();
    let flags = if s.flagged.is_empty() { String::from("0 rows flagged by the limit check") } else { format!("{} rows flagged by the limit check (in the table, not a run failure): {}", s.flagged.len(), s.flagged.join(", ")) };
    let other: Vec<String> = s.rows.iter().filter(|r| !r.selected).map(|r| format!("{}/{}", r.bench, r.row)).collect();
    let other = if other.is_empty() { String::new() } else { format!(" {} counted rows outside the selection (in the table, not in the counts): {};", other.len(), other.join(", ")) };
    format!("sweep: COMPLETE: cpu {}; all {} selected ratio_host rows measured ({} faster, {} parity, {} slower); bench rc {}; {flags};{other}", s.cpu, s.measured, n("faster"), n("parity"), n("slower"), s.rc)
}

fn deferred(row: &str) -> bool {
    DEFERRED.iter().any(|d| row.contains(d))
}

/// One row of one class in a sweep table.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Rec {
    instr: Option<f64>,
    time: Option<f64>,
    spread: Option<f64>,
}

type ClassRows = BTreeMap<String, Rec>;

/// A `sweep.tsv` as `sweep compare` reads it: the judged rows, the rows outside the selection (`selected` is not
/// `yes`), and whether the table has the `selected` column (without it every row is judged).
#[derive(Debug, Default)]
struct SweepTable {
    rows: ClassRows,
    outside: BTreeSet<String>,
    has_selected: bool,
}

fn load_sweep(name: &str, text: &str) -> Result<SweepTable, String> {
    let t = Tsv::parse(name, text, &["bench", "row", "instr", "time", "spread"])?;
    let mut out = SweepTable { has_selected: t.cols.contains(&"selected"), ..SweepTable::default() };
    for r in &t.rows {
        let k = format!("{}/{}", t.get(r, "bench").unwrap_or(""), t.get(r, "row").unwrap_or(""));
        if out.has_selected && t.get(r, "selected") != Some("yes") {
            out.outside.insert(k);
        } else {
            out.rows.insert(k, Rec { instr: num(t.get(r, "instr")), time: num(t.get(r, "time")), spread: num(t.get(r, "spread")) });
        }
    }
    Ok(out)
}

/// The libm families of the per-family table (the grouping of TIMING-SWEEP-LIBM-1107); a row named
/// `<function>_<case>` (an input or rounding-mode case) is in `special`.
const LIBM_FAMILIES: [(&str, &[&str]); 8] = [
    ("exp/log/pow", &["exp", "exp2", "exp10", "expm1", "log", "log2", "log10", "log1p", "pow"]),
    ("trig", &["sin", "cos", "tan", "asin", "acos", "atan", "atan2"]),
    ("hyperbolic", &["sinh", "cosh", "tanh", "asinh", "acosh", "atanh"]),
    ("gamma", &["tgamma", "lgamma"]),
    ("bessel", &["j0", "j1", "y0", "y1", "jn", "yn"]),
    ("erf", &["erf", "erfc"]),
    ("roots/hypot", &["cbrt", "sqrt", "hypot"]),
    ("complex", &["cexp", "clog", "csin", "csqrt", "cpow"]),
];
/// Function-name suffix and precision, in the table's column order.
const LIBM_PRECISIONS: [(&str, &str); 4] = [("f", "f32"), ("", "f64"), ("l", "long double"), ("f128", "f128")];

/// The family and precision of a `cbench_libm` row (`other`, `-` for a name outside the families).
fn libm_class(row: &str) -> (&'static str, &'static str) {
    let head = row.split('_').next().unwrap_or(row);
    for (family, bases) in LIBM_FAMILIES {
        for b in bases {
            let Some(suffix) = head.strip_prefix(b) else { continue };
            if let Some((_, p)) = LIBM_PRECISIONS.iter().find(|(s, _)| *s == suffix) {
                return (if head.len() < row.len() { "special" } else { family }, p);
            }
        }
    }
    ("other", "-")
}

/// The median time ratio per libm family and precision over the timed `cbench_libm` rows (`None`: no such row).
fn libm_families(rows: &ClassRows) -> Option<String> {
    let timed: Vec<(&str, &str, f64)> = rows
        .iter()
        .filter_map(|(k, r)| k.strip_prefix("cbench_libm/").filter(|n| !deferred(n)).zip(r.time))
        .map(|(n, t)| {
            let (f, p) = libm_class(n);
            (f, p, t)
        })
        .collect();
    if timed.is_empty() {
        return None;
    }
    let med = |v: &[f64]| format!("{:.2}", crate::bench::stats(v).0);
    let mut s = String::from("| family | rows | median | f32 | f64 | long double | f128 |\n|---|---:|---:|---:|---:|---:|---:|\n");
    let families = LIBM_FAMILIES.iter().map(|(f, _)| *f).chain(["special", "other", "all"]);
    for family in families {
        let of: Vec<&(&str, &str, f64)> = timed.iter().filter(|r| family == "all" || r.0 == family).collect();
        if of.is_empty() {
            continue;
        }
        let all: Vec<f64> = of.iter().map(|r| r.2).collect();
        let cells: Vec<String> = LIBM_PRECISIONS
            .iter()
            .map(|(_, p)| {
                let v: Vec<f64> = of.iter().filter(|r| r.1 == *p).map(|r| r.2).collect();
                if v.is_empty() { "-".into() } else { format!("{} ({})", med(&v), v.len()) }
            })
            .collect();
        let _ = writeln!(s, "| {family} | {} | {} | {} |", of.len(), med(&all), cells.join(" | "));
    }
    Some(s)
}

/// Upper edges of the time-ratio buckets (SWEEP-DISTRIBUTION); a ratio on an edge is in the lower bucket.
const BUCKET_EDGES: [f64; 4] = [SLOWER, 2.0, 10.0, 100.0];
const BUCKET_NAMES: [&str; 5] = ["<= 1.05", "1.05-2", "2-10", "10-100", "> 100"];
/// The magnitude trigger of the RUT rule: a family whose median time ratio is above this is marked `RETHINK?`.
const RETHINK_ABOVE: f64 = 10.0;
/// How many of the slowest rows the distribution names.
const SLOWEST: usize = 5;

/// The bucket of a time ratio (an index into `BUCKET_NAMES`).
fn bucket(t: f64) -> usize {
    BUCKET_EDGES.iter().position(|e| t <= *e).unwrap_or(BUCKET_EDGES.len())
}

/// The family of a judged row: a `cbench_libm` row by its family and precision (the cells of the libm table),
/// every other row by its bench.
fn family(key: &str) -> String {
    match key.strip_prefix("cbench_libm/") {
        Some(n) => match libm_class(n) {
            (f, "-") => format!("cbench_libm {f}"),
            (f, p) => format!("cbench_libm {f} {p}"),
        },
        None => key.split('/').next().unwrap_or(key).to_string(),
    }
}

/// The distribution of the time ratios of the judged timed rows in fixed buckets, the slowest rows, and the
/// median per family with the families above the magnitude trigger marked `RETHINK?` (`None`: no timed row).
fn distribution(rows: &ClassRows) -> Option<String> {
    let timed: Vec<(&str, f64)> = rows.iter().filter(|(k, _)| !deferred(k)).filter_map(|(k, r)| r.time.map(|t| (k.as_str(), t))).collect();
    if timed.is_empty() {
        return None;
    }
    let mut n = [0usize; BUCKET_NAMES.len()];
    for (_, t) in &timed {
        n[bucket(*t)] += 1;
    }
    let mut s = String::from("| time ratio | rows |\n|---|---:|\n");
    for (name, c) in BUCKET_NAMES.iter().zip(n) {
        let _ = writeln!(s, "| {name} | {c} |");
    }
    let mut slow = timed.clone();
    slow.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
    let slow: Vec<String> = slow.iter().take(SLOWEST).map(|(k, t)| format!("{k} {t:.2}")).collect();
    let _ = write!(s, "\nSlowest {}: {}\n\n", slow.len(), slow.join(", "));
    let mut fam: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for (k, t) in &timed {
        fam.entry(family(k)).or_default().push(*t);
    }
    let meds: Vec<(String, usize, f64)> = fam.into_iter().map(|(f, v)| (f, v.len(), crate::bench::stats(&v).0)).collect();
    let marked: Vec<String> = meds.iter().filter(|m| m.2 > RETHINK_ABOVE).map(|(f, n, m)| format!("{f} {m:.2} ({n})")).collect();
    let marked = if marked.is_empty() { String::from("none") } else { format!("{}: {}", marked.len(), marked.join(", ")) };
    let _ = write!(s, "RETHINK? (family median time ratio above {RETHINK_ABOVE}x, the magnitude trigger): {marked}\n\n| family | rows | median | |\n|---|---:|---:|---|\n");
    for (f, n, m) in &meds {
        let _ = writeln!(s, "| {f} | {n} | {m:.2} |{} |", if *m > RETHINK_ABOVE { " RETHINK?" } else { "" });
    }
    Some(s)
}

/// The combined markdown table of a results file (rows after `## Table`; class from `time <CLASS>` columns).
fn load_legacy(name: &str, text: &str) -> Result<BTreeMap<String, ClassRows>, String> {
    let mut out: BTreeMap<String, ClassRows> = BTreeMap::new();
    let (mut on, mut cols) = (false, Vec::<String>::new());
    for line in text.lines() {
        if line.starts_with("## Table") {
            on = true;
            continue;
        }
        if !on || !line.starts_with('|') {
            continue;
        }
        let c: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
        if c.first() == Some(&"bench/row") {
            cols = c.iter().map(|s| s.to_string()).collect();
            continue;
        }
        if cols.is_empty() || !c[0].contains('/') {
            continue;
        }
        for (i, col) in cols.iter().enumerate() {
            let Some((field, class)) = col.split_once(' ') else { continue };
            if !matches!(field, "instr" | "time" | "spread") || class.contains(' ') {
                continue;
            }
            let rec = out.entry(class.to_lowercase()).or_default().entry(c[0].to_string()).or_default();
            let v = num(c.get(i).copied());
            match field {
                "instr" => rec.instr = v,
                "time" => rec.time = v,
                _ => rec.spread = v,
            }
        }
    }
    if out.is_empty() {
        return Err(format!("{name}: no `## Table` rows with `time <CLASS>` columns"));
    }
    Ok(out)
}

fn compare(args: &[String]) -> Result<String, String> {
    let a = parse_args(args, &["--new", "--old", "--old-table", "--label"])?;
    let label = last(&a, "--label").unwrap_or("previous");
    let read = |p: &str| fs::read_to_string(p).map_err(|e| format!("sweep compare: {p}: {e}"));
    let pairs = |flag: &str| -> Result<Vec<(String, SweepTable)>, String> {
        a.get(flag)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|v| {
                let (class, path) = v.split_once('=').ok_or(format!("sweep compare: {flag} wants <class>=<sweep.tsv>, not `{v}`"))?;
                Ok((class.to_lowercase(), load_sweep(path, &read(path)?)?))
            })
            .collect()
    };
    let new = pairs("--new")?;
    if new.is_empty() {
        return Err(format!("sweep compare: no --new table\n{USAGE}"));
    }
    let old_tables = pairs("--old")?;
    // A table written before the `selected` column judges every row, the calibration rows included.
    let notes: Vec<String> = new
        .iter()
        .map(|(c, t)| ("new", c, t))
        .chain(old_tables.iter().map(|(c, t)| ("old", c, t)))
        .filter(|(_, _, t)| !t.has_selected)
        .map(|(w, c, _)| format!("The {w} `{c}` table has no `selected` column (written before SWEEP-COMPARE-SELECTED): all its rows are judged, calibration rows included."))
        .collect();
    let mut old: BTreeMap<String, ClassRows> = old_tables.into_iter().map(|(c, t)| (c, t.rows)).collect();
    if let Some(p) = last(&a, "--old-table") {
        for (c, rows) in load_legacy(p, &read(p)?)? {
            old.entry(c).or_insert(rows);
        }
    }
    if let Some((c, _)) = new.iter().find(|(c, _)| !old.contains_key(c)) {
        let have: Vec<&str> = old.keys().map(String::as_str).collect();
        return Err(format!("sweep compare: no previous table for class `{c}` (previous classes: {})", have.join(", ")));
    }
    Ok(report(&new, &old, label, &notes))
}

/// The value at fraction `p` of a sorted list (the 1106 analyzers' index rule).
fn pct(s: &[f64], p: f64) -> f64 {
    s[((s.len() as f64 * p) as usize).min(s.len() - 1)]
}

fn dist(v: &mut [f64], lo: f64, hi: f64) -> String {
    if v.is_empty() {
        return "0 | - | - | -".into();
    }
    v.sort_by(f64::total_cmp);
    format!("{} | {:.3} | {:.3} | {:.3}", v.len(), crate::bench::stats(v).0, pct(v, lo), pct(v, hi))
}

/// `notes`: one line per table read without the `selected` column.
fn report(new: &[(String, SweepTable)], old: &BTreeMap<String, ClassRows>, label: &str, notes: &[String]) -> String {
    let empty = ClassRows::new();
    let keys: BTreeSet<&String> = new.iter().flat_map(|(_, t)| t.rows.keys()).collect();
    let mut families = String::new();
    let mut dists = String::new();
    let mut lists: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut counts = String::new();
    let mut spread = String::new();
    for (class, t) in new {
        let rows = &t.rows;
        let o = old.get(class).unwrap_or(&empty);
        if let Some(f) = libm_families(rows) {
            let _ = write!(families, "## libm median time ratio per family and precision: {class}\n\n{f}\n");
        }
        if let Some(d) = distribution(rows) {
            let _ = write!(dists, "## Time ratio distribution and family medians: {class}\n\n{d}\n");
        }
        for k in &t.outside {
            lists.entry("outside").or_default().push(format!("- {k} | {class}"));
        }
        let (mut now_v, mut old_v, mut sp, mut drift) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (k, r) in rows {
            sp.extend(r.spread);
            if deferred(k) || r.time.is_none() {
                continue;
            }
            now_v.push(verdict(r.time));
            if let Some(ot) = o.get(k).and_then(|x| x.time) {
                old_v.push(verdict(Some(ot)));
                drift.push(r.time.unwrap_or(0.0) / ot);
            }
        }
        let n = |v: &[&str], w: &str| v.iter().filter(|x| **x == w).count();
        let _ = writeln!(counts, "| {class} | {} | {} | {} | {} | {} | {} | {} | {} |", n(&now_v, "faster"), n(&now_v, "parity"), n(&now_v, "slower"), now_v.len(), n(&old_v, "faster"), n(&old_v, "parity"), n(&old_v, "slower"), old_v.len());
        let _ = writeln!(spread, "| {class} | {} | {} |", dist(&mut sp, 0.9, 1.0), dist(&mut drift, 0.1, 0.9));
        for (k, r) in o {
            if r.time.is_some() && !rows.contains_key(k) && !t.outside.contains(k) {
                lists.entry("gone").or_default().push(format!("- {k} | {class} | {label} time {}", f3(r.time)));
            }
        }
    }
    for k in &keys {
        for (class, rows) in new {
            let Some(r) = rows.rows.get(*k) else { continue };
            let o = old.get(class).and_then(|c| c.get(*k)).copied().unwrap_or_default();
            let desc = format!("- {k} | {class} | instr {} ({label} {}) | time {} ({label} {}) | spread {}", f3(r.instr), f3(o.instr), f3(r.time), f3(o.time), f3(r.spread));
            if deferred(k) {
                lists.entry("deferred").or_default().push(desc);
                continue;
            }
            let Some(t) = r.time else { continue };
            let (v, ov) = (verdict(r.time), verdict(o.time));
            if v == "slower" && matches!(ov, "parity" | "faster") {
                lists.entry("a").or_default().push(desc.clone());
            }
            if r.instr.is_some_and(|i| i <= SLOWER) && t > SLOWER {
                lists.entry("b").or_default().push(desc.clone());
            }
            if ov == "slower" && matches!(v, "parity" | "faster") {
                lists.entry("c").or_default().push(desc.clone());
            }
            if o.time.is_none() {
                lists.entry("new").or_default().push(desc);
            }
        }
    }
    let classes: Vec<&str> = new.iter().map(|(c, _)| c.as_str()).collect();
    let mut s = format!("# Sweep comparison: {} against {label}\n\n", classes.join(", "));
    let _ = write!(s, "Verdicts on the median Liella/glibc time ratio: faster < {FASTER}, slower > {SLOWER}, parity between; rows named {} are timed, not judged.\n\n", DEFERRED.join(", "));
    for n in notes {
        let _ = writeln!(s, "{n}\n");
    }
    let _ = write!(s, "## Counts\n\n| class | faster | parity | slower | judged | {label} faster | {label} parity | {label} slower | {label} judged (rows measured now) |\n|---|---|---|---|---|---|---|---|---|\n{counts}\n");
    s.push_str(&dists);
    let _ = write!(s, "## Run-to-run spread and change\n\nspread = (max - min) / median of the round ratios; change = time now / time in {label}.\n\n| class | spread n | spread median | spread p90 | spread max | change n | change median | change p10 | change p90 |\n|---|---|---|---|---|---|---|---|---|\n{spread}\n{families}");
    let titles = [
        ("a", format!("(a) Regressions: slower now, parity or faster in {label}")),
        ("b", format!("(b) Reopen: instr <= {SLOWER}, time > {SLOWER}")),
        ("c", format!("(c) Improved: slower in {label}, parity or faster now")),
        ("new", format!("Rows not in {label}")),
        ("gone", format!("Rows in {label} not measured now")),
        ("deferred", String::from("Deferred rows (timed, not judged)")),
        ("outside", String::from("Rows outside the selection (in sweep.tsv, not judged)")),
    ];
    for (tag, title) in titles {
        let l = lists.get(tag).map(Vec::as_slice).unwrap_or(&[]);
        let _ = writeln!(s, "## {title}: {}\n", l.len());
        for x in l {
            let _ = writeln!(s, "{x}");
        }
        if !l.is_empty() {
            s.push('\n');
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    // SWEEP-RUN-TOOL: no spec clause (developer tooling). Fixture tables in the bench's own formats
    // (`ratio_host.tsv`, `rounds.tsv`, `counters.tsv`, `decision.tsv` as bench.rs writes them).
    const RH: &str = "cpu\tbench\trow\thost_median\tliella_median\tmedian_ratio\tworst_ratio\tlimit\tstatus\nCPU X\tcbench_x\ta\t10.00\t9.00\t0.900\t0.950\t1.100\tok\nCPU X\tcbench_x\tb\t10.00\t13.00\t1.300\t1.400\t1.100\tFAIL\n";
    const RH_OK: &str = "cpu\tbench\trow\thost_median\tliella_median\tmedian_ratio\tworst_ratio\tlimit\tstatus\nCPU X\tcbench_x\ta\t10.00\t9.00\t0.900\t0.950\t1.100\tok\nCPU X\tcbench_x\tb\t10.00\t10.00\t1.000\t1.100\t1.100\tok\n";
    const RH_NO_B: &str = "cpu\tbench\trow\thost_median\tliella_median\tmedian_ratio\tworst_ratio\tlimit\tstatus\nCPU X\tcbench_x\ta\t10.00\t9.00\t0.900\t0.950\t1.100\tok\n";
    const RH_CYC: &str = "cpu\tbench\trow\thost_median\tliella_median\tmedian_ratio\tworst_ratio\tlimit\tstatus\nCPU X\tcbench_x\ta\t10.00\t9.00\t0.900\t0.950\t1.100\tadvisory (decided on cycles below)\nCPU X\tcbench_x\tb\t10.00\t13.00\t1.300\t1.400\t1.100\tadvisory (decided on cycles below)\n";
    const DEC: &str = "bench\trow\tmetric\tns_ratio\tcycles_ratio\tghz_l\tghz_g\tlimit\tverdict\ncbench_x\ta\tcycles\t0.900\t0.880\t-\t-\t1.100\tok\ncbench_x\tb\tcycles\t1.300\t1.200\t-\t-\t1.100\tFAIL\n";
    const ROUNDS: &str = "bench\trow\tround\tliella_ns\thost_ns\ncbench_x\ta\t1\t9\t10\ncbench_x\ta\t2\t9.5\t10\ncbench_x\tb\t1\t13\t10\n";
    const CNT: &str = "bench\trow\tns_ratio\tcycles_ratio\tuops_ratio\tinstr_ratio\ncbench_x\ta\t0.900\t0.880\t-\t0.950\n";
    const CNT_EMPTY: &str = "bench\trow\tns_ratio\tcycles_ratio\tuops_ratio\tinstr_ratio\n";
    const LOG_FLAG: &str = "bench: all done\nxtask bench: bench: budget regressions:\n  cbench_x/b: median ratio 1.300 exceeds the limit 1.100 (worst 1.40; why)\n";
    const LOG_CYC: &str = "xtask bench: bench: budget regressions:\n  cbench_x/b: cycles ratio 1.200 exceeds the cycle limit 1.100 (ns median 1.300, advisory) (why)\n  cbench_x/a: 120.0 instructions per call exceed the limit 100 (bench/instr_limits.tsv; INSTR-REGRESS)\n";
    const LOG_UNCOUNTED: &str = "xtask bench: bench: budget regressions:\n  cbench_x/b: median ratio 1.300 exceeds the limit 1.100 (worst 1.40; why)\n  cbench_x/a: no counted instruction value, the limit 100 could not be checked (bench/instr_limits.tsv)\n";

    /// (case, bench exit code, log, tables, Ok(flagged rows) or Err(part of the message)).
    type Case = (&'static str, Option<i32>, &'static str, Tables, Result<Vec<&'static str>, &'static str>);

    fn sel() -> Vec<(String, String)> {
        vec![("cbench_x".into(), "a".into()), ("cbench_x".into(), "b".into())]
    }

    fn tables(rh: Option<&str>, rounds: Option<&str>, cnt: Option<&str>, dec: Option<&str>) -> Tables {
        Tables { ratio_host: rh.map(String::from), rounds: rounds.map(String::from), counters: cnt.map(String::from), decision: dec.map(String::from) }
    }

    #[test]
    fn assess_tells_a_finished_run_from_an_unfinished_one() {
        // (case, bench exit code, log, tables, Ok(flagged rows) or Err(part of the message))
        let full = tables(Some(RH), Some(ROUNDS), Some(CNT), None);
        let cases: Vec<Case> = vec![
            ("clean run, rc 0", Some(0), "bench: all 2 budgets met", tables(Some(RH_OK), Some(ROUNDS), Some(CNT), None), Ok(vec![])),
            ("flagged row, rc 1: finished", Some(1), LOG_FLAG, full.clone(), Ok(vec!["cbench_x/b"])),
            ("cycles CPU: decision.tsv FAIL and an instruction limit, rc 1: finished", Some(1), LOG_CYC, tables(Some(RH_CYC), Some(ROUNDS), Some(CNT), Some(DEC)), Ok(vec!["cbench_x/a", "cbench_x/b"])),
            ("a selected row missing, rc 0", Some(0), "", tables(Some(RH_NO_B), Some(ROUNDS), Some(CNT), None), Err("1 of 2 selected rows not measured")),
            ("a selected row missing, rc 1 with a limit report", Some(1), LOG_FLAG, tables(Some(RH_NO_B), Some(ROUNDS), Some(CNT), None), Err("not measured")),
            ("no ratio_host.tsv: the bench stopped early", Some(1), "xtask bench: cbench_x/b: not measured", tables(None, None, None, None), Err("ratio_host.tsv was not written")),
            ("no counters.tsv", Some(0), "", tables(Some(RH_OK), Some(ROUNDS), None, None), Err("counters.tsv was not written")),
            ("counters.tsv without a counted row", Some(0), "", tables(Some(RH_OK), Some(ROUNDS), Some(CNT_EMPTY), None), Err("no counted row")),
            ("killed by a signal", None, LOG_FLAG, full.clone(), Err("signal")),
            ("rc 1 without the limit report", Some(1), "xtask bench: decision.tsv: Permission denied", full.clone(), Err("without its `bench: budget regressions:` report")),
            ("rc 1, a failure that is not a limit flag", Some(1), LOG_UNCOUNTED, full.clone(), Err("not a limit flag: `cbench_x/a: no counted instruction value")),
            ("rc 1, empty limit report", Some(1), "xtask bench: bench: budget regressions:\n", full, Err("empty")),
        ];
        for (name, code, log, t, want) in cases {
            let got = assess(&sel(), code, log, &t);
            match (got, want) {
                (Ok(s), Ok(f)) => assert_eq!(s.flagged, f, "{name}"),
                (Err(e), Err(part)) => assert!(e.contains(part), "{name}: {e}"),
                (got, want) => panic!("{name}: got {got:?}, want {want:?}"),
            }
        }
    }

    #[test]
    fn the_per_host_table_holds_ratios_spread_counters_and_flags() {
        let s = assess(&sel(), Some(1), LOG_FLAG, &tables(Some(RH), Some(ROUNDS), Some(CNT), None)).unwrap();
        // a: round ratios 0.90 and 0.95, median 0.925, spread 0.05 / 0.925; b: one round, no counted row.
        let want = format!("{SWEEP_HEADER}cbench_x\ta\t0.950\t0.880\t0.925\t0.054\t2\tfaster\t1.100\tok\tno\tyes\ncbench_x\tb\t-\t-\t1.300\t0.000\t1\tslower\t1.100\tFAIL\tyes\tyes\n");
        assert_eq!(render(&s), want);
        let line = summary_line(&s);
        assert!(line.starts_with("sweep: COMPLETE: cpu CPU X; all 2 selected ratio_host rows measured (1 faster, 0 parity, 1 slower); bench rc 1; 1 rows flagged"), "{line}");
        assert!(line.contains("cbench_x/b"), "{line}");
        // TIMING-SWEEP-LIBM-1107 (no spec clause; the libm bench's add_baseline/mxcsr_* rows): a counted row outside
        // the selection stays in the table but not in the selected rows' verdict counts, and the line names it.
        let rounds_z = format!("{ROUNDS}cbench_x\tz\t1\t20\t10\n");
        let cnt_z = format!("{CNT}cbench_x\tz\t2.000\t2.000\t-\t2.000\n");
        let z = assess(&sel(), Some(1), LOG_FLAG, &tables(Some(RH), Some(&rounds_z), Some(&cnt_z), None)).unwrap();
        assert_eq!(render(&z).lines().filter(|l| l.starts_with("cbench_x\tz\t") && l.ends_with("\tno")).count(), 1, "{}", render(&z));
        let line = summary_line(&z);
        assert!(line.contains("all 2 selected ratio_host rows measured (1 faster, 0 parity, 1 slower)"), "{line}");
        assert!(line.ends_with("1 counted rows outside the selection (in the table, not in the counts): cbench_x/z;"), "{line}");
        // The cycles CPU's verdict comes from decision.tsv.
        let c = assess(&sel(), Some(1), LOG_CYC, &tables(Some(RH_CYC), Some(ROUNDS), Some(CNT), Some(DEC))).unwrap();
        assert_eq!(c.rows.iter().map(|r| r.status.as_str()).collect::<Vec<_>>(), ["ok", "FAIL"]);
    }

    #[test]
    fn selection_defaults_to_every_c_group_with_libm() {
        let budgets = "# comment\ncbench_x\ta\tratio_host\t1.0\twhy\ncbench_x\tm\tmax_ns\t10\twhy\ncbench_libm\tz\tratio_host\t1.0\twhy\nbench_mem\tq\tmax_ns\t5\twhy\ncbench_y\tc\tratio_host\t1.0\twhy\n";
        let (g, s) = selection(budgets, None).unwrap();
        assert_eq!(g, ["cbench_libm", "cbench_x", "cbench_y"]);
        assert_eq!(s, [("cbench_x".to_string(), "a".to_string()), ("cbench_libm".to_string(), "z".to_string()), ("cbench_y".to_string(), "c".to_string())]);
        assert_eq!(selection(budgets, Some("cbench_libm")).unwrap().1.len(), 1);
        assert!(selection(budgets, Some("cbench_nope")).unwrap_err().contains("no rows"));
        assert!(selection(budgets, Some("bench_mem")).unwrap_err().contains("no ratio_host row"));
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("xtask-sweep-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("bench")).unwrap();
        fs::write(d.join(crate::benchstage::MANIFEST), "commit x\n").unwrap();
        fs::write(d.join("bench/budgets.tsv"), "cbench_x\ta\tratio_host\t1.0\twhy\ncbench_x\tb\tratio_host\t1.0\twhy\ncbench_libm\texpf\tratio_host\t1.0\twhy\n").unwrap();
        fs::create_dir_all(d.join("fix")).unwrap();
        let rh = format!("{RH}CPU X\tcbench_libm\texpf\t10.00\t25.00\t2.500\t2.600\t1.000\tok\n");
        for (n, t) in [("ratio_host.tsv", rh.as_str()), ("rounds.tsv", ROUNDS), ("counters.tsv", CNT)] {
            fs::write(d.join("fix").join(n), t).unwrap();
        }
        d
    }

    /// A stand-in for `xtask-bin`, run through `sh` (never exec'd, so no ETXTBSY under parallel tests).
    fn fake(d: &Path, script: &str) -> Vec<OsString> {
        fs::write(d.join("fake.sh"), script).unwrap();
        vec!["sh".into(), d.join("fake.sh").into_os_string()]
    }

    #[test]
    fn run_finishes_with_flagged_rows_and_writes_the_table() {
        let d = scratch("flagged");
        let out = d.join("out");
        let script = "echo \"args: $*\"\ncp fix/*.tsv bench-out/\necho 'xtask bench: bench: budget regressions:' >&2\necho '  cbench_x/b: median ratio 1.300 exceeds the limit 1.100 (w)' >&2\nexit 1\n";
        run_sweep(&d, &fake(&d, script), &["--out".into(), out.to_string_lossy().into_owned()]).unwrap();
        let log = fs::read_to_string(out.join("bench.log")).unwrap();
        assert!(log.contains("args: bench --no-build --staged --counters --only cbench_libm,cbench_x\n"), "{log}");
        assert!(fs::read_to_string(out.join("sweep.tsv")).unwrap().contains("cbench_x\tb\t-\t-\t1.300\t0.000\t1\tslower\t1.100\tFAIL\tyes\tyes"));
        let fam = fs::read_to_string(out.join("libm-families.md")).unwrap();
        assert!(fam.contains("| exp/log/pow | 1 | 2.50 | 2.50 (1) | - | - | - |\n| all | 1 | 2.50 |"), "{fam}");
        // SWEEP-DISTRIBUTION: the selected rows' distribution and family medians.
        let dist = fs::read_to_string(out.join("distribution.md")).unwrap();
        assert!(dist.contains("| <= 1.05 | 1 |\n| 1.05-2 | 1 |\n| 2-10 | 1 |\n"), "{dist}");
        assert!(dist.contains("| cbench_libm exp/log/pow f32 | 1 | 2.50 | |\n| cbench_x | 2 | 1.11 | |\n"), "{dist}");
        assert!(fs::read_to_string(out.join("summary.txt")).unwrap().starts_with("sweep: COMPLETE:"));
        assert!(out.join("ratio_host.tsv").is_file());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn run_never_counts_the_tables_of_an_earlier_run() {
        let d = scratch("stale");
        let out = d.join("out");
        // A complete earlier run in bench-out/ and an earlier sweep.tsv in the output directory.
        fs::create_dir_all(d.join("bench-out")).unwrap();
        fs::create_dir_all(&out).unwrap();
        for n in ["ratio_host.tsv", "rounds.tsv", "counters.tsv"] {
            fs::copy(d.join("fix").join(n), d.join("bench-out").join(n)).unwrap();
        }
        fs::write(out.join("sweep.tsv"), "old").unwrap();
        let e = run_sweep(&d, &fake(&d, "echo 'xtask bench: cbench_x/b: not measured' >&2\nexit 1\n"), &["--out".into(), out.to_string_lossy().into_owned()]).unwrap_err();
        assert!(e.contains("INCOMPLETE: ratio_host.tsv was not written"), "{e}");
        assert!(e.contains("cbench_x/b: not measured"), "{e}");
        assert!(!out.join("sweep.tsv").exists());
        assert!(fs::read_to_string(out.join("summary.txt")).unwrap().starts_with("sweep: INCOMPLETE:"));
        let _ = fs::remove_dir_all(&d);
    }

    const OLD: &str = "bench\trow\tinstr\tcycles\ttime\tspread\trounds\tverdict\tlimit\tstatus\tflagged\ncbench_x\ta\t1.000\t-\t0.900\t0.010\t5\tfaster\t-\tok\tno\ncbench_x\tb\t1.000\t-\t1.200\t0.020\t5\tslower\t-\tok\tno\ncbench_x\tc\t1.000\t-\t1.000\t0.030\t5\tparity\t-\tok\tno\ncbench_x\tgone\t1.000\t-\t1.000\t0.030\t5\tparity\t-\tok\tno\ncbench_x\twordexp_1\t1.000\t-\t1.000\t0.030\t5\tparity\t-\tok\tno\n";
    const NEW: &str = "bench\trow\tinstr\tcycles\ttime\tspread\trounds\tverdict\tlimit\tstatus\tflagged\ncbench_x\ta\t1.000\t-\t1.100\t0.040\t5\tslower\t-\tFAIL\tyes\ncbench_x\tb\t1.000\t-\t1.000\t0.010\t5\tparity\t-\tok\tno\ncbench_x\tc\t1.000\t-\t1.020\t0.020\t5\tparity\t-\tok\tno\ncbench_x\td\t0.700\t-\t0.800\t0.030\t5\tfaster\t-\tok\tno\ncbench_x\twordexp_1\t1.000\t-\t1.500\t0.050\t5\tslower\t-\tok\tno\n";

    #[test]
    fn compare_prints_counts_lists_spread_and_change() {
        let mut old = BTreeMap::new();
        old.insert("ivb".to_string(), load_sweep("old", OLD).unwrap().rows);
        let new = vec![("ivb".to_string(), load_sweep("new", NEW).unwrap())];
        let r = report(&new, &old, "1106e", &[]);
        // now: d faster, b c parity, a slower (wordexp deferred); 1106e over the rows measured now: a, b, c.
        assert!(r.contains("| ivb | 1 | 2 | 1 | 4 | 1 | 1 | 1 | 3 |\n"), "{r}");
        // spread over all 5 rows (sorted 0.01 0.02 0.03 0.04 0.05); change a 1.222, b 0.833, c 1.020.
        assert!(r.contains("| ivb | 5 | 0.030 | 0.050 | 0.050 | 3 | 1.020 | 0.833 | 1.222 |\n"), "{r}");
        for want in [
            "## (a) Regressions: slower now, parity or faster in 1106e: 1\n\n- cbench_x/a | ivb | instr 1.000 (1106e 1.000) | time 1.100 (1106e 0.900) | spread 0.040\n\n",
            "## (b) Reopen: instr <= 1.05, time > 1.05: 1\n\n- cbench_x/a | ivb |",
            "## (c) Improved: slower in 1106e, parity or faster now: 1\n\n- cbench_x/b | ivb |",
            "## Rows not in 1106e: 1\n\n- cbench_x/d | ivb |",
            "## Rows in 1106e not measured now: 1\n\n- cbench_x/gone | ivb | 1106e time 1.000\n",
            "## Deferred rows (timed, not judged): 1\n\n- cbench_x/wordexp_1 | ivb |",
        ] {
            assert!(r.contains(want), "{want}\n---\n{r}");
        }
        assert_eq!(r, report(&new, &old, "1106e", &[]));
    }

    /// NEW with a `selected` column (every row `yes`) and a calibration row outside the selection.
    fn new_selected() -> String {
        let mut t: String = NEW.lines().map(|l| format!("{l}\t{}\n", if l.starts_with("bench\t") { "selected" } else { "yes" })).collect();
        t.push_str("cbench_libm\tadd_baseline\t2.000\t-\t3.000\t0.900\t5\tslower\t-\t-\tno\tno\n");
        t
    }

    #[test]
    fn compare_judges_only_the_selected_rows() {
        // SWEEP-COMPARE-SELECTED (no spec clause; developer tooling): the libm bench's calibration rows are in
        // sweep.tsv with `selected` no; counts, spread and the new-row list cover the selected rows only.
        let d = std::env::temp_dir().join(format!("xtask-sweep-cmp-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let (new_p, old_p, legacy_p) = (d.join("new.tsv"), d.join("old.tsv"), d.join("legacy.tsv"));
        fs::write(&new_p, new_selected()).unwrap();
        // The old table times the calibration row too: outside the selection now, so it is not "not measured now".
        fs::write(&old_p, format!("{OLD}cbench_libm\tadd_baseline\t2.000\t-\t3.000\t0.900\t5\tslower\t-\t-\tno\n")).unwrap();
        fs::write(&legacy_p, format!("{NEW}cbench_libm\tadd_baseline\t2.000\t-\t3.000\t0.900\t5\tslower\t-\t-\tno\n")).unwrap();
        let run = |new: &Path| compare(&["--new".into(), format!("ivb={}", new.display()), "--old".into(), format!("ivb={}", old_p.display()), "--label".into(), "1106e".into()]).unwrap();
        let r = run(&new_p);
        assert!(r.contains("| ivb | 1 | 2 | 1 | 4 | 1 | 1 | 1 | 3 |\n"), "{r}");
        assert!(r.contains("| ivb | 5 | 0.030 | 0.050 | 0.050 | 3 |"), "{r}");
        assert!(r.contains("## Rows outside the selection (in sweep.tsv, not judged): 1\n\n- cbench_libm/add_baseline | ivb\n"), "{r}");
        assert!(r.contains("## Rows not in 1106e: 1\n"), "{r}");
        assert!(r.contains("## Rows in 1106e not measured now: 1\n\n- cbench_x/gone |"), "{r}");
        assert!(!r.contains("libm median time ratio"), "{r}");
        // The old table has no `selected` column: judged as before, and the report says so.
        assert!(r.contains("The old `ivb` table has no `selected` column"), "{r}");
        assert!(!r.contains("The new `ivb` table"), "{r}");
        // A new table without the column judges the calibration row too (today's behaviour, said in the report).
        let l = run(&legacy_p);
        assert!(l.contains("| ivb | 1 | 2 | 2 | 5 |"), "{l}");
        assert!(l.contains("| ivb | 6 | 0.035 | 0.900 | 0.900 |"), "{l}");
        assert!(l.contains("The new `ivb` table has no `selected` column"), "{l}");
        assert!(l.contains("libm median time ratio per family and precision: ivb\n\n"), "{l}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn libm_rows_are_grouped_by_family_and_precision() {
        // SWEEP-COMPARE-SELECTED (no spec clause): the per-family table of the libm sweep (TIMING-SWEEP-LIBM-1107).
        for (row, want) in [
            ("expf", ("exp/log/pow", "f32")),
            ("exp10l", ("exp/log/pow", "long double")),
            ("erf", ("erf", "f64")),
            ("erfcf", ("erf", "f32")),
            ("coshf128", ("hyperbolic", "f128")),
            ("atan2", ("trig", "f64")),
            ("cexpl", ("complex", "long double")),
            ("expf_upward", ("special", "f32")),
            ("exp_overflow_errno", ("special", "f64")),
            ("add_baseline", ("other", "-")),
        ] {
            assert_eq!(libm_class(row), want, "{row}");
        }
        let rows: ClassRows = [("expf", 2.0), ("exp", 4.0), ("expl", 9.0), ("logl", 11.0), ("cexp", 300.0), ("sin_huge", 5.0), ("wordexp", 1.0)]
            .iter()
            .map(|(n, t)| (format!("cbench_libm/{n}"), Rec { time: Some(*t), ..Rec::default() }))
            .chain([("cbench_x/a".to_string(), Rec { time: Some(1.0), ..Rec::default() })])
            .collect();
        let want = "| family | rows | median | f32 | f64 | long double | f128 |\n|---|---:|---:|---:|---:|---:|---:|\n\
                    | exp/log/pow | 4 | 6.50 | 2.00 (1) | 4.00 (1) | 10.00 (2) | - |\n\
                    | complex | 1 | 300.00 | - | 300.00 (1) | - | - |\n\
                    | special | 1 | 5.00 | - | 5.00 (1) | - | - |\n\
                    | all | 6 | 7.00 | 2.00 (1) | 5.00 (3) | 10.00 (2) | - |\n";
        assert_eq!(libm_families(&rows).as_deref(), Some(want));
        assert_eq!(libm_families(&ClassRows::new()), None);
    }

    #[test]
    fn time_ratio_buckets_have_exact_edges() {
        // SWEEP-DISTRIBUTION (no spec clause; developer tooling): a ratio on an edge is in the lower bucket.
        for (t, want) in [(0.5, 0), (1.05, 0), (1.051, 1), (2.0, 1), (2.001, 2), (10.0, 2), (10.001, 3), (100.0, 3), (100.001, 4), (800.0, 4)] {
            assert_eq!(bucket(t), want, "{t}");
        }
        // The edges as `sweep.tsv` writes them (three decimals) land in the same buckets.
        for (t, want) in [("1.050", 0), ("2.000", 1), ("10.000", 2), ("100.000", 3)] {
            assert_eq!(bucket(num(Some(t)).unwrap()), want, "{t}");
        }
    }

    fn timed(rows: &[(&str, f64)]) -> ClassRows {
        rows.iter().map(|(k, t)| (k.to_string(), Rec { time: Some(*t), ..Rec::default() })).collect()
    }

    #[test]
    fn distribution_counts_buckets_names_the_slowest_and_marks_families_above_10x() {
        // SWEEP-DISTRIBUTION (no spec clause): the magnitude trigger of the RUT rule (family median above 10x).
        let rows = timed(&[
            ("cbench_x/a", 0.9),
            ("cbench_x/b", 1.05),
            ("cbench_x/c", 2.0),
            ("cbench_x/wordexp_1", 900.0),
            ("cbench_y/a", 10.0),
            ("cbench_y/b", 10.0),
            ("cbench_y/c", 10.0),
            ("cbench_z/a", 100.0),
            ("cbench_z/b", 10.5),
            ("cbench_libm/sinl", 120.0),
            ("cbench_libm/cosl", 50.0),
            ("cbench_libm/sinf", 1.0),
            ("cbench_libm/add_baseline", 3.0),
        ]);
        let d = distribution(&rows).unwrap();
        assert!(d.starts_with("| time ratio | rows |\n|---|---:|\n| <= 1.05 | 3 |\n| 1.05-2 | 1 |\n| 2-10 | 4 |\n| 10-100 | 3 |\n| > 100 | 1 |\n"), "{d}");
        assert!(d.contains("\nSlowest 5: cbench_libm/sinl 120.00, cbench_z/a 100.00, cbench_libm/cosl 50.00, cbench_z/b 10.50, cbench_y/a 10.00\n"), "{d}");
        // A median of exactly 10 is not above the trigger; 55.25 and 85 are.
        assert!(d.contains("RETHINK? (family median time ratio above 10x, the magnitude trigger): 2: cbench_libm trig long double 85.00 (2), cbench_z 55.25 (2)\n"), "{d}");
        assert!(d.contains("| cbench_libm other | 1 | 3.00 | |\n| cbench_libm trig f32 | 1 | 1.00 | |\n| cbench_libm trig long double | 2 | 85.00 | RETHINK? |\n| cbench_x | 3 | 1.05 | |\n| cbench_y | 3 | 10.00 | |\n| cbench_z | 2 | 55.25 | RETHINK? |\n"), "{d}");
        assert!(!d.contains("wordexp"), "{d}");
        let quiet = distribution(&timed(&[("cbench_x/a", 1.0)])).unwrap();
        assert!(quiet.contains("magnitude trigger): none\n"), "{quiet}");
        assert_eq!(distribution(&ClassRows::new()), None);
        // `sweep compare` prints it per class.
        let r = report(&[("ivb".to_string(), SweepTable { rows, ..SweepTable::default() })], &BTreeMap::new(), "old", &[]);
        assert!(r.contains("## Time ratio distribution and family medians: ivb\n\n| time ratio | rows |\n"), "{r}");
        assert!(r.contains("| cbench_z | 2 | 55.25 | RETHINK? |\n"), "{r}");
    }

    #[test]
    fn legacy_results_tables_are_read_per_class() {
        let md = "## Summary\n| cbench_x/zz | 9 | 9 |\n## Table\n\n| bench/row | instr IVB | time IVB | spread IVB | verdict IVB | 1106 time IVB | now/1106 IVB | instr N100 | time N100 | spread N100 | verdict N100 | 1106 time N100 | now/1106 N100 |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|\n| cbench_x/a | 0.950 | 0.900 | 0.010 | faster | 0.800 | 1.125 | - | 1.200 | 0.020 | slower | - | - |\n";
        let t = load_legacy("t.md", md).unwrap();
        assert_eq!(t.keys().collect::<Vec<_>>(), ["ivb", "n100"]);
        assert_eq!(t["ivb"]["cbench_x/a"], Rec { instr: Some(0.95), time: Some(0.9), spread: Some(0.01) });
        assert_eq!(t["n100"]["cbench_x/a"], Rec { instr: None, time: Some(1.2), spread: Some(0.02) });
        assert!(!t["ivb"].contains_key("cbench_x/zz"));
        assert!(load_legacy("t.md", "| cbench_x/a | 1 |\n").is_err());
    }
}
