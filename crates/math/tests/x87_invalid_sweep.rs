#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
//! Every exported `long double` / `_Float64x` function treats an invalid x87 encoding one way
//! (spec §10.3, D-10.20, owner ruling #902, X87-INVALID-ENC-SWEEP): an operand the x87 unit does
//! not generate (pseudo-NaN, pseudo-infinity, unnormal, pseudo-zero) gives the default NaN
//! `ffffc000000000000000` with the invalid flag, beside a number and beside a quiet NaN, in every
//! operand position (each part of a complex operand). The function list is not written here: it is
//! every row of `spec/abi/prototypes.tsv` with a `long double` / `_Float64x` operand that
//! `spec/abi/versions.tsv` exports by default from libm.so.6 or libc.so.6, so a new export is swept
//! as soon as it is in the tables. `EXEMPT` names the functions the standard makes bitwise,
//! classifying, non-computational or a conversion; it must name only such exports, and the
//! classifiers among them must classify every invalid class one fixed way (as NaN).
//! Pseudo-denormals are not swept: the x87 unit accepts them as operands.
//! Not checked: errno (the D-rows give none for invalid encodings).
//! Needs the products (`cargo xtask so`) and a C compiler.

#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

#[path = "../../core/tests/support/oracle.rs"]
mod oracle;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Exports the sweep skips, with the reason from the standard. `l` and `f64x` spellings both.
const EXEMPT: &[(&str, &str)] = &[
    ("fabsl", "sign-bit operation: quiet, copies the encoding (C23 F.3, IEC 60559 5.5.1)"),
    ("fabsf64x", "sign-bit operation: quiet, copies the encoding (C23 F.3, IEC 60559 5.5.1)"),
    ("copysignl", "sign-bit operation: quiet, copies the encoding (C23 F.3, IEC 60559 5.5.1)"),
    ("copysignf64x", "sign-bit operation: quiet, copies the encoding (C23 F.3, IEC 60559 5.5.1)"),
    ("creall", "part access: no floating-point operation (C23 7.3.9.6)"),
    ("crealf64x", "part access: no floating-point operation (C23 7.3.9.6)"),
    ("cimagl", "part access: no floating-point operation (C23 7.3.9.2)"),
    ("cimagf64x", "part access: no floating-point operation (C23 7.3.9.2)"),
    ("conjl", "conjugation is a sign-bit operation on the imaginary part (C23 7.3.9.4)"),
    ("conjf64x", "conjugation is a sign-bit operation on the imaginary part (C23 7.3.9.4)"),
    ("cprojl", "projection returns z unchanged unless a part is infinite (C23 7.3.9.5)"),
    ("cprojf64x", "projection returns z unchanged unless a part is infinite (C23 7.3.9.5)"),
    ("__fpclassifyl", "classification: non-computational, no exception (C23 7.12.3.1, F.3)"),
    ("__isnanl", "classification: non-computational, no exception (C23 7.12.3.4, F.3)"),
    ("isnanl", "classification: non-computational, no exception (C23 7.12.3.4, F.3)"),
    ("__isinfl", "classification: non-computational, no exception (C23 7.12.3.3, F.3)"),
    ("isinfl", "classification: non-computational, no exception (C23 7.12.3.3, F.3)"),
    ("__finitel", "classification: non-computational, no exception (C23 7.12.3.2, F.3)"),
    ("finitel", "classification: non-computational, no exception (C23 7.12.3.2, F.3)"),
    ("__issignalingl", "classification: non-computational, no exception (C23 7.12.3.8, F.3)"),
    ("__iscanonicall", "classification: non-computational, no exception (C23 7.12.3.7, F.3)"),
    ("__signbitl", "sign-bit test: non-computational, no exception (C23 7.12.3.9, F.3)"),
    ("totalorderl", "total order: non-computational, quiet (C23 F.10.12.1, IEC 60559 5.10)"),
    ("totalorderf64x", "total order: non-computational, quiet (C23 F.10.12.1, IEC 60559 5.10)"),
    ("totalordermagl", "total order: non-computational, quiet (C23 F.10.12.2, IEC 60559 5.10)"),
    ("totalordermagf64x", "total order: non-computational, quiet (C23 F.10.12.2, IEC 60559 5.10)"),
    ("getpayloadl", "payload of a NaN; unspecified for a non-NaN (C23 F.10.13.1)"),
    ("getpayloadf64x", "payload of a NaN; unspecified for a non-NaN (C23 F.10.13.1)"),
    ("setpayloadl", "the operand is a payload value: a non-payload gives +0 and nonzero (C23 F.10.13.2)"),
    ("setpayloadf64x", "the operand is a payload value: a non-payload gives +0 and nonzero (C23 F.10.13.2)"),
    ("setpayloadsigl", "the operand is a payload value: a non-payload gives +0 and nonzero (C23 F.10.13.3)"),
    ("setpayloadsigf64x", "the operand is a payload value: a non-payload gives +0 and nonzero (C23 F.10.13.3)"),
    ("canonicalizel", "an encoding with no canonical form gives a nonzero return and no exception (C23 7.12.11.7)"),
    ("canonicalizef64x", "an encoding with no canonical form gives a nonzero return and no exception (C23 7.12.11.7)"),
    ("strfroml", "conversion to text (C23 7.24.1.3)"),
    ("strfromf64x", "conversion to text (C23 7.24.1.3)"),
    ("qecvt", "conversion to text (SVID ecvt family on long double)"),
    ("qfcvt", "conversion to text (SVID ecvt family on long double)"),
    ("qgcvt", "conversion to text (SVID ecvt family on long double)"),
    ("qecvt_r", "conversion to text (SVID ecvt family on long double)"),
    ("qfcvt_r", "conversion to text (SVID ecvt family on long double)"),
];

/// Classifiers (all in `EXEMPT`): checked to classify every invalid class one fixed way.
const CLASSIFIERS: &[&str] =
    &["__fpclassifyl", "__isnanl", "isnanl", "__isinfl", "isinfl", "__finitel", "finitel", "__issignalingl", "__iscanonicall", "__signbitl"];

/// Invalid classes: name, sign+exponent, significand.
const CLASSES: &[(&str, u16, u64)] = &[
    ("pnan", 0x7fff, 0x4000_0000_0000_0001),
    ("-pnan", 0xffff, 0x4000_0000_0000_0000),
    ("pinf", 0x7fff, 0),
    ("-pinf", 0xffff, 0),
    ("unnorm", 0x3fff, 0x4000_0000_0000_0000),
    ("-unnorm", 0xc005, 0x0000_0000_0000_0001),
    ("pzero", 0x4000, 0),
];
const COMPANIONS: &[(&str, u16, u64)] = &[("num", 0x3fff, 0xc000_0000_0000_0000), ("qnan", 0x7fff, 0xc000_0000_0000_0123)];
const DNAN: &str = "ffffc000000000000000";

struct Proto {
    name: String,
    ret: String,
    args: Vec<String>,
}

fn ld(t: &str) -> bool {
    t == "long double" || t == "_Float64x"
}

fn exported_protos() -> Vec<Proto> {
    let r = root();
    let versions = std::fs::read_to_string(r.join("spec/abi/versions.tsv")).unwrap();
    let exported: BTreeSet<&str> = versions
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() > 4 && (f[0] == "libm.so.6" || f[0] == "libc.so.6") && f[3] == "default" && f[4] == "FUNC").then_some(f[1])
        })
        .collect();
    let protos = std::fs::read_to_string(r.join("spec/abi/prototypes.tsv")).unwrap();
    let mut out = Vec::new();
    for l in protos.lines().skip(1) {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() < 3 || !exported.contains(f[0]) || f[2].contains("...") {
            continue;
        }
        let d = f[2].trim_start_matches("extern ");
        let (head, args) = d.split_once(" (").unwrap();
        let ret = head.strip_suffix(f[0]).unwrap().trim().to_string();
        let args: Vec<String> = args.trim_end_matches(')').split(", ").map(str::to_string).collect();
        let operand = args.iter().any(|a| {
            let a = a.trim_start_matches("const ").trim_start_matches("complex ");
            ld(a) || ld(a.trim_end_matches(" *"))
        });
        let ld_out_only = args.iter().all(|a| !ld(a) && !ld(a.trim_start_matches("complex ")) && !a.starts_with("const "));
        if operand && !(ld_out_only && args.iter().all(|a| a.ends_with('*'))) {
            out.push(Proto { name: f[0].to_string(), ret, args });
        }
    }
    out
}

const PRELUDE: &str = r#"#include <stdio.h>
#include <string.h>
#include <errno.h>
#include <fenv.h>
#include <stddef.h>
#define complex _Complex
typedef long __intmax_t;
typedef unsigned long __uintmax_t;
static long double mk(unsigned short se, unsigned long m) { long double v; memset(&v, 0, sizeof v); memcpy(&v, &m, 8); memcpy((char *)&v + 8, &se, 2); return v; }
static void pl(const char *k, long double v) { unsigned char b[10]; memcpy(b, &v, 10); printf(" %s:", k); for (int i = 9; i >= 0; i--) printf("%02x", b[i]); }
static long double complex cx(long double r, long double i) { union { long double complex z; long double p[2]; } u; u.p[0] = r; u.p[1] = i; return u.z; }
static void pc(long double complex z) { union { long double complex z; long double p[2]; } u; u.z = z; pl("C", u.p[0]); pl("C", u.p[1]); }
"#;

/// The C call for one prototype, or why it cannot be generated.
fn gen_fn(p: &Proto, c: &mut String) -> Result<usize, String> {
    let mut slots = 0usize;
    let mut decls = String::new();
    let mut call = Vec::new();
    let mut outs = String::new();
    for (i, a) in p.args.iter().enumerate() {
        let a = a.as_str();
        let base = a.trim_start_matches("complex ");
        if ld(a) {
            call.push(format!("s[{slots}]"));
            slots += 1;
        } else if a.starts_with("complex ") && ld(base) {
            call.push(format!("cx(s[{}], s[{}])", slots, slots + 1));
            slots += 2;
        } else if ld(a.trim_start_matches("const ").trim_end_matches(" *")) && a.starts_with("const ") {
            call.push(format!("&s[{slots}]"));
            slots += 1;
        } else if ld(a.trim_end_matches(" *")) {
            let _ = writeln!(decls, "  long double o{i} = 0;");
            call.push(format!("&o{i}"));
            let _ = writeln!(outs, "  pl(\"L\", o{i});");
        } else if a == "int *" {
            let _ = writeln!(decls, "  int o{i} = 0;");
            call.push(format!("&o{i}"));
        } else if matches!(a, "int" | "long int") {
            call.push("1".into());
        } else if a == "unsigned int" {
            call.push("64".into());
        } else if matches!(a, "float" | "double") {
            call.push("1.5".into());
        } else {
            return Err(format!("{}: parameter type `{a}` not handled", p.name));
        }
    }
    let ret = p.ret.as_str();
    let (rdecl, rprint) = if ld(ret) {
        ("long double r = ", "  pl(\"L\", r);\n".to_string())
    } else if ret.starts_with("complex ") && ld(ret.trim_start_matches("complex ")) {
        ("long double complex r = ", "  pc(r);\n".to_string())
    } else if matches!(ret, "float" | "double" | "_Float32" | "_Float64" | "_Float32x") {
        ("double r = ", "  printf(\" N:%d\", r != r);\n".to_string())
    } else if matches!(ret, "int" | "long int" | "long long int" | "__intmax_t" | "__uintmax_t") {
        ("long long r = ", "  printf(\" I:%lld\", r);\n".to_string())
    } else if ret == "void" {
        ("", String::new())
    } else {
        return Err(format!("{}: return type `{ret}` not handled", p.name));
    };
    let _ = writeln!(c, "extern {} {} ({});", p.ret, p.name, p.args.join(", "));
    let _ = writeln!(
        c,
        "static void t_{n}(int pos, long double v, long double w, const char *kn, const char *cn) {{\n  long double s[{slots}];\n  for (int i = 0; i < {slots}; i++) s[i] = i == pos ? v : w;\n{decls}  feclearexcept(FE_ALL_EXCEPT);\n  {rdecl}{n}({args});\n  int fl = fetestexcept(FE_INVALID) != 0;\n  printf(\"R {n} %d %s %s %d\", pos, kn, cn, fl);\n{rprint}{outs}  printf(\"\\n\");\n}}",
        n = p.name,
        args = call.join(", ")
    );
    Ok(slots)
}

fn gen_c(protos: &[Proto]) -> Result<String, Vec<String>> {
    let mut c = String::from(PRELUDE);
    let mut errs = Vec::new();
    let mut runs = String::new();
    let exempt: BTreeSet<&str> = EXEMPT.iter().map(|e| e.0).collect();
    for p in protos {
        if CLASSIFIERS.contains(&p.name.as_str()) {
            let _ = writeln!(c, "extern {} {} ({});", p.ret, p.name, p.args.join(", "));
            for (k, se, m) in CLASSES {
                let _ = writeln!(runs, "  printf(\"K {n} {k} %d\\n\", (int){n}(mk({se}, {m}ul)));", n = p.name);
            }
            continue;
        }
        if exempt.contains(p.name.as_str()) {
            continue;
        }
        match gen_fn(p, &mut c) {
            Ok(slots) => {
                let comps: &[(&str, u16, u64)] = if slots == 1 { &COMPANIONS[..1] } else { COMPANIONS };
                for pos in 0..slots {
                    for (k, se, m) in CLASSES.iter().chain([("dnan", 0xffff, 0xc000_0000_0000_0000)].iter()) {
                        for (cn, cse, cm) in comps {
                            let _ = writeln!(runs, "  t_{}({pos}, mk({se}, {m}ul), mk({cse}, {cm}ul), \"{k}\", \"{cn}\");", p.name);
                        }
                    }
                }
            }
            Err(e) => errs.push(e),
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    let _ = write!(c, "int main(void) {{\n{runs}  return 0;\n}}\n");
    Ok(c)
}

#[test]
fn exempt_list_names_only_exported_long_double_functions() {
    // Clauses: s10_3_c_0 (D-10.20).
    let names: BTreeSet<String> = exported_protos().into_iter().map(|p| p.name).collect();
    let stale: Vec<&str> = EXEMPT.iter().map(|e| e.0).filter(|n| !names.contains(*n)).collect();
    assert!(stale.is_empty(), "EXEMPT names functions that are not exported long double functions: {stale:?}");
    for c in CLASSIFIERS {
        assert!(EXEMPT.iter().any(|e| e.0 == *c), "classifier {c} is not in EXEMPT");
    }
    assert!(names.len() > 200, "only {} long double exports found in the tables", names.len());
}

#[test]
fn every_long_double_function_gives_the_default_nan_for_an_invalid_encoding() {
    // Clauses: s10_3_c_0 (D-10.20).
    let r = root();
    assert!(r.join("target/liella/libm.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let p = r.join("target/liella");
    let nm = Command::new("nm").args(["-g", "--defined-only", "-P"]).arg(p.join("libm.a")).arg(p.join("libc.a")).output().unwrap();
    let defined: BTreeSet<String> = String::from_utf8_lossy(&nm.stdout).lines().filter_map(|l| l.split_whitespace().next().map(str::to_string)).collect();
    assert!(defined.contains("sinl"), "nm lists no symbols of the products");
    let (protos, absent): (Vec<Proto>, Vec<Proto>) = exported_protos().into_iter().partition(|p| defined.contains(&p.name));
    if !absent.is_empty() {
        // Not built yet: the export-coverage tests own these; the sweep picks them up once they exist.
        let names: Vec<String> = absent.into_iter().map(|p| p.name).collect();
        skip_note::skipped(&format!("x87 invalid sweep: not in the products yet, not swept: {}", names.join(" ")));
    }
    let src = match gen_c(&protos) {
        Ok(s) => s,
        Err(e) => panic!("the sweep cannot call these exports (extend gen_fn or EXEMPT them with a reason):\n{}", e.join("\n")),
    };
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (c, exe) = (dir.join("x87_invalid_sweep.c"), dir.join("x87_invalid_sweep"));
    std::fs::write(&c, src).unwrap();
    let mut cmd = Command::new("cc");
    cmd.args(["-std=gnu17", "-O0", "-static", "-nostdlib", "-fno-builtin", "-w"]).arg(p.join("liella-crt1.o")).arg(&c).arg(p.join("libm.a")).arg(p.join("libc.a")).arg("-lgcc");
    assert!(oracle::build(cmd, &exe), "x87_invalid_sweep.c does not link against the products");
    let out = Command::new(&exe).env_clear().output().unwrap();
    assert!(out.status.success(), "the sweep program failed: {:?}", out.status);
    let text = String::from_utf8(out.stdout).unwrap();
    let mut bad = Vec::new();
    let mut refs: BTreeMap<String, String> = BTreeMap::new();
    let mut classes: BTreeMap<(String, bool), BTreeSet<String>> = BTreeMap::new();
    let mut lines = 0usize;
    for l in text.lines() {
        let w: Vec<&str> = l.split(' ').collect();
        if w[0] == "K" {
            classes.entry((w[1].to_string(), w[2].starts_with('-'))).or_default().insert(w[3].to_string());
            continue;
        }
        if w[3] == "dnan" {
            let key = format!("{} {} {}", w[1], w[2], w[4]);
            refs.insert(key, w[6..].iter().filter(|t| t.starts_with("I:")).copied().collect::<Vec<_>>().join(" "));
        }
    }
    for l in text.lines().filter(|l| l.starts_with("R ")) {
        let w: Vec<&str> = l.split(' ').collect();
        if w[3] == "dnan" {
            continue;
        }
        lines += 1;
        let parts = &w[6..];
        let mut ok = w[5] == "1";
        let cplx: Vec<&str> = parts.iter().filter_map(|t| t.strip_prefix("C:")).collect();
        for t in parts {
            ok &= match t.split_once(':') {
                Some(("L", v)) => v == DNAN,
                Some(("N", v)) => v == "1",
                _ => true,
            };
        }
        if !cplx.is_empty() {
            let nan = |v: &str| (v.starts_with("7fff") || v.starts_with("ffff")) && &v[4..] != "8000000000000000";
            let invalid = |v: &str| {
                let e = u16::from_str_radix(&v[..4], 16).unwrap() & 0x7fff;
                e != 0 && (u64::from_str_radix(&v[4..], 16).unwrap() >> 63) == 0
            };
            ok &= cplx.iter().any(|v| nan(v)) && cplx.iter().all(|v| !invalid(v) && (!nan(v) || *v == DNAN));
        }
        let ints: Vec<&str> = parts.iter().filter(|t| t.starts_with("I:")).copied().collect();
        if !ints.is_empty() {
            ok &= refs.get(&format!("{} {} {}", w[1], w[2], w[4])).is_some_and(|r| *r == ints.join(" "));
        }
        if !ok {
            bad.push(l.to_string());
        }
    }
    assert!(lines > 3_000, "the sweep printed only {lines} lines");
    for ((f, neg), vals) in &classes {
        if vals.len() != 1 {
            bad.push(format!("classifier {f} (negative {neg}) classifies the invalid classes differently: {vals:?}"));
        }
    }
    for (f, want) in [("__fpclassifyl", "0"), ("__isnanl", "1"), ("isnanl", "1"), ("__iscanonicall", "0"), ("__finitel", "0"), ("__isinfl", "0")] {
        for neg in [false, true] {
            if let Some(v) = classes.get(&(f.to_string(), neg)) {
                if !v.contains(want) {
                    bad.push(format!("classifier {f} does not classify invalid encodings as NaN: {v:?}"));
                }
            }
        }
    }
    std::fs::write(dir.join("x87_invalid_sweep_bad.txt"), bad.join("\n")).unwrap();
    let fns: BTreeSet<&str> = bad.iter().filter_map(|l| l.split(' ').nth(1)).collect();
    assert!(bad.is_empty(), "{} lines break the invalid-encoding rule in {:?} (all in target/test-oracles/x87_invalid_sweep_bad.txt); first:\n{}", bad.len(), fns, bad.iter().take(12).cloned().collect::<Vec<_>>().join("\n"));
}
