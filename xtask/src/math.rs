//! `cargo xtask math-gen [--check]`: the math translation layer (spec §10) from three tables.
//!
//! * `crates/boundary/src/posix/s10_math.tsv`: one body per family (the call into
//!   `liella-math`, D-10.1).
//! * `spec/abi/versions.tsv`: the symbols, which library exports each (`libm.so.6`, `libc.so.6`,
//!   or both) and their default versions. Compat-only versions wait for phase 15.
//! * `spec/abi/prototypes.tsv`: each symbol's C signature.
//!
//! Outputs: `crates/boundary/src/posix/s10_math_gen.rs` (one `export!` per symbol, gated by the
//! library that owns it: `libm`, `libc_libm` or the plain libc form) and the C forwarders for the
//! symbols whose signatures carry `long double`, `_Float64x` or `_Float128` by value, which Rust
//! cannot name (OD-C4): `csys/src/adapter/math_libc.c`, `math_both.c`, `math_libm.c`. A symbol
//! whose family has no row yet is left out and counted.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// A floating format.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fmt {
    F32,
    F64,
    F80,
    F128,
}

impl Fmt {
    fn of_suffix(s: &str) -> Option<Fmt> {
        Some(match s {
            "f" | "f32" => Fmt::F32,
            "" | "f64" | "f32x" => Fmt::F64,
            "l" | "f64x" => Fmt::F80,
            "f128" => Fmt::F128,
            _ => return None,
        })
    }
    /// The type `liella-math` computes in.
    fn internal(self) -> &'static str {
        match self {
            Fmt::F32 => "f32",
            Fmt::F64 => "f64",
            Fmt::F80 => "math::F80",
            Fmt::F128 => "math::F128",
        }
    }
    /// The Rust type at the C boundary.
    fn rust(self) -> &'static str {
        match self {
            Fmt::F32 => "f32",
            Fmt::F64 => "f64",
            Fmt::F80 => "LdBits",
            Fmt::F128 => "F128Bits",
        }
    }
    /// Passed through a C forwarder.
    fn bits(self) -> bool {
        matches!(self, Fmt::F80 | Fmt::F128)
    }
    fn c(self) -> &'static str {
        match self {
            Fmt::F32 => "float",
            Fmt::F64 => "double",
            Fmt::F80 => "long double",
            Fmt::F128 => "__float128",
        }
    }
    fn c_bits(self) -> &'static str {
        match self {
            Fmt::F80 => "struct liella_ld_bits",
            Fmt::F128 => "struct liella_f128_bits",
            _ => self.c(),
        }
    }
}

/// A C parameter or result type.
#[derive(Clone, Debug)]
enum CType {
    Void,
    Float(Fmt),
    /// An integer: its C spelling and Rust type.
    Int(&'static str, &'static str),
    /// A pointer to a float or `int`: pointee, mutable.
    PtrFloat(Fmt, bool),
    PtrInt(bool),
    /// `const char *`: a string.
    Str,
    /// `complex float`, `complex double` (`mathret::Complex`, two parts in SSE registers); a
    /// complex `long double` or `_Float128` goes through a C forwarder as a pair of bit structs
    /// (D-10.21).
    Complex(Fmt),
}

fn ctype(s: &str) -> Option<CType> {
    let s = s.trim();
    if let Some(base) = s.strip_prefix("complex ") {
        return match ctype(base)? {
            CType::Float(f) => Some(CType::Complex(f)),
            _ => None,
        };
    }
    let (base, ptr, constant) = match s.strip_suffix('*') {
        Some(b) => {
            let b = b.trim();
            match b.strip_prefix("const ") {
                Some(c) => (c.trim(), true, true),
                None => (b, true, false),
            }
        }
        None => (s, false, false),
    };
    let fmt = match base {
        "float" | "_Float32" => Some(Fmt::F32),
        "double" | "_Float64" | "_Float32x" => Some(Fmt::F64),
        "long double" | "_Float64x" => Some(Fmt::F80),
        "_Float128" => Some(Fmt::F128),
        _ => None,
    };
    Some(match (fmt, ptr) {
        (Some(f), false) => CType::Float(f),
        (Some(f), true) => CType::PtrFloat(f, !constant),
        (None, true) if base == "int" => CType::PtrInt(!constant),
        (None, true) if base == "char" && constant => CType::Str,
        (None, false) => match base {
            "void" => CType::Void,
            "int" => CType::Int("int", "c_int"),
            "unsigned int" => CType::Int("unsigned int", "c_uint"),
            "long int" => CType::Int("long", "c_long"),
            "long long int" => CType::Int("long long", "c_longlong"),
            "__intmax_t" => CType::Int("intmax_t", "i64"),
            "__uintmax_t" => CType::Int("uintmax_t", "u64"),
            _ => return None,
        },
        _ => return None,
    })
}

impl CType {
    fn rust(&self) -> String {
        match self {
            CType::Void => "()".into(),
            CType::Float(f) => f.rust().into(),
            CType::Int(_, r) => (*r).into(),
            CType::PtrFloat(f, m) => format!("Option<&{}{}>", if *m { "mut " } else { "" }, f.rust()),
            CType::PtrInt(m) => format!("Option<&{}c_int>", if *m { "mut " } else { "" }),
            CType::Str => "*const c_char".into(),
            CType::Complex(f) => format!("mret::Complex<{}>", f.rust()),
        }
    }
    fn needs_forwarder(&self) -> bool {
        matches!(self, CType::Float(f) | CType::Complex(f) if f.bits())
    }
    fn c(&self) -> String {
        match self {
            CType::Void => "void".into(),
            CType::Float(f) => f.c().into(),
            CType::Int(c, _) => (*c).into(),
            CType::PtrFloat(f, m) => format!("{}{} *", if *m { "" } else { "const " }, f.c()),
            CType::PtrInt(m) => format!("{}int *", if *m { "" } else { "const " }),
            CType::Str => "const char *".into(),
            // GCC accepts `_Float128 _Complex` but not `__float128 _Complex`.
            CType::Complex(Fmt::F128) => "_Float128 _Complex".into(),
            CType::Complex(f) => format!("{} _Complex", f.c()),
        }
    }
    /// The type the Rust side of a forwarder takes.
    fn c_bits(&self) -> String {
        match self {
            CType::Float(f) => f.c_bits().into(),
            CType::PtrFloat(f, m) => format!("{}{} *", if *m { "" } else { "const " }, f.c_bits()),
            CType::Complex(Fmt::F80) => "struct liella_ldc_bits".into(),
            CType::Complex(Fmt::F128) => "struct liella_f128c_bits".into(),
            _ => self.c(),
        }
    }
    /// A forwarder's argument expression for parameter `a`.
    fn c_arg(&self, a: &str) -> String {
        match self {
            CType::Float(Fmt::F80) => format!("LD_BITS({a})"),
            CType::Float(Fmt::F128) => format!("F128_BITS({a})"),
            CType::Complex(Fmt::F80) => format!("LDC_BITS({a})"),
            CType::Complex(Fmt::F128) => format!("F128C_BITS({a})"),
            CType::PtrFloat(f, _) if f.bits() => format!("({}){a}", self.c_bits()),
            _ => a.into(),
        }
    }
}

/// Which product exports a symbol.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
enum Owner {
    Libc,
    Both,
    Libm,
}

struct Sym {
    name: String,
    version: String,
    owner: Owner,
    ret: CType,
    params: Vec<CType>,
    body: String,
}

/// The `mathret` forms of the software paths that `mret::Soft` has methods for (D-10.22).
const SOFT_FORMS: [&str; 9] = ["ret", "ret_out", "remquo", "intmax", "uintmax", "kept", "pick", "step", "raw"];
/// The operand-returning forms that take the caller's raw `float`/`double` operands: they apply
/// its DAZ themselves (to compare), and return an operand unflushed (probed, D-10.22).
const RAW_FORMS: [&str; 3] = ["pick", "step", "raw"];

const SUFFIXES: [&str; 8]= ["f128", "f32x", "f64x", "f32", "f64", "f", "l", ""];

/// A parsed symbol name: the family row, the narrowing source and destination, the format.
type Parsed = (String, Option<(Fmt, Fmt)>, Fmt);

/// `(family, suffix)` of a regular symbol, or the narrowing operation, source and destination.
fn parse(name: &str, families: &BTreeMap<String, String>) -> Option<Parsed> {
    // Narrowing: <dest><op><source suffix>, e.g. fadd (double to float), daddl, f32mulf128.
    for dest in ["f32x", "f64x", "f32", "f64", "f", "d"] {
        let Some(rest) = name.strip_prefix(dest) else { continue };
        for op in ["add", "sub", "mul", "div", "sqrt", "fma"] {
            let Some(src) = rest.strip_prefix(op) else { continue };
            let d = match dest {
                "f" | "f32" => Fmt::F32,
                "d" | "f32x" | "f64" => Fmt::F64,
                _ => Fmt::F80,
            };
            let s = match src {
                "" if dest == "f" => Fmt::F64,
                "l" if matches!(dest, "f" | "d") => Fmt::F80,
                "f32x" | "f64" | "f64x" | "f128" if !matches!(dest, "f" | "d") => Fmt::of_suffix(src)?,
                _ => continue,
            };
            let key = format!("@{op}");
            if families.contains_key(&key) {
                return Some((key, Some((s, d)), s));
            }
        }
    }
    for suf in SUFFIXES {
        if let Some(fam) = name.strip_suffix(suf) {
            if families.contains_key(fam) {
                return Some((fam.to_string(), None, Fmt::of_suffix(suf)?));
            }
        }
    }
    // Reentrant forms put the format suffix before `_r` (`lgammaf_r`, family `lgamma_r`).
    if let Some(stem) = name.strip_suffix("_r") {
        for suf in SUFFIXES {
            if let Some(base) = stem.strip_suffix(suf) {
                let fam = format!("{base}_r");
                if families.contains_key(&fam) {
                    return Some((fam, None, Fmt::of_suffix(suf)?));
                }
            }
        }
    }
    None
}

fn tsv(path: &Path) -> Result<Vec<Vec<String>>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).map(|l| l.split('\t').map(str::to_string).collect()).collect())
}

/// Parse `extern RET NAME (ARGS)`.
fn prototype(decl: &str, name: &str) -> Option<(CType, Vec<CType>)> {
    let decl = decl.strip_prefix("extern ")?;
    let at = decl.find(&format!(" {name} ("))?;
    let ret = ctype(&decl[..at])?;
    let args = decl[at + name.len() + 2..].trim().strip_prefix('(')?.strip_suffix(')')?;
    let params = if args.trim() == "void" { Vec::new() } else { args.split(',').map(ctype).collect::<Option<Vec<_>>>()? };
    Some((ret, params))
}

fn generate(root: &Path) -> Result<(String, [String; 3], String), String> {
    let mut families = BTreeMap::new();
    // Formats a family is implemented for (f, d, l, q), when not all.
    let mut only: BTreeMap<String, String> = BTreeMap::new();
    // The body of a family for one format letter, from a row that names its formats.
    let mut per_fmt: BTreeMap<(String, char), String> = BTreeMap::new();
    for row in tsv(&root.join("crates/boundary/src/posix/s10_math.tsv"))? {
        match row.as_slice() {
            [fam, body] => {
                families.insert(fam.clone(), body.clone());
            }
            // A family may have several rows with disjoint formats, each with its own body (the
            // `float` exp/log fast entry, MATH-M6-ENVELOPE-COST-2): the family's formats are their union.
            [fam, fmts, body] => {
                families.entry(fam.clone()).or_insert_with(|| body.clone());
                only.entry(fam.clone()).or_default().push_str(fmts);
                for c in fmts.chars() {
                    per_fmt.insert((fam.clone(), c), body.clone());
                }
            }
            _ => {}
        }
    }
    let mut protos = BTreeMap::new();
    for row in tsv(&root.join("spec/abi/prototypes.tsv"))?.into_iter().skip(1) {
        if let [name, _, decl] = row.as_slice() {
            protos.insert(name.clone(), decl.clone());
        }
    }
    // name -> (libc version, libm version)
    let mut libs: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
    for row in tsv(&root.join("spec/abi/versions.tsv"))?.into_iter().skip(1) {
        if let [lib, sym, ver, binding, ..] = row.as_slice() {
            if binding != "default" {
                continue;
            }
            let e = libs.entry(sym.clone()).or_default();
            match lib.as_str() {
                "libc.so.6" => e.0 = Some(ver.clone()),
                "libm.so.6" => e.1 = Some(ver.clone()),
                _ => {}
            }
        }
    }
    let mut syms = Vec::new();
    let mut missing: BTreeMap<String, usize> = BTreeMap::new();
    for (name, (c, m)) in &libs {
        let Some((family, narrow, fmt)) = parse(name, &families) else {
            if m.is_some() {
                *missing.entry(name.clone()).or_default() += 1;
            }
            continue;
        };
        let letter = match fmt {
            Fmt::F32 => 'f',
            Fmt::F64 => 'd',
            Fmt::F80 => 'l',
            Fmt::F128 => 'q',
        };
        if only.get(&family).is_some_and(|f| !f.contains(letter)) {
            if m.is_some() {
                *missing.entry(name.clone()).or_default() += 1;
            }
            continue;
        }
        let owner = match (c, m) {
            (Some(_), Some(_)) => Owner::Both,
            (Some(_), None) => Owner::Libc,
            _ => Owner::Libm,
        };
        let version = m.clone().or_else(|| c.clone()).unwrap_or_default();
        let decl = protos.get(name).ok_or_else(|| format!("{name}: no prototype in spec/abi/prototypes.tsv"))?;
        let (ret, params) = prototype(decl, name).ok_or_else(|| format!("{name}: cannot parse `{decl}`"))?;
        // A `\n` in the table continues the body on the next line.
        let row = per_fmt.get(&(family.clone(), letter)).unwrap_or(&families[&family]);
        let mut body = row.replace("\\n", "\n");
        // A software form with a `float` or `double` operand or result runs under `mret::Soft`
        // (D-10.22): the caller's DAZ on those operands, its FTZ on the result, the internal
        // arithmetic in the default word.
        let is_sse = |t: &CType| matches!(t, CType::Float(Fmt::F32 | Fmt::F64));
        let soft = SOFT_FORMS.iter().any(|f| body.starts_with(&format!("mret::{f}("))) && (is_sse(&ret) || params.iter().any(is_sse));
        let raw = RAW_FORMS.iter().any(|f| body.starts_with(&format!("mret::{f}(")));
        for (i, p) in params.iter().enumerate() {
            let arg = ["x", "y", "z"].get(i).copied().ok_or_else(|| format!("{name}: too many parameters"))?;
            let conv = match p {
                CType::Float(Fmt::F80) => format!("mret::f80({arg})"),
                CType::Float(Fmt::F128) => format!("mret::f128({arg})"),
                p if soft && is_sse(p) && !raw => format!("c.daz({arg})"),
                _ => arg.to_string(),
            };
            body = body.replace(&format!("{{{arg}}}"), &conv);
        }
        let x87 = fmt == Fmt::F80 || narrow.is_some_and(|(s, d)| s == Fmt::F80 || d == Fmt::F80);
        let unit = if x87 { "mret::Unit::X87" } else { "mret::Unit::Sse" };
        if soft {
            body = format!("let c = mret::Soft::enter({unit});\n    c.{}", &body["mret::".len()..]).replace("{M}", "c.mode()");
        }
        body = body.replace("{M}", if x87 { "mret::mode87()" } else { "mret::mode()" }).replace("{T}", fmt.internal());
        body = body.replace("{E}", unit);
        if let Some((s, d)) = narrow {
            body = body.replace("{S}", s.internal()).replace("{D}", d.internal());
        }
        if ["{x}", "{y}", "{z}", "{T}", "{M}", "{E}", "{S}", "{D}"].iter().any(|p| body.contains(p)) {
            return Err(format!("{name}: unexpanded placeholder in `{body}`"));
        }
        syms.push(Sym { name: name.clone(), version, owner, ret, params, body });
    }

    // The Rust exports.
    let mut rs = String::new();
    rs.push_str("// @generated by `cargo xtask math-gen` from crates/boundary/src/posix/s10_math.tsv,\n");
    rs.push_str("// spec/abi/versions.tsv and spec/abi/prototypes.tsv. Do not edit.\n\n");
    rs.push_str("//! The math exports (spec §10): one per symbol, each a call into `liella-math` (D-10.1) with the\n");
    rs.push_str("//! conversions of `crate::mathret` (D-10.2). Symbols with `long double` or `_Float128` values are the\n");
    rs.push_str("//! `liella_<name>_bits` functions behind the C forwarders `csys/src/adapter/math_*.c` (OD-C4).\n");
    rs.push_str("#![allow(non_snake_case, clippy::too_many_lines)]\n\n");
    rs.push_str("use liella_abi::ctypes::{c_char, c_int, c_long, c_longlong, c_uint, F128Bits, LdBits};\n");
    rs.push_str("use liella_math as math;\n\n");
    rs.push_str("use crate::export;\nuse crate::mathret as mret;\nuse crate::ret;\nuse crate::strz;\n");
    let mut c_files: [String; 3] = Default::default();
    for s in &syms {
        let fwd = s.ret.needs_forwarder() || s.params.iter().any(CType::needs_forwarder);
        let rname = if fwd { format!("liella_{}_bits", s.name) } else { s.name.clone() };
        let gate = match s.owner {
            Owner::Libc => "",
            Owner::Both => "libc_libm ",
            Owner::Libm => "libm ",
        };
        let version = if fwd { "LIELLA_PRIVATE".to_string() } else { s.version.clone() };
        let params: Vec<String> = s.params.iter().enumerate().map(|(i, p)| format!("{}: {}", ["x", "y", "z"][i], p.rust())).collect();
        let ret = match s.ret {
            CType::Void => String::new(),
            ref r => format!(" -> {}", r.rust()),
        };
        let _ = write!(rs, "\nexport! {{ {gate}\"{version}\";\npub extern \"C\" fn {rname}({}){ret} {{\n    {}\n}}}}\n", params.join(", "), s.body);
        if fwd {
            let file = &mut c_files[match s.owner {
                Owner::Libc => 0,
                Owner::Both => 1,
                Owner::Libm => 2,
            }];
            let cparams: Vec<String> = s.params.iter().enumerate().map(|(i, p)| format!("{} a{i}", p.c())).collect();
            let bparams: Vec<String> = s.params.iter().map(CType::c_bits).collect();
            let args: Vec<String> = s.params.iter().enumerate().map(|(i, p)| p.c_arg(&format!("a{i}"))).collect();
            let call = format!("{rname}({})", args.join(", "));
            let ret_expr = match &s.ret {
                CType::Float(Fmt::F80) => format!("return LD({call});"),
                CType::Float(Fmt::F128) => format!("return F128({call});"),
                CType::Complex(Fmt::F80) => format!("return LDC({call});"),
                CType::Complex(Fmt::F128) => format!("return F128C({call});"),
                CType::Void => format!("{call};"),
                _ => format!("return {call};"),
            };
            let _ = writeln!(file, "{} {rname}({});", s.ret.c_bits(), if bparams.is_empty() { "void".into() } else { bparams.join(", ") });
            let _ = writeln!(file, "{} MATH_FWD({})({}) {{ {ret_expr} }}", s.ret.c(), s.name, if cparams.is_empty() { "void".into() } else { cparams.join(", ") });
        }
    }
    let headers = [
        ("libc.so.6 (feature `export`)", "LIELLA_EXPORT"),
        ("both libc.so.6 and libm.so.6 (feature `export` or `libm`)", "LIELLA_EXPORT) || defined(LIELLA_LIBM"),
        ("libm.so.6 (feature `libm`)", "LIELLA_LIBM"),
    ];
    let mut out: [String; 3] = Default::default();
    for (i, body) in c_files.iter().enumerate() {
        let (who, macro_) = headers[i];
        let mut c = String::new();
        c.push_str("/* @generated by `cargo xtask math-gen`. Do not edit.\n *\n");
        let _ = writeln!(c, " * Class adapter, floating ABI (OD-C4): the math exports of {who} whose values are");
        c.push_str(" * `long double` or `_Float128`, real or complex, which Rust cannot name. Each converts between\n");
        c.push_str(" * the C type and a bit struct (a pair of them for a complex value, passed and returned in\n");
        c.push_str(" * memory, D-10.21) and calls the Rust function (`liella_<name>_bits`, crates/boundary/src/posix/\n");
        c.push_str(" * s10_math_gen.rs), which does all the work. Without the product's feature the forwarders\n");
        c.push_str(" * carry `liella_fwd_` names and are only compiled for checking (build.rs). */\n");
        c.push_str("#include <stddef.h>\n#include <stdint.h>\n#include \"liella_csys.h\"\n\n");
        let _ = writeln!(c, "#if defined({macro_})\n#define MATH_FWD(name) name\n#else\n#define MATH_FWD(name) liella_fwd_##name\n#endif");
        c.push_str("struct liella_ld_bits { uint64_t mant; uint16_t se; uint16_t pad[3]; };\n");
        c.push_str("struct liella_f128_bits { uint64_t lo; uint64_t hi; };\n");
        c.push_str("union liella_ld { long double value; struct liella_ld_bits bits; };\n");
        c.push_str("union liella_f128 { __float128 value; struct liella_f128_bits bits; };\n");
        c.push_str("#define LD(expr) (((union liella_ld){ .bits = (expr) }).value)\n");
        c.push_str("#define LD_BITS(x) (((union liella_ld){ .value = (x) }).bits)\n");
        c.push_str("#define F128(expr) (((union liella_f128){ .bits = (expr) }).value)\n");
        c.push_str("#define F128_BITS(x) (((union liella_f128){ .value = (x) }).bits)\n");
        c.push_str("struct liella_ldc_bits { struct liella_ld_bits re; struct liella_ld_bits im; };\n");
        c.push_str("struct liella_f128c_bits { struct liella_f128_bits re; struct liella_f128_bits im; };\n");
        c.push_str("union liella_ldc { long double _Complex value; struct liella_ldc_bits bits; };\n");
        c.push_str("union liella_f128c { _Float128 _Complex value; struct liella_f128c_bits bits; };\n");
        c.push_str("_Static_assert(sizeof(union liella_ldc) == 32 && sizeof(union liella_f128c) == 32, \"complex bit pairs\");\n");
        c.push_str("#define LDC(expr) (((union liella_ldc){ .bits = (expr) }).value)\n");
        c.push_str("#define LDC_BITS(x) (((union liella_ldc){ .value = (x) }).bits)\n");
        c.push_str("#define F128C(expr) (((union liella_f128c){ .bits = (expr) }).value)\n");
        c.push_str("#define F128C_BITS(x) (((union liella_f128c){ .value = (x) }).bits)\n\n");
        c.push_str(body);
        out[i] = c;
    }
    let mut report = format!("math-gen: {} symbols (libc {}, both {}, libm {}), {} with C forwarders", syms.len(),
        syms.iter().filter(|s| s.owner == Owner::Libc).count(), syms.iter().filter(|s| s.owner == Owner::Both).count(),
        syms.iter().filter(|s| s.owner == Owner::Libm).count(),
        syms.iter().filter(|s| s.ret.needs_forwarder() || s.params.iter().any(CType::needs_forwarder)).count());
    let pending: BTreeSet<&String> = missing.keys().collect();
    let _ = write!(report, "; {} libm symbols without a family row yet", pending.len());
    Ok((rs, out, report))
}

/// Run the generator; with `check`, fail if a generated file is stale.
pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let (rs, c, report) = generate(root)?;
    let files = [
        ("crates/boundary/src/posix/s10_math_gen.rs", rs),
        ("csys/src/adapter/math_libc.c", c[0].clone()),
        ("csys/src/adapter/math_both.c", c[1].clone()),
        ("csys/src/adapter/math_libm.c", c[2].clone()),
    ];
    for (path, text) in &files {
        let p = root.join(path);
        if check {
            let have = fs::read_to_string(&p).unwrap_or_default();
            if have != *text {
                return Err(format!("{path} is stale: run `cargo xtask math-gen`"));
            }
        } else {
            fs::write(&p, text).map_err(|e| format!("{path}: {e}"))?;
        }
    }
    println!("{report}");
    Ok(())
}

/// Symbol sets: libc only, both libraries, libm only.
pub type Owners = (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>);

/// The math symbols by owner, as the generator sees them: `(libc-only, both, libm-only)`.
pub fn owners(root: &Path) -> Result<Owners, String> {
    // The generated exports and the hand-written ones (the floating-point environment).
    let (generated, _, _) = generate(root)?;
    let rs = generated + &fs::read_to_string(root.join("crates/boundary/src/posix/s10_math.rs")).map_err(|e| e.to_string())?;
    let mut libc = BTreeSet::new();
    let mut libm = BTreeSet::new();
    let mut both = BTreeSet::new();
    let mut gate = "";
    for line in rs.lines() {
        if let Some(g) = line.strip_prefix("export! { ") {
            gate = if g.starts_with("libc_libm") { "both" } else if g.starts_with("libm") { "libm" } else { "libc" };
        } else if let Some(rest) = line.strip_prefix("pub extern \"C\" fn ") {
            // A forwarded symbol's Rust function is `liella_<name>_bits`; the C forwarder carries the name.
            let rname = rest.split('(').next().unwrap_or_default();
            let name = rname.strip_prefix("liella_").and_then(|n| n.strip_suffix("_bits")).unwrap_or(rname).to_string();
            match gate {
                "both" => {
                    both.insert(name);
                }
                "libm" => {
                    libm.insert(name);
                }
                _ => {
                    libc.insert(name);
                }
            }
        }
    }
    Ok((libc, both, libm))
}

fn sh(cmd: &mut std::process::Command) -> Result<(), String> {
    let st = cmd.status().map_err(|e| format!("cannot run {cmd:?}: {e}"))?;
    if st.success() { Ok(()) } else { Err(format!("{cmd:?} failed: {st}")) }
}

/// `cargo xtask mathcmp FUNC FMT [options]` (D-10.3): build the measurement probe against the
/// host C library and, with `--liella`, statically against the products (`cargo xtask so`), then
/// run `mathcmp` (crates/mathtools/src/bin/mathcmp.rs) with the options.
pub fn run_mathcmp(root: &Path, args: &[String]) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let dir = root.join("target/mathcmp");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let src = root.join("crates/mathtools/probe/mathcmp.c");
    let host = dir.join("probe_host");
    sh(std::process::Command::new(&cc).args(["-std=gnu17", "-O2", "-fno-builtin"]).arg(&src).arg("-lm").arg("-o").arg(&host))?;
    let mut pass: Vec<String> = args.iter().filter(|a| *a != "--liella").cloned().collect();
    pass.extend(["--host".to_string(), host.display().to_string()]);
    if args.iter().any(|a| a == "--liella") {
        let p = root.join("target/liella");
        let ours = dir.join("probe_liella");
        sh(std::process::Command::new(&cc)
            .args(["-std=gnu17", "-O2", "-fno-builtin", "-static", "-nostdlib", "-DLIELLA_SIDE"])
            .arg(p.join("liella-crt1.o"))
            .arg(&src)
            .arg(p.join("libm.a"))
            .arg(p.join("libc.a"))
            .arg("-lgcc")
            .arg("-o")
            .arg(&ours))?;
        pass.extend(["--liella".to_string(), ours.display().to_string()]);
    }
    sh(std::process::Command::new(cargo).current_dir(root).args(["run", "-q", "--release", "-p", "liella-mathtools", "--bin", "mathcmp", "--"]).args(&pass))
}

/// `cargo xtask remez [--check] FIT...`: the minimax fitting tool (D-10.3).
pub fn run_remez(root: &Path, args: &[String]) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    sh(std::process::Command::new(cargo).current_dir(root).args(["run", "-q", "--release", "-p", "liella-mathtools", "--bin", "remez", "--"]).args(args))
}

/// `cargo xtask tables [--check]`: the elementary functions' constant tables (D-10.3).
pub fn run_tables(root: &Path, args: &[String]) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    sh(std::process::Command::new(cargo).current_dir(root).args(["run", "-q", "--release", "-p", "liella-mathtools", "--bin", "tables", "--"]).args(args))
}
