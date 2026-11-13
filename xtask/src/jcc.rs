//! `cargo xtask jcc-check`: the hot machine assembly keeps every jump off 32-byte boundaries (D-4.67,
//! MACHINE-JCC-PAD).
//!
//! Specification: Intel, "Mitigations for Jump Conditional Code Erratum" (2019). On the cores with the erratum, a
//! 32-byte window holding a jump that crosses a 32-byte boundary or ends on one is not cached as decoded uops; the
//! jumps are Jcc, a macro-fused compare-and-Jcc pair (the pair counts as one), JMP, indirect jumps, calls and
//! returns. Instruction counts do not show the loss, so this check keeps the layout true instead: in every code
//! section of the files in `crates/boundary/jcc_pad.rs` (assembled with its flags), no jump unit `[start, end)` has
//! `start / 32 != (end - 1) / 32` or `end % 32 == 0`, and every section holding a jump is aligned to at least 32
//! bytes (so the layout survives linking).
//!
//! Readers: `objdump -d -w` (instruction addresses and byte lengths) and `objdump -h -w` (section alignment), per
//! archive member. `cargo xtask static` assembles the files as the products do (no product needed); `cargo xtask so`
//! checks the built `libc.a` (the sections whose names the files declare); `cargo xtask jcc-check <file>...`
//! checks the archives or objects named, every code section in them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

mod pad {
    #![allow(dead_code)]
    include!("../../crates/boundary/jcc_pad.rs");
}
pub use pad::{JCC_PAD_FILES, JCC_PAD_FLAGS};

pub(crate) const BOUNDARY: u64 = 32;
/// The files that must stay padded (the string and memory kernels measured in D-4.67).
const REQUIRED: &[&str] = &["fold.S", "scan.S"];

/// One decoded instruction.
#[derive(Debug, Clone)]
struct Insn {
    addr: u64,
    len: u64,
    mnem: String,
    ops: String,
}

/// The mnemonic without the prefixes objdump prints as separate words (segment prefixes are what the padding adds).
pub(crate) fn split_insn(text: &str) -> (String, String) {
    const PREFIXES: &[&str] = &["cs", "ds", "es", "ss", "fs", "gs", "bnd", "notrack", "lock", "rep", "repz", "repnz", "repe", "repne", "data16", "addr32", "rex", "rex.W", "rex.R", "rex.X", "rex.B"];
    let mut words = text.split_whitespace().peekable();
    while let Some(w) = words.peek() {
        if PREFIXES.contains(w) || w.starts_with("rex.") {
            words.next();
        } else {
            break;
        }
    }
    let mnem = words.next().unwrap_or("").to_string();
    let ops = words.collect::<Vec<_>>().join(" ");
    (mnem, ops)
}

fn is_jump(m: &str) -> bool {
    m.starts_with('j') || m.starts_with("call") || m.starts_with("ret")
}

fn is_jcc(m: &str) -> bool {
    m.starts_with('j') && !m.starts_with("jmp")
}

/// Macro-fusion of `first` with the Jcc `jcc` (Intel optimization reference manual, macro-fusion rules): TEST and
/// AND with every Jcc; CMP, ADD and SUB with the carry, zero, sign-of-compare and above/below forms; INC and DEC with
/// the zero and signed-compare forms; never when the first instruction has both an immediate and a memory operand
/// or a RIP-relative operand.
fn fuses(first: &Insn, jcc: &str) -> bool {
    // objdump adds one size letter where the operands do not give the size (`cmpb $0x0,(%rdi)`).
    const OPS: &[&str] = &["test", "and", "cmp", "add", "sub", "inc", "dec"];
    let m = first.mnem.as_str();
    let Some(op) = OPS.iter().copied().find(|o| m == *o || (m.len() == o.len() + 1 && m.starts_with(o) && m.ends_with(['b', 'w', 'l', 'q']))) else {
        return false;
    };
    if first.ops.contains("%rip") || (first.ops.contains('$') && first.ops.contains('(')) {
        return false;
    }
    const ARITH: &[&str] = &["jb", "jae", "je", "jne", "jbe", "ja", "jl", "jge", "jle", "jg"];
    const INCDEC: &[&str] = &["je", "jne", "jl", "jge", "jle", "jg"];
    match op {
        "test" | "and" => true,
        "cmp" | "add" | "sub" => ARITH.contains(&jcc),
        "inc" | "dec" => INCDEC.contains(&jcc),
        _ => false,
    }
}

/// The jump units of one section that cross or end on a 32-byte boundary: `(start, end, text)`.
fn section_violations(insns: &[Insn]) -> Vec<(u64, u64, String)> {
    let mut bad = Vec::new();
    for (i, j) in insns.iter().enumerate() {
        if !is_jump(&j.mnem) {
            continue;
        }
        let mut start = j.addr;
        let mut what = j.mnem.clone();
        if is_jcc(&j.mnem) && i > 0 {
            let p = &insns[i - 1];
            if p.addr + p.len == j.addr && fuses(p, &j.mnem) {
                start = p.addr;
                what = format!("{}+{}", p.mnem, j.mnem);
            }
        }
        let end = j.addr + j.len;
        if start / BOUNDARY != (end - 1) / BOUNDARY || end % BOUNDARY == 0 {
            bad.push((start, end, what));
        }
    }
    bad
}

/// `objdump -d -w` text -> (member, section) -> instructions.
fn parse_disasm(text: &str) -> BTreeMap<(String, String), Vec<Insn>> {
    let mut out: BTreeMap<(String, String), Vec<Insn>> = BTreeMap::new();
    let (mut member, mut section) = (String::new(), String::new());
    for line in text.lines() {
        if let Some((m, _)) = line.split_once(":     file format") {
            member = m.trim().to_string();
            continue;
        }
        if let Some(s) = line.strip_prefix("Disassembly of section ") {
            section = s.trim_end_matches(':').to_string();
            continue;
        }
        let mut f = line.splitn(3, '\t');
        let (Some(a), Some(bytes), Some(text)) = (f.next(), f.next(), f.next()) else { continue };
        let Some(a) = a.trim().strip_suffix(':') else { continue };
        let Ok(addr) = u64::from_str_radix(a, 16) else { continue };
        let len = bytes.split_whitespace().count() as u64;
        let (mnem, ops) = split_insn(text);
        if len == 0 || mnem.is_empty() || mnem == "(bad)" {
            continue;
        }
        out.entry((member.clone(), section.clone())).or_default().push(Insn { addr, len, mnem, ops });
    }
    out
}

/// `objdump -h -w` text -> (member, section) -> alignment in bytes.
fn parse_headers(text: &str) -> BTreeMap<(String, String), u64> {
    let mut out = BTreeMap::new();
    let mut member = String::new();
    for line in text.lines() {
        if let Some((m, _)) = line.split_once(":     file format") {
            member = m.trim().to_string();
            continue;
        }
        let w: Vec<&str> = line.split_whitespace().collect();
        if w.len() >= 7 && w[0].parse::<u32>().is_ok() {
            if let Some(p) = w[6].strip_prefix("2**").and_then(|p| p.parse::<u32>().ok()) {
                out.insert((member.clone(), w[1].to_string()), 1u64 << p);
            }
        }
    }
    out
}

/// A section name without the hot-text prefixes (`.text.sorted.h<n>.`, `.text.hot.`) or `.text.`.
fn base_name(section: &str) -> &str {
    if let Some(r) = section.strip_prefix(".text.sorted.h") {
        return r.split_once('.').map_or(r, |(_, f)| f);
    }
    section.strip_prefix(".text.hot.").or_else(|| section.strip_prefix(".text.")).unwrap_or(section)
}

/// The section names of `objdump -h -w` text (any member) whose base name is in `only`, sorted, each once.
fn selected_sections(headers: &str, only: &BTreeSet<String>) -> Vec<String> {
    let names: BTreeSet<String> = parse_headers(headers).into_keys().map(|(_, s)| s).filter(|s| only.contains(base_name(s))).collect();
    names.into_iter().collect()
}

/// The errors of one disassembly: every section (or, with `only`, the sections whose base name is in it).
fn check_text(disasm: &str, headers: &str, only: Option<&BTreeSet<String>>) -> (usize, Vec<String>) {
    let aligns = parse_headers(headers);
    let mut errors = Vec::new();
    let mut jumps = 0;
    for ((member, section), insns) in parse_disasm(disasm) {
        if only.is_some_and(|o| !o.contains(base_name(&section))) {
            continue;
        }
        let n = insns.iter().filter(|i| is_jump(&i.mnem)).count();
        if n == 0 {
            continue;
        }
        jumps += n;
        let at = if member.is_empty() { section.clone() } else { format!("{member}({section})") };
        match aligns.get(&(member.clone(), section.clone())) {
            Some(&a) if a >= BOUNDARY => {}
            a => errors.push(format!("{at}: aligned to {a:?} bytes, below {BOUNDARY}: the padding does not survive linking")),
        }
        for (s, e, what) in section_violations(&insns) {
            errors.push(format!("{at}: {what} at {s:#x}..{e:#x} crosses or ends on a 32-byte boundary"));
        }
    }
    (jumps, errors)
}

fn tool(cmd: &mut Command, what: &str) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{what} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Check one archive or object; `only`: the section base names to check (None: every code section).
pub fn check_file(path: &Path, only: Option<&BTreeSet<String>>) -> Result<usize, String> {
    let h = tool(Command::new("objdump").args(["-h", "-w"]).arg(path), "objdump -h")?;
    let d = match only {
        None => tool(Command::new("objdump").args(["-d", "-w"]).arg(path), "objdump -d")?,
        // Only the sections `check_text` reads: objdump -d of the whole 14 MB libc.a took 225 s, of its 47 hot
        // sections 0.1 s (LAYOUT-CHECK-VERDICT-CACHE). None of them: nothing to disassemble (no jump, an error below).
        Some(o) => match selected_sections(&h, o) {
            s if s.is_empty() => String::new(),
            s => tool(Command::new("objdump").args(["-d", "-w"]).args(s.iter().flat_map(|n| ["-j", n.as_str()])).arg(path), "objdump -d -j <the hot sections>")?,
        },
    };
    let (jumps, errors) = check_text(&d, &h, only);
    if jumps == 0 {
        return Err(format!("jcc-check: {}: no jump found in the checked sections (nothing was checked)", path.display()));
    }
    if !errors.is_empty() {
        let n = errors.len();
        let mut e: Vec<String> = errors.into_iter().take(20).collect();
        e.push(format!("{n} jump(s) on 32-byte boundaries in {} (D-4.67: assemble with crates/boundary/jcc_pad.rs's flags)", path.display()));
        return Err(format!("jcc-check: {}", e.join("\n  ")));
    }
    println!("jcc-check: {}: {jumps} jumps, none crossing or ending on a 32-byte boundary (D-4.67)", path.display());
    Ok(jumps)
}

/// The code section names the padded files declare (`.section .text.<name>`), for the archive check.
pub fn hot_sections(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    for f in JCC_PAD_FILES {
        let p = root.join("csys/src/machine").join(f);
        let s = std::fs::read_to_string(&p).map_err(|e| format!("jcc-check: {}: {e}", p.display()))?;
        for l in s.lines() {
            if let Some(r) = l.trim().strip_prefix(".section .text.") {
                out.insert(r.split(',').next().unwrap_or(r).trim().to_string());
            }
        }
    }
    Ok(out)
}

/// `cargo xtask so`: the built archive, the sections of the padded files.
pub fn check_archive(root: &Path, lib: &Path) -> Result<(), String> {
    check_file(lib, Some(&hot_sections(root)?)).map(|_| ())
}

/// `cargo xtask static`: assemble each padded file as crates/boundary/build.rs does and check every code section.
pub fn static_check(root: &Path) -> Result<(), String> {
    // The check reads its file list from jcc_pad.rs, so a file dropped there would drop out of the check too.
    if let Some(f) = REQUIRED.iter().find(|f| !JCC_PAD_FILES.contains(f)) {
        return Err(format!("jcc-check: crates/boundary/jcc_pad.rs no longer pads csys/src/machine/{f} (D-4.67)"));
    }
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let dir = std::env::temp_dir().join(format!("liella-jcc-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("jcc-check: {}: {e}", dir.display()))?;
    let mut res = Ok(());
    for f in JCC_PAD_FILES {
        let obj = dir.join(format!("{f}.o"));
        let src = root.join("csys/src/machine").join(f);
        let r = tool(Command::new(&cc).args(["-c", "-fPIC", "-DLIELLA_EXPORT", "-DLIELLA_RUNTIME"]).args(JCC_PAD_FLAGS).arg("-I").arg(root.join("csys/include")).arg(&src).arg("-o").arg(&obj), &format!("assembling csys/src/machine/{f}"))
            .and_then(|_| check_file(&obj, None));
        if let Err(e) = r {
            res = Err(e);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    res
}

/// `cargo xtask jcc-check [<file>...]`: the files named (every code section), or (none) the source as `static` does.
pub fn run(root: &Path, files: &[String]) -> Result<(), String> {
    if files.is_empty() {
        return static_check(root);
    }
    let bad: Vec<String> = files.iter().filter_map(|f| check_file(Path::new(f), None).err()).collect();
    if bad.is_empty() { Ok(()) } else { Err(bad.join("\n")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ins(addr: u64, len: u64, text: &str) -> Insn {
        let (mnem, ops) = split_insn(text);
        Insn { addr, len, mnem, ops }
    }

    #[test]
    fn jump_crossing_or_ending_on_a_boundary_is_reported() {
        // no spec clause (D-4.67; Intel JCC erratum white paper, 2019): the two failing layouts and their neighbours.
        let ends = [ins(0x1a, 4, "mov %eax,%ecx"), ins(0x1e, 2, "jmp 0x40")];
        assert_eq!(section_violations(&ends).len(), 1, "a jump ending at 0x20 ends on the boundary");
        let crosses = [ins(0x1f, 2, "ret"), ins(0x21, 1, "nop")];
        assert_eq!(section_violations(&crosses).len(), 1, "a jump over 0x20 crosses it");
        let fine = [ins(0x1d, 2, "je 0x40"), ins(0x20, 5, "call 0x80"), ins(0x3e, 1, "ret")];
        assert!(section_violations(&fine).is_empty(), "inside one window and ending before the boundary");
    }

    #[test]
    fn fused_pair_counts_from_the_compare() {
        // no spec clause (D-4.67): a macro-fused cmp+jcc pair is one unit; a pair split by the boundary is reported
        // even though the jcc alone sits inside one window.
        let pair = [ins(0x1e, 3, "cmp %rdx,%rcx"), ins(0x21, 2, "jb 0x80")];
        assert_eq!(section_violations(&pair).len(), 1);
        let not_fused = [ins(0x1e, 3, "mov %rdx,%rcx"), ins(0x21, 2, "jb 0x80")];
        assert!(section_violations(&not_fused).is_empty(), "mov does not fuse");
        let imm_mem = [ins(0x1b, 7, "cmpl $0x1,(%rdi)"), ins(0x22, 2, "je 0x80")];
        assert!(section_violations(&imm_mem).is_empty(), "immediate + memory does not fuse");
        let incjb = [ins(0x1e, 3, "inc %rcx"), ins(0x21, 2, "jb 0x80")];
        assert!(section_violations(&incjb).is_empty(), "inc does not fuse with jb");
        let test_js = [ins(0x1e, 3, "test %eax,%eax"), ins(0x21, 2, "js 0x80")];
        assert_eq!(section_violations(&test_js).len(), 1, "test fuses with every jcc");
    }

    #[test]
    fn padding_prefixes_are_not_the_mnemonic() {
        // no spec clause (D-4.67): the assembler's segment-prefix padding must not hide a jump or a fusible compare.
        assert_eq!(split_insn("cs cs jmp 0x40").0, "jmp");
        assert_eq!(split_insn("ds cmp %rdx,%rcx").0, "cmp");
        assert_eq!(split_insn("notrack jmp *%rax").0, "jmp");
    }

    #[test]
    fn objdump_text_is_read_per_member_and_section() {
        // no spec clause (D-4.67): an archive's members and sections, and the alignment column.
        let d = "\nm1.o:     file format elf64-x86-64\n\n\nDisassembly of section .text.sorted.h3.liella_machine_x:\n\n0000000000000000 <f>:\n   0:\t48 89 f8             \tmov    %rdi,%rax\n   3:\tc3                   \tret\n";
        let h = "\nm1.o:     file format elf64-x86-64\n\nSections:\nIdx Name          Size      VMA               LMA               File off  Algn  Flags\n  0 .text.sorted.h3.liella_machine_x 00000004  0000000000000000  0000000000000000  00000040  2**4  CONTENTS, ALLOC, LOAD, READONLY, CODE\n";
        let only: BTreeSet<String> = ["liella_machine_x".to_string()].into();
        let (jumps, errors) = check_text(d, h, Some(&only));
        assert_eq!(jumps, 1);
        assert_eq!(errors.len(), 1, "2**4 is below 32: {errors:?}");
        assert!(errors[0].contains("m1.o(.text.sorted.h3.liella_machine_x)"));
        let other: BTreeSet<String> = ["y".to_string()].into();
        assert_eq!(check_text(d, h, Some(&other)).0, 0, "a section not named is not checked");
        let h32 = h.replace("2**4", "2**5");
        assert!(check_text(d, &h32, None).1.is_empty());
    }

    #[test]
    fn archive_check_disassembles_exactly_the_named_sections() {
        // no spec clause (D-4.67, LAYOUT-CHECK-VERDICT-CACHE): the -j list of the archive check is every section,
        // in any member, whose base name the padded files declare (hot prefixes or none), each once, and no other.
        let row = |m: &str, s: &str, a: u32| format!("\n{m}:     file format elf64-x86-64\n\nSections:\nIdx Name Size VMA LMA File off Algn Flags\n  0 {s} 00000004  0000000000000000  0000000000000000  00000040  2**{a}  CONTENTS, CODE\n");
        let h = [row("a.o", ".text.sorted.h3.liella_machine_x", 5), row("b.o", ".text.liella_machine_x", 5), row("c.o", ".text.hot.liella_machine_y", 5), row("d.o", ".text.other", 5), row("e.o", ".text.sorted.h3.liella_machine_x", 6)].concat();
        let only: BTreeSet<String> = ["liella_machine_x".to_string(), "liella_machine_y".to_string()].into();
        assert_eq!(selected_sections(&h, &only), [".text.hot.liella_machine_y", ".text.liella_machine_x", ".text.sorted.h3.liella_machine_x"]);
        assert!(selected_sections(&h, &["z".to_string()].into()).is_empty(), "no hot section: nothing selected");
    }
}
