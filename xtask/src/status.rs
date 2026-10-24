//! Pending versus specified tests, per spec section and suite.
//!
//! A *pending* test is a `clause!`/`symbol!`/`family!`/`primitive!` stub. A *specified* test
//! is a `#[test]` function written from the spec. A `commentary!` item (an item the spec marks
//! `⚑ COMMENTARY`) is neither; it is counted on its own. Phase exit criteria (implementation-plan.md
//! §6) are stated in these numbers. The table is followed by the schedule report (who owns the
//! pending tests, `spec/schedule.tsv`); `--schedule` lists every pending test under its owner
//! and fails when any is flagged (see `schedule`).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Default)]
struct Row {
    pending: usize,
    specified: usize,
    commentary: usize,
}

pub fn run(root: &Path, schedule: bool) -> Result<(), String> {
    let mut rows: BTreeMap<(String, String), Row> = BTreeMap::new();
    let crates = fs::read_dir(root.join("crates")).map_err(|e| e.to_string())?;
    for c in crates.flatten() {
        let tests = c.path().join("tests");
        let Ok(rd) = fs::read_dir(&tests) else { continue };
        for f in rd.flatten() {
            let p = f.path();
            if p.extension().is_none_or(|x| x != "rs") {
                continue;
            }
            let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let section = name.split('_').next().unwrap_or("").to_string();
            let suite = if name.ends_with("_symbols") { "translation" } else if name.ends_with("_clauses") { "clauses" } else { "other" };
            let text = fs::read_to_string(&p).map_err(|e| e.to_string())?;
            let row = rows.entry((section, suite.to_string())).or_default();
            for l in text.lines().map(str::trim_start) {
                if l.starts_with("clause!(") || l.starts_with("symbol!(") || l.starts_with("family!(") || l.starts_with("primitive!(") || l.starts_with("layout!(") {
                    row.pending += 1;
                } else if l == "#[test]" {
                    row.specified += 1;
                } else if l.starts_with("commentary!(") {
                    row.commentary += 1;
                }
            }
        }
    }
    let (mut tp, mut ts, mut tc) = (0, 0, 0);
    println!("{:<22} {:<12} {:>8} {:>10} {:>11}", "section", "suite", "pending", "specified", "commentary");
    for ((sec, suite), r) in &rows {
        println!("{sec:<22} {suite:<12} {:>8} {:>10} {:>11}", r.pending, r.specified, r.commentary);
        tp += r.pending;
        ts += r.specified;
        tc += r.commentary;
    }
    println!("{:<22} {:<12} {tp:>8} {ts:>10} {tc:>11}", "TOTAL", "");
    let report = crate::schedule::report(root, schedule)?;
    report.print();
    if schedule && (report.flagged > 0 || report.problems > 0) {
        return Err(format!("{} pending tests flagged, {} schedule table problems", report.flagged, report.problems));
    }
    Ok(())
}
