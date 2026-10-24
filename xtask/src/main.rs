//! `cargo xtask <command>`: project tooling.
//!
//! | Command        | What it does |
//! |----------------|--------------|
//! | `stubs`        | Parse `glibc-functional-spec.md`; (re)generate pending test stubs per clause, per exported symbol and per kernel primitive; write `spec/clauses.tsv`, `spec/symbols.tsv`, `spec/primitives.tsv`. Files that already contain specified tests are never overwritten; instead their coverage of the spec is checked. |
//! | `trace`        | Clause traceability: every test, every row of `docs/differences-from-glibc.md` and every behaviour document names the clause IDs it bears on (or says why it has none), and every clause ID named exists in `spec/clauses.tsv`. Also run by `stubs` and `test`. |
//! | `symgap`       | Report the ABI names glibc's installed `libc.a` defines and Liella's does not, apart from those phases 14 and 15 own (report only). Also run by `test`. |
//! | `compat-plan`  | Check that `spec/abi/compat-plan.tsv` (the phase-15 dispatch table: kind, owning story and D-row of every compat version, LG-0a) matches the `compat` rows of `spec/abi/versions.tsv` row for row; also part of `static`. |
//! | `symcov`       | Report every name and version of `spec/abi/versions.tsv` (all libraries) that `target/liella` does not define, grouped by library, version class and owning phase or milestone; `--tsv <file>` writes the per-name table (report only). |
//! | `status`       | Pending versus specified tests, per spec section, then the schedule report (`spec/schedule.tsv`: the owner of the pending tests). `--schedule` lists each pending test with its owner and fails when one belongs to a finished phase, a track not started or past its join point, or nobody. |
//! | `purity`       | Reject logic in the POSIX translation layer (implementation-plan.md §8.2). |
//! | `unsafe-audit` | Enforce the unsafe policy: none outside liella-boundary; five documented categories inside it, inline assembly only in the tb-field site (plan §2.1). |
//! | `csys-gen`     | Generate the C layer's syscall wrappers and their Rust declarations from `csys/syscalls.tsv` (K5); `--check` fails if stale. |
//! | `abi-gen`      | Generate `liella_abi::layout` and the Appendix C layout tests from `spec/abi/layouts.tsv`; `--check` fails if stale. |
//! | `unicode-gen`  | Generate the Unicode tables (`crates/core/src/unicode/tables.rs`) from `spec/unicode/15.1.0` (IP-19, D-5.4); `--check` fails if stale. |
//! | `native-gen`   | Generate the native C ABI's header (`include/liella/native.h`) from `spec/abi/native.tsv` (D-X.10); `--check` fails if stale or if the exports differ from the table. |
//! | `compat-gen`   | Generate `csys/src/machine/compat_aliases.S` and `crates/libm-so/compat_aliases.S`, the `.symver` stubs of libc.so.6's and libm.so.6's compat versions, from `spec/abi/versions.tsv` and `spec/abi/compat-plan.tsv` (LG-1, LG-4, R15-1); `--check` fails if stale. |
//! | `locale-gen`   | Generate the built-in locales' `nl_langinfo` items (`crates/core/src/locale/builtin_items.rs`) from `spec/abi/langinfo.tsv` (D-7.10); `--check` fails if stale. |
//! | `math-gen`     | Generate the math exports (`crates/boundary/src/posix/s10_math_gen.rs`) and their C forwarders (`csys/src/adapter/math_*.c`) from `crates/boundary/src/posix/s10_math.tsv` and spec Appendix C (D-10.7); `--check` fails if stale. |
//! | `mathcmp`      | Compare glibc and Liella on one math function and format, bit for bit, and classify each result against the test oracle (D-10.3; `crates/mathtools`). |
//! | `remez`        | The minimax (Remez) fitting tool: generate or `--check` the coefficient files of `crates/math/fits/*.fit` (D-10.3). |
//! | `tables`       | Generate or `--check` the elementary functions' constant tables, `crates/math/src/coeffs/tables.rs` (D-10.3). |

//! | `bench`        | Run the release-mode benchmarks and check `bench/budgets.tsv` (rule P6); `ratio_host` rows are gated on the median Liella/glibc ratio against the limit for this CPU model in `bench/host_limits.tsv` (`--runs N`, default 5 rounds; `--build-only` builds everything and stops, `--no-build` measures only and fails on a missing or stale program (the timing-lock split of `vm_bench.sh`), `BENCH_SETTLE_SECS` (default 12) idles before the first round; `--only a,b` runs just those benchmarks and says PARTIAL BENCH, not a merge bench); a CPU without entries is reported, not gated. With `--compare [--runs N] [<cbench>...]` (rule P10 (9)): build each C benchmark (default `cbench_opt`) against the products and against the host C library (dynamic and static), run them alternately N times after a discarded warm-up run (default 5), and print medians with min and max and the same-run ratio; no budgets are checked. Measurement columns (MEASURE-1): `--v3` (archive built with `-Ctarget-cpu=x86-64-v3`), `--sorted` (Liella linked with `--sort-section=name`: layout effect), `--counters` (per-row aggregate perf counters, `CBENCH_COUNTERS=1`; the summary table shows instr/op for Liella and glibc and the ratio); all columns in `target/liella/bench/measure.tsv`. `--stress env,path,fds,names,locale,deep,threads` (or `all`, STRESS-ENV-AUDIT, -2): advisory runs of the C benchmarks in a large environment (2,000 extra variables, a 200-directory `PATH`, 10,000 open descriptors; `names`: 50,000 users and groups, `locale`: 500 more names in the locale archive, both through an unprivileged user and mount namespace; `deep`: the fstree rows 64 directories down; `threads`: 64 idle threads), normally with `--counters-only`, compared with the same command without `--stress` to find costs that grow with the environment; limits are not set for it. |
//! | `csys-audit`   | Enforce the C syscall layer's rules: single-statement wrappers, freestanding, assembly only in machine/ (plan §2.5). |
//! | `env-audit`    | Reject an in-process `setenv`/`putenv`/`unsetenv`/`clearenv` in a test binary that also starts processes, unless it runs in `in_child`/`alone` or carries `// env-ok: <why>` (FL-1: the library's `environ` is the process's). |
//! | `opaque-audit` | Reject a direct call to an exported allocator-family name (`malloc`, `free`, ...) in a test file unless it goes through `selftest::opaque` or the file says `// opaque-ok: <why>` (GATE-SPEED-5: LLVM folds those names at opt-level >= 1); and a test file linking `liella_boundary` that calls any LLVM-builtin libc name (`labs`, `strlen`, `fabs`, ...) directly unless it is `#![no_builtins]` (OPAQUE-AUDIT). |
//! | `crash-audit`  | Reject a test that expects a core-dumping signal (`.signal(), Some(6)`, `Signaled(SIGABRT)`, `EXPECT-SIGNAL: 6`, `WTERMSIG(st) == SIGSEGV`) without as many `expect_crash` helper calls (`selftest::expect_crash`/`in_child_quiet`, `conformance/c/expect_crash.h`) unless the file says `dump-ok: <why>` (CORE-STORM: no core handler per deliberate crash). |
//! | `test`         | policy checks, the workspace tests (internal + translation suites; under cargo-nextest when installed, `LIELLA_NO_NEXTEST=1` for `cargo test`, see `nextest.rs`), then the C conformance suite, `native` and `rust-dylib`. |
//! | `ctest`        | Only the C conformance suite (`conformance/run.sh`). |
//! | `nextest`      | `cargo nextest run <args>` with the summary lines of a `cargo test` log added (`nextest.rs`); `affected --run` runs its tests through it. |
//! | `affected`     | Run only the tests a change can affect (conservative selection; see `affected.rs`): `--list` (default) prints the selection with a reason per item, `--run` runs it and prints a summary; `--base <ref>` (default `origin/main`), `--files <path>...` instead of the git query. Escalates to the full gate for `csys/`, `crates/abi`, build and generated files. The full gate still runs once at merge. |
//! | `so`           | Build the products: `libc.a`, `libc.so.6` (version script from Appendix C.3 and the native C ABI's nodes, D-X.10) and `liella-crt1.o` in `target/liella` (never on Cargo's library path). Built with `[profile.fast]` (thin LTO, 16 codegen units; TOOL-PROFILE) for tests and iteration; `LIELLA_FULL_PROFILE=1` uses the shipped `release` profile (fat LTO). `bench` always builds `release` and refuses fast products (`target/liella/.profile` records which). |
//! | `repro`        | The products must not depend on the build directory (ruling #713): builds them in two fresh source copies under `target/repro` (`--dir <base>` elsewhere; not `/dev/shm` under `~/bin/gate`, #714) with the `fast` profile (`--release`: the shipped one) and compares every file of `target/liella`: whole files, then ELF sections (archive members too), accepting differences in `.symtab`/`.strtab` only. Fails naming the first differing file, member and section (`repro.rs`). A nightly job, not part of the gate. |
//! | `native`       | The native Rust interface (D-X.10): `liella-native` with each feature on its own (clippy, unit tests); its static example built against `libc.a` and run, its dynamic example built against `libc.so.6` and checked with `readelf`. |
//! | `fuzz`         | The fuzz harnesses of `fuzz/` (a workspace of its own, nightly, libFuzzer through `cargo fuzz`): `--list`; `<target> --time <minutes>` (or `--all`) runs in chunks resuming from the corpus outside the repository (`~/liella-fuzz-corpus`); `--repro`, `--tmin <file>`, `--cmin` for triage (implementation-plan.md §4.5). |
//! | `bench-rows`   | The bench rows a change can move plus the sentinel rows (`bench/sentinels.tsv`), for a TARGETED bench (`benchrows.rs`, BENCH-AFF): `--base <ref>` / `--range <a>..<b>` / `--files <p>,...`, `--out <file>` for `bench --rows-file`. `bench --affected <base>` selects and times only those rows (`bench/c/cbench_rows.h`). FULL for build, link, allocator, startup and harness changes. The full bench on main stays the safety net. |
//! | `fuzz-check`  | Type-check every fuzz target on the pinned nightly (`cargo check --bins` in `fuzz/`; no fuzzer runtime). Part of `test` and of `affected` when `crates/` or `fuzz/` sources change: the harnesses are a workspace of their own and a library signature change breaks them silently (HYGIENE-1). |
//! | `rust-dylib`   | The Rust dylib `libliella_rust.so.<abi>` (D-X.10) of a version in `spec/abi/rust-abi.toml` (default: the current one): builds it with that version's pinned rustup toolchain (failing if it is absent), after checking that toolchain's `rustc -vV` against the definition; builds it and a program against it in `target/liella/rust/<abi>`, checks soname, note, `LIELLA_RUST_ABI` and `DT_NEEDED` with `readelf`; builds a synthetic second version beside it and checks that both coexist. |
#![allow(clippy::disallowed_methods)] // a host tool: S8 governs the library crates only

#![forbid(unsafe_code)]

mod abbench;
mod abi;
mod affected;
mod align;
mod arch;
mod bench;
mod forkalloc;
mod fs_order;
mod benchrows;
mod coldbss;
mod compat;
mod copies;
mod csys;
mod fuzz;
mod hottext;
mod langinfo;
mod math;
mod native;
mod native_examples;
mod nextest;
mod policy;
mod remap;
mod repro;
mod rust_dylib;
mod schedule;
mod so;
mod spec;
mod status;
mod symcov;
mod symgap;
mod trace;
mod unicode;
mod compat_plan;
mod compatprobe;
mod compat_objects;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The tree to work on, found at run time: the nearest ancestor of the current directory holding
/// `Cargo.toml` and `xtask/Cargo.toml`. The build-time `CARGO_MANIFEST_DIR` is only the fallback: a
/// binary reused by a copied tree (plant.sh's `<role>-plant`) would otherwise act on the original
/// tree (TOOLS-10).
fn root() -> PathBuf {
    let here = std::env::current_dir().ok();
    let found = here.as_deref().and_then(|d| d.ancestors().find(|a| a.join("Cargo.toml").is_file() && a.join("xtask/Cargo.toml").is_file()));
    found.map(Path::to_path_buf).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_default())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let root = root();
    let result = match cmd {
        "stubs" => spec::stubs(&root, args.iter().any(|a| a == "--force")),
        "status" => status::run(&root, args.iter().any(|a| a == "--schedule")),
        "symgap" => symgap::run(&root),
        "symcov" => symcov::run(&root, &args[1..]),
        "trace" => trace::run(&root),
        "static" => static_checks(&root),
        "compat-plan" => compat_plan::check(&root),
        "machine-exports" if args.iter().any(|a| a == "--list") => affected::machine_exports_list(args.iter().skip(1).find(|a| !a.starts_with("--")).map_or(root.as_path(), |d| Path::new(d.as_str()))),
        "machine-exports" => affected::machine_exports_check(&root),
        "clippy" => clippy_gate(&root),
        "arch-audit" => arch::audit(&root, args.iter().any(|a| a == "--list"), &arch_tools()),
        "purity" => policy::purity(&root),
        "unsafe-audit" => policy::unsafe_audit(&root),
        "csys-audit" => policy::csys_audit(&root),
        "env-audit" => policy::env_audit(&root),
        "opaque-audit" => policy::opaque_audit(&root),
        "crash-audit" => policy::crash_audit(&root),
        "csys-gen" => csys::run(&root, args.iter().any(|a| a == "--check")),
        "abi-gen" => abi::run(&root, args.iter().any(|a| a == "--check")),
        "unicode-gen" => unicode::run(&root, args.iter().any(|a| a == "--check")),
        "locale-gen" => langinfo::run(&root, args.iter().any(|a| a == "--check")),
        "native-gen" => native::run(&root, args.iter().any(|a| a == "--check")),
        "compat-gen" => compat::run(&root, args.iter().any(|a| a == "--check")),
        "math-gen" => math::run(&root, args.iter().any(|a| a == "--check")),
        "mathcmp" => math::run_mathcmp(&root, &args[1..]),
        "remez" => math::run_remez(&root, &args[1..]),
        "tables" => math::run_tables(&root, &args[1..]),
        "bench" if args.iter().any(|a| a == "--compare") => bench::compare(&root, &args[1..]),
        "bench" => bench::run(&root, &args[1..]),
        "fuzz" => fuzz::run(&root, &args[1..]),
        "fuzz-check" => fuzz::check(&root),
        "test" => test_all(&root, &args[1..]),
        "ctest" => ctest(&root),
        "nextest" => nextest::run(&std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()), &root, &args[1..]),
        "affected" => affected::run(&root, &args[1..]),
        "bench-rows" => benchrows::run(&root, &args[1..]),
        "so" => so::run_so(&root),
        "repro" => repro::run(&root, &args[1..]),
        "native" => native_examples::run_native(&root),
        "rust-dylib" => rust_dylib::run_rust_dylib(&root, args.get(1).map(String::as_str)),
        _ => {
            println!("usage: cargo xtask <stubs [--force] | trace | static | clippy | status [--schedule] | symgap | symcov [--tsv <file | compat-plan>] | arch-audit [--list] | purity | unsafe-audit | csys-audit | env-audit | opaque-audit | crash-audit | csys-gen [--check] | abi-gen [--check] | unicode-gen [--check] | locale-gen [--check] | native-gen [--check] | compat-gen [--check] | math-gen [--check] | mathcmp FUNC FMT [opts] | remez [--check] FIT... | tables [--check] | bench [--runs N] [--only <bench>,...] [--rows <bench>/<row>,... | --rows-file F | --affected <base>] [--build-only | --no-build] | bench-rows [--base <ref> | --range <a>..<b> | --files <p>,...] [--out F] [--compare [--runs N] [--v3] [--sorted] [--counters] [<cbench>...]] | fuzz <target> --time <minutes> | fuzz-check | so | test | ctest | nextest <args> | affected [--base <ref>] [--files <path>...] [--list | --run] | native | rust-dylib [<abi>] | repro [--release] [--dir <base>]>");
            Ok(())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {cmd}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("cannot run {cmd:?}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{cmd:?} failed: {status}")) }
}

fn arch_tools() -> arch::Tools {
    arch::Tools { rust_files: policy::rust_files, strip: policy::strip }
}

/// `cargo xtask static`: the gate's static checks (the first steps of `test`: arch-audit (ARCH-SEAMS), purity, unsafe-audit, csys-audit,
/// env-audit, opaque-audit, crash-audit, k5 (ffi.rs against liella_csys.h), align, trace, fuzz-check, compat-probes) in one command, all of them run (a failure does not hide the next), so they can be run
/// on keke before a long gate. `~/bin/static-checks` on the VMs is the same list.
fn static_checks(root: &Path) -> Result<(), String> {
    type Check = fn(&Path) -> Result<(), String>;
    let checks: [(&str, Check); 18] =
        [("align", align::static_check), ("arch-audit", |r| arch::audit(r, false, &arch_tools())), ("remap", remap::static_check), ("k5", csys::machine_decls_match), ("purity", policy::purity), ("unsafe-audit", policy::unsafe_audit), ("csys-audit", policy::csys_audit), ("env-audit", policy::env_audit), ("opaque-audit", policy::opaque_audit), ("crash-audit", policy::crash_audit), ("fork-child-alloc", forkalloc::fork_child_alloc), ("fs-order", fs_order::fs_order), ("trace", trace::run), ("fuzz-check", fuzz::check), ("machine-exports", affected::machine_exports_check), ("compat-gen", |r| compat::run(r, true)), ("compat-plan", compat_plan::check), ("compat-probes", compatprobe::check)];
    let failed: Vec<&str> = checks.iter().filter(|(name, f)| f(root).map_err(|e| eprintln!("static: {name} FAILED: {e}")).is_err()).map(|(name, _)| *name).collect();
    if failed.is_empty() { Ok(()) } else { Err(format!("failed: {}", failed.join(", "))) }
}

/// `cargo xtask clippy`: clippy with exactly the flags the gate and `affected` use (the whole workspace, all targets,
/// all features, warnings denied). Plain `cargo clippy` without `--all-features` misses (or invents) lints, e.g. on
/// boundary: per-package feature unification differs. Incremental, so cheap after the first run.
fn clippy_gate(root: &Path) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    run(Command::new(&cargo).current_dir(root).args(["clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"]))
}

fn test_all(root: &Path, extra: &[String]) -> Result<(), String> {
    policy::purity(root)?;
    policy::unsafe_audit(root)?;
    policy::csys_audit(root)?;
    policy::env_audit(root)?;
    policy::opaque_audit(root)?;
    policy::crash_audit(root)?;
    trace::run(root)?;
    // The fuzz harnesses still compile against the libraries (HYGIENE-1).
    fuzz::check(root)?;
    csys::run(root, true)?;
    abi::run(root, true)?;
    unicode::run(root, true)?;
    langinfo::run(root, true)?;
    native::run(root, true)?;
    compat::run(root, true)?;
    math::run(root, true)?;
    // The generated coefficients still follow from their fit files (D-10.12).
    let mut fits: Vec<String> = std::fs::read_dir(root.join("crates/math/fits")).map_err(|e| e.to_string())?.flatten().map(|e| e.path().display().to_string()).filter(|p| p.ends_with(".fit")).collect();
    fits.sort();
    fits.insert(0, "--check".into());
    math::run_remez(root, &fits)?;
    math::run_tables(root, &["--check".to_string()])?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    // The products first: tests of what only a process Liella started can show run C
    // conformance programs against them.
    so::run_so(root)?;
    workspace_tests(root, &cargo, None, extra)?;
    // Again with the exports interposed on the test processes (D-4.13): the harness then runs
    // on Liella's own `malloc` and `memcpy`, so a recursion in their paths crashes the run. Not
    // with `runtime`: a test process must keep the host C library's start and exit.
    workspace_tests(root, &cargo, Some("liella-boundary/export,liella-boundary/selftest"), extra)?;
    ctest(root)?;
    // The native Rust interface's examples, on the products built above (D-X.10).
    native_examples::run_native(root)?;
    // The Rust dylib of the current Rust ABI version and its program (D-X.10).
    rust_dylib::run_rust_dylib(root, None)?;
    symgap::run(root)?;
    // The status table and the schedule report: a report only, not failing the gate, until the
    // owners of the flagged work are settled (implementation-plan.md §4.3).
    status::run(root, false)
}

/// The workspace's tests: under nextest when it is available (`nextest.rs`; the doctests then
/// with `cargo test --doc`, which nextest does not run), else one `cargo test --workspace`.
/// `extra` goes to whichever runs the tests.
fn workspace_tests(root: &Path, cargo: &str, features: Option<&str>, extra: &[String]) -> Result<(), String> {
    let feat: Vec<String> = features.map(|f| vec!["--features".to_string(), f.to_string()]).unwrap_or_default();
    if !nextest::available(cargo) {
        return run(nextest::poison(&mut Command::new(cargo)).current_dir(root).args(["test", "--workspace"]).args(&feat).args(extra));
    }
    let mut args = vec!["--workspace".to_string()];
    args.extend(feat.iter().cloned());
    // The exports-interposed pass leaves out the pure-Rust sweeps (`nextest::PASS0_ONLY`, #889).
    if features.is_some() {
        args.extend(nextest::pass1_filter(extra).unwrap_or_default());
    }
    args.extend(extra.iter().cloned());
    // Every failure of both steps is reported before the gate stops.
    let tests = nextest::run(cargo, root, &args);
    let docs = run(nextest::poison(&mut Command::new(cargo)).current_dir(root).args(["test", "--workspace", "--doc"]).args(&feat));
    tests.and(docs)
}

fn ctest(root: &Path) -> Result<(), String> {
    run(Command::new("sh").current_dir(root).arg("conformance/run.sh"))
}
