//! `spec/abi/compat-plan.tsv`, the phase-15 dispatch table (LG-0a, LEGACY-COMPAT-TABLE).
//!
//! One row for every `compat` row of `spec/abi/versions.tsv` (library, symbol, version), sorted
//! by those three columns, with how the old version is made and who makes it:
//!
//! * `kind`: `alias` (the same contract as a current function: the default version of the same
//!   name, or the function named by `target=<name>` in the note), `behaviour` (an own entry: the
//!   old contract differs, or a compat-only name with its own logic), `stub` (`ENOSYS` or a fixed
//!   result), `object` (data with the manifest's size; exactly the `OBJECT` rows), `subsystem`
//!   (part of RPC/XDR or NIS);
//! * `story`: the owning story (`LG-<n>` of plan/06-phases/18-phase-15-legacy-tier-and.md, or a
//!   phase-14 `DL-<n>`);
//! * `drow`: the governing decision row, `-` when the owning story still has to write one;
//! * `note`: free text; an `alias` whose name has no default version in its library must start
//!   with `target=<name>`, and `<name>` must have a default version in `versions.tsv`.
//!
//! `check` (part of `cargo xtask static`, alone as `cargo xtask compat-plan`) fails when the two
//! tables do not match row for row, or a row breaks the rules above.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

const HEADER: &str = "library\tsymbol\tversion\tkind\tstory\tdrow\tnote";
const KINDS: [&str; 5] = ["alias", "behaviour", "stub", "object", "subsystem"];

pub fn check(root: &Path) -> Result<(), String> {
    let read = |p: &str| std::fs::read_to_string(root.join(p)).map_err(|e| format!("{p}: {e}"));
    let errors = check_tables(&read("spec/abi/versions.tsv")?, &read("spec/abi/compat-plan.tsv")?);
    if errors.is_empty() {
        return Ok(());
    }
    for e in errors.iter().take(40) {
        eprintln!("compat-plan: {e}");
    }
    Err(format!("spec/abi/compat-plan.tsv: {} problem(s)", errors.len()))
}

/// Every mismatch between `versions` (versions.tsv text) and `plan` (compat-plan.tsv text).
fn check_tables(versions: &str, plan: &str) -> Vec<String> {
    let mut errors = Vec::new();
    // (library, symbol, version) -> type of every compat row; (library, symbol) and symbol of every default row.
    let mut compat: Vec<(String, String, String, String)> = Vec::new();
    let mut default_in: HashSet<(String, String)> = HashSet::new();
    let mut default_any: HashSet<String> = HashSet::new();
    for line in versions.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if let [lib, sym, ver, binding, typ, ..] = f.as_slice() {
            match *binding {
                "compat" => compat.push(((*lib).into(), (*sym).into(), (*ver).into(), (*typ).into())),
                "default" => {
                    default_in.insert(((*lib).into(), (*sym).into()));
                    default_any.insert((*sym).into());
                }
                _ => {}
            }
        }
    }
    compat.sort();
    let mut lines = plan.lines();
    if lines.next() != Some(HEADER) {
        errors.push(format!("line 1: the header must be `{}`", HEADER.replace('\t', "<TAB>")));
    }
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split('\t').collect()).collect();
    let planned: BTreeSet<(&str, &str, &str)> = rows.iter().filter(|f| f.len() >= 3).map(|f| (f[0], f[1], f[2])).collect();
    let wanted: BTreeSet<(&str, &str, &str)> = compat.iter().map(|c| (c.0.as_str(), c.1.as_str(), c.2.as_str())).collect();
    for (lib, sym, ver) in wanted.difference(&planned) {
        errors.push(format!("missing row for {lib} {sym}@{ver} (a compat row of versions.tsv)"));
    }
    for (lib, sym, ver) in planned.difference(&wanted) {
        errors.push(format!("{lib} {sym}@{ver} is not a compat row of versions.tsv"));
    }
    if rows.len() != compat.len() {
        errors.push(format!("{} plan rows for {} compat rows of versions.tsv", rows.len(), compat.len()));
    }
    for (i, f) in rows.iter().enumerate() {
        let at = format!("line {}", i + 2);
        let [lib, sym, ver, kind, story, drow, note] = f.as_slice() else {
            errors.push(format!("{at}: {} columns, want 7", f.len()));
            continue;
        };
        if let Some(c) = compat.get(i) {
            if (c.0.as_str(), c.1.as_str(), c.2.as_str()) != (*lib, *sym, *ver) {
                errors.push(format!("{at}: {lib} {sym}@{ver} where versions.tsv's sorted compat rows have {} {}@{}", c.0, c.1, c.2));
            } else if (*kind == "object") != (c.3 == "OBJECT") {
                errors.push(format!("{at}: {sym}@{ver} is {} in versions.tsv but kind `{kind}`", c.3));
            }
        }
        if !KINDS.contains(kind) {
            errors.push(format!("{at}: kind `{kind}` is not one of {}", KINDS.join("/")));
        }
        if !is_story(story) {
            errors.push(format!("{at}: story `{story}` is not LG-<n> or DL-<n>"));
        }
        if *drow != "-" && !is_drow(drow) {
            errors.push(format!("{at}: drow `{drow}` is not `-` or D-<n>.<m>"));
        }
        let target = note.strip_prefix("target=").map(|t| t.split([';', ' ']).next().unwrap_or(""));
        if *kind == "alias" && !default_in.contains(&((*lib).to_string(), (*sym).to_string())) && target.is_none() {
            errors.push(format!("{at}: alias {sym}@{ver} has no default version in {lib}: name it with target=<name>"));
        }
        if let Some(t) = target {
            if !default_any.contains(t) {
                errors.push(format!("{at}: target `{t}` has no default version in versions.tsv"));
            }
        }
    }
    errors
}

fn is_story(s: &str) -> bool {
    let n = s.strip_prefix("LG-").or_else(|| s.strip_prefix("DL-")).unwrap_or("");
    let n = n.strip_suffix(|c: char| c.is_ascii_lowercase()).unwrap_or(n);
    !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
}

fn is_drow(s: &str) -> bool {
    s.strip_prefix("D-")
        .and_then(|r| r.split_once('.'))
        .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && a.bytes().all(|c| c.is_ascii_digit()) && b.bytes().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERSIONS: &str = "library\tsymbol\tversion\tbinding\ttype\tscope\tsize\n\
        libc.so.6\tfoo\tGLIBC_2.34\tdefault\tFUNC\tGLOBAL\t\n\
        libc.so.6\tfoo\tGLIBC_2.2.5\tcompat\tFUNC\tGLOBAL\t\n\
        libc.so.6\told\tGLIBC_2.2.5\tcompat\tFUNC\tGLOBAL\t\n\
        libc.so.6\tvar\tGLIBC_2.2.5\tcompat\tOBJECT\tGLOBAL\t8\n";

    fn plan(rows: &[&str]) -> String {
        let mut s = format!("{HEADER}\n");
        for r in rows {
            s.push_str(&r.replace('|', "\t"));
            s.push('\n');
        }
        s
    }

    const GOOD: [&str; 3] = ["libc.so.6|foo|GLIBC_2.2.5|alias|LG-1|-|", "libc.so.6|old|GLIBC_2.2.5|alias|LG-1|-|target=foo", "libc.so.6|var|GLIBC_2.2.5|object|LG-6|D-3.5|"];

    #[test]
    fn matching_tables_pass() {
        assert_eq!(check_tables(VERSIONS, &plan(&GOOD)), Vec::<String>::new());
    }

    #[test]
    fn a_deleted_row_fails() {
        let e = check_tables(VERSIONS, &plan(&[GOOD[0], GOOD[2]]));
        assert!(e.iter().any(|m| m.contains("missing row for libc.so.6 old@GLIBC_2.2.5")), "{e:?}");
    }

    #[test]
    fn an_extra_or_misordered_row_fails() {
        let e = check_tables(VERSIONS, &plan(&[GOOD[0], GOOD[1], GOOD[2], "libc.so.6|bar|GLIBC_2.2.5|stub|LG-5|-|"]));
        assert!(e.iter().any(|m| m.contains("bar@GLIBC_2.2.5 is not a compat row")), "{e:?}");
        let e = check_tables(VERSIONS, &plan(&[GOOD[1], GOOD[0], GOOD[2]]));
        assert!(e.iter().any(|m| m.contains("sorted compat rows")), "{e:?}");
    }

    #[test]
    fn bad_fields_fail() {
        let e = check_tables(VERSIONS, &plan(&[GOOD[0], "libc.so.6|old|GLIBC_2.2.5|alias|LG-1|-|", "libc.so.6|var|GLIBC_2.2.5|stub|X|D3|"]));
        assert!(e.iter().any(|m| m.contains("target=<name>")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("is OBJECT in versions.tsv")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("story `X`")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("drow `D3`")), "{e:?}");
        let e = check_tables(VERSIONS, &plan(&[GOOD[0], "libc.so.6|old|GLIBC_2.2.5|alias|LG-1|-|target=nope", GOOD[2]]));
        assert!(e.iter().any(|m| m.contains("target `nope`")), "{e:?}");
    }

    #[test]
    fn the_repository_tables_match() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace root");
        check(root).expect("spec/abi/compat-plan.tsv matches spec/abi/versions.tsv");
    }
}
