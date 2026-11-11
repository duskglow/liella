//! `cargo xtask partial-write`: no libm export's hot path writes part of an `xmm` register whose old value it does not
//! own (D-10.68, LIBM-PARTIAL-WRITE-CHECK).
//!
//! # The hazard
//!
//! Intel SDM vol. 2 (the instruction pages) gives the legacy-SSE scalar forms of several instructions a MERGE result:
//! they write the low element and leave the rest of the destination unmodified (`DEST[MAXVL-1:32/64]` "unmodified"),
//! so the result depends on the destination's previous value. For the two-operand arithmetic forms (`addsd`,
//! `mulss`, ...) the destination is an input anyway; for the forms whose low element does NOT depend on the
//! destination, the dependency is false but the hardware still waits for the old value's producer. The Intel 64 and
//! IA-32 Optimization Reference Manual (partial-register and "dependency-breaking idiom" sections) names the cure:
//! write the whole register first (a zero idiom such as `xorps`/`pxor` of a register with itself, which the renamer
//! completes without waiting, or any full-width write), or use the VEX form, whose upper element comes from an
//! explicit first source (VEX.vvvv) and whose bits above 127 are zeroed. A libm entry that merges into a register it
//! has not written chains every call to the previous call's last write of that register: back-to-back calls become
//! one latency chain (D-10.67 measured this on expf: 3.11x -> 1.66x the reference's time on the N100 class).
//!
//! The merge set checked (legacy SSE; the low element is computed from the sources only):
//! * conversions `cvtss2sd`, `cvtsd2ss`, `cvtsi2sd`, `cvtsi2ss` (register and memory sources, `l`/`q` forms),
//!   `cvtpi2ps` (writes the low 64 bits);
//! * unary arithmetic `sqrtss`, `sqrtsd`, `rcpss`, `rsqrtss`, `roundss`, `roundsd`;
//! * register-to-register `movss`/`movsd` (the memory-source forms zero the rest and are full writes);
//! * half-register moves `movlps`, `movlpd`, `movhps`, `movhpd` (load forms), `movhlps`, `movlhps`;
//! * element inserts `pinsrb`, `pinsrw`, `pinsrd`, `pinsrq`, `insertps`.
//!
//! Each of these is CHECKED (not forbidden): it is fine when its destination is "owned", i.e. written earlier on
//! the same path (by anything: once the register holds a value this call made, the merge depends on this call only),
//! an argument register of the export, or one of the instruction's own sources (`sqrtsd %xmm0,%xmm0`: the old value
//! is a true input; `cvtss2sd %xmm3,%xmm3`, the boundary's in-place widening (D-10.79), passes even when `%xmm3` is not
//! owned, since the merge then depends on the operand only, while `cvtss2sd %xmm3,%xmm4` into an unowned `%xmm4` is
//! reported). The VEX forms (`vcvtss2sd a,b,c`, `vmovsd a,b,c`, `vpinsrd i,r,b,c`, ...) merge from their
//! first source `b` instead of the destination; they are checked the same way on `b` (fine when `b` is owned, which
//! the usual `vcvtss2sd %xmm0,%xmm0,%xmm1` is). Any other instruction that names an `xmm`/`ymm`/`zmm` register last
//! (AT&T: the destination) makes that register owned; compares (`ucomis*`, `comis*`, `ptest`, `vtestp*`) write no
//! register.
//!
//! # Scope
//!
//! The hot path of every function `libm.so.6` exports: all paths from the entry, following conditional and direct
//! jumps (both outcomes: the FMA and SSE2 builds are branches of one export), up to the first `call`, return or
//! indirect jump. A merge after a call is not on a chained path (the call itself serialises far more than a merge)
//! and is not reported. The state at a join is the intersection (a register is owned only when every path into the
//! join owns it). Argument registers come from the export's Rust signature (`f32`, `f64`, `Complex<f32>`: one;
//! `Complex<f64>`: two; integer, pointer and x87 types: none; any other type: all eight, which can only hide a
//! finding, never invent one).
//!
//! Readers: `nm -D --defined-only` (the exports) and `objdump -d -w --no-show-raw-insn` (the code). `cargo xtask so`
//! runs the check on the `libm.so.6` it builds (a finding fails the build) and records the digest of the sources beside
//! it; `cargo xtask static` runs it on `target/liella/libm.so.6` when that digest matches the current tree and prints
//! why it skipped otherwise (a product left by another tree is not this tree's); `cargo xtask partial-write
//! [<libm.so.6>...]` checks the files named and prints every finding as `function+offset: instruction (why) [entry]`.
//! [`ALLOW`] lists accepted findings, each with its reason.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Accepted findings: `(containing function, instruction text as printed, reason)`. A ratchet: the findings present
/// when the check landed, each to be removed by the story that fixes it (a fix that changes the register or the
/// instruction makes the entry dead; a NEW finding fails). Anything else on the product is a failure.
/// Empty since LIBM-F32-ONEPASS-REST-3 (D-10.72): the last five findings (sinf, cosf, tanf, asinhf, acoshf) build
/// their `double` operand from the bits.
pub(crate) const ALLOW: &[(&str, &str, &str)] = &[];

/// Legacy-SSE mnemonics whose low element is computed from the sources only while the rest of the destination is
/// kept (the merge set of the module doc). `movss`/`movsd` and the half-register loads merge only with a register
/// (for `movss`/`movsd`) or memory (for `movlps` ...) source as noted in [`merges`].
const MERGE: &[&str] = &["cvtss2sd", "cvtsd2ss", "cvtsi2sd", "cvtsi2ss", "cvtpi2ps", "sqrtss", "sqrtsd", "rcpss", "rsqrtss", "roundss", "roundsd", "movss", "movsd", "movlps", "movlpd", "movhps", "movhpd", "movhlps", "movlhps", "pinsrb", "pinsrw", "pinsrd", "pinsrq", "insertps"];

/// Instructions that name a vector register last but write none.
const NO_WRITE: &[&str] = &["ucomiss", "ucomisd", "comiss", "comisd", "ptest", "vucomiss", "vucomisd", "vcomiss", "vcomisd", "vptest", "vtestps", "vtestpd"];

/// One decoded instruction.
#[derive(Debug, Clone)]
pub(crate) struct Insn {
    pub mnem: String,
    pub ops: Vec<String>,
    pub text: String,
}

/// A finding: the instruction, the function holding it, and the export whose hot path reaches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub func: String,
    pub off: u64,
    pub text: String,
    pub why: String,
    pub entry: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}+{:#x}: {} ({}) [hot path of {}]", self.func, self.off, self.text, self.why, self.entry)
    }
}

/// The mnemonic with objdump's operand-size suffix removed where it names a merge form (`cvtsi2sdl` -> `cvtsi2sd`).
fn base_mnem(m: &str) -> &str {
    for b in ["cvtsi2sd", "cvtsi2ss", "vcvtsi2sd", "vcvtsi2ss"] {
        if m.len() == b.len() + 1 && m.starts_with(b) && (m.ends_with('l') || m.ends_with('q')) {
            return &m[..b.len()];
        }
    }
    m
}

/// Operands split on commas outside parentheses.
fn split_ops(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0i32);
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// The vector register number of `%xmmN` / `%ymmN` / `%zmmN`.
fn vreg(op: &str) -> Option<u32> {
    let r = op.strip_prefix('%')?;
    let n = r.strip_prefix("xmm").or_else(|| r.strip_prefix("ymm")).or_else(|| r.strip_prefix("zmm"))?;
    n.parse().ok().filter(|n| *n < 32)
}

/// `objdump -d -w --no-show-raw-insn` text: the instructions by address and the symbol headers (`<name>:`).
pub(crate) fn parse_objdump(text: &str) -> (BTreeMap<u64, Insn>, BTreeMap<u64, String>) {
    let (mut insns, mut syms) = (BTreeMap::new(), BTreeMap::new());
    for line in text.lines() {
        if let Some((a, rest)) = line.split_once(" <") {
            if let (Ok(addr), Some(name)) = (u64::from_str_radix(a.trim(), 16), rest.strip_suffix(">:")) {
                syms.insert(addr, name.to_string());
                continue;
            }
        }
        let Some((a, rest)) = line.split_once(":\t") else { continue };
        let Ok(addr) = u64::from_str_radix(a.trim(), 16) else { continue };
        let body = rest.split('#').next().unwrap_or("").trim();
        let (mnem, ops) = crate::jcc::split_insn(body);
        if mnem.is_empty() || mnem.starts_with('(') {
            continue;
        }
        let ops = split_ops(ops.split(" <").next().unwrap_or("").trim());
        let text = format!("{mnem} {}", ops.join(",")).trim_end().to_string();
        insns.insert(addr, Insn { mnem, ops, text });
    }
    (insns, syms)
}

/// Whether `i` merges into a register, and the register whose old value the merge reads: the destination for the
/// legacy forms, the first source for the VEX forms.
fn merges(i: &Insn) -> Option<u32> {
    let m = base_mnem(&i.mnem);
    let (vex, b) = match m.strip_prefix('v') {
        Some(b) if MERGE.contains(&b) => (true, b),
        _ if MERGE.contains(&m) => (false, m),
        _ => return None,
    };
    // A store form (`movlps %xmm0,(mem)`) names no vector register last: no merge.
    let dest = vreg(i.ops.last()?)?;
    let n = i.ops.len();
    match b {
        // Memory-source movss/movsd zero the rest: full writes. Register-source: merge.
        "movss" | "movsd" if vreg(&i.ops[0]).is_none() => None,
        // VEX: `op src2, src1, dest` (an immediate may come first); a two-operand VEX form writes the full width.
        _ if vex => (n >= 3).then(|| vreg(&i.ops[n - 2])).flatten(),
        _ => Some(dest),
    }
}

/// How many of `i`'s source operands (every operand but the last) are vector register `r`.
fn uses(i: &Insn, r: u32) -> usize {
    i.ops.iter().take(i.ops.len().saturating_sub(1)).filter(|o| vreg(o) == Some(r)).count()
}

/// The vector register `i` writes, if any.
fn writes(i: &Insn) -> Option<u32> {
    if NO_WRITE.contains(&i.mnem.as_str()) {
        return None;
    }
    vreg(i.ops.last()?)
}

/// The direct target of a jump, from objdump's `jne 1a2c8 <...>` operand.
fn target(i: &Insn) -> Option<u64> {
    i.ops.first().and_then(|o| u64::from_str_radix(o.trim(), 16).ok())
}

/// Every hot-path merge into an unowned register reachable from `entry` (address) with `owned` argument registers.
pub(crate) fn walk(insns: &BTreeMap<u64, Insn>, entry: u64, owned: u32) -> Vec<(u64, String)> {
    let mut state: BTreeMap<u64, u32> = BTreeMap::new();
    let mut work = vec![(entry, owned)];
    let mut found: BTreeMap<u64, String> = BTreeMap::new();
    let mut steps = 0usize;
    while let Some((start, mut own)) = work.pop() {
        let mut at = start;
        loop {
            steps += 1;
            if steps > 200_000 {
                return vec![(entry, "walk did not converge (200000 steps)".into())];
            }
            match state.get(&at) {
                Some(&s) if s & own == s => break, // nothing new: every register owned here was owned before
                Some(&s) => own &= s,
                None => {}
            }
            state.insert(at, own);
            let Some(i) = insns.get(&at) else { break };
            if let Some(r) = merges(i) {
                // The old value is a true input when the instruction also reads it as a data source: the legacy
                // destination named as a source, or the VEX merge source named a second time.
                let vex = i.mnem.starts_with('v');
                let true_input = uses(i, r) > usize::from(vex);
                if own & (1 << r) == 0 && !true_input {
                    let why = if !vex {
                        format!("merges into %xmm{r}, not written earlier on this path: waits for its previous writer")
                    } else {
                        format!("VEX merge source %xmm{r} not written earlier on this path: waits for its previous writer")
                    };
                    found.entry(at).or_insert(why);
                }
            }
            if let Some(w) = writes(i) {
                own |= 1 << w;
            }
            let m = i.mnem.as_str();
            if m.starts_with("call") || m.starts_with("ret") || m.starts_with("hlt") || m == "ud2" {
                break;
            }
            if m.starts_with('j') {
                let Some(t) = target(i) else { break }; // indirect: ends the path
                if m == "jmp" {
                    at = t;
                    continue;
                }
                work.push((t, own));
            }
            let Some((&next, _)) = insns.range(at + 1..).next() else { break };
            at = next;
        }
    }
    found.into_iter().collect()
}

/// The xmm argument registers of each `pub extern "C" fn` in `crates/boundary/src` (bit mask; unknown types: all 8).
fn arg_masks(root: &Path) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    let mut files = Vec::new();
    collect_rs(&root.join("crates/boundary/src"), &mut files);
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let mut rest = text.as_str();
        while let Some(p) = rest.find("pub extern \"C\" fn ") {
            rest = &rest[p + 18..];
            let Some(open) = rest.find('(') else { break };
            let name = rest[..open].trim().to_string();
            let mut depth = 0i32;
            let mut end = open;
            for (k, c) in rest[open..].char_indices() {
                match c {
                    '(' | '<' => depth += 1,
                    ')' | '>' => depth -= 1,
                    _ => {}
                }
                if depth == 0 {
                    end = open + k;
                    break;
                }
            }
            out.insert(name, params_mask(&rest[open + 1..end]));
        }
    }
    out
}

/// The xmm argument mask of a Rust parameter list (psABI classification of the types the libm exports use).
fn params_mask(params: &str) -> u32 {
    let mut n = 0u32;
    let mut depth = 0i32;
    let mut cur = String::new();
    let mut tys = Vec::new();
    for c in params.chars() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => depth -= 1,
            ',' if depth == 0 => {
                tys.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    tys.push(cur);
    for t in tys.iter().filter_map(|p| p.split_once(':').map(|(_, t)| t.trim().to_string())).filter(|t| !t.is_empty()) {
        n += match t.as_str() {
            "f32" | "f64" | "Complex<f32>" => 1,
            "Complex<f64>" => 2,
            "LdBits" | "Complex<LdBits>" => 0,
            t if t.starts_with("c_") || t.starts_with('*') || t.starts_with("Option<&") || t.starts_with('&') || ["i32", "u32", "i64", "u64", "isize", "usize"].contains(&t) => 0,
            _ => return 0xff,
        };
    }
    (1u32 << n.min(8)) - 1
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut ents: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    ents.sort();
    for p in ents {
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

fn tool_output(cmd: &mut Command) -> Result<String, String> {
    let o = cmd.output().map_err(|e| format!("{cmd:?}: {e}"))?;
    if !o.status.success() {
        return Err(format!("{cmd:?}: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Every finding in `so` (a `libm.so.6`), allow-listed ones included, and the number of exports walked.
pub(crate) fn findings(root: &Path, so: &Path) -> Result<(Vec<Finding>, usize), String> {
    let dis = tool_output(Command::new("objdump").args(["-d", "-w", "--no-show-raw-insn"]).arg(so))?;
    let nm = tool_output(Command::new("nm").args(["-D", "--defined-only"]).arg(so))?;
    let (insns, syms) = parse_objdump(&dis);
    let masks = arg_masks(root);
    // Exports by address (aliases share one walk; the mask is the union of the known signatures' masks, so an
    // alias with an unknown signature can only hide a finding).
    let mut entries: BTreeMap<u64, (String, u32)> = BTreeMap::new();
    for line in nm.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let [a, ty, name] = f[..] else { continue };
        if !matches!(ty, "T" | "W" | "i") {
            continue;
        }
        let Ok(addr) = u64::from_str_radix(a, 16) else { continue };
        let name = name.split('@').next().unwrap_or(name).to_string();
        let mask = masks.get(&name).copied().unwrap_or(0xff);
        entries.entry(addr).and_modify(|(n, m)| {
            *m |= mask;
            if name < *n {
                *n = name.clone();
            }
        }).or_insert((name, mask));
    }
    let mut out: BTreeMap<u64, Finding> = BTreeMap::new();
    for (&addr, (name, mask)) in &entries {
        for (at, why) in walk(&insns, addr, *mask) {
            let (fa, func) = syms.range(..=at).next_back().map(|(a, n)| (*a, n.clone())).unwrap_or((0, "?".into()));
            let text = insns.get(&at).map(|i| i.text.clone()).unwrap_or_default();
            out.entry(at).or_insert(Finding { func, off: at - fa, text, why, entry: name.clone() });
        }
    }
    Ok((out.into_values().collect(), entries.len()))
}

fn allowed(f: &Finding) -> Option<&'static str> {
    ALLOW.iter().find(|(func, text, _)| *func == f.func && *text == f.text).map(|(_, _, why)| *why)
}

/// Check one `libm.so.6`: print every finding, fail on any that [`ALLOW`] does not list.
pub(crate) fn check_file(root: &Path, so: &Path) -> Result<(), String> {
    let (all, n) = findings(root, so)?;
    let mut bad = Vec::new();
    for f in &all {
        match allowed(f) {
            Some(why) => println!("partial-write: allowed: {f}: {why}"),
            None => bad.push(f.to_string()),
        }
    }
    println!("partial-write: {}: {n} exports walked, {} findings ({} allowed)", so.display(), all.len(), all.len() - bad.len());
    if bad.is_empty() { Ok(()) } else { Err(format!("{} partial-register merge(s) on libm hot paths (D-10.68):\n  {}", bad.len(), bad.join("\n  "))) }
}

/// The file beside a `libm.so.6` that `cargo xtask so` built and checked: [`source_digest`] of the tree it was built
/// from.
fn stamp_path(so: &Path) -> PathBuf {
    so.with_file_name("libm.so.6.source-digest")
}

/// The inputs of the product build, by path and contents (64-bit FNV-1a, stable across xtask builds): every file under
/// `crates/` and `csys/` (no `target/` directory) and the workspace's `Cargo.toml`, `Cargo.lock`,
/// `rust-toolchain.toml` and `.cargo/config.toml`. Equal digests mean `libm.so.6` came from these sources.
pub(crate) fn source_digest(root: &Path) -> String {
    fn files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        v.sort();
        for p in v {
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "target") {
                    files(&p, out);
                }
            } else {
                out.push(p);
            }
        }
    }
    let mut all = Vec::new();
    files(&root.join("crates"), &mut all);
    files(&root.join("csys"), &mut all);
    all.extend(["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml"].iter().map(|f| root.join(f)).filter(|p| p.is_file()));
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for p in &all {
        let data = std::fs::read(p).unwrap_or_default();
        eat(p.strip_prefix(root).unwrap_or(p).to_string_lossy().as_bytes());
        eat(&(data.len() as u64).to_le_bytes());
        eat(&data);
    }
    format!("{h:016x} {} files", all.len())
}

/// `cargo xtask so`: forget the digest before checking a new `libm.so.6`, record it once the check passed.
pub(crate) fn check_built(root: &Path, so: &Path) -> Result<(), String> {
    let stamp = stamp_path(so);
    let _ = std::fs::remove_file(&stamp);
    check_file(root, so)?;
    std::fs::write(&stamp, source_digest(root)).map_err(|e| format!("{}: {e}", stamp.display()))
}

/// `cargo xtask static`: check `target/liella/libm.so.6` when it was built from this tree (its digest, written by
/// `cargo xtask so`, matches). A product left by an earlier tree (a role or CI directory reused across branches) says
/// nothing about this one's [`ALLOW`] list: it is skipped, and `cargo xtask so` builds and checks the current one.
pub fn static_check(root: &Path) -> Result<(), String> {
    let so = root.join("target/liella/libm.so.6");
    if !so.exists() {
        println!("partial-write: skipped: {} not built (`cargo xtask so` builds and checks it)", so.display());
        return Ok(());
    }
    let built = std::fs::read_to_string(stamp_path(&so)).unwrap_or_default();
    let now = source_digest(root);
    if built != now {
        let built = if built.is_empty() { "no digest" } else { built.as_str() };
        println!("partial-write: skipped: {} was not built from this tree (built: {built}; tree: {now}; `cargo xtask so` builds and checks it)", so.display());
        return Ok(());
    }
    check_file(root, &so)
}

/// `cargo xtask partial-write [<libm.so.6>...]`.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return check_file(root, &root.join("target/liella/libm.so.6"));
    }
    let errs: Vec<String> = args.iter().filter_map(|a| check_file(root, Path::new(a)).err()).collect();
    if errs.is_empty() { Ok(()) } else { Err(errs.join("\n")) }
}

#[cfg(test)]
mod tests {
    //! no spec clause: the checker's rule (D-10.68) on synthetic objdump text.
    use super::*;

    fn dis(lines: &[&str]) -> BTreeMap<u64, Insn> {
        let text: String = lines.iter().enumerate().map(|(k, l)| format!("  {:x}:\t{l}\n", 0x1000 + 4 * k)).collect();
        parse_objdump(&format!("0000000000001000 <expf>:\n{text}")).0
    }

    /// A `libm.so.6` left by another tree is skipped, one built from this tree is checked (here: a file that is not
    /// an ELF, so checking it fails), and the digest follows a source file's contents.
    #[test]
    fn static_checks_only_a_product_of_this_tree() {
        let root = std::env::temp_dir().join(format!("xtask-partial-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates/math/src")).unwrap();
        std::fs::create_dir_all(root.join("target/liella")).unwrap();
        let src = root.join("crates/math/src/lib.rs");
        std::fs::write(&src, "a").unwrap();
        let so = root.join("target/liella/libm.so.6");
        std::fs::write(&so, "not an ELF").unwrap();
        assert!(static_check(&root).is_ok(), "no digest: skipped");
        std::fs::write(stamp_path(&so), source_digest(&root)).unwrap();
        assert!(static_check(&root).is_err(), "built from this tree: checked");
        std::fs::write(&src, "b").unwrap();
        assert!(static_check(&root).is_ok(), "a source changed since the build: skipped");
        std::fs::write(&src, "a").unwrap();
        assert!(static_check(&root).is_err(), "back to the built sources: checked");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unbroken_cvtss2sd_is_reported() {
        let f = walk(&dis(&["movd   %xmm0,%eax", "cvtss2sd %xmm0,%xmm1", "mulsd  %xmm1,%xmm0", "ret"]), 0x1000, 1);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].0, 0x1004);
    }

    /// D-10.79: a conversion whose destination is its source (`cvtss2sd %xmm3,%xmm3`, the boundary's in-place
    /// widening) merges the operand's own register: dependency-broken even with nothing owned. The same
    /// conversion into another unowned register still waits for that register's last writer.
    #[test]
    fn in_place_conversion_passes_and_another_destination_does_not() {
        assert!(walk(&dis(&["cvtss2sd %xmm3,%xmm3", "ret"]), 0x1000, 0).is_empty());
        let f = walk(&dis(&["cvtss2sd %xmm3,%xmm4", "ret"]), 0x1000, 0);
        assert_eq!(f.iter().map(|f| f.0).collect::<Vec<_>>(), vec![0x1000]);
    }

    #[test]
    fn zero_idiom_break_passes() {
        assert!(walk(&dis(&["pxor   %xmm1,%xmm1", "cvtss2sd %xmm0,%xmm1", "ret"]), 0x1000, 1).is_empty());
        assert!(walk(&dis(&["xorps  %xmm1,%xmm1", "sqrtsd %xmm0,%xmm1", "ret"]), 0x1000, 1).is_empty());
        assert!(walk(&dis(&["movq   %rax,%xmm1", "cvtsi2sd %eax,%xmm1", "ret"]), 0x1000, 1).is_empty());
    }

    #[test]
    fn merge_after_a_call_is_not_reported() {
        assert!(walk(&dis(&["call   2000 <foo>", "cvtss2sd %xmm0,%xmm1", "ret"]), 0x1000, 1).is_empty());
    }

    #[test]
    fn both_branch_outcomes_are_walked_and_joins_intersect() {
        // 1000: jne 100c; 1004: pxor xmm1; 1008: jmp 100c; 100c: cvtss2sd -> unowned on the jne path.
        let f = walk(&dis(&["jne    100c <expf+0xc>", "pxor   %xmm1,%xmm1", "jmp    100c <expf+0xc>", "cvtss2sd %xmm0,%xmm1", "ret"]), 0x1000, 1);
        assert_eq!(f.iter().map(|f| f.0).collect::<Vec<_>>(), vec![0x100c]);
    }

    #[test]
    fn merge_forms_and_their_exemptions() {
        // Own source, memory-source movsd (full write), VEX with an owned merge source: not reported.
        assert!(walk(&dis(&["sqrtsd %xmm0,%xmm0", "movsd  0x8(%rsp),%xmm2", "vcvtss2sd %xmm0,%xmm0,%xmm3", "ret"]), 0x1000, 1).is_empty());
        // Register movss, cvtsi2sdl from memory, roundsd, VEX with an unowned merge source: each reported.
        let f = walk(&dis(&["movss  %xmm0,%xmm4", "cvtsi2sdl 0x8(%rsp),%xmm5", "roundsd $0x9,%xmm0,%xmm6", "vsqrtsd %xmm0,%xmm7,%xmm8", "ret"]), 0x1000, 1);
        assert_eq!(f.len(), 4, "{f:?}");
        // Arguments are owned: a two-argument export may merge into %xmm1.
        assert!(walk(&dis(&["cvtss2sd %xmm0,%xmm1", "ret"]), 0x1000, 3).is_empty());
    }

    #[test]
    fn signature_masks() {
        assert_eq!(params_mask("x: f32"), 1);
        assert_eq!(params_mask("x: f64, y: f64"), 3);
        assert_eq!(params_mask("z: Complex<f64>"), 3);
        assert_eq!(params_mask("x: f64, e: Option<&mut c_int>"), 1);
        assert_eq!(params_mask("x: LdBits"), 0);
        assert_eq!(params_mask("x: F128Bits"), 0xff);
    }
}
