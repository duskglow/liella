//! Repetition after an anchor (`^* ^+ ^? ^{n} $* $+ $? ${n}`) in ERE and BRE, with and without
//! REG_NEWLINE, at the start, in the middle, after `(` and after `|` (REGEX-ANCHOR-REPEAT, #1783,
//! D-14 syntax row). The table `data/regex_anchor_repeat.ref` is the reference's own answer
//! (regcomp code, then the span regexec gives on each subject below), probed black-box on the
//! conformance host; each cell is checked as the reference answers it, cell for cell.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, missing_docs)]

use liella::locale::Locale;
use liella::pattern::regex::{self, Options, Window};

const REF: &str = include_str!("data/regex_anchor_repeat.ref");
const SUBJECTS: [&str; 15] = ["x", "a", "*", "^", "$", "^x", "x$", "ax", "xa", "", "aa", "a^", "a$", "x^", "$x"];

fn cell(line: &str) -> Option<String> {
    let f: Vec<&str> = line.split(' ').collect();
    let (ere, nl, pat) = (f[0] == "E", f[1] == "1", f[2]);
    let want_rc: i32 = f[3].strip_prefix("rc=").unwrap().parse().unwrap();
    let cflags = i32::from(ere) | if nl { 4 } else { 0 };
    let prog = match regex::compile(pat.as_bytes(), regex::regcomp_syntax(cflags), &Locale::C) {
        Ok(p) => p,
        Err(e) if regex::regcomp_code(e) == want_rc => return None,
        Err(e) => return Some(format!("{line}: Liella rc={}", regex::regcomp_code(e))),
    };
    if want_rc != 0 {
        return Some(format!("{line}: Liella compiles"));
    }
    let opts = Options { newline_anchor: nl, ..Options::default() };
    for (i, s) in SUBJECTS.iter().enumerate() {
        let w = Window { first: 0, last: s.len(), backward: false };
        let got = match regex::exec(&prog, &Locale::C, s.as_bytes(), w, &opts, true).unwrap() {
            Some(m) => format!("{}-{}", m.start, m.end),
            None => "-".to_string(),
        };
        if f.iter().skip(4).nth(i).copied() != Some(got.as_str()) {
            return Some(format!("{line}: subject {s:?} Liella {got}"));
        }
    }
    None
}

#[test]
// Trace: no spec clause (regcomp syntax edge cases follow the reference); D-14 syntax row.
fn anchor_repeat_matrix_matches_reference() {
    let bad: Vec<String> = REF.lines().filter_map(cell).collect();
    assert!(bad.is_empty(), "{} of {} cells differ:\n{}", bad.len(), REF.lines().count(), bad.join("\n"));
}
