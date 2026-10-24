//! `compat-probes` (part of `cargo xtask static`, LG-0b): the compat-version probe list
//! `tools/probe/compat/LIST.tsv` stays true. Every row is a `compat` row of `spec/abi/versions.tsv`, its probe source
//! binds `symbol@version` with `.symver`, and `spec/behaviour/compat.glibc.txt` has a line starting with
//! `symbol@version `/`:`. When `spec/abi/compat-plan.tsv` exists (LG-0a), each `behaviour` row of a STARTED story must be
//! listed: a story is started when it is LG-2 (the first behaviour story, ruling #1306) or when LIST.tsv already probes
//! at least one of its plan rows (provisional call on #1330). Rows of unstarted stories are not required: the plan has
//! behaviour rows for many LG stories, each of which adds its own probes, so requiring them all would make every branch
//! and every combination of LG branches red until the last story lands.

use std::collections::HashSet;
use std::path::Path;

fn read(root: &Path, rel: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))
}

/// The problems found, given the four files' contents (`plan` is None when compat-plan.tsv does not exist yet).
pub fn problems(list: &str, versions: &str, record: &str, plan: Option<&str>, probe_src: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut out = Vec::new();
    let compat: HashSet<(String, String, String)> = versions.lines().skip(1).filter_map(|l| {
        let f: Vec<&str> = l.split('\t').collect();
        (f.len() > 3 && f[3] == "compat").then(|| (f[0].to_string(), f[1].to_string(), f[2].to_string()))
    }).collect();
    let mut listed = HashSet::new();
    for (n, l) in list.lines().enumerate().skip(1) {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() != 4 { out.push(format!("LIST.tsv line {}: want 4 fields", n + 1)); continue; }
        let key = (f[0].to_string(), f[1].to_string(), f[2].to_string());
        let sv = format!("{}@{}", f[1], f[2]);
        if !compat.contains(&key) { out.push(format!("LIST.tsv line {}: {} {sv} is not a compat row of versions.tsv", n + 1, f[0])); }
        match probe_src(f[3]) {
            None => out.push(format!("LIST.tsv line {}: probe tools/probe/{} missing", n + 1, f[3])),
            Some(src) => {
                let direct = src.contains(&format!(", {sv}\")"));
                let by_macro = src.contains(&format!("{}, {})", f[1], f[2])) || src.contains(&format!("{}, {},", f[1], f[2]));
                if !direct && !by_macro { out.push(format!("LIST.tsv line {}: {} does not bind {sv}", n + 1, f[3])); }
            }
        }
        if !record.lines().any(|r| r.starts_with(&format!("{sv} ")) || r.starts_with(&format!("{sv}:"))) {
            out.push(format!("compat.glibc.txt has no line for {sv}"));
        }
        listed.insert(key);
    }
    if let Some(plan) = plan {
        let mut lines = plan.lines();
        let head: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
        let col = |name: &str| head.iter().position(|h| h.eq_ignore_ascii_case(name));
        let (Some(lib), Some(name), Some(ver), Some(kind)) = (col("library"), col("name").or(col("symbol")), col("version"), col("kind")) else {
            out.push("compat-plan.tsv: header lacks library/name/version/kind".into());
            return out;
        };
        let story = col("story");
        let rows: Vec<Vec<&str>> = lines.map(|l| l.split('\t').collect()).collect();
        let get = |f: &[&str], i: usize| f.get(i).copied().unwrap_or("").to_string();
        let key = |f: &[&str]| (get(f, lib), get(f, name), get(f, ver));
        // Without a story column every behaviour row counts (the LG-0b rule before #1330).
        let mut started: HashSet<String> = HashSet::from(["LG-2".to_string()]);
        if let Some(st) = story {
            started.extend(rows.iter().filter(|f| listed.contains(&key(f))).map(|f| get(f, st)));
        }
        for f in &rows {
            let in_scope = story.is_none_or(|st| started.contains(&get(f, st)));
            if get(f, kind) == "behaviour" && in_scope && !listed.contains(&key(f)) {
                let get = |i: usize| get(f, i);
                out.push(format!("compat-plan.tsv behaviour row {} {}@{} has no probe in LIST.tsv", get(lib), get(name), get(ver)));
            }
        }
    }
    out
}

pub fn check(root: &Path) -> Result<(), String> {
    let list = read(root, "tools/probe/compat/LIST.tsv")?;
    let versions = read(root, "spec/abi/versions.tsv")?;
    let record = read(root, "spec/behaviour/compat.glibc.txt")?;
    let plan = std::fs::read_to_string(root.join("spec/abi/compat-plan.tsv")).ok();
    let src = |p: &str| std::fs::read_to_string(root.join("tools/probe").join(p)).ok();
    let p = problems(&list, &versions, &record, plan.as_deref(), &src);
    if p.is_empty() { Ok(()) } else { Err(p.join("\n")) }
}

#[cfg(test)]
mod tests {
    use super::problems;

    const V: &str = "library\tsymbol\tversion\tbinding\nlibc.so.6\tmemcpy\tGLIBC_2.2.5\tcompat\nlibc.so.6\tmemcpy\tGLIBC_2.14\tdefault\n";
    const L: &str = "library\tsymbol\tversion\tprobe\nlibc.so.6\tmemcpy\tGLIBC_2.2.5\tcompat/a.c\n";
    const SRC: &str = "__asm__(\".symver memcpy_225, memcpy@GLIBC_2.2.5\");";
    fn src(p: &str) -> Option<String> { (p == "compat/a.c").then(|| SRC.to_string()) }

    #[test]
    fn a_true_list_passes() {
        assert!(problems(L, V, "memcpy@GLIBC_2.2.5 overlap n=1: none\n", None, &src).is_empty());
    }

    #[test]
    fn missing_record_line_non_compat_row_and_unbound_probe_fail() {
        let p = problems(L, V, "== x\n", None, &src);
        assert!(p.iter().any(|m| m.contains("no line for memcpy@GLIBC_2.2.5")), "{p:?}");
        let l2 = L.replace("GLIBC_2.2.5", "GLIBC_2.14");
        let p = problems(&l2, V, "memcpy@GLIBC_2.14 x\n", None, &src);
        assert!(p.iter().any(|m| m.contains("not a compat row")) && p.iter().any(|m| m.contains("does not bind")), "{p:?}");
    }

    #[test]
    fn an_unprobed_behaviour_row_of_the_plan_fails() {
        let plan = "library\tname\tversion\tkind\nlibc.so.6\tmemcpy\tGLIBC_2.2.5\tbehaviour\nlibc.so.6\trealpath\tGLIBC_2.2.5\tbehaviour\nlibc.so.6\tstime\tGLIBC_2.2.5\talias\n";
        let p = problems(L, V, "memcpy@GLIBC_2.2.5 x\n", Some(plan), &src);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].contains("realpath@GLIBC_2.2.5"), "{p:?}");
    }

    #[test]
    fn only_started_stories_need_probes() {
        // LG-2 is always started; LG-5 is started by its listed memcpy row; LG-9 has no listed row.
        let plan = "library\tsymbol\tversion\tkind\tstory\n\
            libc.so.6\trealpath\tGLIBC_2.2.5\tbehaviour\tLG-2\n\
            libc.so.6\tmemcpy\tGLIBC_2.2.5\tbehaviour\tLG-5\n\
            libc.so.6\tglob\tGLIBC_2.2.5\tbehaviour\tLG-5\n\
            libm.so.6\tlgamma\tGLIBC_2.2.5\tbehaviour\tLG-9\n";
        let p = problems(L, V, "memcpy@GLIBC_2.2.5 x\n", Some(plan), &src);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p.iter().any(|m| m.contains("realpath@GLIBC_2.2.5")) && p.iter().any(|m| m.contains("glob@GLIBC_2.2.5")), "{p:?}");
        assert!(!p.iter().any(|m| m.contains("lgamma")), "an unstarted story's row is not required: {p:?}");
    }
}
