//! `cargo xtask symcov [--tsv <file>]`: every name and version node of the glibc 2.39 ABI
//! (`spec/abi/versions.tsv`, Appendix C) that Liella's products do not define (README-COVERAGE-TRUTH,
//! ruling #811). Unlike `symgap` (glibc's `libc.a` only) and the stem list in `spec/symbols.tsv`
//! (which names a family once), this walks every row of the version table, in every library.
//!
//! A row `library symbol version binding` is present when the library's `.so` in `target/liella/`
//! defines `symbol@@version` (a default row; an unversioned definition also counts) or
//! `symbol@version` / `symbol@@version` (a compatibility row), by `nm -D --defined-only`. For a
//! default row of a library with a static archive, the name must also be defined by the `.a`
//! (`nm --defined-only`). Missing names are grouped by library, version class (`default`,
//! `compat`: every missing version is a compatibility version, `private`: `GLIBC_PRIVATE`) and
//! the phase or milestone the plan gives them ([`owner`]). The command reports and never fails:
//! the list is a work list and the source of `docs/spec-symbol-gaps.md` / `.tsv`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

/// The §12.13 libio internals phase 15 owns (F32, D-12.15): the vtable objects and the file,
/// string, default, marker, backup-area and mode-switch operations behind them, 72 names, all
/// default versions at GLIBC_2.2.5 (`spec/abi/versions.tsv`). `_IO_proc_open` and
/// `_IO_proc_close` are not among them: D-12.15 gives them to `popen` (§18), so `symgap` counts
/// them as gaps (validation T25). Since IO-VTABLES (LG-8) the products export all 72 and both
/// `_IO_proc_*` names: `_IO_sungetc` and the two jump tables work (R15-3), the other 69 and
/// the `_IO_proc_*` pair are fixed-failure stubs (ruling #1064). This list now classifies one
/// only if a product stops exporting it (a regression shows as a phase-15 `e` row, not as done).
pub(crate) const F32_LIBIO: [&str; 72] = [
    "_IO_adjust_column", "_IO_adjust_wcolumn", "_IO_default_doallocate", "_IO_default_finish",
    "_IO_default_pbackfail", "_IO_default_uflow", "_IO_default_xsgetn", "_IO_default_xsputn",
    "_IO_do_write", "_IO_doallocbuf", "_IO_file_attach", "_IO_file_close", "_IO_file_close_it",
    "_IO_file_doallocate", "_IO_file_finish", "_IO_file_fopen", "_IO_file_init", "_IO_file_jumps",
    "_IO_file_open", "_IO_file_overflow", "_IO_file_read", "_IO_file_seek", "_IO_file_seekoff",
    "_IO_file_setbuf", "_IO_file_stat", "_IO_file_sync", "_IO_file_underflow", "_IO_file_write",
    "_IO_file_xsputn", "_IO_free_backup_area", "_IO_free_wbackup_area", "_IO_init",
    "_IO_init_marker", "_IO_init_wmarker", "_IO_least_wmarker", "_IO_link_in", "_IO_marker_delta",
    "_IO_marker_difference", "_IO_remove_marker", "_IO_seekmark", "_IO_seekwmark", "_IO_setb",
    "_IO_str_init_readonly", "_IO_str_init_static", "_IO_str_overflow", "_IO_str_pbackfail",
    "_IO_str_seekoff", "_IO_str_underflow", "_IO_sungetc", "_IO_switch_to_get_mode",
    "_IO_switch_to_main_wget_area", "_IO_switch_to_wbackup_area", "_IO_switch_to_wget_mode",
    "_IO_un_link", "_IO_unsave_markers", "_IO_unsave_wmarkers", "_IO_wdefault_doallocate",
    "_IO_wdefault_finish", "_IO_wdefault_pbackfail", "_IO_wdefault_uflow", "_IO_wdefault_xsgetn",
    "_IO_wdefault_xsputn", "_IO_wdo_write", "_IO_wdoallocbuf", "_IO_wfile_jumps",
    "_IO_wfile_overflow", "_IO_wfile_seekoff", "_IO_wfile_sync", "_IO_wfile_underflow",
    "_IO_wfile_xsputn", "_IO_wmarker_delta", "_IO_wsetb",
];

/// The 23 complex families of §10.5 (milestone 4a delivered their `float` and `double` members).
const COMPLEX: [&str; 23] = [
    "cabs", "carg", "creal", "cimag", "conj", "cproj", "cexp", "clog", "clog10", "cpow", "csqrt", "csin", "ccos", "ctan",
    "csinh", "ccosh", "ctanh", "casin", "cacos", "catan", "casinh", "cacosh", "catanh",
];

/// The §12.13 libio internals phase 15 owns (F32, D-12.15), with the two `_IO_proc_*` names
/// (default versions, legacy tier).
fn libio_internal(name: &str) -> bool {
    name == "_IO_proc_open" || name == "_IO_proc_close" || F32_LIBIO.contains(&name)
}

/// The version class of a missing name from its missing rows `(version, binding)`.
pub fn version_class(rows: &[(String, String)]) -> &'static str {
    if rows.iter().all(|(v, _)| v == "GLIBC_PRIVATE") {
        "private"
    } else if rows.iter().any(|(v, b)| b == "default" && v != "GLIBC_PRIVATE") {
        "default"
    } else {
        "compat"
    }
}

/// Whether a `libm` name is a member Track M milestone 5 still owes: the `_Float128` elementary
/// and special functions and the complex `long double` (`l`, `f64x`) and `_Float128` members
/// (plan/06-phases/04-track-m-parallel-from-p2.md, "Left for milestone 5").
pub fn milestone5(name: &str) -> bool {
    let stem = name.trim_start_matches('_');
    if stem.contains("f128") {
        return true;
    }
    ["l", "f64x"].iter().any(|s| stem.strip_suffix(s).is_some_and(|f| COMPLEX.contains(&f)))
}

/// The class letter (as in the gap doc) and the owner in the plan of a missing name.
/// `shipped`: Liella builds this library; `section`: the name's §, when `spec/symbols.tsv` lists it.
pub fn owner(library: &str, name: &str, class: &str, shipped: bool, section: Option<u32>) -> (&'static str, &'static str) {
    if library.starts_with("ld-linux") || library == "libthread_db.so.1" || library == "libc_malloc_debug.so.0" || name.ends_with("_version_placeholder") || section == Some(27) {
        ("a", "phase 14")
    } else if !shipped {
        ("c", if library == "libnsl.so.1" { "phase 15" } else { "not in plan" })
    } else if class == "private" {
        // Ruling #822: GLIBC_PRIVATE names are glibc-internal (programs cannot link them); Liella
        // replaces those components together with its own internal interfaces. Phase 14 revisits
        // them only if Liella's ld.so must interoperate with glibc libraries.
        ("p", "out of scope")
    } else if class == "compat" {
        ("b", "phase 15")
    } else if matches!(section, Some(28 | 29)) || libio_internal(name) {
        ("e", "phase 15")
    } else if library == "libm.so.6" && milestone5(name) {
        ("m", "milestone 5 (libm)")
    } else if section.is_some() {
        ("d", "phases 1-13")
    } else {
        // A default name of a library Liella ships that `spec/symbols.tsv` does not list.
        ("n", "not in spec")
    }
}

/// `(name, version, default)` of each line of `nm -D --defined-only` (`name@@V`, `name@V` or `name`).
pub fn parse_dynamic(text: &str) -> BTreeSet<(String, String, bool)> {
    text.lines()
        .filter_map(|l| {
            let sym = l.split_whitespace().last()?;
            Some(match sym.split_once("@@") {
                Some((n, v)) => (n.to_string(), v.to_string(), true),
                None => match sym.split_once('@') {
                    Some((n, v)) => (n.to_string(), v.to_string(), false),
                    None => (sym.to_string(), String::new(), true),
                },
            })
        })
        .collect()
}

fn nm(args: &[&str], path: &Path) -> Result<String, String> {
    let out = Command::new("nm").args(args).arg(path).output().map_err(|e| format!("nm: {e}"))?;
    if !out.status.success() {
        return Err(format!("nm {} failed", path.display()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Defined global and weak names of an archive (`nm --defined-only -P`).
fn archive_names(path: &Path) -> Result<BTreeSet<String>, String> {
    Ok(nm(&["--defined-only", "-P"], path)?.lines().filter_map(|l| {
        let mut f = l.split_whitespace();
        let (name, ty) = (f.next()?, f.next()?);
        (ty.chars().next().is_some_and(|c| c.is_ascii_uppercase()) || ty == "i").then(|| name.to_string())
    }).collect())
}

/// A missing name: library, name, its missing rows, and where it is missing (`so`, `a`, `so+a`).
pub struct Missing {
    pub library: String,
    pub name: String,
    pub rows: Vec<(String, String)>,
    pub missing_in: &'static str,
}

/// Whether one version-table row is defined by the products.
pub fn row_present(dynsyms: &BTreeSet<(String, String, bool)>, name: &str, version: &str, binding: &str) -> bool {
    dynsyms.contains(&(name.to_string(), version.to_string(), true))
        || (binding == "default" && dynsyms.contains(&(name.to_string(), String::new(), true)))
        || (binding != "default" && dynsyms.contains(&(name.to_string(), version.to_string(), false)))
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let products = root.join("target/liella");
    if !products.join("libc.so.6").exists() {
        return Err("target/liella/libc.so.6 is missing: run `cargo xtask so`".into());
    }
    let table = fs::read_to_string(root.join("spec/symbols.tsv")).map_err(|e| format!("symbols.tsv: {e}"))?;
    let sections: BTreeMap<&str, u32> = table.lines().skip(1).filter_map(|l| {
        let mut f = l.split('\t');
        Some((f.next()?, f.next()?.parse().ok()?))
    }).collect();
    let versions = fs::read_to_string(root.join("spec/abi/versions.tsv")).map_err(|e| format!("versions.tsv: {e}"))?;
    // library -> name -> rows (version, binding)
    type Rows<'a> = BTreeMap<&'a str, Vec<(&'a str, &'a str)>>;
    let mut abi: BTreeMap<&str, Rows> = BTreeMap::new();
    for l in versions.lines().skip(1) {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() >= 4 {
            abi.entry(f[0]).or_default().entry(f[1]).or_default().push((f[2], f[3]));
        }
    }
    // Every name the archives define: a static program linked with `-lresolv` (an empty archive)
    // takes the resolver from `libc.a`, so an archive-only library counts as covered by any archive.
    let mut all_static = BTreeSet::new();
    for e in fs::read_dir(&products).map_err(|e| format!("target/liella: {e}"))?.flatten() {
        if e.path().extension().is_some_and(|x| x == "a") {
            all_static.extend(archive_names(&e.path())?);
        }
    }
    let mut missing = Vec::new();
    let mut total_rows = 0;
    for (lib, names) in &abi {
        let so = products.join(lib);
        let ar = products.join(format!("{}.a", lib.split(".so").next().unwrap_or(lib)));
        let (has_so, has_ar) = (so.exists(), ar.exists());
        let dynsyms = if has_so { parse_dynamic(&nm(&["-D", "--defined-only"], &so)?) } else { BTreeSet::new() };
        let statics: Option<BTreeSet<String>> = if has_ar {
            let mut names = archive_names(&ar)?;
            // libm.a leaves the names both libraries export to libc.a (LIBC-A-PLACEMENT-2: glibc's libm.a
            // does not define them), so a static link with `-lm` finds them in libc.a.
            if *lib == "libm.so.6" {
                names.extend(archive_names(&products.join("libc.a"))?);
            }
            Some(names)
        } else {
            None
        };
        for (name, rows) in names {
            total_rows += rows.len();
            let gone: Vec<(String, String)> = rows.iter().filter(|(v, b)| !if has_so {
                row_present(&dynsyms, name, v, b)
            } else {
                // A library shipped as an archive only: its default names must be in the archive.
                *b == "default" && *v != "GLIBC_PRIVATE" && all_static.contains(*name)
            })
                .map(|(v, b)| (v.to_string(), b.to_string())).collect();
            let needs_static = rows.iter().any(|(v, b)| *b == "default" && *v != "GLIBC_PRIVATE");
            let static_gone = needs_static && has_so && statics.as_ref().is_some_and(|s| !s.contains(*name));
            let missing_in = match (gone.is_empty(), static_gone) {
                (true, false) => continue,
                (true, true) => "a",
                (false, true) if has_so => "so+a",
                (false, true) => "a",
                (false, false) => "so",
            };
            let rows = if gone.is_empty() { rows.iter().map(|(v, b)| (v.to_string(), b.to_string())).collect() } else { gone };
            missing.push(Missing { library: lib.to_string(), name: name.to_string(), rows, missing_in });
        }
    }
    // group: (class letter, owner, library, version class) -> names
    let mut groups: BTreeMap<(&str, &str, &str, &str), Vec<&Missing>> = BTreeMap::new();
    let mut tsv = String::from("class\tsymbol\tlibrary\tversion_class\tmissing_versions\tmissing_in\towner\tspec_section\n");
    for m in &missing {
        let class = version_class(&m.rows);
        let stem = m.library.split(".so").next().unwrap_or(&m.library);
        let shipped = products.join(&m.library).exists() || products.join(format!("{stem}.a")).exists();
        let sec = sections.get(m.name.as_str()).copied();
        let (letter, own) = owner(&m.library, &m.name, class, shipped, sec);
        groups.entry((letter, own, m.library.as_str(), class)).or_default().push(m);
        let vers: Vec<&str> = m.rows.iter().map(|(v, _)| v.as_str()).collect();
        tsv.push_str(&format!("{letter}\t{}\t{}\t{class}\t{}\t{}\t{own}\t{}\n", m.name, m.library, vers.join(","), m.missing_in,
            sec.map_or(String::new(), |s| s.to_string())));
    }
    println!("symcov: {} of {} names ({total_rows} version rows) in spec/abi/versions.tsv are not defined by target/liella", missing.len(),
        abi.values().map(BTreeMap::len).sum::<usize>());
    let mut by_owner: BTreeMap<&str, usize> = BTreeMap::new();
    for ((_, own, _, _), v) in &groups {
        *by_owner.entry(own).or_default() += v.len();
    }
    let parts: Vec<String> = by_owner.iter().map(|(o, n)| format!("{o} {n}")).collect();
    println!("symcov: by owner: {}", parts.join(", "));
    for ((letter, own, lib, class), names) in &groups {
        let shown: Vec<String> = names.iter().map(|m| if m.missing_in == "so" { m.name.clone() } else { format!("{}[{}]", m.name, m.missing_in) }).collect();
        println!("  ({letter}) {own:<18} {lib:<20} {class:<7} {:>4}: {}", names.len(), shown.join(" "));
    }
    if let Some(path) = args.iter().position(|a| a == "--tsv").and_then(|i| args.get(i + 1)) {
        fs::write(path, tsv).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(r: &[(&str, &str)]) -> Vec<(String, String)> {
        r.iter().map(|(v, b)| (v.to_string(), b.to_string())).collect()
    }

    #[test]
    fn version_classes() {
        assert_eq!(version_class(&rows(&[("GLIBC_2.2.5", "compat"), ("GLIBC_2.34", "default")])), "default");
        assert_eq!(version_class(&rows(&[("GLIBC_2.2.5", "compat"), ("GLIBC_2.3", "compat")])), "compat");
        assert_eq!(version_class(&rows(&[("GLIBC_PRIVATE", "default")])), "private");
    }

    #[test]
    fn milestone5_members() {
        for n in ["sinf128", "lgammaf128_r", "cexpl", "cabsf64x", "csqrtf128", "__clog10l", "clog10f64x"] {
            assert!(milestone5(n), "{n}");
        }
        for n in ["sinl", "sinf64x", "cexp", "cexpf", "cexpf32x", "ceill", "coshl", "clog10", "conjf64"] {
            assert!(!milestone5(n), "{n}");
        }
    }

    #[test]
    fn owners() {
        assert_eq!(owner("ld-linux-x86-64.so.2", "_dl_find_object", "default", true, None), ("a", "phase 14"));
        assert_eq!(owner("libthread_db.so.1", "td_init", "default", false, None), ("a", "phase 14"));
        assert_eq!(owner("libpthread.so.0", "__libpthread_version_placeholder", "compat", true, None), ("a", "phase 14"));
        assert_eq!(owner("libc.so.6", "dlopen", "default", true, Some(27)), ("a", "phase 14"));
        assert_eq!(owner("libnsl.so.1", "yp_match", "compat", false, None), ("c", "phase 15"));
        assert_eq!(owner("libc.so.6", "__libc_foo", "private", true, None), ("p", "out of scope"));
        assert_eq!(owner("libc_malloc_debug.so.0", "mcheck", "compat", false, None), ("a", "phase 14"));
        assert_eq!(owner("libBrokenLocale.so.1", "__ctype_get_mb_cur_max", "default", false, None), ("c", "not in plan"));
        assert_eq!(owner("libm.so.6", "matherr", "compat", true, Some(10)), ("b", "phase 15"));
        assert_eq!(owner("libc.so.6", "profil", "default", true, Some(28)), ("e", "phase 15"));
        assert_eq!(owner("libc.so.6", "_IO_proc_open", "default", true, Some(12)), ("e", "phase 15"));
        assert_eq!(owner("libc.so.6", "_IO_file_jumps", "default", true, None), ("e", "phase 15"));
        assert_eq!(owner("libm.so.6", "expf128", "default", true, None), ("m", "milestone 5 (libm)"));
        assert_eq!(owner("libc.so.6", "strfromf128", "default", true, Some(4)), ("d", "phases 1-13"));
        assert_eq!(owner("libm.so.6", "sinl", "default", true, Some(10)), ("d", "phases 1-13"));
        assert_eq!(owner("libresolv.so.2", "__b64_ntop", "default", true, None), ("n", "not in spec"));
    }

    #[test]
    fn dynamic_rows() {
        let d = parse_dynamic("0000 T fgetc@@GLIBC_2.2.5\n0000 T stime@GLIBC_2.2.5\n0000 W plain\n");
        assert!(row_present(&d, "fgetc", "GLIBC_2.2.5", "default"));
        assert!(!row_present(&d, "fgetc", "GLIBC_2.3", "default"));
        assert!(row_present(&d, "stime", "GLIBC_2.2.5", "compat"));
        assert!(!row_present(&d, "stime", "GLIBC_2.2.5", "default"));
        assert!(row_present(&d, "plain", "GLIBC_2.2.5", "default"));
        assert!(!row_present(&d, "plain", "GLIBC_2.2.5", "compat"));
    }
}
