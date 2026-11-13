//! Layout contracts (LAYOUT-MANIFEST, D-X.48): every deliberate code or data layout of the products is a row of
//! `layout/contracts.tsv` (`id kind subjects rule reason decision`), checked on the built products by
//! `cargo xtask layout-check [<products dir>]`, which `cargo xtask so` runs after the last product of either profile.
//!
//! Each row's `kind` names its check; the existing checks (`align`, `classed`, `jcc`, `coldbss`, `remap`) are called,
//! not copied. A broken contract prints `layout: <ID> BROKEN: <symbol or section>: <what moved>: <rule> (decision
//! <D-row>)`. An unknown kind, a subject absent from the products or a rule the checker does not hold is an error,
//! never a skip.
//!
//! `static` (and `layout-check --static`) checks the manifest without products: every row names a D-row of
//! `plan/07-decisions/` or a decision record (`records/decisions/`); every `LAYOUT(<ID>)` tag in `crates/` and
//! `csys/` names a row and sits on, or in the comment directly above, an alignment or offset directive (`.p2align`, `.balign`, `.align`, `.org`, `align(`), so deleting a
//! tagged directive fails; every row of a kind with a source site has a tag; and (rule d) with the branch's changed
//! files (as `affected` reads them), a removed row or a changed kind, subjects or rule needs a decision change
//! (`DECISION_DIRS`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

/// The manifest, relative to the workspace root.
pub const MANIFEST: &str = "layout/contracts.tsv";
const HEADER: [&str; 6] = ["id", "kind", "subjects", "rule", "reason", "decision"];
/// Kinds whose contract is held by a directive in the source (alignment attribute, `.p2align`, `.org`): a row of one
/// of them needs a `LAYOUT(<ID>)` tag at that directive.
const SITE_KINDS: &[&str] = &["classed-xref", "object-window", "object-align", "section-offset", "label-align"];
const KINDS: &[&str] = &["fn-align", "classed-xref", "jcc", "section-rename", "object-window", "object-align", "section-offset", "label-align", "remap", "startup-text-windows", "startup-data-windows"];
const DIRECTIVES: &[&str] = &[".p2align", ".balign", ".align ", ".org ", "align("];

#[derive(Clone, Debug, PartialEq)]
pub struct Contract {
    pub id: String,
    pub kind: String,
    pub subjects: Vec<String>,
    pub rule: String,
    pub reason: String,
    pub decision: String,
}

impl Contract {
    /// The rule's `key=value` pairs (`; `-separated).
    pub(crate) fn params(&self) -> Result<BTreeMap<String, String>, String> {
        let mut m = BTreeMap::new();
        for p in self.rule.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let (k, v) = p.split_once('=').ok_or_else(|| format!("layout: {}: rule part `{p}` is not key=value", self.id))?;
            m.insert(k.trim().to_string(), v.trim().to_string());
        }
        Ok(m)
    }

    fn broken(&self, what: &str, moved: &str) -> String {
        format!("layout: {} BROKEN: {what}: {moved}: {} (decision {})", self.id, self.rule, self.decision)
    }
}

/// Parse the manifest text: the header row, then six tab-separated columns per row, ids unique.
pub fn parse(text: &str) -> Result<Vec<Contract>, String> {
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    if head != HEADER {
        return Err(format!("layout: {MANIFEST}: the header must be `{}`", HEADER.join("\\t")));
    }
    let mut out: Vec<Contract> = Vec::new();
    for (n, l) in lines.enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        let f: Vec<&str> = l.split('\t').collect();
        let [id, kind, subjects, rule, reason, decision] = f.as_slice() else {
            return Err(format!("layout: {MANIFEST} line {}: {} columns, want 6", n + 2, f.len()));
        };
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-') {
            return Err(format!("layout: {MANIFEST} line {}: id `{id}` is not [A-Z0-9-]+", n + 2));
        }
        if out.iter().any(|c| c.id == *id) {
            return Err(format!("layout: {MANIFEST}: id {id} twice"));
        }
        let subjects: Vec<String> = subjects.split_whitespace().map(String::from).collect();
        if subjects.is_empty() || reason.trim().is_empty() {
            return Err(format!("layout: {MANIFEST}: {id}: empty subjects or reason"));
        }
        out.push(Contract { id: id.to_string(), kind: kind.to_string(), subjects, rule: rule.to_string(), reason: reason.to_string(), decision: decision.to_string() });
    }
    if out.is_empty() {
        return Err(format!("layout: {MANIFEST} has no rows"));
    }
    Ok(out)
}

pub fn load(root: &Path) -> Result<Vec<Contract>, String> {
    parse(&std::fs::read_to_string(root.join(MANIFEST)).map_err(|e| format!("layout: {MANIFEST}: {e}"))?)
}

// ---------------------------------------------------------------------------------------------------------------
// Products

#[derive(Debug, Default)]
struct Section {
    name: String,
    ty: String,
    size: u64,
    align: u64,
}

#[derive(Debug)]
struct Symbol {
    value: u64,
    size: u64,
    ty: String,
    ndx: Option<usize>,
    name: String,
}

#[derive(Debug, Default)]
struct Member {
    name: String,
    sections: BTreeMap<usize, Section>,
    symbols: Vec<Symbol>,
}

fn num(s: &str) -> Option<u64> {
    match s.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

/// `readelf -SWs` of an archive (or one object): section headers and symbols per member.
fn parse_readelf(text: &str) -> Vec<Member> {
    let mut out: Vec<Member> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("File: ") {
            let name = rest.rsplit_once('(').map(|(_, m)| m.trim_end_matches(')')).unwrap_or(rest);
            out.push(Member { name: name.to_string(), ..Member::default() });
            continue;
        }
        let t = line.trim_start();
        if let Some(r) = t.strip_prefix('[') {
            // "[Nr] Name Type Address Off Size ES Flg Lk Inf Al" (Flg may be empty)
            let Some((nr, after)) = r.split_once(']') else { continue };
            let Ok(nr) = nr.trim().parse::<usize>() else { continue };
            let f: Vec<&str> = after.split_whitespace().collect();
            if f.len() < 9 {
                continue;
            }
            let (Some(size), Some(align)) = (u64::from_str_radix(f[4], 16).ok(), f[f.len() - 1].parse().ok()) else { continue };
            if out.is_empty() {
                out.push(Member::default());
            }
            out.last_mut().unwrap().sections.insert(nr, Section { name: f[0].to_string(), ty: f[1].to_string(), size, align });
            continue;
        }
        // "Num: Value Size Type Bind Vis Ndx Name"
        let f: Vec<&str> = t.split_whitespace().collect();
        if f.len() < 8 || !f[0].ends_with(':') || f[0][..f[0].len() - 1].parse::<usize>().is_err() {
            continue;
        }
        let (Ok(value), Some(size)) = (u64::from_str_radix(f[1], 16), num(f[2])) else { continue };
        if out.is_empty() {
            out.push(Member::default());
        }
        out.last_mut().unwrap().symbols.push(Symbol { value, size, ty: f[3].to_string(), ndx: f[6].parse().ok(), name: f[7].to_string() });
    }
    out
}

/// The instruction text at each offset of section `section` of member `member`, from `objdump -d -w`.
fn parse_objdump(text: &str, member: &str, section: &str) -> BTreeMap<u64, String> {
    let mut out = BTreeMap::new();
    let (mut in_member, mut in_section) = (false, false);
    for line in text.lines() {
        if let Some((m, _)) = line.split_once(":     file format ") {
            if in_member && in_section && !out.is_empty() {
                break;
            }
            in_member = m.trim() == member;
            in_section = false;
            continue;
        }
        if let Some(s) = line.strip_prefix("Disassembly of section ") {
            in_section = in_member && s.trim_end_matches(':') == section;
            continue;
        }
        if !in_section {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        let (Some(addr), Some(insn)) = (parts.first().and_then(|a| a.trim().strip_suffix(':')), parts.get(2)) else { continue };
        if let Ok(a) = u64::from_str_radix(addr, 16) {
            out.insert(a, insn.trim().to_string());
        }
    }
    out
}

/// `insn` without the padding prefixes the JCC padding adds (`cs cs punpcklbw ...`).
fn bare_insn(insn: &str) -> &str {
    let mut s = insn.trim();
    while let Some((w, rest)) = s.split_once(' ') {
        if !matches!(w, "cs" | "ds" | "es" | "ss" | "fs" | "gs" | "data16") {
            break;
        }
        s = rest.trim_start();
    }
    s
}

fn tool(cmd: &mut Command, what: &str) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("layout: {what}: {e}"))?;
    if !out.status.success() {
        return Err(format!("layout: {what} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The products being checked, with the `readelf` index of each file read once.
struct Products<'a> {
    root: &'a Path,
    dir: &'a Path,
    /// The build profile of the products (`fast`, `release`), when known: `so` passes it, `layout-check <dir>` reads
    /// `<dir>/.profile`. Kinds with a budget per profile need it.
    profile: Option<String>,
    index: BTreeMap<String, Vec<Member>>,
}

impl Products<'_> {
    fn file(&self, name: &str) -> Result<std::path::PathBuf, String> {
        let p = self.dir.join(name);
        if p.is_file() { Ok(p) } else { Err(format!("{name} is not in {}", self.dir.display())) }
    }

    fn members(&mut self, file: &str) -> Result<&Vec<Member>, String> {
        if !self.index.contains_key(file) {
            let p = self.file(file)?;
            let text = tool(Command::new("readelf").args(["-SWs"]).arg(&p), &format!("readelf -SWs {file}"))?;
            self.index.insert(file.to_string(), parse_readelf(&text));
        }
        Ok(&self.index[file])
    }

    /// The defined symbols of `subject` (`<file>:<name>`; `exact`: the whole name, else a part of it), with their
    /// member and section.
    fn symbols(&mut self, subject: &str, exact: bool) -> Result<Vec<(String, &Symbol, &Section)>, String> {
        let (file, want) = subject.split_once(':').ok_or_else(|| format!("subject `{subject}` is not <file>:<symbol>"))?;
        let mut found = Vec::new();
        for m in self.members(file)? {
            for s in &m.symbols {
                let hit = if exact { s.name == want } else { s.name.contains(want) };
                if let (true, Some(sec)) = (hit && s.ty != "SECTION", s.ndx.and_then(|n| m.sections.get(&n))) {
                    found.push((m.name.clone(), s, sec));
                }
            }
        }
        if found.is_empty() {
            return Err(format!("{want}: no defined symbol in {file}"));
        }
        Ok(found)
    }
}

fn param<'a>(p: &'a BTreeMap<String, String>, c: &Contract, k: &str) -> Result<&'a str, String> {
    p.get(k).map(String::as_str).ok_or_else(|| format!("layout: {}: rule has no `{k}=`", c.id))
}

fn param_num(p: &BTreeMap<String, String>, c: &Contract, k: &str) -> Result<u64, String> {
    let v = param(p, c, k)?;
    num(v).ok_or_else(|| format!("layout: {}: `{k}={v}` is not a number", c.id))
}

/// The rule value the existing checker hard-codes must be the manifest's: a checker changed without its row (or the
/// reverse) is an error.
fn same(c: &Contract, k: &str, manifest: u64, checker: u64) -> Result<(), String> {
    if manifest == checker { Ok(()) } else { Err(format!("layout: {}: rule {k}={manifest} but the checker holds {checker}", c.id)) }
}

/// Check one contract on the products: `Ok(summary)`, `Err(lines)` (BROKEN lines or an error).
fn check_one(c: &Contract, pr: &mut Products) -> Result<String, Vec<String>> {
    let one = |e: String| vec![e];
    let p = c.params().map_err(one)?;
    match c.kind.as_str() {
        "fn-align" => {
            same(c, "align", param_num(&p, c, "align").map_err(one)?, 64).map_err(one)?;
            let mut lines = Vec::new();
            let mut total = 0;
            for f in &c.subjects {
                let path = pr.file(f).map_err(|e| one(c.broken(f, &e)))?;
                let (bad, checked) = crate::align::misaligned(&path).map_err(one)?;
                if checked == 0 {
                    return Err(one(c.broken(f, "no exported function found")));
                }
                total += checked;
                lines.extend(bad.iter().map(|(n, a)| c.broken(n, &format!("entry at {a:#x} ({} mod 64) in {f}", a % 64))));
            }
            if lines.is_empty() { Ok(format!("{total} exported function entries 64-byte aligned")) } else { Err(lines) }
        }
        "classed-xref" => {
            same(c, "bias", param_num(&p, c, "bias").map_err(one)?, crate::classed::PAGE as u64).map_err(one)?;
            let mut n = 0;
            for f in &c.subjects {
                let path = pr.file(f).map_err(|e| one(c.broken(f, &e)))?;
                n += crate::classed::check_file(&path).map_err(|e| one(c.broken(f, &e)))?;
            }
            Ok(format!("{n} out-of-page reference pairs, page 1 = page 0 + 4096"))
        }
        "jcc" => {
            same(c, "boundary", param_num(&p, c, "boundary").map_err(one)?, crate::jcc::BOUNDARY).map_err(one)?;
            for f in &c.subjects {
                let path = pr.file(f).map_err(|e| one(c.broken(f, &e)))?;
                crate::jcc::check_archive(pr.root, &path).map_err(|e| one(c.broken(f, &e)))?;
            }
            Ok("no jump of the padded files on a 32-byte boundary".into())
        }
        "section-rename" => {
            let (from, to) = (param(&p, c, "from").map_err(one)?, param(&p, c, "to").map_err(one)?);
            let min = param_num(&p, c, "min").map_err(one)?;
            same(c, "min", min, crate::coldbss::MIN).map_err(one)?;
            let min_count = param_num(&p, c, "min-count").map_err(one)?;
            let mut lines = Vec::new();
            let mut moved = 0;
            for f in &c.subjects {
                for m in pr.members(f).map_err(|e| one(c.broken(f, &e)))? {
                    for s in m.sections.values().filter(|s| s.ty == "NOBITS") {
                        if s.name.starts_with(from) && s.size >= min {
                            lines.push(c.broken(&format!("{}({})", m.name, s.name), &format!("{} bytes left in {from}*", s.size)));
                        } else if s.name.starts_with(to) {
                            moved += 1;
                            if s.size < min {
                                lines.push(c.broken(&format!("{}({})", m.name, s.name), &format!("{} bytes in {to}* (below {min})", s.size)));
                            }
                        }
                    }
                }
            }
            if moved < min_count {
                lines.push(c.broken(&c.subjects.join(" "), &format!("only {moved} {to}* sections (the rename did not run?)")));
            }
            if lines.is_empty() { Ok(format!("{moved} sections of {min} bytes or more in {to}*, none left in {from}*")) } else { Err(lines) }
        }
        "object-window" | "object-align" => {
            let mut lines = Vec::new();
            let mut n = 0;
            for subj in &c.subjects {
                let syms = pr.symbols(subj, false).map_err(|e| one(c.broken(subj, &e)))?;
                for (member, s, sec) in syms {
                    n += 1;
                    let at = format!("{} in {member}({}) at {:#x}, {} bytes, section alignment {}", s.name, sec.name, s.value, s.size, sec.align);
                    if c.kind == "object-window" {
                        let want = param(&p, c, "section").map_err(one)?;
                        let w = param_num(&p, c, "window").map_err(one)?;
                        // Placed at any multiple of the section's alignment, the object stays in one window iff it
                        // fits in the part of the window that its offset modulo min(alignment, window) leaves.
                        let g = sec.align.max(1).min(w);
                        if sec.name != want {
                            lines.push(c.broken(&s.name, &format!("in section {} of {member}, not {want}", sec.name)));
                        } else if s.size == 0 || s.value % g + s.size > g {
                            lines.push(c.broken(&s.name, &format!("{at}: can cross a {w}-byte window")));
                        }
                    } else {
                        let a = param_num(&p, c, "align").map_err(one)?;
                        let max = param_num(&p, c, "max-size").map_err(one)?;
                        if sec.align < a || s.value % a != 0 || s.size > max {
                            lines.push(c.broken(&s.name, &format!("{at}: not on its own {a}-byte boundary")));
                        }
                    }
                }
            }
            if lines.is_empty() { Ok(format!("{n} object(s) placed")) } else { Err(lines) }
        }
        "section-offset" | "label-align" => {
            let off = param_num(&p, c, "offset").map_err(one)?;
            let insn = param(&p, c, "insn").map_err(one)?;
            let sec_align = if c.kind == "label-align" { param_num(&p, c, "align").map_err(one)? } else { param_num(&p, c, "section-align").map_err(one)? };
            let mut lines = Vec::new();
            for subj in &c.subjects {
                let file = subj.split_once(':').map(|(f, _)| f).unwrap_or(subj);
                let found: Vec<(String, u64, String, u64)> = pr.symbols(subj, true).map_err(|e| one(c.broken(subj, &e)))?.into_iter().map(|(m, s, sec)| (m, s.value, sec.name.clone(), sec.align)).collect();
                let path = pr.file(file).map_err(one)?;
                for (member, value, sec, align) in found {
                    let at = value + off;
                    if align < sec_align {
                        lines.push(c.broken(&sec, &format!("section alignment {align} in {member}, below {sec_align}")));
                    }
                    if c.kind == "label-align" && at % sec_align != 0 {
                        lines.push(c.broken(subj, &format!("{at:#x} is {} mod {sec_align}", at % sec_align)));
                    }
                    let dis = tool(Command::new("objdump").args(["-d", "-w", "-j", &sec]).arg(&path), &format!("objdump -d -j {sec} {file}")).map_err(one)?;
                    let code = parse_objdump(&dis, &member, &sec);
                    match code.get(&at) {
                        Some(i) if bare_insn(i).starts_with(insn) => {}
                        Some(i) => lines.push(c.broken(subj, &format!("{sec}+{at:#x} holds `{i}`, not `{insn}`"))),
                        None => lines.push(c.broken(subj, &format!("no instruction starts at {sec}+{at:#x} ({} instructions read)", code.len()))),
                    }
                }
            }
            if lines.is_empty() { Ok(format!("`{insn}` at entry + {off:#x}")) } else { Err(lines) }
        }
        "remap" => {
            let want: BTreeSet<&str> = c.subjects.iter().map(String::as_str).collect();
            let have: BTreeSet<&str> = crate::remap::PRODUCTS.iter().copied().collect();
            if want != have {
                return Err(one(format!("layout: {}: subjects {want:?} but the remap check scans {have:?}", c.id)));
            }
            for f in &c.subjects {
                pr.file(f).map_err(|e| one(c.broken(f, &e)))?;
            }
            let n = crate::remap::check_dir(pr.dir, &crate::remap::needles(pr.root)).map_err(|e| one(c.broken(&c.subjects.join(" "), &e)))?;
            Ok(format!("{n} products carry no build path"))
        }
        "startup-text-windows" => check_startup_windows(c, &p, pr),
        "startup-data-windows" => check_startup_data(c, &p, pr),
        k => Err(one(format!("layout: {}: unknown kind `{k}` (known: {})", c.id, KINDS.join(", ")))),
    }
}

/// Check every contract on the products in `dir` (built with `profile`, when known); one line per contract, every
/// failure listed. A pass already proven for the same products, inputs, tools and checker is served from the verdict
/// cache (`layout_verdict`), except under the CI train.
pub fn check_products(root: &Path, dir: &Path, profile: Option<&str>) -> Result<(), String> {
    crate::layout_verdict::check(root, dir, profile, || check_products_full(root, dir, profile))
}

/// Every contract checked on the products; on a pass, the lines printed (for the verdict cache).
fn check_products_full(root: &Path, dir: &Path, profile: Option<&str>) -> Result<Vec<String>, String> {
    let contracts = load(root)?;
    let mut pr = Products { root, dir, profile: profile.map(str::to_string), index: BTreeMap::new() };
    let mut bad = Vec::new();
    let mut ok = Vec::new();
    let (all, mut slowest) = (std::time::Instant::now(), (0.0, String::new()));
    for c in &contracts {
        let t = std::time::Instant::now();
        let r = check_one(c, &mut pr);
        let s = t.elapsed().as_secs_f64();
        if s > slowest.0 {
            slowest = (s, c.id.clone());
        }
        match r {
            Ok(s) => {
                ok.push(format!("layout: {} ok: {s} ({})", c.id, c.decision));
                println!("{}", ok[ok.len() - 1]);
            }
            Err(lines) => {
                for l in &lines {
                    eprintln!("{l}");
                }
                eprintln!("layout: {} protects: {}", c.id, c.reason);
                bad.push(c.id.clone());
            }
        }
    }
    println!("layout: {} contracts checked in {:.1} s (slowest {} {:.1} s)", contracts.len(), all.elapsed().as_secs_f64(), slowest.1, slowest.0);
    if bad.is_empty() {
        ok.push(format!("layout: all {} contracts of {MANIFEST} hold in {}", contracts.len(), dir.display()));
        println!("{}", ok[ok.len() - 1]);
        Ok(ok)
    } else {
        Err(format!("layout: {} of {} contracts broken: {}", bad.len(), contracts.len(), bad.join(", ")))
    }
}

// ---------------------------------------------------------------------------------------------------------------
// STARTUP-TEXT-WINDOWS (D-X.49): the start-up text of the static probe in few 64 KiB windows
//
// A static program's text is a file mapping, and the first touch of each 64 KiB window of it costs one page fault
// (fault-around maps the window). The functions `page_faults true` runs (keys in the row's `keys=` file, written by
// `tools/hottext/regen.sh`) must occupy at most `budget-<profile>=` distinct windows of `page_faults.c` linked from the
// products as `crates/core/tests/hot_text_products.rs` links it, and none outside the hot group (D-X.20) may straddle
// a window boundary. STARTUP-TEXT-GROUPED (`grouped=error|warn`): every start-up function of `libc.a` lies in the
// hot group. Products and counters only: no timing, no tracing at check time.

/// The window the start-up text is counted in (bytes). The row's `window=` must equal it.
const STARTUP_WINDOW: u64 = 1 << 16;

/// A defined function of the linked probe: `[start, end)` and its linked name.
#[derive(Clone, Debug, PartialEq)]
struct Func {
    start: u64,
    end: u64,
    name: String,
}

/// `nm -S --defined-only` output -> the text symbols by build-stable key (`hottext::key`, the keys of
/// `hot-text.list`). An assembly entry without `.size` prints no size; it counts its entry byte, so the window it
/// starts in is still counted.
fn parse_nm(text: &str) -> BTreeMap<String, Vec<Func>> {
    let mut out: BTreeMap<String, Vec<Func>> = BTreeMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (addr, size, ty, name) = match f.as_slice() {
            [a, s, t, n] => (*a, Some(*s), *t, *n),
            [a, t, n] => (*a, None, *t, *n),
            _ => continue,
        };
        if !matches!(ty, "t" | "T" | "w" | "W" | "i" | "I") {
            continue;
        }
        let (Ok(start), Ok(size)) = (u64::from_str_radix(addr, 16), size.map_or(Ok(0), |s| u64::from_str_radix(s, 16))) else {
            continue;
        };
        out.entry(crate::hottext::key(name)).or_default().push(Func { start, end: start + size.max(1), name: name.to_string() });
    }
    out
}

/// A `.text*` input section of a GNU ld link map: `[start, end)`, its name and the file it came from.
#[derive(Clone, Debug, PartialEq)]
struct InputText {
    start: u64,
    end: u64,
    name: String,
    file: String,
}

/// The `.text*` input sections of a link map (` .text.f 0x<addr> 0x<size> <file>`; a long name puts the address on
/// the next line). Empty sections are dropped.
fn parse_map(text: &str) -> Vec<InputText> {
    let mut out = Vec::new();
    let mut pending: Option<String> = None;
    for line in text.lines() {
        let (name, rest) = if line.starts_with(" .text") {
            let mut it = line[1..].splitn(2, char::is_whitespace);
            (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").trim().to_string())
        } else if let (Some(n), true) = (pending.take(), line.starts_with("  ")) {
            (n, line.trim().to_string())
        } else {
            continue;
        };
        let f: Vec<&str> = rest.split_whitespace().collect();
        if f.is_empty() {
            pending = Some(name);
            continue;
        }
        let hex = |s: &str| s.strip_prefix("0x").and_then(|h| u64::from_str_radix(h, 16).ok());
        if let (Some(start), Some(size), Some(file)) = (f.first().and_then(|s| hex(s)), f.get(1).and_then(|s| hex(s)), f.get(2)) {
            if size > 0 {
                out.push(InputText { start, end: start + size, name, file: file.to_string() });
            }
        }
    }
    out
}

/// A grouped hot-text input section (D-X.20): `.text.sorted.h<n>.<f>`, or `.text.hot.<f>` of an older archive.
fn is_hot_section(name: &str) -> bool {
    if name.starts_with(".text.hot.") {
        return true;
    }
    name.strip_prefix(".text.sorted.h").is_some_and(|r| {
        let d = r.bytes().take_while(u8::is_ascii_digit).count();
        d > 0 && r[d..].starts_with('.')
    })
}

/// The start-up keys of `profile` from the keys file (`<key> TAB <tier> TAB <profiles>`, `#` comments), in order.
fn startup_keys(text: &str, profile: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (n, l) in text.lines().enumerate() {
        if l.trim().is_empty() || l.starts_with('#') {
            continue;
        }
        match l.split('\t').collect::<Vec<_>>().as_slice() {
            [k, _tier, profiles] => {
                if profiles.split(',').any(|p| p == profile) {
                    out.push(k.to_string());
                }
            }
            _ => return Err(format!("line {}: not `<key> TAB <tier> TAB <profiles>`: {l}", n + 1)),
        }
    }
    Ok(out)
}

/// What the start-up functions occupy in the linked probe.
#[derive(Debug, Default, PartialEq)]
struct StartupText {
    /// `[lo, hi)` of the grouped hot text.
    group: (u64, u64),
    /// Every window (address / window size) a start-up function touches, with the functions in it.
    windows: BTreeMap<u64, Vec<Func>>,
    /// Start-up functions outside the group whose first and last byte lie in different windows.
    straddling: Vec<Func>,
    /// Start-up functions of `libc.a` outside the group (STARTUP-TEXT-GROUPED).
    ungrouped: Vec<Func>,
    /// Keys with no function in this link (inlined or renamed in this build).
    missing: Vec<String>,
}

fn window_of(a: u64, window: u64) -> u64 {
    a / window
}

/// Map the start-up keys to the linked functions and count their windows.
fn startup_text(keys: &[String], fns: &BTreeMap<String, Vec<Func>>, sections: &[InputText], window: u64) -> Result<StartupText, String> {
    let hot: Vec<&InputText> = sections.iter().filter(|s| is_hot_section(&s.name)).collect();
    let (Some(lo), Some(hi)) = (hot.iter().map(|s| s.start).min(), hot.iter().map(|s| s.end).max()) else {
        return Err("no grouped hot text in the link map (D-X.20 grouping missing?)".into());
    };
    let mut t = StartupText { group: (lo, hi), ..Default::default() };
    let mut seen = BTreeSet::new();
    for k in keys {
        let Some(list) = fns.get(k) else {
            t.missing.push(k.clone());
            continue;
        };
        for f in list {
            if !seen.insert((f.start, f.name.clone())) {
                continue;
            }
            for w in window_of(f.start, window)..=window_of(f.end - 1, window) {
                t.windows.entry(w).or_default().push(f.clone());
            }
            if (lo..hi).contains(&f.start) {
                continue;
            }
            if window_of(f.start, window) != window_of(f.end - 1, window) {
                t.straddling.push(f.clone());
            }
            if sections.iter().any(|s| (s.start..s.end).contains(&f.start) && s.file.contains("libc.a(")) {
                t.ungrouped.push(f.clone());
            }
        }
    }
    Ok(t)
}

/// The verdict on `t`: `Ok(summary)` or `Err(lines)` naming the contract, every function in a window outside the
/// group, and each straddling or (with `grouped_error`) ungrouped function.
fn startup_verdict(c: &Contract, t: &StartupText, window: u64, budget: u64, grouped_error: bool, profile: &str) -> Result<String, Vec<String>> {
    let (lo, hi) = t.group;
    let group: BTreeSet<u64> = (window_of(lo, window)..=window_of(hi - 1, window)).collect();
    let outside: Vec<u64> = t.windows.keys().copied().filter(|w| !group.contains(w)).collect();
    let n = t.windows.len() as u64;
    let hex = |f: &Func| format!("{} 0x{:x}-0x{:x}", f.name, f.start, f.end);
    let mut summary = format!(
        "{n} start-up text windows of {window} bytes (budget-{profile} {budget}); hot group 0x{lo:x}-0x{hi:x} ({} used); outside it: {}",
        t.windows.keys().filter(|w| group.contains(w)).count(),
        if outside.is_empty() { "none".to_string() } else { outside.iter().map(|w| format!("0x{:x}", w * window)).collect::<Vec<_>>().join(", ") }
    );
    if !t.missing.is_empty() {
        summary += &format!("; not in this link (inlined or renamed): {}", t.missing.join(" "));
    }
    let mut bad = Vec::new();
    if n > budget {
        bad.push(c.broken(&format!("{n} windows"), &format!("start-up text spans {n} windows of {window} bytes > budget-{profile} {budget}")));
    }
    for f in &t.straddling {
        let b = window_of(f.end - 1, window) * window;
        bad.push(c.broken(&hex(f), &format!("outside the hot group, straddles the {window}-byte window boundary 0x{b:x}")));
    }
    let grouped: Vec<String> = t.ungrouped.iter().map(|f| format!("{} (0x{:x})", f.name, f.start)).collect();
    if !grouped.is_empty() {
        let what = format!("STARTUP-TEXT-GROUPED: {} start-up functions of libc.a outside the hot group 0x{lo:x}-0x{hi:x}: {}", grouped.len(), grouped.join(", "));
        if grouped_error {
            bad.push(c.broken(&what, "add their keys to crates/libc-so/hot-text.list (tools/hottext/regen.sh)"));
        } else {
            summary += &format!("\nlayout: {} WARNING {what}", c.id);
        }
    }
    if bad.is_empty() {
        return Ok(summary);
    }
    let mut lines = vec![format!("layout: {}: {summary}", c.id)];
    for w in &outside {
        for f in &t.windows[w] {
            lines.push(format!("layout: {}:   window 0x{:x} (outside the hot group): {}", c.id, w * window, hex(f)));
        }
    }
    lines.extend(bad);
    Err(lines)
}

/// The linked start-up probe, as text: `nm -S`, the link map, `readelf -SW`, and (when asked) `objdump -d`.
struct Probe {
    nm: String,
    map: String,
    sections: String,
    disasm: String,
}

/// Link `page_faults.c` against the products as `hot_text_products.rs` does, with a link map; `disasm`: also
/// disassemble it.
fn link_startup_probe(pr: &Products, disasm: bool) -> Result<Probe, String> {
    let work = std::env::temp_dir().join(format!("liella-layout-startup-{}", std::process::id()));
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let (exe, map) = (work.join("page_faults"), work.join("page_faults.map"));
    let linked = tool(
        Command::new("cc")
            .args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"])
            .arg(pr.file("liella-crt1.o")?)
            .arg(pr.root.join("crates/core/tests/oracle/page_faults.c"))
            .arg(pr.file("libc.a")?)
            .arg("-lgcc")
            .arg(format!("-Wl,-Map,{}", map.display()))
            .arg("-o")
            .arg(&exe),
        "cc page_faults.c (the start-up probe)",
    )
    .and_then(|_| tool(Command::new("nm").args(["-S", "--defined-only"]).arg(&exe), "nm -S page_faults"))
    .and_then(|nm| std::fs::read_to_string(&map).map(|m| (nm, m)).map_err(|e| format!("{}: {e}", map.display())))
    .and_then(|(nm, map)| tool(Command::new("readelf").arg("-SW").arg(&exe), "readelf -SW page_faults").map(|sections| Probe { nm, map, sections, disasm: String::new() }))
    .and_then(|p| match disasm {
        true => tool(Command::new("objdump").args(["-d", "--no-show-raw-insn"]).arg(&exe), "objdump -d page_faults").map(|disasm| Probe { disasm, ..p }),
        false => Ok(p),
    });
    let _ = std::fs::remove_dir_all(&work);
    linked
}

fn check_startup_windows(c: &Contract, p: &BTreeMap<String, String>, pr: &mut Products) -> Result<String, Vec<String>> {
    let one = |e: String| vec![e];
    let window = param_num(p, c, "window").map_err(one)?;
    same(c, "window", window, STARTUP_WINDOW).map_err(one)?;
    let profile = pr.profile.clone().ok_or_else(|| one(format!("layout: {}: the products' profile is unknown (no {}/.profile)", c.id, pr.dir.display())))?;
    let budget = param_num(p, c, &format!("budget-{profile}")).map_err(one)?;
    let grouped_error = match param(p, c, "grouped").map_err(one)? {
        "error" => true,
        "warn" => false,
        g => return Err(one(format!("layout: {}: grouped={g}: want error or warn", c.id))),
    };
    let path = param(p, c, "keys").map_err(one)?;
    let text = std::fs::read_to_string(pr.root.join(path)).map_err(|e| one(format!("layout: {}: {path}: {e}", c.id)))?;
    let keys = startup_keys(&text, &profile).map_err(|e| one(format!("layout: {}: {path}: {e}", c.id)))?;
    if keys.is_empty() {
        return Err(one(format!("layout: {}: {path}: no start-up key traced in profile {profile} (run tools/hottext/regen.sh)", c.id)));
    }
    for f in &c.subjects {
        pr.file(f).map_err(|e| one(c.broken(f, &e)))?;
    }
    let probe = link_startup_probe(pr, false).map_err(|e| one(format!("layout: {}: {e}", c.id)))?;
    let t = startup_text(&keys, &parse_nm(&probe.nm), &parse_map(&probe.map), window).map_err(|e| one(format!("layout: {}: {e}", c.id)))?;
    if t.missing.len() == keys.len() {
        return Err(one(c.broken("start-up keys", &format!("none of the {} keys of {path} names a function of the probe", keys.len()))));
    }
    startup_verdict(c, &t, window, budget, grouped_error, &profile).map(|s| format!("{s}; {} of {} start-up functions found", keys.len() - t.missing.len(), keys.len()))
}

// ---------------------------------------------------------------------------------------------------------------
// STARTUP-DATA-WINDOWS (D-X.50): the start-up data objects of the static probe in few windows and pages
//
// The read-only objects start-up reads sit in the file mapping: the first touch of each 64 KiB window costs one fault
// (fault-around), as for text. The writable ones (.data, .bss, .lbss, ...) are private: every 4 KiB page start-up
// touches costs its own fault. The objects (keys in the row's `keys=` file, written by `tools/hottext/startup_data.py`
// from `perf record -e page-faults -d` and the start-up functions' RIP-relative operands) must occupy at most
// `ro-budget-<profile>=` windows and `rw-budget-<profile>=` pages of `page_faults.c` linked as for STARTUP-TEXT-WINDOWS.
// The kind is the section's in the link being checked, so an object moved between .rodata and .data is counted where
// it now lies. An object larger than a window or page counts the units of its head only. The start-up functions'
// RIP-relative operands are also resolved at check time (`text-keys=`), named or not, so an object no key names (a
// start-up name moved out of STARTUP_NAMES into a literal of its own) is counted too. Over a budget, the objects alone
// in a window or page are named: each costs a fault of its own.

/// The page the written start-up objects are counted in (bytes). The row's `page=` must equal it.
const STARTUP_PAGE: u64 = 1 << 12;

/// An allocated section of the linked probe.
#[derive(Clone, Debug, PartialEq)]
struct ExeSection {
    start: u64,
    end: u64,
    name: String,
    writable: bool,
    /// Code (`X`): not where a data object is counted.
    skip: bool,
}

/// `readelf -SW` of the linked probe -> its allocated sections.
fn parse_exe_sections(text: &str) -> Vec<ExeSection> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((_, rest)) = line.split_once(']') else { continue };
        let f: Vec<&str> = rest.split_whitespace().collect();
        // Name Type Address Off Size ES Flg Lk Inf Al: a section without flags has one field fewer.
        // TLS sections are left out: in a linked program a TLS symbol's value is its offset in the TLS block, and the
        // section's address range overlaps the next section's.
        if f.len() != 10 || !f[6].contains('A') || f[6].contains('T') {
            continue;
        }
        let (Ok(start), Ok(size)) = (u64::from_str_radix(f[2], 16), u64::from_str_radix(f[4], 16)) else { continue };
        out.push(ExeSection { start, end: start + size, name: f[0].to_string(), writable: f[6].contains('W'), skip: f[6].contains('X') });
    }
    out
}

/// A data object of the linked probe: `[start, end)`, its linked name, and whether its section is writable.
#[derive(Clone, Debug, PartialEq)]
struct DataObj {
    start: u64,
    end: u64,
    name: String,
    rw: bool,
}

/// `nm -S --defined-only` -> the data objects by build-stable key, classed by the section they lie in (TLS and objects
/// outside an allocated section are not counted).
fn parse_nm_data(text: &str, secs: &[ExeSection]) -> BTreeMap<String, Vec<DataObj>> {
    let mut out: BTreeMap<String, Vec<DataObj>> = BTreeMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let (addr, size, ty, name) = match f.as_slice() {
            [a, s, t, n] => (*a, Some(*s), *t, *n),
            [a, t, n] => (*a, None, *t, *n),
            _ => continue,
        };
        if !matches!(ty, "d" | "D" | "b" | "B" | "r" | "R" | "v" | "V" | "g" | "G" | "s" | "S") {
            continue;
        }
        let (Ok(start), Ok(size)) = (u64::from_str_radix(addr, 16), size.map_or(Ok(0), |s| u64::from_str_radix(s, 16))) else {
            continue;
        };
        let Some(sec) = secs.iter().find(|s| (s.start..s.end).contains(&start)) else { continue };
        if sec.skip {
            continue;
        }
        out.entry(crate::hottext::key(name)).or_default().push(DataObj { start, end: start + size.max(1), name: name.to_string(), rw: sec.writable });
    }
    out
}

/// What the start-up data objects occupy in the linked probe.
#[derive(Debug, Default, PartialEq)]
struct StartupData {
    /// Every 64 KiB window a read-only start-up object touches, with the objects in it.
    windows: BTreeMap<u64, Vec<DataObj>>,
    /// Every 4 KiB page a written start-up object touches, with the objects in it.
    pages: BTreeMap<u64, Vec<DataObj>>,
    /// Keys with no object in this link.
    missing: Vec<String>,
}

/// The RIP-relative targets (`# <addr>` in `objdump -d`) of the functions whose key is in `fns`: `(address, function)`.
fn operand_targets(disasm: &str, fns: &BTreeSet<String>) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    for line in disasm.lines() {
        if let Some(head) = line.strip_suffix(">:") {
            cur = head.split_once(" <").map(|(_, n)| n.to_string()).filter(|n| fns.contains(&crate::hottext::key(n)));
            continue;
        }
        let (Some(f), Some((_, t))) = (&cur, line.split_once("# ")) else { continue };
        if let Some(a) = t.split_whitespace().next().and_then(|h| u64::from_str_radix(h, 16).ok()) {
            out.push((a, f.clone()));
        }
    }
    out
}

/// The objects holding `targets`: the smallest named object around each address, else the unnamed byte itself (a
/// string literal, a constant of no symbol), named by its section and the start-up function that addresses it. Text and
/// TLS targets are not data.
fn target_objects(targets: &[(u64, String)], objs: &BTreeMap<String, Vec<DataObj>>, secs: &[ExeSection]) -> Vec<DataObj> {
    let all: Vec<&DataObj> = objs.values().flatten().collect();
    let mut out = Vec::new();
    for (a, f) in targets {
        let Some(sec) = secs.iter().find(|s| (s.start..s.end).contains(a)) else { continue };
        if sec.skip {
            continue;
        }
        match all.iter().filter(|o| (o.start..o.end).contains(a)).min_by_key(|o| o.end - o.start) {
            Some(o) => out.push((*o).clone()),
            None => out.push(DataObj { start: *a, end: a + 1, name: format!("<unnamed {} byte addressed by {f}>", sec.name), rw: sec.writable }),
        }
    }
    out
}

/// Count the objects of `keys` and the `operands` objects of the start-up functions in windows (read-only) and pages
/// (written).
fn startup_data(keys: &[String], objs: &BTreeMap<String, Vec<DataObj>>, operands: &[DataObj], window: u64, page: u64) -> StartupData {
    let mut d = StartupData::default();
    let mut seen = BTreeSet::new();
    let mut found: Vec<&DataObj> = Vec::new();
    for k in keys {
        match objs.get(k) {
            Some(list) => found.extend(list),
            None => d.missing.push(k.clone()),
        }
    }
    found.extend(operands);
    {
        for o in found {
            if !seen.insert((o.start, o.name.clone())) {
                continue;
            }
            let (map, unit) = if o.rw { (&mut d.pages, page) } else { (&mut d.windows, window) };
            // An object larger than the unit counts the units of its first `unit` bytes: start-up touches the head of a
            // large buffer (the heap's reserve), and counting all of it would hide a one-unit move behind its size.
            for u in window_of(o.start, unit)..=window_of(o.end.min(o.start + unit) - 1, unit) {
                map.entry(u).or_default().push(o.clone());
            }
        }
    }
    d
}

/// The verdict on `d`: `Ok(summary)` or `Err(lines)` listing every window or page of an over-budget class and naming,
/// as BROKEN, each object that sits alone in one.
fn startup_data_verdict(c: &Contract, d: &StartupData, (window, page): (u64, u64), (ro_budget, rw_budget): (u64, u64), profile: &str) -> Result<String, Vec<String>> {
    let (nro, nrw) = (d.windows.len() as u64, d.pages.len() as u64);
    let mut summary = format!(
        "{nro} read-only start-up data windows of {window} bytes (ro-budget-{profile} {ro_budget}), {nrw} written start-up data pages of {page} bytes (rw-budget-{profile} {rw_budget})"
    );
    if !d.missing.is_empty() {
        summary += &format!("; not in this link: {}", d.missing.join(" "));
    }
    let mut lines = vec![format!("layout: {}: {summary}", c.id)];
    let mut bad = Vec::new();
    for (what, units, unit, n, budget, class) in
        [("window", &d.windows, window, nro, ro_budget, "ro"), ("page", &d.pages, page, nrw, rw_budget, "rw")]
    {
        if n <= budget {
            continue;
        }
        let alone = bad.len();
        for (u, objs) in units {
            let names: Vec<String> = objs.iter().map(|o| format!("{} 0x{:x}-0x{:x}", o.name, o.start, o.end)).collect();
            lines.push(format!("layout: {}:   {what} 0x{:x}: {}", c.id, u * unit, names.join(", ")));
            if let [o] = objs.as_slice() {
                bad.push(c.broken(&o.name, &format!("sits alone in the {unit}-byte {what} 0x{:x}: {n} {what}s > {class}-budget-{profile} {budget}", u * unit)));
            }
        }
        if bad.len() == alone {
            bad.push(c.broken(&format!("{n} {what}s"), &format!("start-up {class} data spans {n} {what}s of {unit} bytes > {class}-budget-{profile} {budget}")));
        }
    }
    if bad.is_empty() {
        return Ok(summary);
    }
    lines.extend(bad);
    Err(lines)
}

fn check_startup_data(c: &Contract, p: &BTreeMap<String, String>, pr: &mut Products) -> Result<String, Vec<String>> {
    let one = |e: String| vec![e];
    let window = param_num(p, c, "window").map_err(one)?;
    same(c, "window", window, STARTUP_WINDOW).map_err(one)?;
    let page = param_num(p, c, "page").map_err(one)?;
    same(c, "page", page, STARTUP_PAGE).map_err(one)?;
    let profile = pr.profile.clone().ok_or_else(|| one(format!("layout: {}: the products' profile is unknown (no {}/.profile)", c.id, pr.dir.display())))?;
    let ro_budget = param_num(p, c, &format!("ro-budget-{profile}")).map_err(one)?;
    let rw_budget = param_num(p, c, &format!("rw-budget-{profile}")).map_err(one)?;
    let path = param(p, c, "keys").map_err(one)?;
    let text = std::fs::read_to_string(pr.root.join(path)).map_err(|e| one(format!("layout: {}: {path}: {e}", c.id)))?;
    let keys = startup_keys(&text, &profile).map_err(|e| one(format!("layout: {}: {path}: {e}", c.id)))?;
    if keys.is_empty() {
        return Err(one(format!("layout: {}: {path}: no start-up object traced in profile {profile} (run tools/hottext/startup_data.sh)", c.id)));
    }
    for f in &c.subjects {
        pr.file(f).map_err(|e| one(c.broken(f, &e)))?;
    }
    let tpath = param(p, c, "text-keys").map_err(one)?;
    let ttext = std::fs::read_to_string(pr.root.join(tpath)).map_err(|e| one(format!("layout: {}: {tpath}: {e}", c.id)))?;
    let fns: BTreeSet<String> = startup_keys(&ttext, &profile).map_err(|e| one(format!("layout: {}: {tpath}: {e}", c.id)))?.into_iter().collect();
    let probe = link_startup_probe(pr, true).map_err(|e| one(format!("layout: {}: {e}", c.id)))?;
    let secs = parse_exe_sections(&probe.sections);
    let objs = parse_nm_data(&probe.nm, &secs);
    let operands = target_objects(&operand_targets(&probe.disasm, &fns), &objs, &secs);
    let d = startup_data(&keys, &objs, &operands, window, page);
    if d.missing.len() == keys.len() {
        return Err(one(c.broken("start-up objects", &format!("none of the {} keys of {path} names an object of the probe", keys.len()))));
    }
    startup_data_verdict(c, &d, (window, page), (ro_budget, rw_budget), &profile).map(|s| format!("{s}; {} of {} start-up objects found", keys.len() - d.missing.len(), keys.len()))
}

/// The profile recorded in `<dir>/.profile` (written last by `cargo xtask so`).
fn marker_profile(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(".profile")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------------------------------------------
// Static checks

/// Where decisions live: the D-row tables, and the decision records (D-X.55: one TOML file per decision,
/// `records/decisions/<series>/<id>.toml`, from which the tables are generated). A change under either is a decision
/// change for rule d, and a row may name a decision found in either.
const DECISION_DIRS: &[&str] = &["plan/07-decisions/", "records/decisions/"];

/// The decision ids: the D-rows of `plan/07-decisions/*.md` (`| D-<section>.<n> |` at the start of a line) and the
/// file stems of `records/decisions/**/D-*.toml`. A missing directory adds nothing; neither present is an error.
fn decision_ids(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    let (tables, records) = (root.join(DECISION_DIRS[0]), root.join(DECISION_DIRS[1]));
    if !tables.is_dir() && !records.is_dir() {
        return Err(format!("layout: neither {} nor {} exists", tables.display(), records.display()));
    }
    for e in std::fs::read_dir(&tables).into_iter().flatten() {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.extension().is_some_and(|x| x == "md") {
            for l in std::fs::read_to_string(&p).map_err(|e| format!("layout: {}: {e}", p.display()))?.lines() {
                if let Some(id) = l.strip_prefix("| D-").and_then(|r| r.split_whitespace().next()) {
                    out.insert(format!("D-{id}"));
                }
            }
        }
    }
    let mut stack = vec![records];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten() {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "toml")
                && let Some(id) = p.file_stem().and_then(|s| s.to_str()).filter(|s| s.starts_with("D-"))
            {
                out.insert(id.to_string());
            }
        }
    }
    Ok(out)
}

/// Whether `changed` holds a decision change (a file under one of `DECISION_DIRS`).
fn decision_change(changed: &[String]) -> bool {
    changed.iter().any(|f| DECISION_DIRS.iter().any(|d| f.starts_with(d)))
}

/// The rows' own problems: kind known, D-row present.
pub fn row_errors(contracts: &[Contract], decisions: &BTreeSet<String>) -> Vec<String> {
    let mut e = Vec::new();
    for c in contracts {
        if !KINDS.contains(&c.kind.as_str()) {
            e.push(format!("{}: unknown kind `{}` (known: {})", c.id, c.kind, KINDS.join(", ")));
        }
        if !decisions.contains(&c.decision) {
            e.push(format!("{}: decision {} is not a row of plan/07-decisions/ or a record of records/decisions/", c.id, c.decision));
        }
    }
    e
}

fn is_comment(l: &str) -> bool {
    let t = l.trim_start();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')
}

/// A line of code (not a comment line) holding an alignment or offset directive.
fn is_directive(l: &str) -> bool {
    !is_comment(l) && DIRECTIVES.iter().any(|d| l.contains(d))
}

/// The `LAYOUT(<ID>)` tags of one source text: `(line, id, problem)`. A tag sits on a directive's line or in the
/// comment lines directly above it.
pub fn tags(text: &str) -> Vec<(usize, String, Option<String>)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let mut rest = *l;
        while let Some(at) = rest.find("LAYOUT(") {
            let tail = &rest[at + 7..];
            let Some(end) = tail.find(')') else { break };
            let id = &tail[..end];
            rest = &tail[end..];
            if id.is_empty() || !id.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-') {
                continue;
            }
            let on_line = is_directive(l);
            let below = lines[i + 1..].iter().find(|n| !is_comment(n)).copied().unwrap_or("");
            let ok = on_line || (is_comment(l) && is_directive(below));
            out.push((i + 1, id.to_string(), (!ok).then(|| "no alignment or offset directive on this line or directly below the comment (was it deleted?)".to_string())));
        }
    }
    out
}

fn source_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "target") {
                source_files(&p, out);
            }
        } else if p.extension().is_some_and(|x| ["rs", "S", "s", "c", "h"].iter().any(|k| x == *k)) {
            out.push(p);
        }
    }
}

/// Rule d: with `changed` (the branch's changed files) and `base` (the manifest at the merge base: `Some(rows)`,
/// empty when it did not exist; `None` when unknown, e.g. a role tree without history), a removed row or a changed
/// kind, subjects or rule fails unless a decision changed too (a file under `DECISION_DIRS`).
pub fn rule_d(changed: &[String], base: Option<&[Contract]>, now: &[Contract]) -> Vec<String> {
    if !changed.iter().any(|f| f == MANIFEST) || decision_change(changed) {
        return Vec::new();
    }
    let Some(base) = base else {
        return vec![format!("{MANIFEST} changed without a decision change (plan/07-decisions/ or records/decisions/) (no history here to tell an added row from a changed one)")];
    };
    let mut e = Vec::new();
    for b in base {
        match now.iter().find(|c| c.id == b.id) {
            None => e.push(format!("{}: row removed without a decision change (plan/07-decisions/ or records/decisions/) (removing a contract needs a decision)", b.id)),
            Some(c) if (&c.kind, &c.subjects, &c.rule) != (&b.kind, &b.subjects, &b.rule) => {
                e.push(format!("{}: kind, subjects or rule changed (`{}` -> `{}`) without a decision change (plan/07-decisions/ or records/decisions/)", b.id, b.rule, c.rule));
            }
            Some(_) => {}
        }
    }
    e
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The base the branch is compared with, as `affected` does: `origin/next` where it exists, else `origin/main`.
fn rule_d_inputs(root: &Path) -> Result<(Vec<String>, Option<Vec<Contract>>), String> {
    let base = if git(root, &["rev-parse", "--verify", "--quiet", "origin/next"]).is_some() { "origin/next" } else { "origin/main" };
    let changed = crate::affected::changed_files(root, base)?;
    if !changed.iter().any(|f| f == MANIFEST) {
        return Ok((changed, Some(Vec::new())));
    }
    let base_rows = git(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).and_then(|_| git(root, &["merge-base", "HEAD", base])).map(|mb| match git(root, &["show", &format!("{}:{MANIFEST}", mb.trim())]) {
        Some(t) => parse(&t).unwrap_or_default(),
        None => Vec::new(),
    });
    Ok((changed, base_rows))
}

/// `cargo xtask static` (and `layout-check --static`): the manifest, the D-rows, the site tags and rule d.
pub fn static_check(root: &Path) -> Result<(), String> {
    let contracts = load(root)?;
    let mut errors = row_errors(&contracts, &decision_ids(root)?);
    let ids: BTreeSet<&str> = contracts.iter().map(|c| c.id.as_str()).collect();
    let mut files = Vec::new();
    source_files(&root.join("crates"), &mut files);
    source_files(&root.join("csys"), &mut files);
    files.sort();
    let mut tagged: BTreeSet<String> = BTreeSet::new();
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        let rel = f.strip_prefix(root).unwrap_or(f).display().to_string();
        for (line, id, problem) in tags(&text) {
            if !ids.contains(id.as_str()) {
                errors.push(format!("{rel}:{line}: LAYOUT({id}) names no row of {MANIFEST}"));
            }
            if let Some(p) = problem {
                errors.push(format!("{rel}:{line}: LAYOUT({id}): {p}"));
            }
            tagged.insert(id);
        }
    }
    for c in contracts.iter().filter(|c| SITE_KINDS.contains(&c.kind.as_str()) && !tagged.contains(&c.id)) {
        errors.push(format!("{}: kind {} holds its layout with a source directive, but no LAYOUT({}) tag marks it", c.id, c.kind, c.id));
    }
    match rule_d_inputs(root) {
        Ok((changed, base)) => errors.extend(rule_d(&changed, base.as_deref(), &contracts)),
        Err(e) => println!("layout: rule d not evaluated (no changed-file list: {e})"),
    }
    if errors.is_empty() {
        println!("layout: {MANIFEST}: {} contracts, D-rows present, {} tagged in the source", contracts.len(), tagged.len());
        Ok(())
    } else {
        Err(format!("layout: {}", errors.join("\n  ")))
    }
}

/// `cargo xtask layout-check [--static | <products dir>]` (default `target/liella`).
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("--static") => static_check(root),
        Some(d) if d.starts_with('-') => Err(format!("layout-check: unknown option {d} (usage: layout-check [--static | <products dir>])")),
        Some(d) => check_products(root, Path::new(d), marker_profile(Path::new(d)).as_deref()),
        None => {
            let d = root.join("target/liella");
            check_products(root, &d, marker_profile(&d).as_deref())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, rule: &str) -> Contract {
        Contract { id: id.into(), kind: "object-align".into(), subjects: vec!["libc.a:X".into()], rule: rule.into(), reason: "r".into(), decision: "D-X.48".into() }
    }

    #[test]
    fn the_manifest_parses_and_names_known_kinds() {
        // no spec clause: tooling (D-X.48).
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let rows = load(root).unwrap();
        assert!(rows.len() >= 10);
        assert!(rows.iter().all(|c| KINDS.contains(&c.kind.as_str())), "{rows:?}");
        assert!(row_errors(&rows, &decision_ids(root).unwrap()).is_empty());
        assert!(parse("id\tkind\n").is_err(), "a wrong header is an error");
        assert!(parse("id\tkind\tsubjects\trule\treason\tdecision\nA\tfn-align\tx\ta=1\tr\n").is_err(), "five columns");
    }

    #[test]
    fn rows_name_existing_decisions_and_kinds() {
        // no spec clause: tooling (D-X.48, rule d).
        let d: BTreeSet<String> = ["D-X.48".to_string()].into();
        assert!(row_errors(&[row("A", "align=1")], &d).is_empty());
        let mut r = row("A", "align=1");
        r.decision = "D-4.999".into();
        r.kind = "nope".into();
        assert_eq!(row_errors(&[r], &d).len(), 2);
    }

    #[test]
    fn rule_d_needs_a_decision_for_a_changed_or_removed_row() {
        // no spec clause: tooling (D-X.48, rule d).
        let (a, b) = (row("A", "align=4096"), row("B", "align=64"));
        let m = vec![MANIFEST.to_string()];
        let md = vec![MANIFEST.to_string(), "plan/07-decisions/x.md".to_string()];
        let other = vec!["xtask/src/so.rs".to_string()];
        let base = [a.clone(), b.clone()];
        let loosened = [row("A", "align=2048"), b.clone()];
        assert!(rule_d(&m, Some(&base), &base).is_empty(), "unchanged rows");
        assert_eq!(rule_d(&m, Some(&base), &loosened).len(), 1, "a changed rule without a decision change");
        assert_eq!(rule_d(&m, Some(&base), std::slice::from_ref(&a)).len(), 1, "a removed row without a decision change");
        assert!(rule_d(&md, Some(&base), &loosened).is_empty(), "with a decision change");
        let rec = vec![MANIFEST.to_string(), "records/decisions/D-X/D-X.48.toml".to_string()];
        assert!(rule_d(&rec, Some(&base), &loosened).is_empty(), "with a decision record change (D-X.55)");
        let near = vec![MANIFEST.to_string(), "records/compat/COMPAT-1.toml".to_string()];
        assert_eq!(rule_d(&near, Some(&base), &loosened).len(), 1, "another record is not a decision change");
        assert!(rule_d(&other, Some(&base), &loosened).is_empty(), "the manifest did not change");
        assert!(rule_d(&m, Some(&base), &[a.clone(), b.clone(), row("C", "align=8")]).is_empty(), "an added row");
        assert!(rule_d(&m, Some(&[]), &base).is_empty(), "a new manifest");
        assert_eq!(rule_d(&m, None, &base).len(), 1, "unknown base: the change needs a decision change");
    }

    #[test]
    fn decisions_are_read_from_the_tables_and_the_records() {
        // no spec clause: tooling (D-X.48 rows name decisions; D-X.55 moved them to records/decisions/).
        let dir = std::env::temp_dir().join(format!("liella-layout-decisions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(decision_ids(&dir).is_err(), "neither place exists");
        std::fs::create_dir_all(dir.join("records/decisions/D-X")).unwrap();
        std::fs::write(dir.join("records/decisions/D-X/D-X.48.toml"), "id = \"D-X.48\"\n").unwrap();
        std::fs::write(dir.join("records/decisions/D-X/notes.txt"), "D-X.77\n").unwrap();
        assert_eq!(decision_ids(&dir).unwrap(), BTreeSet::from(["D-X.48".to_string()]), "records only");
        std::fs::create_dir_all(dir.join("plan/07-decisions")).unwrap();
        std::fs::write(dir.join("plan/07-decisions/x.md"), "| D-4.63 | row |\n| not a row\n").unwrap();
        assert_eq!(decision_ids(&dir).unwrap(), BTreeSet::from(["D-4.63".to_string(), "D-X.48".to_string()]), "both");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tags_sit_on_their_directive() {
        // no spec clause: tooling (D-X.48, A3 site tags).
        let s = "\t/* LAYOUT(MEMCHR-SSE2-PIN): fill\n\t * more */\n\t.org x + 0x231, 0xcc\n\t/* LAYOUT(GONE) */\n\tmovq %rax, %rax\n#[repr(C, align(4096))] // LAYOUT(SLOTS)\n/// see LAYOUT(<id>)\n";
        let t = tags(s);
        assert_eq!(t.len(), 3, "{t:?}");
        assert_eq!((t[0].1.as_str(), t[0].2.is_none()), ("MEMCHR-SSE2-PIN", true));
        assert_eq!((t[1].1.as_str(), t[1].2.is_some()), ("GONE", true), "the tagged directive was deleted");
        assert_eq!((t[2].1.as_str(), t[2].2.is_none()), ("SLOTS", true));
    }

    const READELF: &str = "\nFile: libc.a(csys_23.o)\nThere are 4 section headers, starting at offset 0x9298:\n\nSection Headers:\n  [Nr] Name              Type            Address          Off    Size   ES Flg Lk Inf Al\n  [ 0]                   NULL            0000000000000000 000000 000000 00      0   0  0\n  [27] .text.liella_machine_rawmemchr_entry PROGBITS        0000000000000000 001800 000636 00  AX  0   0 64\n  [28] .rodata1          PROGBITS        0000000000000000 3b7000 000200 00   A  0   0 512\n  [29] .lbss.T NOBITS          0000000000000000 42f000 001000 00  WA  0   0 4096\n\nSymbol table '.symtab' contains 3 entries:\n   Num:    Value          Size Type    Bind   Vis      Ndx Name\n   119: 0000000000000000     0 FUNC    WEAK   DEFAULT   27 liella_machine_rawmemchr_entry\n 13517: 0000000000000000   512 OBJECT  LOCAL  DEFAULT   28 _RN13startup_names13STARTUP_NAMES\n 13518: 0000000000000000  4096 OBJECT  LOCAL  DEFAULT   29 _RN4intl8RESOLVED\n     5: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  UND memcpy\n";

    #[test]
    fn readelf_index_reads_sections_and_symbols() {
        // no spec clause: tooling (D-X.48).
        let m = parse_readelf(READELF);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].name, "csys_23.o");
        assert_eq!(m[0].sections[&27].align, 64);
        assert_eq!((m[0].sections[&28].name.as_str(), m[0].sections[&28].size, m[0].sections[&28].align), (".rodata1", 512, 512));
        assert_eq!((m[0].sections[&29].ty.as_str(), m[0].sections[&29].size), ("NOBITS", 4096));
        assert_eq!(m[0].symbols.len(), 4);
        assert_eq!((m[0].symbols[1].size, m[0].symbols[1].ndx), (512, Some(28)));
        assert_eq!(m[0].symbols[3].ndx, None);
    }

    #[test]
    fn objdump_offsets_skip_padding_prefixes() {
        // no spec clause: tooling (D-X.48).
        let d = "\nIn archive libc.a:\n\nother.o:     file format elf64-x86-64\n\nDisassembly of section .text.x:\n\n 231:\t90                   \tnop\n\ncsys_23.o:     file format elf64-x86-64\n\n\nDisassembly of section .text.x:\n\n0000000000000000 <e>:\n 1ec:\tcc                   \tint3\n 1ed:\t2e 2e 66 0f 60 c0    \tcs cs punpcklbw %xmm0,%xmm0\n";
        let code = parse_objdump(d, "csys_23.o", ".text.x");
        assert_eq!(code.len(), 2);
        assert_eq!(bare_insn(&code[&0x1ed]), "punpcklbw %xmm0,%xmm0");
        assert_eq!(code.get(&0x231), None, "another member's instruction is not read");
    }

    /// A probe link: grouped hot text 0x401000-0x411200 (windows 0x40, 0x41), crt1's `_start` before it, and the
    /// libc.a function `cold_b` of `size` bytes at `at` in cold text.
    fn probe(at: u64, size: u64) -> (String, String) {
        let map = format!(
            "Linker script and memory map\n\n.text           0x0000000000400100   0x400000\n \
             .text          0x0000000000400100       0x40 /x/liella-crt1.o\n \
             .text.sorted.h00001.__libc_start_main\n                0x0000000000401000      0x200 /x/libc.a(start.o)\n \
             .text.sorted.h00002.foo 0x0000000000401200 0x10000 /x/libc.a(foo.o)\n \
             .text.empty    0x0000000000411200        0x0 /x/libc.a(e.o)\n \
             .text.cold_b   0x{at:016x} 0x{size:x} /x/libc.a(b.o)\n"
        );
        let nm = format!(
            "0000000000401000 0000000000000200 T __libc_start_main\n0000000000401200 0000000000010000 t foo\n\
             0000000000400100 T _start\n{at:016x} {size:016x} t cold_b\n0000000000600000 0000000000000010 D data_obj\n\
             0000000000400140 0000000000000008 T _ZN4core3fmt5write17h0123456789abcdefE\n"
        );
        (nm, map)
    }

    fn startup_row(rule: &str) -> Contract {
        Contract { kind: "startup-text-windows".into(), subjects: vec!["libc.a".into(), "liella-crt1.o".into()], decision: "D-X.49".into(), ..row("STARTUP-TEXT-WINDOWS", rule) }
    }

    fn keys() -> Vec<String> {
        ["_start", "__libc_start_main", "foo", "cold_b", "absent"].map(String::from).to_vec()
    }

    #[test]
    fn startup_nm_and_map_are_read_by_key_and_section() {
        // no spec clause: tooling (D-X.49).
        let (nm, map) = probe(0x6f9680, 0x1476);
        let f = parse_nm(&nm);
        assert_eq!(f["_start"], vec![Func { start: 0x400100, end: 0x400101, name: "_start".into() }], "a size-less entry counts its entry byte");
        assert!(f.contains_key("_ZN4core3fmt5writeE"), "keys are the build-stable keys of hot-text.list");
        assert!(!f.contains_key("data_obj"), "objects are not text");
        let s = parse_map(&map);
        let names: Vec<&str> = s.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, [".text", ".text.sorted.h00001.__libc_start_main", ".text.sorted.h00002.foo", ".text.cold_b"], "a wrapped name reads the next line; empty sections are dropped");
        assert_eq!((s[1].start, s[1].end, s[1].file.as_str()), (0x401000, 0x401200, "/x/libc.a(start.o)"));
        assert!(is_hot_section(".text.sorted.h00002.foo") && is_hot_section(".text.hot.foo"));
        assert!(!is_hot_section(".text.sorted.foo") && !is_hot_section(".text.cold_b"));
        assert_eq!(startup_keys("# c\na\tstartup\tfast,release\nb\tstartup\trelease\n", "fast").unwrap(), ["a"]);
        assert!(startup_keys("a\n", "fast").is_err(), "a line without tier and profiles is an error");
    }

    #[test]
    fn startup_windows_hold_within_the_budget_and_fail_one_below() {
        // no spec clause: tooling (D-X.49); the origin/next case of finding 1 (cold function in one window).
        let (nm, map) = probe(0x6f9680, 0x1476);
        let t = startup_text(&keys(), &parse_nm(&nm), &parse_map(&map), STARTUP_WINDOW).unwrap();
        assert_eq!(t.windows.keys().copied().collect::<Vec<_>>(), [0x40, 0x41, 0x6f]);
        assert_eq!(t.missing, ["absent"]);
        assert!(t.straddling.is_empty());
        let names: Vec<&str> = t.ungrouped.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["cold_b"], "crt1's _start is not a libc.a function");
        let c = startup_row("window=65536; budget-fast=3; grouped=warn; keys=k");
        let ok = startup_verdict(&c, &t, STARTUP_WINDOW, 3, false, "fast").unwrap();
        assert!(ok.contains("3 start-up text windows") && ok.contains("outside it: 0x6f0000"), "{ok}");
        assert!(ok.contains("WARNING STARTUP-TEXT-GROUPED") && ok.contains("cold_b"), "{ok}");
        let e = startup_verdict(&c, &t, STARTUP_WINDOW, 2, false, "fast").unwrap_err().join("\n");
        assert!(e.contains("STARTUP-TEXT-WINDOWS BROKEN: 3 windows") && e.contains("> budget-fast 2"), "{e}");
        assert!(e.contains("window 0x6f0000 (outside the hot group): cold_b 0x6f9680-0x6faaf6"), "{e}");
        let g = startup_verdict(&c, &t, STARTUP_WINDOW, 3, true, "fast").unwrap_err().join("\n");
        assert!(g.contains("BROKEN: STARTUP-TEXT-GROUPED: 1 start-up functions of libc.a outside the hot group") && g.contains("cold_b (0x6f9680)"), "{g}");
    }

    #[test]
    fn a_cold_startup_function_across_a_window_boundary_fails() {
        // no spec clause: tooling (D-X.49); the LIBM-F64-LOG-FAMILY-WIRE-2 case of finding 1.
        let (nm, map) = probe(0x6ef200, 0x1476);
        let t = startup_text(&keys(), &parse_nm(&nm), &parse_map(&map), STARTUP_WINDOW).unwrap();
        assert_eq!(t.windows.keys().copied().collect::<Vec<_>>(), [0x40, 0x41, 0x6e, 0x6f]);
        assert_eq!(t.straddling.len(), 1);
        let c = startup_row("window=65536; budget-fast=9; grouped=warn; keys=k");
        let e = startup_verdict(&c, &t, STARTUP_WINDOW, 9, false, "fast").unwrap_err().join("\n");
        assert!(e.contains("BROKEN: cold_b 0x6ef200-0x6f0676: outside the hot group, straddles the 65536-byte window boundary 0x6f0000"), "{e}");
        assert!(!e.contains("BROKEN: 4 windows"), "within the budget, only the straddle fails: {e}");
        // A function inside the group may cross a boundary: the group's windows are counted anyway.
        let (nm, map) = probe(0x6f9680, 0x1476);
        let t = startup_text(&keys(), &parse_nm(&nm), &parse_map(&map), STARTUP_WINDOW).unwrap();
        assert!(t.windows[&0x40].iter().any(|f| f.name == "foo") && t.windows[&0x41].iter().any(|f| f.name == "foo") && t.straddling.is_empty());
        assert!(startup_text(&keys(), &parse_nm(&nm), &[], STARTUP_WINDOW).is_err(), "no hot group: cannot check");
    }

    /// `readelf -SW` of a probe: text, read-only data, TLS, writable data and bss, and a non-allocated section.
    const DATA_SECTIONS: &str = "  [Nr] Name              Type            Address          Off    Size   ES Flg Lk Inf Al
  [ 1] .text             PROGBITS        0000000000401000 001000 0f0000 00  AX  0   0 64
  [ 2] .rodata           PROGBITS        0000000000500000 100000 020000 00   A  0   0 64
  [ 3] .rodata1          PROGBITS        0000000000520000 120000 000200 00   A  0   0 64
  [ 4] .tbss             NOBITS          0000000000600000 200000 000100 00 WAT  0   0 64
  [ 5] .data             PROGBITS        0000000000600000 200000 003000 00  WA  0   0 64
  [ 6] .bss              NOBITS          0000000000603000 203000 002000 00  WA  0   0 64
  [ 7] .comment          PROGBITS        0000000000000000 203000 000020 01  MS  0   0  1
";

    fn data_nm(nscache_at: u64) -> String {
        [
            "0000000000401000 0000000000000010 T _start".to_string(),
            "0000000000500100 0000000000000040 r _ZN4core3fmt3FMT17h0123456789abcdefE".into(),
            "0000000000520000 0000000000000200 R _ZN15liella_boundary13startup_names13STARTUP_NAMES17h0123456789abcdefE".into(),
            "0000000000000008 0000000000000008 B TLS_WORD".into(),
            "0000000000600040 0000000000000010 D AUXV".into(),
            format!("{nscache_at:016x} 0000000000000200 d NSCACHE"),
            "0000000000603100 0000000000000020 b HEAP".into(),
        ]
        .join("\n")
    }

    fn data_keys() -> Vec<String> {
        ["_ZN4core3fmt3FMTE", "_ZN15liella_boundary13startup_names13STARTUP_NAMESE", "TLS_WORD", "AUXV", "NSCACHE", "HEAP", "_start", "gone"].map(String::from).to_vec()
    }

    fn data_row() -> Contract {
        let rule = "keys=k; text-keys=t; window=65536; page=4096; ro-budget-fast=2; rw-budget-fast=2";
        Contract { kind: "startup-data-windows".into(), subjects: vec!["libc.a".into(), "liella-crt1.o".into()], decision: "D-X.50".into(), ..row("STARTUP-DATA-WINDOWS", rule) }
    }

    #[test]
    fn startup_data_objects_are_classed_by_their_section() {
        // no spec clause: tooling (D-X.50).
        let secs = parse_exe_sections(DATA_SECTIONS);
        assert_eq!(secs.len(), 5, "neither the non-allocated .comment nor TLS is read: {secs:?}");
        assert!(secs[0].skip && !secs[1].writable && secs[3].writable && !secs[3].skip && secs[3].name == ".data");
        let o = parse_nm_data(&data_nm(0x601f00), &secs);
        assert!(!o.contains_key("_start"), "text is not data");
        assert!(!o.contains_key("TLS_WORD"), "a TLS object (its value is a TLS-block offset) lies in no section and is skipped");
        assert!(!o["_ZN4core3fmt3FMTE"][0].rw && o["NSCACHE"][0].rw && o["HEAP"][0].rw, "keys are hottext keys; .data and .bss are written");
        let d = startup_data(&data_keys(), &o, &[], STARTUP_WINDOW, STARTUP_PAGE);
        assert_eq!(d.windows.keys().copied().collect::<Vec<_>>(), [0x50, 0x52]);
        assert_eq!(d.pages.keys().copied().collect::<Vec<_>>(), [0x600, 0x601, 0x602, 0x603], "NSCACHE across a page boundary counts both");
        assert_eq!(d.missing, ["TLS_WORD", "_start", "gone"]);
        let big = parse_nm_data("0000000000600800 0000000000010000 b RESERVE", &secs);
        let d = startup_data(&["RESERVE".to_string()], &big, &[], STARTUP_WINDOW, STARTUP_PAGE);
        assert_eq!(d.pages.keys().copied().collect::<Vec<_>>(), [0x600, 0x601], "a large object counts the pages of its first 4 KiB");
    }

    #[test]
    fn startup_data_holds_within_the_budget_and_names_the_lone_object_one_below() {
        // no spec clause: tooling (D-X.50); the UTMP-Y2038-UNSIGNED-2 case (a written object alone on a page).
        let secs = parse_exe_sections(DATA_SECTIONS);
        let c = data_row();
        let packed = startup_data(&data_keys(), &parse_nm_data(&data_nm(0x600100), &secs), &[], STARTUP_WINDOW, STARTUP_PAGE);
        assert_eq!(packed.pages.keys().copied().collect::<Vec<_>>(), [0x600, 0x603]);
        let ok = startup_data_verdict(&c, &packed, (STARTUP_WINDOW, STARTUP_PAGE), (2, 2), "fast").unwrap();
        assert!(ok.contains("2 read-only start-up data windows") && ok.contains("2 written start-up data pages"), "{ok}");
        let alone = startup_data(&data_keys(), &parse_nm_data(&data_nm(0x601800), &secs), &[], STARTUP_WINDOW, STARTUP_PAGE);
        let e = startup_data_verdict(&c, &alone, (STARTUP_WINDOW, STARTUP_PAGE), (2, 2), "fast").unwrap_err().join("\n");
        assert!(e.contains("STARTUP-DATA-WINDOWS BROKEN: NSCACHE: sits alone in the 4096-byte page 0x601000: 3 pages > rw-budget-fast 2"), "{e}");
        assert!(e.contains("BROKEN: HEAP: sits alone"), "{e}");
        assert!(e.contains("BROKEN: AUXV: sits alone"), "every lone object of the over-budget class is named: {e}");
        assert!(e.contains("page 0x601000: NSCACHE 0x601800-0x601a00"), "{e}");
        let w = startup_data_verdict(&c, &packed, (STARTUP_WINDOW, STARTUP_PAGE), (1, 2), "fast").unwrap_err().join("\n");
        assert!(w.contains("BROKEN: _ZN15liella_boundary13startup_names13STARTUP_NAMES17h0123456789abcdefE: sits alone in the 65536-byte window 0x520000: 2 windows > ro-budget-fast 1"), "{w}");
        assert!(!w.contains("pages >"), "the rw class is within its budget: {w}");
    }

    #[test]
    fn startup_function_operands_count_unnamed_and_unkeyed_objects() {
        // no spec clause: tooling (D-X.50); a start-up name moved out of STARTUP_NAMES is an unnamed literal no key names.
        let secs = parse_exe_sections(DATA_SECTIONS);
        let objs = parse_nm_data(&data_nm(0x600100), &secs);
        let disasm = "0000000000401000 <_start>:\n  401000:\tlea    0x1234(%rip),%rdi        # 50f000 <x>\n\n\
                      0000000000401100 <other>:\n  401100:\tlea    0x1(%rip),%rdi        # 510000 <y>\n\n\
                      0000000000401200 <_ZN4core3fmt5write17h0123456789abcdefE>:\n  401200:\tmov    0x2(%rip),%rax        # 600048 <AUXV+0x8>\n\
                        401208:\tmov    0x3(%rip),%rax        # 402000 <_start+0x1000>\n";
        let fns: BTreeSet<String> = ["_start", "_ZN4core3fmt5writeE"].map(String::from).into();
        let t = operand_targets(disasm, &fns);
        assert_eq!(t, [(0x50f000, "_start".to_string()), (0x600048, "_ZN4core3fmt5write17h0123456789abcdefE".to_string()), (0x402000, "_ZN4core3fmt5write17h0123456789abcdefE".to_string())], "functions not in the set are not read");
        let o = target_objects(&t, &objs, &secs);
        assert_eq!(o.len(), 2, "a text target is not data: {o:?}");
        assert_eq!((o[0].start, o[0].name.as_str(), o[0].rw), (0x50f000, "<unnamed .rodata byte addressed by _start>", false));
        assert_eq!((o[1].name.as_str(), o[1].rw), ("AUXV", true));
        let d = startup_data(&["HEAP".to_string()], &objs, &o, STARTUP_WINDOW, STARTUP_PAGE);
        assert_eq!(d.windows.keys().copied().collect::<Vec<_>>(), [0x50]);
        assert_eq!(d.pages.keys().copied().collect::<Vec<_>>(), [0x600, 0x603]);
    }
}
