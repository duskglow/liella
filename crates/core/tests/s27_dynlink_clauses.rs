// Specified from spec §27 (phase 14: the dlopen/dlsym clauses, pending on Liella's loader).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §27 (19 clauses).
//! The dlopen and dlsym clauses are checked by dynamic-link conformance programs
//! (`conformance/run.sh`, LINK-HELPERS / DLOPEN-HELPERS), run under the reference and under
//! Liella's ld.so. A program for behaviour Liella's loader does not have yet carries
//! PENDING-LIELLA: the reference must meet the expectations, Liella's run is reported PENDING and
//! counted, and it turns into a FAIL the day Liella's run passes (then drop the directive and the
//! pending assertion here). `s27_dlopen_failures` (DL-16a) and `s27_dlsym_handles` (DL-16b) pass
//! on both.

#![allow(clippy::unwrap_used, missing_docs)]

use spectest::clause;

/// Build and run dynamic-link conformance programs (`conformance/run.sh`); its output.
fn conformance(names: &[&str]) -> String {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.so.6", "target/liella/ld-linux-x86-64.so.2"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(o.status.success(), "{out}");
    out
}

/// Asserts that `name` ran as a full program, not pending: run.sh (whose success [`conformance`]
/// checks) passed both the reference's run and Liella's.
fn assert_not_pending(out: &str, name: &str) {
    let line = format!("conformance: PENDING {name} ");
    assert!(!out.lines().any(|l| l.starts_with(&line)), "{name} is still reported pending:\n{out}");
}

clause!(s27_1_c_1, "27.1", "Contract", r##"**`dlopen(path, mode)`** loads an ELF shared object and its dependencies into the process and runs their constructors. It returns a handle, and repeated opens are reference-counted."##);
clause!(s27_1_c_1_1, "27.1", "Contract", r##"**Path search** applies when `path` contains no slash: `DT_RPATH`, `LD_LIBRARY_PATH`, `DT_RUNPATH`, `/etc/ld.so.cache`, then the default directories."##);
clause!(s27_1_c_1_2, "27.1", "Contract", r##"**Modes:** `RTLD_LAZY`/`RTLD_NOW` (binding time), `RTLD_GLOBAL`/`RTLD_LOCAL` (symbol visibility to later loads), `RTLD_NODELETE`, `RTLD_NOLOAD` (only return an existing handle), `RTLD_DEEPBIND` (prefer the object's own symbols)."##);
clause!(s27_1_c_1_3, "27.1", "Contract", r##"**`dlopen(NULL)`** returns the global scope."##);
clause!(s27_1_c_2, "27.1", "Contract", r##"**`dlmopen(lmid, ...)`** loads into a separate *link-map namespace* (`LM_ID_NEWLM`), giving an isolated copy of the libraries."##);
/// §27.1 Contract: **`dlsym(handle, name)`** returns the address of a symbol. The special handles are `RTLD_DEFAULT` (global lookup order) and `RTLD_NEXT` (the next occurrence after the caller's object, for interposition). **`dlvsym`** requests a specific symbol version.
///
/// `s27_dlsym_handles`: RTLD_DEFAULT and RTLD_NEXT through two helpers, dlvsym, and the dlerror
/// texts of the failing lookups. Passes under the reference and under Liella's ld.so (DL-16b,
/// DL-DLSYM).
#[test]
fn s27_1_c_3() {
    let out = conformance(&["s27_dlsym_handles"]);
    assert_not_pending(&out, "s27_dlsym_handles");
}
clause!(s27_1_c_4, "27.1", "Contract", r##"**`dlclose`** decrements the reference count. At zero, it runs destructors and unmaps the object, unless the object is `NODELETE` or has unique symbols or TLS in use."##);
clause!(s27_1_c_5, "27.1", "Contract", r##"**`dlerror()`** returns a Text describing the last failure on this thread, and clears it. It returns `NULL` if there was no error. ⚑ HIST: this is yet another error channel. `dlsym` returning `NULL` is ambiguous for symbols whose value is 0, so callers must check `dlerror`."##);
clause!(s27_1_c_6, "27.1", "Contract", r##"**`dladdr(addr, &info)`** / **`dladdr1`** map an address to its object and nearest symbol."##);
clause!(s27_1_c_7, "27.1", "Contract", r##"**`dlinfo(handle, request, arg)`** queries a handle: its link map, origin, search paths, TLS module ID and block, and namespace."##);
clause!(s27_1_c_8, "27.1", "Contract", r##"**`dl_iterate_phdr(cb, data)`** calls `cb` for each loaded object, with its load address and program headers. The loader lock is held during the callback."##);
clause!(s27_1_c_9, "27.1", "Contract", r##"**`_dl_find_object(pc, &res)`** (2.35) finds an object and its `PT_GNU_EH_FRAME` for a program counter without taking locks. It exists for unwinders."##);
clause!(s27_1_g_0, "27.1", "Guarantees", r##"Symbol resolution follows ELF rules (global scope order, symbol versioning, `STB_GNU_UNIQUE`, IFUNC resolvers run at bind time). Constructors run in dependency order."##);
/// §27.1 Failure: `NULL` plus `dlerror` text: file not found, wrong ELF class, undefined symbol (with `RTLD_NOW`), static TLS exhaustion, or a version mismatch.
///
/// `s27_dlopen_failures`: the failure corpus (a missing file, files that are not loadable
/// objects, the wrong ELF class, a missing dependency, an undefined symbol under RTLD_NOW, a
/// missing version, bad modes, RTLD_NOLOAD),
/// each a NULL handle with the reference's dlerror text. Static TLS exhaustion is not covered
/// (no spec clause test yet). Passes under the reference and under Liella's ld.so (DL-16a,
/// DL-DLOPEN-LOAD).
#[test]
fn s27_1_f_0() {
    let out = conformance(&["s27_dlopen_failures"]);
    assert_not_pending(&out, "s27_dlopen_failures");
}
clause!(s27_2_c_0, "27.2", "Contract", r##"Libraries named in `LD_AUDIT` (or `DT_AUDIT`) receive callbacks from the dynamic linker for library search, object load and unload, symbol binding (they can redirect bindings), and PLT entry and exit."##);
clause!(s27_3_c_0, "27.3", "Contract", r##"Return the address of a TLS variable, given a module ID and an offset, for the calling thread. The block is allocated on first access for modules loaded by `dlopen`."##);
clause!(s27_3_f_0, "27.3", "Failure", r##"None. Allocation failure aborts the process. ⚑ ARB: there is no error channel for a lazy allocation."##);
clause!(s27_4_c_0, "27.4", "Contract", r##"`_r_debug` and `_dl_debug_state` implement the debugger rendezvous. The debugger sets a breakpoint on `_dl_debug_state` and walks the `r_debug.r_map` list after each change. Since 2.35 `r_version` is 2, which adds a linked list of namespaces."##);
clause!(s27_5_c_0, "27.5", "Contract", r##"Report CPU features as detected by the dynamic linker (CPUID plus OS support via XCR0, masked by `glibc.cpu.hwcaps` tunables): "present" means the CPU supports the feature, and "active" means it is also usable."##);
