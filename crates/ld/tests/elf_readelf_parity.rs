//! Parity of the ELF reader (`liella_ld::elf`, IP-24 part 1) with `readelf -h`, `-l`, `-d` and `-n` over a
//! corpus this test builds with the system compiler and linker: a static program, a PIE (RELR, both hash
//! styles), a non-PIE with `DT_RPATH`, a shared object with soname and `DT_RUNPATH`, and a shared object
//! without notes. `readelf -n` reads a copy without section headers (`objcopy --strip-section-headers`),
//! so it reports the `PT_NOTE` segments the reader walks rather than note sections.
//!
//! no spec clause: a black-box cross-check of the reader's decoding against binutils (plan/ph14/design.md §2).
//! Skips (with the reason) when `cc`, `readelf` or `objcopy` is absent.
// ARCH(x86_64): the corpus is built by the host toolchain and checked for the x86-64 machine name.
#![cfg(target_arch = "x86_64")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use liella_ld::arch::{RelocKind, X86_64};
use liella_ld::elf::{dt, pt, Elf};

struct Member {
    name: &'static str,
    path: PathBuf,
    /// The same file without section headers (for `readelf -n`).
    stripped: PathBuf,
}

fn tool_ok(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok_and(|o| o.status.success())
}

fn run(tool: &str, args: &[&str]) -> String {
    let o = Command::new(tool).args(args).output().unwrap_or_else(|e| panic!("{tool}: {e}"));
    assert!(o.status.success(), "{tool} {args:?} failed: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

fn corpus() -> Option<&'static [Member]> {
    static CORPUS: OnceLock<Option<Vec<Member>>> = OnceLock::new();
    CORPUS
        .get_or_init(|| {
            for t in ["cc", "readelf", "objcopy"] {
                if !tool_ok(t) {
                    println!("SKIP: `{t}` is not available, the readelf parity corpus cannot be built");
                    return None;
                }
            }
            let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("elf_readelf_parity_{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let main = dir.join("main.c");
            let lib = dir.join("lib.c");
            std::fs::write(&main, "int main(void) { return 0; }\n").unwrap();
            std::fs::write(&lib, "int corpus_f(void) { return 1; }\n").unwrap();
            // A versioned library: definitions with a parent, a hidden old version, data pointers (RELR or
            // RELATIVE), PLT calls, TLS (DTPMOD64/DTPOFF64) and a needed version from the C library.
            let vlib = dir.join("vlib.c");
            let vmap = dir.join("v.map");
            std::fs::write(&vmap, "VER_1 { global: f1; g; local: *; };\nVER_2 { global: f2; t; } VER_1;\n").unwrap();
            std::fs::write(
                &vlib,
                "#include <stdio.h>\nint f1(void) { return 1; }\nint f2(void) { return 2; }\nint g_old(void) { return 3; }\n__asm__(\".symver g_old,g@VER_1\");\nint g_new(void) { return 4; }\n__asm__(\".symver g_new,g@@VER_2\");\n__thread int t;\nstatic int x; int *px = &x; int *py[3] = {&x, &x, &x};\nint use(void) { printf(\"%d\\n\", f1() + t); return puts(\"x\"); }\n",
            )
            .unwrap();
            let (main, lib) = (main.to_str().unwrap().to_string(), lib.to_str().unwrap().to_string());
            let (vlib, vscript) = (vlib.to_str().unwrap().to_string(), format!("-Wl,--version-script={}", vmap.to_str().unwrap()));
            let members: [(&'static str, Vec<&str>); 8] = [
                ("static", vec!["-static", &main]),
                ("pie", vec!["-fPIE", "-pie", "-Wl,-z,pack-relative-relocs", "-Wl,--hash-style=both", &main]),
                ("nopie-rpath", vec!["-fno-PIE", "-no-pie", "-Wl,--disable-new-dtags,-rpath,/opt/corpus-r", &main]),
                ("shared", vec!["-shared", "-fPIC", "-Wl,-soname,libcorpus.so.1", "-Wl,--enable-new-dtags,-rpath,/opt/corpus-run", &lib]),
                ("shared-nonotes", vec!["-shared", "-fPIC", "-nostdlib", "-fcf-protection=none", "-Wl,--build-id=none", &lib]),
                ("versioned-relr", vec!["-shared", "-fPIC", &vscript, "-Wl,-z,pack-relative-relocs", "-Wl,--hash-style=both", &vlib]),
                ("versioned-sysv", vec!["-shared", "-fPIC", &vscript, "-Wl,-z,nopack-relative-relocs", "-Wl,--hash-style=sysv", &vlib]),
                ("versioned-gnu", vec!["-shared", "-fPIC", &vscript, "-Wl,--hash-style=gnu", "-Wl,-z,now", &vlib]),
            ];
            let mut out = Vec::new();
            for (name, args) in members {
                let path = dir.join(name);
                let stripped = dir.join(format!("{name}.nosh"));
                let p = path.to_str().unwrap();
                let mut a = vec!["-o", p];
                a.extend(args);
                run("cc", &a);
                run("objcopy", &["--strip-section-headers", p, stripped.to_str().unwrap()]);
                out.push(Member { name, path, stripped });
            }
            Some(out)
        })
        .as_deref()
}

fn readelf(opt: &str, p: &Path) -> String {
    run("readelf", &[opt, "-W", p.to_str().unwrap()])
}

/// A `readelf` number: `0x` hex or decimal, the first token of `s`.
fn num(s: &str) -> u64 {
    let t = s.split_whitespace().next().unwrap_or("");
    t.strip_prefix("0x").map_or_else(|| t.parse().unwrap(), |h| u64::from_str_radix(h, 16).unwrap())
}

#[test]
fn readelf_h_parity() {
    // no spec clause: readelf -h parity (see the file comment).
    let Some(c) = corpus() else { return };
    for m in c {
        let bytes = std::fs::read(&m.path).unwrap();
        let h = *Elf::parse::<X86_64>(&bytes).unwrap().header();
        let out = readelf("-h", &m.path);
        let kv: Vec<(&str, &str)> = out.lines().filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim(), v.trim())).collect();
        let get = |k: &str, nth: usize| kv.iter().filter(|(key, _)| *key == k).nth(nth).map(|(_, v)| *v).unwrap_or_else(|| panic!("{}: no {k}", m.name));
        let ctx = m.name;
        assert_eq!(get("Class", 0), "ELF64", "{ctx}");
        assert_eq!(get("Data", 0), "2's complement, little endian", "{ctx}");
        assert_eq!(num(get("Version", 0)), u64::from(h.ident_version), "{ctx}");
        assert_eq!(num(get("Version", 1)), u64::from(h.version), "{ctx}");
        let osabi = match h.osabi {
            0 => "UNIX - System V",
            3 => "UNIX - GNU",
            o => panic!("{ctx}: osabi {o}"),
        };
        assert_eq!(get("OS/ABI", 0), osabi, "{ctx}");
        assert_eq!(num(get("ABI Version", 0)), u64::from(h.abi_version), "{ctx}");
        let ty = match h.e_type {
            2 => "EXEC",
            3 => "DYN",
            t => panic!("{ctx}: type {t}"),
        };
        assert_eq!(get("Type", 0).split_whitespace().next(), Some(ty), "{ctx}");
        assert_eq!(get("Machine", 0), "Advanced Micro Devices X86-64", "{ctx}");
        for (k, v) in [
            ("Entry point address", h.entry),
            ("Start of program headers", h.phoff),
            ("Start of section headers", h.shoff),
            ("Flags", u64::from(h.flags)),
            ("Size of this header", u64::from(h.ehsize)),
            ("Size of program headers", u64::from(h.phentsize)),
            ("Number of program headers", u64::from(h.phnum)),
            ("Size of section headers", u64::from(h.shentsize)),
            ("Number of section headers", u64::from(h.shnum)),
            ("Section header string table index", u64::from(h.shstrndx)),
        ] {
            assert_eq!(num(get(k, 0)), v, "{ctx}: {k}");
        }
    }
}

fn type_name(t: u32) -> &'static str {
    match t {
        pt::LOAD => "LOAD",
        pt::DYNAMIC => "DYNAMIC",
        pt::INTERP => "INTERP",
        pt::NOTE => "NOTE",
        pt::PHDR => "PHDR",
        pt::TLS => "TLS",
        pt::GNU_EH_FRAME => "GNU_EH_FRAME",
        pt::GNU_STACK => "GNU_STACK",
        pt::GNU_RELRO => "GNU_RELRO",
        pt::GNU_PROPERTY => "GNU_PROPERTY",
        t => panic!("program header type {t:#x} not in the corpus table"),
    }
}

#[test]
fn readelf_l_parity() {
    // no spec clause: readelf -l parity (see the file comment).
    let Some(c) = corpus() else { return };
    for m in c {
        let bytes = std::fs::read(&m.path).unwrap();
        let elf = Elf::parse::<X86_64>(&bytes).unwrap();
        let out = readelf("-l", &m.path);
        let ctx = m.name;
        let entry = out.lines().find_map(|l| l.strip_prefix("Entry point ")).unwrap();
        assert_eq!(num(entry), elf.header().entry, "{ctx}");
        let there = out.lines().find(|l| l.starts_with("There are")).unwrap();
        let w: Vec<&str> = there.split_whitespace().collect();
        assert_eq!((num(w[2]), num(w[8])), (u64::from(elf.header().phnum), elf.header().phoff), "{ctx}: {there}");
        let rows: Vec<Vec<&str>> = out
            .lines()
            .skip_while(|l| !l.starts_with("Program Headers:"))
            .skip(2)
            .take_while(|l| !l.trim().is_empty())
            .filter(|l| !l.trim_start().starts_with('['))
            .map(|l| l.split_whitespace().collect())
            .collect();
        let phs: Vec<_> = elf.program_headers().collect();
        assert_eq!(rows.len(), phs.len(), "{ctx}");
        for (r, p) in rows.iter().zip(&phs) {
            let flags: String = [(4, 'R'), (2, 'W'), (1, 'E')].iter().filter(|(b, _)| p.flags & b != 0).map(|(_, c)| *c).collect();
            assert_eq!(r[0], type_name(p.p_type), "{ctx}: {r:?}");
            assert_eq!([num(r[1]), num(r[2]), num(r[3]), num(r[4]), num(r[5])], [p.offset, p.vaddr, p.paddr, p.filesz, p.memsz], "{ctx}: {r:?}");
            assert_eq!(r[6..r.len() - 1].concat(), flags, "{ctx}: {r:?}");
            assert_eq!(num(r[r.len() - 1]), p.align, "{ctx}: {r:?}");
        }
        let req = out.lines().find_map(|l| l.trim().strip_prefix("[Requesting program interpreter: ")).map(|s| s.trim_end_matches(']'));
        assert_eq!(req.map(str::as_bytes), elf.interp().unwrap(), "{ctx}");
        assert_eq!(elf.interp().unwrap().is_some(), m.name == "pie" || m.name == "nopie-rpath", "{ctx}: corpus shape");
    }
}

#[test]
fn readelf_d_parity() {
    // no spec clause: readelf -d parity (see the file comment).
    let Some(c) = corpus() else { return };
    for m in c {
        let bytes = std::fs::read(&m.path).unwrap();
        let elf = Elf::parse::<X86_64>(&bytes).unwrap();
        let out = readelf("-d", &m.path);
        let ctx = m.name;
        let Some(d) = elf.dynamic().unwrap() else {
            assert!(out.contains("There is no dynamic section"), "{ctx}: {out}");
            assert_eq!(m.name, "static", "corpus shape");
            continue;
        };
        let head = out.lines().find(|l| l.starts_with("Dynamic section at offset")).unwrap();
        let w: Vec<&str> = head.split_whitespace().collect();
        assert_eq!((num(w[4]), num(w[6])), (elf.find(pt::DYNAMIC).unwrap().offset, d.len() as u64), "{ctx}: {head}");
        let rows: Vec<&str> = out.lines().map(str::trim).filter(|l| l.starts_with("0x")).collect();
        let entries: Vec<_> = d.entries().collect();
        assert_eq!(rows.len(), entries.len(), "{ctx}");
        let mut strings = 0;
        for (r, e) in rows.iter().zip(&entries) {
            let (tag, rest) = r.split_once(' ').unwrap();
            assert_eq!(num(tag), e.tag, "{ctx}: {r}");
            let value = rest.split_once(')').unwrap().1.trim();
            if let (Some(open), true) = (value.find('['), value.ends_with(']')) {
                assert_eq!(d.string(e.val).unwrap(), &value.as_bytes()[open + 1..value.len() - 1], "{ctx}: {r}");
                strings += 1;
            } else if value.starts_with("0x") || value.split_whitespace().next().is_some_and(|t| t.bytes().all(|b| b.is_ascii_digit())) {
                assert_eq!(num(value), e.val, "{ctx}: {r}");
            }
        }
        let has = |t: u64| d.get(t).is_some();
        let shape = match m.name {
            "pie" => has(dt::NEEDED) && has(0x24) && has(dt::STRTAB),
            "nopie-rpath" => has(dt::NEEDED) && has(dt::RPATH),
            "shared" => has(dt::SONAME) && has(dt::RUNPATH),
            _ => true,
        };
        assert!(shape && (strings > 0 || m.name == "shared-nonotes"), "{ctx}: corpus shape ({strings} strings)");
    }
}

/// One `readelf -n` block: (file offset, length) of the segment and the whitespace-split note rows.
type NoteBlock<'a> = ((u64, u64), Vec<Vec<&'a str>>);

fn note_type(name: &[u8], t: u32) -> &'static str {
    match (name, t) {
        (b"GNU", 1) => "NT_GNU_ABI_TAG",
        (b"GNU", 2) => "NT_GNU_HWCAP",
        (b"GNU", 3) => "NT_GNU_BUILD_ID",
        (b"GNU", 4) => "NT_GNU_GOLD_VERSION",
        (b"GNU", 5) => "NT_GNU_PROPERTY_TYPE_0",
        _ => panic!("note {:?} type {t} not in the corpus table", String::from_utf8_lossy(name)),
    }
}

#[test]
fn readelf_n_parity() {
    // no spec clause: readelf -n parity (see the file comment).
    let Some(c) = corpus() else { return };
    for m in c {
        let bytes = std::fs::read(&m.stripped).unwrap();
        let elf = Elf::parse::<X86_64>(&bytes).unwrap();
        let out = readelf("-n", &m.stripped);
        let ctx = m.name;
        // readelf: one block per segment, each with its rows.
        let mut blocks: Vec<NoteBlock<'_>> = Vec::new();
        for l in out.lines() {
            if let Some(rest) = l.strip_prefix("Displaying notes found at file offset ") {
                let w: Vec<&str> = rest.split_whitespace().collect();
                blocks.push(((num(w[0]), num(w[3].trim_end_matches(':'))), Vec::new()));
            } else if l.starts_with("  ") && !l.trim_start().starts_with("Owner") && !l.trim().is_empty() {
                blocks.last_mut().unwrap().1.push(l.split_whitespace().collect());
            }
        }
        let segs: Vec<_> = elf.program_headers().filter(|p| p.p_type == pt::NOTE).collect();
        assert_eq!(blocks.len(), segs.len(), "{ctx}: {out}");
        assert_eq!(segs.is_empty(), m.name == "shared-nonotes", "{ctx}: corpus shape");
        for ((range, rows), p) in blocks.iter().zip(&segs) {
            assert_eq!(*range, (p.offset, p.filesz), "{ctx}");
            let notes: Vec<_> = elf.notes(p).unwrap().map(Result::unwrap).collect();
            assert_eq!(rows.len(), notes.len(), "{ctx}: {rows:?}");
            for (r, n) in rows.iter().zip(&notes) {
                assert_eq!(r[0].as_bytes(), n.name, "{ctx}: {r:?}");
                assert_eq!(num(r[1]), n.desc.len() as u64, "{ctx}: {r:?}");
                assert_eq!(r[2], note_type(n.name, n.n_type), "{ctx}: {r:?}");
                let line = r.join(" ");
                if let Some(id) = line.split("Build ID: ").nth(1) {
                    let hex: String = n.desc.iter().map(|b| format!("{b:02x}")).collect();
                    assert_eq!(id, hex, "{ctx}");
                }
                if let Some(abi) = line.split("OS: Linux, ABI: ").nth(1) {
                    let w: Vec<u32> = n.desc.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
                    assert_eq!((w[0], abi.to_string()), (0, format!("{}.{}.{}", w[1], w[2], w[3])), "{ctx}");
                }
                if n.n_type == 5 {
                    let props: Vec<_> = n.properties().map(Result::unwrap).collect();
                    assert!(!props.is_empty(), "{ctx}: an empty property note");
                }
            }
        }
    }
}

/// The `readelf -r` name of an x86_64 relocation kind (the x86-64 psABI names).
fn reloc_name(k: RelocKind) -> &'static str {
    match k {
        RelocKind::None => "R_X86_64_NONE",
        RelocKind::Relative => "R_X86_64_RELATIVE",
        RelocKind::Abs64 => "R_X86_64_64",
        RelocKind::GlobDat => "R_X86_64_GLOB_DAT",
        RelocKind::JumpSlot => "R_X86_64_JUMP_SLOT",
        RelocKind::Copy => "R_X86_64_COPY",
        RelocKind::Irelative => "R_X86_64_IRELATIVE",
        RelocKind::DtpMod => "R_X86_64_DTPMOD64",
        RelocKind::DtpOff => "R_X86_64_DTPOFF64",
        RelocKind::TpOff => "R_X86_64_TPOFF64",
        RelocKind::TlsDesc => "R_X86_64_TLSDESC",
        RelocKind::TextRel { pc_relative: true, .. } => "R_X86_64_PC32",
        RelocKind::TextRel { signed: true, .. } => "R_X86_64_32S",
        RelocKind::TextRel { .. } => "R_X86_64_32",
        RelocKind::Unsupported(_) => "?",
    }
}

/// The rows of each `readelf` section block whose header line contains `start`, keyed by section name.
fn blocks(text: &str, start: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines() {
        if line.contains(start) {
            let name = line.split('\'').nth(1).unwrap_or("").to_string();
            out.push((name, Vec::new()));
        } else if let Some(b) = out.last_mut() {
            if !line.trim().is_empty() {
                b.1.push(line.to_string());
            }
        }
    }
    out
}

#[test]
fn readelf_r_parity() {
    // no spec clause: readelf -r parity of the RELA, PLT and RELR iterators (see the file comment).
    let Some(corpus) = corpus() else { return };
    let mut seen_relr = 0;
    for m in corpus {
        let bytes = std::fs::read(&m.path).unwrap();
        let elf = Elf::parse::<X86_64>(&bytes).unwrap();
        let Some(d) = elf.dynamic().unwrap() else { continue };
        let text = readelf("-r", &m.path);
        let mut ours: Vec<(String, Vec<String>)> = Vec::new();
        for (sec, it) in [(".rela.dyn", d.rela::<X86_64>().unwrap()), (".rela.plt", d.jmprel::<X86_64>().unwrap())] {
            let rows = it.into_iter().flatten().map(|r| {
                assert!(!matches!(r.kind, RelocKind::Unsupported(_)), "{}: type {} unsupported", m.name, r.r_type);
                format!("{:016x} {:08x}{:08x} {}", r.offset, r.sym, r.r_type, reloc_name(r.kind))
            });
            let rows: Vec<String> = rows.collect();
            if !rows.is_empty() {
                ours.push((sec.to_string(), rows));
            }
        }
        if let Some(r) = d.relr().unwrap() {
            let addrs: Vec<String> = r.map(|a| format!("{:016x}", a.unwrap())).collect();
            ours.push((".relr.dyn".to_string(), addrs));
            seen_relr += 1;
        }
        let theirs: Vec<(String, Vec<String>)> = blocks(&text, "Relocation section")
            .into_iter()
            .map(|(name, rows)| {
                let rows = if name.starts_with(".relr") {
                    rows.iter().filter(|l| !l.contains("offsets")).map(|l| l.split_whitespace().next().unwrap().to_string()).collect()
                } else {
                    rows.iter().skip(1).map(|l| l.split_whitespace().take(3).collect::<Vec<_>>().join(" ")).collect()
                };
                (name, rows)
            })
            .collect();
        assert_eq!(ours, theirs, "{}: readelf -r", m.name);
    }
    assert!(seen_relr >= 2, "the corpus has RELR members");
}

#[test]
fn readelf_v_and_dyn_syms_parity() {
    // no spec clause: readelf -V parity of versym/verdef/verneed and --dyn-syms parity of the hash-table
    // symbol counts (see the file comment).
    let Some(corpus) = corpus() else { return };
    let mut seen = 0;
    for m in corpus {
        let bytes = std::fs::read(&m.path).unwrap();
        let elf = Elf::parse::<X86_64>(&bytes).unwrap();
        let Some(d) = elf.dynamic().unwrap() else { continue };
        let syms = readelf("--dyn-syms", &m.path);
        let n: u32 = syms.lines().find(|l| l.contains("contains")).map_or(0, |l| num(l.split("contains ").nth(1).unwrap()) as u32);
        assert_eq!(d.symbol_count().unwrap(), Some(n), "{}: --dyn-syms count", m.name);
        if let Some(g) = d.gnu_hash().unwrap() {
            assert_eq!(g.symbol_count().unwrap(), n, "{}: GNU hash count", m.name);
        }
        if let Some(h) = d.sysv_hash().unwrap() {
            assert_eq!(h.nchain(), n, "{}: SysV nchain", m.name);
        }
        let text = readelf("-V", &m.path);
        let blk = blocks(&text, " section '");
        // Version names by index, from the reader.
        let mut names = std::collections::BTreeMap::new();
        let mut ours_d = Vec::new();
        if let Some(defs) = d.verdef().unwrap() {
            for v in defs {
                let v = v.unwrap();
                let ns: Vec<String> = v.names().map(|a| String::from_utf8(a.unwrap().name.to_vec()).unwrap()).collect();
                let flags = match v.flags { 0 => "none", 1 => "BASE", 2 => "WEAK", _ => "?" };
                ours_d.push(format!("{} {} {} {} {}", v.offset, flags, v.ndx, v.cnt, ns[0]));
                for (k, p) in ns.iter().enumerate().skip(1) {
                    ours_d.push(format!("Parent {k}: {p}"));
                }
                names.insert(v.ndx, ns[0].clone());
            }
        }
        let mut ours_n = Vec::new();
        if let Some(needs) = d.verneed().unwrap() {
            for v in needs {
                let v = v.unwrap();
                ours_n.push(format!("{} {} {}", v.offset, String::from_utf8_lossy(v.file), v.cnt));
                for a in v.auxes() {
                    let a = a.unwrap();
                    let name = String::from_utf8(a.name.to_vec()).unwrap();
                    ours_n.push(format!("{} {} {}", a.offset, name, a.other));
                    names.insert(a.other, name);
                }
            }
        }
        let rows = |sec: &str| blk.iter().find(|b| b.0 == sec).map(|b| b.1.iter().skip(1).cloned().collect::<Vec<_>>()).unwrap_or_default();
        let field = |l: &str, key: &str| l.split(key).nth(1).unwrap().split_whitespace().next().unwrap().to_string();
        let off = |l: &str| num(l.trim_start().split(':').next().unwrap()).to_string();
        let theirs_d: Vec<String> = rows(".gnu.version_d")
            .iter()
            .map(|l| if l.contains("Parent") { l.split_once(": ").unwrap().1.trim().to_string() } else { format!("{} {} {} {} {}", off(l), field(l, "Flags: "), field(l, "Index: "), field(l, "Cnt: "), field(l, "Name: ")) })
            .collect();
        assert_eq!(ours_d, theirs_d, "{}: readelf -V definitions", m.name);
        let theirs_n: Vec<String> = rows(".gnu.version_r")
            .iter()
            .map(|l| if l.contains("File: ") { format!("{} {} {}", off(l), field(l, "File: "), field(l, "Cnt: ")) } else { format!("{} {} {}", off(l), field(l, "Name: "), field(l, "Version: ")) })
            .collect();
        assert_eq!(ours_n, theirs_n, "{}: readelf -V requirements", m.name);
        let Some(vs) = d.versym().unwrap() else { continue };
        seen += 1;
        let ours_s: Vec<String> = (0..vs.count())
            .map(|i| {
                let v = vs.get(i).unwrap();
                let name = match v.index { 0 => "*local*".to_string(), 1 => "*global*".to_string(), k => names[&k].clone() };
                format!("{}{}({name})", v.index, if v.hidden { "h" } else { "" })
            })
            .collect();
        let theirs_s: Vec<String> = rows(".gnu.version")
            .iter()
            .flat_map(|l| l.split_once(':').unwrap().1.split(')').map(|c| c.replace(' ', "")).filter(|c| !c.is_empty()).map(|c| format!("{c})")).collect::<Vec<_>>())
            .collect();
        assert_eq!(ours_s, theirs_s, "{}: readelf -V versym", m.name);
    }
    assert!(seen >= 3, "the corpus has versioned members");
}
