//! Compile the C syscall layer (`csys/`) into a static library and link it.
//!
//! The C layer is freestanding: it includes only its own header and the compiler-provided
//! <stddef.h>, <stdint.h> and <stdarg.h>, and is built with -Werror. No external build crates
//! are used (zero-dependency policy); the compiler is `$CC` or `cc`, the archiver `$AR` or `ar`.
#![allow(clippy::disallowed_methods)] // a host tool: S8 governs the library crates only

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            sources(&p, out);
        } else if matches!(p.extension().and_then(|e| e.to_str()), Some("c" | "S")) {
            out.push(p);
        }
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("liella-boundary build: {msg}");
    std::process::exit(1)
}

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let csys = manifest.join("../../csys");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap_or_default());
    let cc = env::var("CC").unwrap_or_else(|_| "cc".into());
    let ar = env::var("AR").unwrap_or_else(|_| "ar".into());
    println!("cargo:rerun-if-changed={}", csys.display());
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");

    // The shipped products define LIELLA_EXPORT: the C adapters then carry the exported glibc
    // names (variadic entry points, the floating-ABI forwarders of OD-C4).
    let export = env::var_os("CARGO_FEATURE_EXPORT").is_some();
    // The products' runtime (`__libc_start_main`, `environ`, ...): LIELLA_RUNTIME.
    let runtime = env::var_os("CARGO_FEATURE_RUNTIME").is_some();

    // The libm product (`crates/libm-so`): LIELLA_LIBM names the math forwarders it owns.
    let libm = env::var_os("CARGO_FEATURE_LIBM").is_some();

    let mut srcs = Vec::new();
    sources(&csys.join("src"), &mut srcs);
    // `_start` belongs only to the start object (`cargo xtask so`), never to an archive that
    // test executables link next to the host's own start files.
    // The compat versions' `.symver` stubs belong to libc.so.6 alone (`cargo xtask so`, LG-1,
    // R15-2): a static link cannot bind a non-default version.
    srcs.retain(|p| !matches!(p.file_name().and_then(|n| n.to_str()), Some("entry.S" | "compat_aliases.S")));
    let mut objs = Vec::new();
    for (i, src) in srcs.iter().enumerate() {
        // The math forwarders call Rust functions that exist only in the products that own
        // them (D-10.7): elsewhere they are compiled for checking but not archived.
        let linked = match src.file_name().and_then(|n| n.to_str()) {
            Some("math_libc.c") => export,
            Some("math_both.c") => export || libm,
            Some("math_libm.c") => libm,
            _ => true,
        };
        let obj = out.join(format!("csys_{i}.o"));
        let mut cmd = Command::new(&cc);
        // One section per function, so the products drop what they do not use (`--gc-sections`).
        // Function entries on 64 bytes, like the Rust functions (D-4.33, `-align-all-functions=6`).
        cmd.args(["-std=c17", "-O2", "-fPIC", "-ffreestanding", "-fno-builtin", "-nostdlib", "-ffunction-sections", "-fdata-sections",
                  "-fno-stack-protector", "-falign-functions=64", "-Wall", "-Wextra", "-Werror"])
            .arg(if linked { "-c" } else { "-fsyntax-only" })
            .args(export.then_some("-DLIELLA_EXPORT"))
            .args(runtime.then_some("-DLIELLA_RUNTIME"))
            .args(libm.then_some("-DLIELLA_LIBM"))
            .arg("-I").arg(csys.join("include"))
            .arg(src);
        if linked {
            cmd.arg("-o").arg(&obj);
        }
        match cmd.status() {
            Ok(s) if s.success() => {
                if linked {
                    objs.push(obj);
                }
            }
            other => fail(&format!("compiling {} failed: {other:?}", src.display())),
        }
    }
    let lib = out.join("libliella_csys.a");
    let _ = fs::remove_file(&lib);
    let status = Command::new(&ar).arg("rcs").arg(&lib).args(&objs).status();
    if !matches!(status, Ok(s) if s.success()) {
        fail(&format!("archiving the C syscall layer failed: {status:?}"));
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=liella_csys");
}
