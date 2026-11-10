//! `cargo xtask classed-xref`: the classed pages' XREF bias rule (D-4.53, D-X.34; CLASSED-SLOT-XREF-CHECK).
//!
//! Page 1 of the classed section is assembled for page 0's address (`csys/src/machine/fold.S`): after the swap it
//! runs there, so every reference it makes out of its own page must address page 0's target + 4096. The slot tests
//! check results only: a page 1 reference that loses the bias reads whatever lies 4096 bytes below the intended
//! word, which gives the right bytes in one link layout and a wrong path in another. This check pairs the n-th
//! out-of-page reference of page 0 with the n-th of page 1 (both pages expand one macro, so the references come in
//! the same order) and requires the same instruction and symbol and page 1's target = page 0's + 4096. A reference
//! without a partner is an error, never a skip, and so is a section with no reference to check.
//!
//! Two readers: an object or archive (`readelf`: relocations of the section defining `liella_classed_page0`, same
//! type and symbol, page 1's addend = page 0's + 4096) and a linked object (`objdump -d` of the two pages: same
//! mnemonic, page 1's target address = page 0's + 4096). `cargo xtask static` assembles `fold.S` (no product
//! needed); `cargo xtask so` checks the built `libc.a` and `libc.so.6`; `cargo xtask classed-xref <file>...` checks
//! the files named.

use std::path::Path;
use std::process::Command;

pub(crate) const PAGE: i64 = 4096;
/// The symbol at the start of page 0 (`csys/src/machine/fold.S`).
const PAGE0: &str = "liella_classed_page0";

/// One out-of-page reference of a page.
#[derive(Debug, Clone)]
struct Ref {
    /// Offset of the instruction (linked) or relocated field (object) in its page.
    off: i64,
    /// Mnemonic (linked) or relocation type (object).
    what: String,
    /// Symbol (object; empty when linked: the address says it).
    sym: String,
    /// Address (linked) or addend (object).
    target: i64,
    /// objdump's name for the address (linked; report only).
    label: String,
}

impl Ref {
    fn show_target(&self, t: i64) -> String {
        if self.sym.is_empty() {
            format!("{t:#x}")
        } else if t < 0 {
            format!("{}-{:#x}", self.sym, -t)
        } else {
            format!("{}+{t:#x}", self.sym)
        }
    }
    fn show(&self) -> String {
        let label = if self.label.is_empty() { String::new() } else { format!(" <{}>", self.label) };
        format!("+{:#x} {} -> {}{label}", self.off, self.what, self.show_target(self.target))
    }
}

/// The assembler writes `jz sym` as a PLT32 relocation and `jz sym+4096` as a PC32 one; both resolve to
/// symbol + addend - place for the hidden symbols the pages use, so they compare as one kind.
fn kind(what: &str) -> &str {
    if what == "R_X86_64_PLT32" { "R_X86_64_PC32" } else { what }
}

/// The rule over the two pages' references, in order. Returns the number of pairs, or every problem.
fn pair(page0: &[Ref], page1: &[Ref]) -> Result<usize, Vec<String>> {
    let mut bad = Vec::new();
    if page0.is_empty() && page1.is_empty() {
        bad.push("no out-of-page reference found on either page: nothing was checked".to_string());
    }
    for (i, (a, b)) in page0.iter().zip(page1).enumerate() {
        let want = a.target + PAGE;
        if kind(&a.what) != kind(&b.what) || a.sym != b.sym || b.target != want {
            bad.push(format!("reference {i}: page 1 {} does not match page 0 {}: page 1 must address page 0's target + 4096 ({} {})", b.show(), a.show(), a.what, a.show_target(want)));
        }
    }
    if page0.len() != page1.len() {
        let (k, longer) = if page0.len() > page1.len() { (0, page0) } else { (1, page1) };
        for r in &longer[page0.len().min(page1.len())..] {
            bad.push(format!("page 0 has {} out-of-page references and page 1 has {}: page {k} {} has no partner", page0.len(), page1.len(), r.show()));
        }
    }
    if bad.is_empty() { Ok(page0.len()) } else { Err(bad) }
}

fn tool(cmd: &mut Command, what: &str) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{what} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// readelf output split per archive member ("File: lib.a(m.o)" lines); one unnamed group for a plain object.
fn members(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![(String::new(), String::new())];
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("File: ") {
            out.push((name.trim().to_string(), String::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push_str(line);
            last.1.push('\n');
        }
    }
    out.retain(|(n, t)| !n.is_empty() || !t.trim().is_empty());
    out
}

fn hex(s: &str) -> Option<i64> {
    i64::from_str_radix(s.trim_start_matches("0x"), 16).ok()
}

/// `readelf -sW`: the defined `liella_classed_page0` of a member, as (section index, value).
fn page0_symbol(syms: &str) -> Option<(usize, i64)> {
    syms.lines().find_map(|l| {
        let t: Vec<&str> = l.split_whitespace().collect();
        if t.len() >= 8 && t[t.len() - 1] == PAGE0 && t[0].ends_with(':') {
            Some((t[6].parse().ok()?, hex(t[1])?))
        } else {
            None
        }
    })
}

/// `readelf -SW`: the name of section `index`.
fn section_name(sections: &str, index: usize) -> Option<String> {
    sections.lines().find_map(|l| {
        let rest = l.trim_start().strip_prefix('[')?;
        let (n, rest) = rest.split_once(']')?;
        (n.trim().parse::<usize>().ok()? == index).then(|| rest.split_whitespace().next().map(str::to_string))?
    })
}

/// `readelf -rW`: the entries of relocation section `.rela<section>` as (offset, type, symbol, addend).
fn relocations(relocs: &str, section: &str) -> Result<Vec<(i64, String, String, i64)>, String> {
    let head = format!("Relocation section '.rela{section}'");
    let mut out = Vec::new();
    let mut inside = false;
    for l in relocs.lines() {
        if l.starts_with("Relocation section ") {
            inside = l.starts_with(&head);
            continue;
        }
        let t: Vec<&str> = l.split_whitespace().collect();
        if !inside || t.len() < 6 || hex(t[0]).is_none() || !t[2].starts_with("R_") {
            continue;
        }
        let sign = t[t.len() - 2];
        let addend = hex(t[t.len() - 1]).ok_or_else(|| format!("unreadable relocation: {l}"))?;
        let addend = match sign {
            "+" => addend,
            "-" => -addend,
            _ => return Err(format!("unreadable relocation addend: {l}")),
        };
        out.push((hex(t[0]).unwrap_or_default(), t[2].to_string(), t[4..t.len() - 2].join(" "), addend));
    }
    out.sort_by_key(|r| r.0);
    Ok(out)
}

/// An object or archive: the pairs of the one member defining `liella_classed_page0`.
fn check_relocatable(path: &Path) -> Result<usize, String> {
    let syms = tool(Command::new("readelf").arg("-sW").arg(path), "readelf -sW")?;
    let sections = tool(Command::new("readelf").arg("-SW").arg(path), "readelf -SW")?;
    let relocs = tool(Command::new("readelf").arg("-rW").arg(path), "readelf -rW")?;
    let (sec_m, rel_m) = (members(&sections), members(&relocs));
    let found: Vec<(String, usize, i64)> = members(&syms).into_iter().filter_map(|(m, t)| page0_symbol(&t).map(|(i, v)| (m, i, v))).collect();
    let [(member, index, base)] = found.as_slice() else {
        return Err(format!("{} defines {PAGE0} {} times, not once", path.display(), found.len()));
    };
    let of = |groups: &[(String, String)]| groups.iter().find(|(m, _)| m == member).map(|(_, t)| t.clone()).unwrap_or_default();
    let name = section_name(&of(&sec_m), *index).ok_or_else(|| format!("{}: no section {index} in {member}", path.display()))?;
    let (mut page0, mut page1) = (Vec::new(), Vec::new());
    for (off, what, sym, addend) in relocations(&of(&rel_m), &name)? {
        let r = Ref { off: (off - base) % PAGE, what, sym, target: addend, label: String::new() };
        match (off - base).div_euclid(PAGE) {
            0 => page0.push(r),
            1 => page1.push(r),
            _ => return Err(format!("{}: relocation at {name}+{off:#x} lies outside the two classed pages", path.display())),
        }
    }
    pair(&page0, &page1).map_err(|bad| format!("{} ({member} {name}):\n  {}", path.display(), bad.join("\n  ")))
}

/// An instruction's address, mnemonic, and target with objdump's name for it.
type Insn = (i64, String, Option<(i64, String)>);

/// One `objdump -d` line: the target is set for a direct branch or call, or an instruction with a RIP-relative
/// operand (objdump's `# <address>` comment).
fn instruction(line: &str) -> Option<Insn> {
    let (addr, rest) = line.split_once(":\t")?;
    let addr = hex(addr.trim())?;
    let mut t = rest.split_whitespace().skip_while(|w| matches!(*w, "bnd" | "notrack" | "lock" | "rex" | "rex.W" | "data16" | "cs" | "ds"));
    let mnem = t.next()?.to_string();
    let label = |s: &str| s.split_once('<').and_then(|(_, r)| r.split_once('>')).map(|(l, _)| l.to_string()).unwrap_or_default();
    if let Some((_, c)) = rest.split_once("# ") {
        let a = hex(c.split_whitespace().next()?)?;
        return Some((addr, mnem, Some((a, label(c)))));
    }
    let direct = mnem.starts_with('j') || mnem.starts_with("call") || mnem.starts_with("loop");
    let target = if direct { t.next().and_then(hex).map(|a| (a, label(rest))) } else { None };
    Some((addr, mnem, target))
}

/// A linked object: the pairs of the two pages at `liella_classed_page0` (its symbol table must have it).
fn check_linked(path: &Path) -> Result<usize, String> {
    let syms = tool(Command::new("readelf").arg("-sW").arg(path), "readelf -sW")?;
    let base = members(&syms).iter().find_map(|(_, t)| page0_symbol(t)).map(|(_, v)| v).ok_or_else(|| format!("{}: no {PAGE0} in its symbol table", path.display()))?;
    let dis = tool(Command::new("objdump").args(["-d", "--no-show-raw-insn", "-w"]).arg(format!("--start-address={base:#x}")).arg(format!("--stop-address={:#x}", base + 2 * PAGE)).arg(path), "objdump -d")?;
    let (mut page0, mut page1) = (Vec::new(), Vec::new());
    for (addr, what, target) in dis.lines().filter_map(instruction) {
        let Some((target, label)) = target else { continue };
        let page = (addr - base).div_euclid(PAGE);
        let start = base + page * PAGE;
        if (start..start + PAGE).contains(&target) {
            continue;
        }
        let r = Ref { off: addr - start, what, sym: String::new(), target, label };
        match page {
            0 => page0.push(r),
            1 => page1.push(r),
            _ => return Err(format!("{}: objdump listed {addr:#x}, outside the two classed pages", path.display())),
        }
    }
    pair(&page0, &page1).map_err(|bad| format!("{} (pages at {base:#x}):\n  {}", path.display(), bad.join("\n  ")))
}

/// Check one file: an archive or relocatable object by its relocations, a linked object by its instructions.
pub fn check_file(path: &Path) -> Result<usize, String> {
    let head = std::fs::read(path).map_err(|e| format!("classed-xref: {}: {e}", path.display()))?;
    let relocatable = head.starts_with(b"!<arch>\n") || (head.starts_with(b"\x7fELF") && head.get(16) == Some(&1));
    let n = if relocatable { check_relocatable(path) } else { check_linked(path) }.map_err(|e| format!("classed-xref: {e}"))?;
    println!("classed-xref: {}: {n} out-of-page reference pairs, page 1 = page 0 + 4096 on each (D-4.53 XREF)", path.display());
    Ok(n)
}

/// `cargo xtask static`: assemble `csys/src/machine/fold.S` as the products do and check the object.
pub fn static_check(root: &Path) -> Result<(), String> {
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let dir = std::env::temp_dir().join(format!("liella-classed-xref-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("classed-xref: {}: {e}", dir.display()))?;
    let obj = dir.join("fold.o");
    let built = tool(Command::new(&cc).args(["-c", "-fPIC", "-DLIELLA_EXPORT", "-DLIELLA_RUNTIME"]).args(crate::jcc::JCC_PAD_FLAGS).arg("-I").arg(root.join("csys/include")).arg(root.join("csys/src/machine/fold.S")).arg("-o").arg(&obj), "assembling csys/src/machine/fold.S");
    let res = built.and_then(|_| check_file(&obj));
    let _ = std::fs::remove_dir_all(&dir);
    res.map(|_| ())
}

/// `cargo xtask classed-xref [<file>...]`: the files named, or (none) the source as `static` does.
pub fn run(root: &Path, files: &[String]) -> Result<(), String> {
    if files.is_empty() {
        return static_check(root);
    }
    let bad: Vec<String> = files.iter().filter_map(|f| check_file(Path::new(f)).err()).collect();
    if bad.is_empty() { Ok(()) } else { Err(bad.join("\n")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-page classed section like fold.S's: one RIP-relative load and one jump out of the page on each page
    /// (page 1's with `B1`/`B2` as bias), an in-page jump on each, and `EXTRA` on page 1.
    fn synthetic(b1: u32, b2: u32, extra: &str) -> String {
        format!(
            "\t.data\n\t.globl w\n\t.hidden w\n\t.quad 0\nw:\t.quad 0\n\t.text\n\t.globl f\n\t.hidden f\nf:\tret\n\
             \t.section .text.cls,\"ax\",@progbits\n\t.balign 4096\n\t.globl {PAGE0}\n\t.hidden {PAGE0}\n{PAGE0}:\n\
             \tcmpq w(%rip), %rdx\n\tja 1f\n\tjz f\n1:\tret\n\t.org {PAGE0} + 4096\n\
             \tcmpq w+{b1}(%rip), %rdx\n\tja 1f\n\tjz f+{b2}\n{extra}\n1:\tret\n\t.org {PAGE0} + 8192\n"
        )
    }

    /// Assemble `src` to an object and link it into a shared object; check both.
    fn both(dir: &Path, name: &str, src: &str) -> (Result<usize, String>, Result<usize, String>) {
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
        let (s, o, so) = (dir.join(format!("{name}.S")), dir.join(format!("{name}.o")), dir.join(format!("{name}.so")));
        std::fs::write(&s, src).unwrap();
        tool(Command::new(&cc).arg("-c").arg(&s).arg("-o").arg(&o), "cc -c").unwrap();
        tool(Command::new(&cc).args(["-shared", "-nostdlib"]).arg(&o).arg("-o").arg(&so), "cc -shared").unwrap();
        (check_file(&o), check_file(&so))
    }

    // no spec clause (CLASSED-SLOT-XREF-CHECK, developer tooling): the XREF rule on a synthetic two-page object, as
    // an object and linked: a correct pair passes, a biasless load or jump on page 1 fails naming both targets, and
    // a page 1 reference with no page 0 partner fails.
    #[test]
    fn the_xref_rule_on_a_synthetic_two_page_object() {
        let dir = std::env::temp_dir().join(format!("liella-classed-xref-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = both(&dir, "ok", &synthetic(4096, 4096, ""));
        let load = both(&dir, "load", &synthetic(0, 4096, ""));
        let jump = both(&dir, "jump", &synthetic(4096, 0, ""));
        let extra = both(&dir, "extra", &synthetic(4096, 4096, "\tjnz f+4096"));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(ok.0, Ok(2), "object");
        assert_eq!(ok.1, Ok(2), "linked");
        for (case, r) in [("load .o", &load.0), ("load .so", &load.1), ("jump .o", &jump.0), ("jump .so", &jump.1)] {
            let e = r.as_ref().expect_err(case);
            assert!(e.contains("does not match page 0") && e.contains("must address page 0's target + 4096"), "{case}: {e}");
        }
        // The biasless load names page 0's target and page 1's: w-0x4 against w+0xffc (object).
        let e = load.0.unwrap_err();
        assert!(e.contains("R_X86_64_PC32 -> w-0x4") && e.contains("(R_X86_64_PC32 w+0xffc)"), "{e}");
        for (case, r) in [("extra .o", &extra.0), ("extra .so", &extra.1)] {
            let e = r.as_ref().expect_err(case);
            assert!(e.contains("page 0 has 2 out-of-page references and page 1 has 3") && e.contains("page 1 +0x") && e.contains("has no partner"), "{case}: {e}");
        }
    }

    // no spec clause (CLASSED-SLOT-XREF-CHECK): the readers' parsing of readelf and objdump lines.
    #[test]
    fn the_readers_parse_relocations_and_instructions() {
        let rel = "Relocation section '.rela.text.cls' at offset 0x1 contains 2 entries:\n    Offset Info Type Value Name + Addend\n\
                   0000000000001006  0000000b00000002 R_X86_64_PC32          0000000000000000 w + ffc\n\
                   0000000000000006  0000000b00000002 R_X86_64_PC32          0000000000000000 w - 4\n\
                   Relocation section '.rela.text.other' at offset 0x2 contains 1 entry:\n\
                   0000000000000009  0000000b00000002 R_X86_64_PC32          0000000000000000 x - 4\n";
        // A section with nothing to check is an error, not a pass (a reader that finds nothing must not look green).
        assert!(pair(&[], &[]).is_err_and(|e| e[0].contains("nothing was checked")));
        let r = relocations(rel, ".text.cls").unwrap();
        assert_eq!(r, vec![(6, "R_X86_64_PC32".into(), "w".into(), -4), (0x1006, "R_X86_64_PC32".into(), "w".into(), 0xffc)]);
        let i = instruction("   4a003:\tcmp    0x3ba876(%rip),%rdx        # 404880 <x+0xb8>").unwrap();
        assert_eq!((i.0, i.1.as_str(), i.2), (0x4a003, "cmp", Some((0x404880, "x+0xb8".to_string()))));
        let j = instruction("   490a7:\tje     91c80 <liella_hot_fill_rest>").unwrap();
        assert_eq!(j.2, Some((0x91c80, "liella_hot_fill_rest".to_string())));
        assert_eq!(instruction("   490b0:\tmovups %xmm0,(%rdi)").unwrap().2, None);
        assert_eq!(page0_symbol(&format!("    18: 0000000000000000     0 NOTYPE  WEAK   HIDDEN     8 {PAGE0}\n 17541: 0000000000000000     0 NOTYPE  GLOBAL DEFAULT  UND {PAGE0}\n")), Some((8, 0)));
        assert_eq!(section_name("  [ 8] .text.sorted.h00108.liella_machine_memset PROGBITS 0 340 21f 00 AX 0 0 64\n", 8).as_deref(), Some(".text.sorted.h00108.liella_machine_memset"));
    }
}
