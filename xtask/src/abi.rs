//! `cargo xtask abi-gen [--check]`: generate `liella_abi::layout` (Rust definitions of the spec
//! Appendix C types) and the Appendix C layout tests from `spec/abi/layouts.tsv`.
//!
//! The generated definitions carry **types, not offsets**: every member is declared with the
//! Rust type of its C type (`int` → `i32`, `char *` → `usize`, `struct timespec` → the generated
//! `struct_timespec`, nested anonymous structs and unions → generated types), and `#[repr(C)]`
//! lets rustc lay them out. The tests then compare rustc's `size_of`, `align_of` and
//! `offset_of!` with the table, which comes from the reference system's headers (owner decision
//! OD-1). A wrong type mapping, a missing member or a wrong union/struct decision therefore fails
//! a test; nothing is padded to make the numbers agree.
//!
//! Where the table gives a member's C type but neither a mapping nor sub-members (a handful of
//! typedefs such as `gregset_t`), the member is declared as an array of the widest integer that
//! divides both its offset and its extent. Those members are listed in the generated module
//! documentation as *opaque*, because their inner layout is not checked.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
struct Row {
    path: Vec<String>,
    offset: u64,
    ctype: String,
    bitfield: bool,
    bits: u64,
}

#[derive(Debug)]
struct Ty {
    name: String,
    size: u64,
    align: u64,
    rows: Vec<Row>,
}

fn load(root: &Path) -> Result<Vec<Ty>, String> {
    let text = fs::read_to_string(root.join("spec/abi/layouts.tsv")).map_err(|e| format!("layouts.tsv: {e}"))?;
    let mut out: Vec<Ty> = Vec::new();
    for (n, line) in text.lines().enumerate().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 5 {
            return Err(format!("layouts.tsv:{}: short row", n + 1));
        }
        let num = |s: &str| s.trim().parse::<u64>().map_err(|_| format!("layouts.tsv:{}: bad number `{s}`", n + 1));
        if f[1].is_empty() {
            out.push(Ty { name: f[0].to_string(), size: num(f[2])?, align: num(f[3])?, rows: Vec::new() });
        } else {
            let ty = out.last_mut().ok_or(format!("layouts.tsv:{}: member before type", n + 1))?;
            ty.rows.push(Row {
                path: f[1].split('.').map(str::to_string).collect(),
                offset: num(f[2])?,
                ctype: f[4].trim().to_string(),
                bitfield: f.get(5).is_some_and(|b| !b.trim().is_empty()),
                bits: f.get(5).and_then(|b| b.trim().strip_prefix("bits ")).and_then(|b| b.split('@').next()).and_then(|w| w.parse().ok()).unwrap_or(0),
            });
        }
    }
    Ok(out)
}

/// Rust spelling of a C type name: `struct sigaction` → `struct_sigaction`.
pub fn rust_name(c: &str) -> String {
    c.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }).collect()
}

const KEYWORDS: &[&str] = &["type", "ref", "fn", "in", "match", "mod", "move", "use", "where", "loop", "box", "crate", "self", "super", "static", "const", "impl", "trait", "true", "false", "async", "await", "dyn", "struct", "enum", "union", "let", "if", "else", "for", "while", "return", "break", "continue", "as", "extern", "pub", "unsafe", "priv", "override", "macro", "final", "yield", "virtual", "typeof", "unsized", "abstract", "become", "do", "try", "gen"];

fn field(name: &str) -> String {
    if KEYWORDS.contains(&name) { format!("r#{name}") } else { name.to_string() }
}

/// Scalar C types and their Rust equivalents on x86_64 (System V ABI, LP64).
fn scalar(c: &str) -> Option<&'static str> {
    let c = c.trim_start_matches("const ").trim_start_matches("volatile ").trim();
    Some(match c {
        "char" | "signed char" | "__int8_t" | "int8_t" => "i8",
        "unsigned char" | "uint8_t" | "__uint8_t" | "__u8" | "cc_t" | "u_char" | "u_int8_t" => "u8",
        "short int" | "int16_t" | "__int16_t" | "__s16" => "i16",
        "short unsigned int" | "uint16_t" | "__uint16_t" | "__u16" | "sa_family_t" | "in_port_t" | "u_short" | "u_int16_t" => "u16",
        "int" | "int32_t" | "__int32_t" | "__s32" | "__pid_t" | "pid_t" | "__key_t" | "regoff_t" | "wchar_t" | "__clockid_t" | "clockid_t" | "__daddr_t" | "__ssize_t32" | "__sig_atomic_t" | "_Bool32" => "i32",
        "unsigned int" | "uint32_t" | "__uint32_t" | "__u32" | "__uid_t" | "__gid_t" | "uid_t" | "gid_t" | "__mode_t" | "mode_t" | "tcflag_t" | "speed_t" | "in_addr_t" | "socklen_t" | "__socklen_t" | "__useconds_t" | "u_int" | "u_int32_t" | "__id_t" | "id_t" | "nfds_t32" => "u32",
        "long int" | "long long int" | "int64_t" | "__int64_t" | "__s64" | "__time_t" | "time_t" | "__clock_t" | "clock_t" | "__suseconds_t" | "suseconds_t" | "__syscall_slong_t" | "__off_t" | "__off64_t" | "off_t" | "off64_t" | "__fsword_t" | "__blksize_t" | "__blkcnt_t" | "__blkcnt64_t" | "__fd_mask" | "ssize_t" | "__ssize_t" | "ptrdiff_t" | "intptr_t" | "__intptr_t" | "__quad_t" | "Elf64_Sxword" | "__int128_low" | "long" => "i64",
        "long unsigned int" | "long long unsigned int" | "uint64_t" | "__uint64_t" | "__u64" | "size_t" | "__syscall_ulong_t" | "__kernel_ulong_t" | "__fsblkcnt_t" | "__fsfilcnt_t" | "__fsblkcnt64_t" | "__fsfilcnt64_t" | "__dev_t" | "dev_t" | "__ino_t" | "__ino64_t" | "ino_t" | "__nlink_t" | "nlink_t" | "rlim_t" | "__rlim_t" | "__rlim64_t" | "__re_long_size_t" | "Elf64_Addr" | "Elf64_Off" | "Elf64_Xword" | "uintptr_t" | "pthread_t" | "__cpu_mask" | "u_long" | "__u_quad_t" | "nfds_t" | "__ino64" | "Lmid_t_u" => "u64",
        "__kernel_long_t" | "reg_syntax_t" | "__caddr_t_int" => "i64",
        "msgqnum_t" | "msglen_t" | "shmatt_t" | "__kernel_ulong_t2" => "u64",
        "__re_size_t" | "ns_sect" | "enum <anon>" => "u32",
        "Elf64_Half" => "u16",
        "long double" => "u128",
        "float" => "f32",
        "double" => "f64",
        "_Bool" | "bool" => "u8",
        _ => return None,
    })
}

/// Typedefs of the reference headers that name a table type or a fixed array (⚑ ABI).
fn alias(c: &str) -> Option<&'static str> {
    Some(match c {
        "__sigval_t" | "sigval_t" => "union_sigval",
        "__jmp_buf" => "[i64; 8]",
        "gregset_t" => "[i64; 23]",
        "fpregset_t" | "__caddr_t" => "usize",
        "__sigset_t" => "sigset_t",
        _ => return None,
    })
}

/// Unnamed members at the end of a type (`int :32;` padding in the headers), which the table
/// cannot list because they have no name: (type, Rust declaration of the padding).
fn unnamed_tail(ty: &str) -> Option<&'static str> {
    match ty {
        // <sys/timex.h>: `int  :32; int  :32; ...` eleven times after `tai`.
        "struct timex" => Some("[i32; 11]"),
        _ => None,
    }
}

/// Split `T[n]` / `T[]` into (T, Some(n) / Some(0)).
fn split_array(c: &str) -> (String, Option<u64>) {
    if let Some(open) = c.rfind('[') {
        if c.ends_with(']') {
            let inner = &c[open + 1..c.len() - 1];
            let n = if inner.is_empty() { 0 } else { inner.parse().unwrap_or(0) };
            let (base, outer) = split_array(c[..open].trim());
            return match outer {
                // multi-dimensional: T[a][b] — fold into one count
                Some(m) => (base, Some(m * n)),
                None => (base, Some(n)),
            };
        }
    }
    (c.to_string(), None)
}

struct Gen<'a> {
    known: &'a BTreeMap<String, ()>,
    out: String,
    opaque: Vec<String>,
    /// Unnamed tail padding for the composite being emitted next (top level only).
    tail: Option<&'static str>,
    /// Types whose C alignment Rust cannot express (see `render`).
    underaligned: Vec<(String, u64, u64)>,
}

/// A node in the member tree of one type.
#[derive(Debug)]
struct Node {
    /// Rust field name (a synthesized `__anon_<offset>` for C11 anonymous members).
    name: String,
    /// The member's name in the table, or `None` for a synthesized grouping node.
    c_name: Option<String>,
    offset: u64,
    ctype: String,
    bitfield: bool,
    /// For a bitfield group: the total width in bits of the consecutive bitfields it stands for.
    bits: u64,
    children: Vec<Node>,
}

fn build(rows: &[Row], depth: usize) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::new();
    let level: Vec<&Row> = rows.iter().filter(|r| r.path.len() == depth + 1).collect();
    let mut i = 0;
    while i < level.len() {
        let r = level[i];
        i += 1;
        // Consecutive bitfields sharing a storage unit form one group: the first stands for all,
        // with the summed width.
        if r.bitfield {
            if let Some(l) = out.last_mut().filter(|l| l.bitfield && l.offset == r.offset) {
                l.bits += r.bits;
                continue;
            }
        }
        // A C11 anonymous union: the table lists `<anon>` and then its members at this level,
        // all at the same offset. Group them under a synthesized field.
        if r.path[depth] == "<anon>" && r.ctype.starts_with("union") {
            let mut children = Vec::new();
            while i < level.len() && level[i].offset == r.offset && level[i].path[depth] != "<anon>" {
                let m = level[i];
                children.push(Node { name: m.path[depth].clone(), c_name: Some(m.path[depth].clone()), offset: m.offset, ctype: m.ctype.clone(), bitfield: m.bitfield, bits: m.bits, children: Vec::new() });
                i += 1;
            }
            out.push(Node { name: format!("__anon_{}", r.offset), c_name: None, offset: r.offset, ctype: r.ctype.clone(), bitfield: false, bits: 0, children });
            continue;
        }
        let prefix = &r.path[..=depth];
        let sub: Vec<Row> = rows.iter().filter(|c| c.path.len() > depth + 1 && c.path[..=depth] == *prefix).cloned().collect();
        out.push(Node { name: r.path[depth].clone(), c_name: Some(r.path[depth].clone()), offset: r.offset, ctype: r.ctype.clone(), bitfield: r.bitfield, bits: r.bits, children: build(&sub, depth + 1) });
    }
    out
}

fn is_union(node_ctype: &str, children: &[Node]) -> bool {
    node_ctype.starts_with("union") || (children.len() > 1 && children.iter().all(|c| c.offset == children[0].offset))
}

impl Gen<'_> {
    /// Rust type for a leaf C type, or None if unknown.
    fn leaf(&self, c: &str) -> Option<String> {
        let (base, count) = split_array(c);
        let base = base.trim();
        let elem = if base.ends_with('*') || base.starts_with("fn(") || base.contains("(*)") || base == "__sighandler_t" || base == "__compar_fn_t" {
            "usize".to_string()
        } else if let Some(s) = scalar(base) {
            s.to_string()
        } else if self.known.contains_key(base) {
            rust_name(base)
        } else {
            alias(base)?.to_string()
        };
        Some(match count {
            Some(n) => format!("[{elem}; {n}]"),
            None => elem,
        })
    }

    /// An opaque stand-in of `size` bytes at `offset`: the widest integer dividing both.
    fn opaque(&mut self, what: String, offset: u64, size: u64) -> String {
        self.opaque.push(what);
        let w = [8u64, 4, 2, 1].into_iter().find(|w| offset % w == 0 && size % w == 0).unwrap_or(1);
        let t = match w { 8 => "u64", 4 => "u32", 2 => "u16", _ => "u8" };
        format!("[{t}; {}]", size / w)
    }

    /// Emit a composite type `name` for `children` (a struct or union), returning its Rust name.
    fn composite(&mut self, name: &str, union: bool, children: &[Node], end: u64, align: Option<u64>, packed: bool) -> String {
        // Taken first: nested composites are emitted while the fields are resolved.
        let tail = self.tail.take();
        let mut fields = Vec::new();
        let mut sorted: Vec<&Node> = children.iter().collect();
        if !union {
            sorted.sort_by_key(|c| c.offset);
        }
        for (i, c) in sorted.iter().enumerate() {
            let extent_end = if union { end } else { sorted[i + 1..].iter().map(|n| n.offset).find(|&o| o > c.offset).unwrap_or(end) };
            let ty = self.member_type(name, c, extent_end);
            fields.push((field(&c.name), ty));
        }
        let kw = if union { "union" } else { "struct" };
        let mut attrs = String::from("C");
        if packed {
            attrs.push_str(", packed");
        } else if let Some(a) = align {
            if a > 1 {
                let _ = write!(attrs, ", align({a})");
            }
        }
        let _ = writeln!(self.out, "#[repr({attrs})]\n#[derive(Clone, Copy)]\npub {kw} {name} {{");
        for (f, t) in &fields {
            let _ = writeln!(self.out, "    pub {f}: {t},");
        }
        if let Some(tail) = tail {
            let _ = writeln!(self.out, "    /// Unnamed padding members of the C definition.\n    pub __unnamed_tail: {tail},");
        }
        let _ = writeln!(self.out, "}}\n");
        name.to_string()
    }

    fn member_type(&mut self, parent: &str, c: &Node, extent_end: u64) -> String {
        let extent = extent_end.saturating_sub(c.offset);
        if c.bitfield {
            // A group of bitfields occupies the bytes its widths need, starting where the
            // previous member ended (the table gives only the storage unit's offset).
            return format!("[u8; {}]", c.bits.div_ceil(8).max(1));
        }
        if !c.children.is_empty() {
            let (base, count) = split_array(&c.ctype);
            if count.is_none() && self.known.contains_key(base.trim()) {
                return rust_name(base.trim());
            }
            if count.is_none() {
                let sub = format!("{parent}__{}", c.name.trim_start_matches('_'));
                let union = is_union(&base, &c.children);
                // Children offsets are absolute; rebase them to this member.
                let rebased: Vec<Node> = c.children.iter().map(|n| rebase(n, c.offset)).collect();
                return self.composite(&sub, union, &rebased, extent, None, false);
            }
        }
        if let Some(t) = self.leaf(&c.ctype) {
            return t;
        }
        self.opaque(format!("{parent}.{} ({})", c.name, c.ctype), c.offset, extent)
    }
}

fn rebase(n: &Node, base: u64) -> Node {
    Node {
        name: n.name.clone(),
        c_name: n.c_name.clone(),
        offset: n.offset - base,
        ctype: n.ctype.clone(),
        bitfield: n.bitfield,
        bits: n.bits,
        children: n.children.iter().map(|c| rebase(c, base)).collect(),
    }
}

fn render(root: &Path) -> Result<(String, String), String> {
    let types = load(root)?;
    let known: BTreeMap<String, ()> = types.iter().map(|t| (t.name.clone(), ())).collect();
    let mut g = Gen { known: &known, out: String::new(), opaque: Vec::new(), tail: None, underaligned: Vec::new() };
    for t in &types {
        let nodes = build(&t.rows, 0);
        let name = rust_name(&t.name);
        if nodes.is_empty() {
            // Scalar or pointer typedef (pthread_t, pthread_key_t, ...): a transparent newtype.
            let inner = match (t.size, t.align) {
                (8, 8) => "u64",
                (4, 4) => "u32",
                (2, 2) => "u16",
                (1, 1) => "u8",
                _ => return Err(format!("{}: no members and no scalar of size {} align {}", t.name, t.size, t.align)),
            };
            let _ = writeln!(g.out, "#[repr(transparent)]\n#[derive(Clone, Copy)]\npub struct {name}(pub {inner});\n");
            continue;
        }
        let union = t.name.starts_with("union ") || is_union("", &nodes);
        // Types whose alignment is smaller than their members' natural alignment are packed
        // (struct epoll_event on x86_64).
        let packed = t.align == 1 && t.size > 1 && nodes.iter().any(|n| scalar(&n.ctype).is_some_and(|s| s != "u8" && s != "i8"));
        // A typedef with an alignment attribute can have a size that is not a multiple of its
        // alignment (`__pthread_unwind_buf_t`: 104 bytes, aligned to 16). Rust cannot express
        // that; such types get the largest alignment that divides their size, and the tests
        // check the declared alignment separately (finding F51).
        let mut align = t.align;
        if t.size % align != 0 {
            align = [16u64, 8, 4, 2, 1].into_iter().find(|a| *a <= t.align && t.size % a == 0).unwrap_or(1);
            g.underaligned.push((t.name.clone(), t.align, align));
        }
        g.tail = unnamed_tail(&t.name);
        g.composite(&name, union, &nodes, t.size, Some(align), packed);
    }

    let mut layout = String::from("// @generated by `cargo xtask abi-gen` from spec/abi/layouts.tsv. Do not edit.\n\
//! Rust definitions of the glibc ABI types of spec Appendix C (x86_64).\n\
//!\n\
//! Members are declared with the Rust type of their C type and laid out by rustc (`#[repr(C)]`);\n\
//! `crates/abi/tests/appendix_c_layouts.rs` checks every size, alignment and member offset against\n\
//! the table. Pointers are `usize` (an address the boundary converts), C unions are Rust unions\n\
//! (declaring and building one is safe; reading a field is the boundary's business).\n//!\n");
    let _ = writeln!(layout, "//! Bitfields: each run of bitfields sharing a storage unit is one `[u8; n]` member named after\n//! its first bitfield, `n` covering the summed widths (the table gives only the storage unit's\n//! offset, so bitfield offsets are not tested; the members after them are).\n//!");
    let _ = writeln!(layout, "//! Opaque members (inner layout not checked; declared from their table extent):");
    for o in &g.opaque {
        let _ = writeln!(layout, "//! * `{o}`");
    }
    layout.push_str("\n#![allow(non_camel_case_types, non_snake_case, missing_docs, clippy::all)]\n\n");
    layout.push_str(&g.out);

    // Tests.
    let mut tests = String::from("// @generated by `cargo xtask abi-gen` from spec/abi/layouts.tsv (spec Appendix C).\n\
// Each test checks one type: rustc's size, alignment and member offsets of the\n\
// `liella_abi::layout` definition against the spec table, read at test time.\n\
\n\
//! ABI data tests, one per type layout and static initializer of spec Appendix C (171 items).\n\
\n\
#![allow(non_snake_case, clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]\n\
\n\
use core::mem::{align_of, offset_of, size_of};\n\
use liella_abi::layout::*;\n\
\n\
#[path = \"appendix_c/table.rs\"]\n\
mod table;\n\
use table::{expect_initializer, expect_member, expect_type};\n\n");
    for t in &types {
        let name = rust_name(&t.name);
        let test = crate::spec::collapse(&format!("abi_{}", crate::spec::ident_part(&t.name).trim_matches('_')));
        let _ = writeln!(tests, "#[test]\nfn {test}() {{");
        match g.underaligned.iter().find(|u| u.0 == t.name) {
            Some((_, c_align, rust_align)) => {
                let _ = writeln!(tests, "    // C: aligned({c_align}) on a typedef of a {}-byte struct; Rust cannot express a size", t.size);
                let _ = writeln!(tests, "    // that is not a multiple of the alignment, so Liella's type is aligned to {rust_align} and");
                let _ = writeln!(tests, "    // objects of it are only reached through C-allocated memory (finding F51).");
                let _ = writeln!(tests, "    table::expect_underaligned({:?}, size_of::<{name}>(), align_of::<{name}>());", t.name);
            }
            None => {
                let _ = writeln!(tests, "    expect_type({:?}, size_of::<{name}>(), align_of::<{name}>());", t.name);
            }
        }
        // Member offsets, following the generated structure (bitfield followers and members of
        // opaque or array-of-composite parents are not addressable).
        let nodes = build(&t.rows, 0);
        let mut paths = Vec::new();
        collect_paths(&nodes, &mut Vec::new(), &mut paths);
        for (dotted, rust_path) in paths {
            let _ = writeln!(tests, "    expect_member({:?}, {:?}, offset_of!({name}, {rust_path}));", t.name, dotted);
        }
        let _ = writeln!(tests, "}}\n");
    }
    // Static initializers (spec/abi/initializers.tsv): Liella's values, built from the typed
    // definitions in `liella_abi::layout_init`, against the reference bytes.
    let ini = fs::read_to_string(root.join("spec/abi/initializers.tsv")).map_err(|e| format!("initializers.tsv: {e}"))?;
    for line in ini.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 4 {
            continue;
        }
        let key = if f[1].starts_with('{') { f[0] } else { f[1] };
        let id = crate::spec::collapse(&format!("init_{}", crate::spec::ident_part(key).trim_matches('_')));
        let konst = rust_name(key).to_uppercase();
        let _ = writeln!(tests, "#[test]\nfn {id}() {{\n    expect_initializer({:?}, {:?}, &liella_abi::layout_init::{konst});\n}}\n", f[0], f[1]);
    }
    Ok((layout, tests))
}

fn collect_paths(nodes: &[Node], prefix: &mut Vec<(Option<String>, String)>, out: &mut Vec<(String, String)>) {
    for n in nodes {
        prefix.push((n.c_name.clone(), field(&n.name)));
        let dotted = prefix.iter().filter_map(|p| p.0.as_deref()).collect::<Vec<_>>().join(".");
        let rust = prefix.iter().map(|p| p.1.as_str()).collect::<Vec<_>>().join(".");
        if n.c_name.is_some() && !n.bitfield {
            out.push((dotted, rust));
        }
        let (_, count) = split_array(&n.ctype);
        // Recurse only into composites the generator emitted (not arrays, not opaque).
        if !n.children.is_empty() && count.is_none() {
            collect_paths(&n.children, prefix, out);
        }
        prefix.pop();
    }
}

pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let (layout, tests) = render(root)?;
    let files = [("crates/abi/src/layout.rs", layout), ("crates/abi/tests/appendix_c_layouts.rs", tests)];
    let mut stale = Vec::new();
    for (rel, content) in &files {
        let path = root.join(rel);
        if fs::read_to_string(&path).unwrap_or_default() != *content {
            if check {
                stale.push(*rel);
            } else {
                fs::write(&path, content).map_err(|e| format!("{rel}: {e}"))?;
            }
        }
    }
    if !stale.is_empty() {
        return Err(format!("generated files are stale (run `cargo xtask abi-gen`): {}", stale.join(", ")));
    }
    println!("abi-gen: {}", if check { "up to date" } else { "written" });
    Ok(())
}
