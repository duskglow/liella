//! `cargo xtask <command>`: project tooling.
//!
//! | Command        | What it does |
//! |----------------|--------------|
//! | `stubs`        | Parse `glibc-functional-spec.md`; (re)generate pending test stubs per clause, per exported symbol and per kernel primitive; write `spec/clauses.tsv`, `spec/symbols.tsv`, `spec/primitives.tsv`. Files that already contain specified tests are never overwritten; instead their coverage of the spec is checked. |
//! | `trace`        | Clause traceability: every test, every row of `docs/differences-from-glibc.md` and every behaviour document names the clause IDs it bears on (or says why it has none), and every clause ID named exists in `spec/clauses.tsv`. Also run by `stubs` and `test`. `trace --untested [--out F]` is report only (never fails): every clause no test names, by section, with counts. |
//! | `symgap`       | Report the ABI names glibc's installed `libc.a` defines and Liella's does not, apart from those phases 14 and 15 own (report only). Also run by `test`. |
//! | `register`     | Check the saturation register layout (`docs/saturation-register.md` index, one file per section in `docs/saturation/`: section numbers unique (gaps allowed), headings, row ids unique across the SR records and the hand-written sections; the index is a generated view); also part of `static`. |
//! | `records-gen`  | Render the generated views of `records/views.toml` (decision tables, `docs/compat-flags.md`, the saturation index and sections 1-7) from the records; `--check` names each stale view. Also part of `static` (advisory on a story branch, fatal under `LIELLA_TRAIN=1`). |
//! | `records`      | `records check` validates every record (also part of `static`); `records fmt [--check]` rewrites them canonically; `records new <kind> <id>` writes a skeleton (RECORDS-1, D-X.55). |
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
//! | `tables`       | Generate or `--check` the elementary functions' constant tables, one `@generated` file per family in `crates/math/src/coeffs/` (D-10.3); also regenerates or checks `crates/math/KERNELS.md` (`kernel-index`). |
//! | `kernel-index` | Generate or `--check` `crates/math/KERNELS.md`, the one-page index of the libm fast-tier kernels (file:line, signature, doc tags). |

//! | `bench`        | Run the release-mode benchmarks and check `bench/budgets.tsv` (rule P6); `ratio_host` rows are gated on the median Liella/glibc ratio against the limit for this CPU model in `bench/host_limits.tsv` (`--runs N`, default 5 rounds; `--build-only` builds everything and stops, `--no-build` measures only and fails on a missing or stale program (the timing-lock split of `vm_bench.sh`), `BENCH_SETTLE_SECS` (default 12) idles before the first round; `--only a,b` runs just those benchmarks and says PARTIAL BENCH, not a merge bench); a CPU without entries is reported, not gated. With `--compare [--runs N] [<cbench>...]` (rule P10 (9); names are checked against `bench/c/*.c` before anything is built; `--compare` has no row filter: `--rows`/`--affected` are plain-`bench` only): build each C benchmark (default `cbench_opt`) against the products and against the host C library (dynamic and static), run them alternately N times after a discarded warm-up run (default 5), and print medians with min and max and the same-run ratio; no budgets are checked. Measurement columns (MEASURE-1): `--v3` (archive built with `-Ctarget-cpu=x86-64-v3`), `--sorted` (Liella linked with `--sort-section=name`: layout effect), `--counters` (per-row aggregate perf counters, `CBENCH_COUNTERS=1`; the summary table shows instr/op for Liella and glibc and the ratio); all columns in `target/liella/bench/measure.tsv`. `--stress env,path,fds,names,locale,deep,threads` (or `all`, STRESS-ENV-AUDIT, -2): advisory runs of the C benchmarks in a large environment (2,000 extra variables, a 200-directory `PATH`, 10,000 open descriptors; `names`: 50,000 users and groups, `locale`: 500 more names in the locale archive, both through an unprivileged user and mount namespace; `deep`: the fstree rows 64 directories down; `threads`: 64 idle threads), normally with `--counters-only`, compared with the same command without `--stress` to find costs that grow with the environment; limits are not set for it. |
//! | `csys-audit`   | Enforce the C syscall layer's rules: single-statement wrappers, freestanding, assembly only in machine/ (plan §2.5). |
//! | `env-audit`    | Reject an in-process `setenv`/`putenv`/`unsetenv`/`clearenv` in a test binary that also starts processes, unless it runs in `in_child`/`alone` or carries `// env-ok: <why>` (FL-1: the library's `environ` is the process's). |
//! | `opaque-audit` | Reject a direct call to an exported allocator-family name (`malloc`, `free`, ...) in a test file unless it goes through `selftest::opaque` or the file says `// opaque-ok: <why>` (GATE-SPEED-5: LLVM folds those names at opt-level >= 1); and a test file linking `liella_boundary` that calls any LLVM-builtin libc name (`labs`, `strlen`, `fabs`, ...) directly unless it is `#![no_builtins]` (OPAQUE-AUDIT). |
//! | `crash-audit`  | Reject a test that expects a core-dumping signal (`.signal(), Some(6)`, `Signaled(SIGABRT)`, `EXPECT-SIGNAL: 6`, `WTERMSIG(st) == SIGSEGV`) without as many `expect_crash` helper calls (`selftest::expect_crash`/`in_child_quiet`, `conformance/c/expect_crash.h`) unless the file says `dump-ok: <why>` (CORE-STORM: no core handler per deliberate crash). |
//! | `test`         | policy checks, the workspace tests (internal + translation suites; under cargo-nextest when installed, `LIELLA_NO_NEXTEST=1` for `cargo test`, see `nextest.rs`), then the C conformance suite, `native` and `rust-dylib`. The math accuracy sweeps skip a shard whose complete input key matches its last green run (prints `verdict cached: <name> <key>`; records in `target/test-verdicts/`, D-10.57); `LIELLA_NO_VERDICT_CACHE=1` runs every shard (the nightly and release runs). Each step prints `step: <name> <seconds>s` as it ends and the run ends with `step-summary:` lines, slowest first, and `step-total:` (`steptime.rs`; `affected --run` does the same when it escalates to FULL). |
//! | `ctest`        | Only the C conformance suite (`conformance/run.sh`). |
//! | `nextest`      | `cargo nextest run <args>` with the summary lines of a `cargo test` log added (`nextest.rs`); `affected --run` runs its tests through it. |
//! | `affected`     | Run only the tests a change can affect (conservative selection; see `affected.rs`): `--list` prints the selection with a reason per item, `--run` runs it and prints a summary; one of the two is required (without either: the one-line usage, exit 2; XTASK-AFFECTED-EXPLICIT); `--base <ref>` (default `origin/main`), `--files <path>...` instead of the git query. Escalates to the full gate for `csys/`, `crates/abi`, build and generated files. The full gate still runs once at merge. |
//! | `so`           | Build the products: `libc.a`, `libc.so.6` (version script from Appendix C.3 and the native C ABI's nodes, D-X.10) and `liella-crt1.o` in `target/liella` (never on Cargo's library path). Built with `[profile.fast]` (thin LTO, 16 codegen units; TOOL-PROFILE) for tests and iteration; `LIELLA_FULL_PROFILE=1` uses the shipped `release` profile (fat LTO). `bench` always builds `release` and refuses fast products (`target/liella/.profile` records which). |
//! | `repro`        | The products must not depend on the build directory (ruling #713): builds them in two fresh source copies under `target/repro` (`--dir <base>` elsewhere; not `/dev/shm` under `~/bin/gate`, #714) with the `fast` profile (`--release`: the shipped one) and compares every file of `target/liella`: whole files, then ELF sections (archive members too), accepting differences in `.symtab`/`.strtab` only. Fails naming the first differing file, member and section (`repro.rs`). A nightly job, not part of the gate. |
//! | `native`       | The native Rust interface (D-X.10): `liella-native` with each feature on its own (clippy, unit tests); its static example built against `libc.a` and run, its dynamic example built against `libc.so.6` and checked with `readelf`. |
//! | `fuzz`         | The fuzz harnesses of `fuzz/` (a workspace of its own, nightly, libFuzzer through `cargo fuzz`): `--list`; `<target> --time <minutes>` (or `--all`) runs in chunks resuming from the corpus outside the repository (`~/liella-fuzz-corpus`); `--repro`, `--tmin <file>`, `--cmin` for triage (implementation-plan.md §4.5). |
//! | `bench-rows`   | The bench rows a change can move plus the sentinel rows (`bench/sentinels.tsv`), for a TARGETED bench (`benchrows.rs`, BENCH-AFF): `--base <ref>` / `--range <a>..<b>` / `--files <p>,...`, `--out <file>` for `bench --rows-file`. `bench --affected <base>` selects and times only those rows (`bench/c/cbench_rows.h`). FULL for build, link, allocator, startup and harness changes. The full bench on main stays the safety net. |
//! | `sweep`        | The timing sweep (`sweep.rs`, SWEEP-RUN-TOOL): `run --out <dir> [--only a,b]` in a staged tree runs `bench --no-build --staged --counters` and exits 0 when every selected row was measured and `<dir>/sweep.tsv` written, flagged rows or not (nonzero: the measurement did not finish); `compare --new <class>=<sweep.tsv>... (--old <class>=<sweep.tsv>... \| --old-table <results.md>)` prints the report sections against the previous sweep. |
//! | `fuzz-check`  | Type-check every fuzz target on the pinned nightly (`cargo check --bins` in `fuzz/`; no fuzzer runtime). Part of `test` and of `affected` when `crates/` or `fuzz/` sources change: the harnesses are a workspace of their own and a library signature change breaks them silently (HYGIENE-1). |
//! | `rust-dylib`   | The Rust dylib `libliella_rust.so.<abi>` (D-X.10) of a version in `spec/abi/rust-abi.toml` (default: the current one): builds it with that version's pinned rustup toolchain (failing if it is absent), after checking that toolchain's `rustc -vV` against the definition; builds it and a program against it in `target/liella/rust/<abi>`, checks soname, note, `LIELLA_RUST_ABI` and `DT_NEEDED` with `readelf`; builds a synthetic second version beside it and checks that both coexist. |
//! | `classed-xref` | The classed pages' XREF bias rule (D-4.53, D-X.34): every out-of-page reference of page 1 addresses page 0's target + 4096, paired in order (an unpaired reference is an error). No file: assemble `csys/src/machine/fold.S` (also a `static` check); `<file>...`: check those archives, objects or linked objects (`so` checks `libc.a` and `libc.so.6`). |
//! | `jcc-check` | The hot machine assembly keeps every jump off 32-byte boundaries (D-4.67, JCC erratum): no jump or fused compare-and-jump crosses or ends on one, and each section holding one is 32-byte aligned. No file: assemble the files of `crates/boundary/jcc_pad.rs` with its flags (also a `static` check); `<file>...`: check those archives or objects (`so` checks `libc.a`). |
//! | `plants`       | Run a story's plants from a plants file (`plants.rs`, `docs/plants.md`): `<file> --dir <new RAM dir>` copies the tree there without `target/` (an existing `--dir` is refused), runs `cargo xtask so` and the baseline (failed: exit 2), applies each plant (exactly one site; `.S` plants name their `liella_machine_<fn>:` label), runs its command, restores by writing, and prints CAUGHT / SURVIVED / EQUIVALENT / WARN-RC-ONLY / WARN-NO-MSG; any SURVIVED exits 1. |
#![allow(clippy::disallowed_methods)] // a host tool: S8 governs the library crates only

#![forbid(unsafe_code)]

mod abbench;
mod abi;
mod affected;
mod align;
mod bench;
mod classed;
mod jcc;
mod benchstage;
mod forkalloc;
mod fs_order;
mod benchrows;
mod coldbss;
mod compat;
mod compatprobe;
mod compat_objects;
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
mod steptime;
mod symcov;
mod symgap;
mod sweep;
mod trace;
mod unicode;
mod compat_ids;
mod compat_plan;
mod register;
mod arch;
mod plants;
mod placeholders;
mod labnames;
mod privpaths;
mod kernel_index;
mod records;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The tree to work on, found at run time: the nearest ancestor of the current directory holding
/// `Cargo.toml` and `xtask/Cargo.toml`. The build-time `CARGO_MANIFEST_DIR` is only the fallback: a
/// binary reused by a copied tree (plant.sh's `<role>-plant`) would otherwise act on the original
/// tree (TOOLS-10). A staged tree (`bench-stage.manifest`) is a root too; no root at all is an error
/// (`benchstage::find_root`), never the build host's path on another machine.
fn root() -> Result<PathBuf, String> {
    let here = std::env::current_dir().ok();
    let built = Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_default();
    benchstage::find_root(here.as_deref(), &built)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let root = match root() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("xtask {cmd}: {e}");
            return ExitCode::from(2);
        }
    };
    let result = match cmd {
        "stubs" => spec::stubs(&root, args.iter().any(|a| a == "--force")),
        "status" => status::run(&root, args.iter().any(|a| a == "--schedule")),
        "symgap" => symgap::run(&root),
        "symcov" => symcov::run(&root, &args[1..]),
        "trace" if args.iter().any(|a| a == "--untested") => trace::untested(&root, args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)).map(Path::new)),
        "trace" => trace::run(&root),
        "static" => static_checks(&root),
        "compat-plan" => compat_plan::check(&root),
        "compat-ids" => compat_ids::check(&root),
        "register" => register::check(&root),
        "records-gen" => records::gen_cmd(&root, args[1..].iter().any(|a| a == "--check")),
        "records" => records::cmd(&root, &args[1..]),
        "machine-exports" if args.iter().any(|a| a == "--list") => affected::machine_exports_list(args.iter().skip(1).find(|a| !a.starts_with("--")).map_or(root.as_path(), |d| Path::new(d.as_str()))),
        "machine-exports" => affected::machine_exports_check(&root),
        "clippy" => clippy_gate(&root),
        "arch-audit" => arch::audit(&root, args.iter().any(|a| a == "--list"), &arch_tools()),
        "arch-check" => arch_check(&root),
        "purity" => policy::purity(&root),
        "unsafe-audit" => policy::unsafe_audit(&root),
        "csys-audit" => policy::csys_audit(&root),
        "env-audit" => policy::env_audit(&root),
        "opaque-audit" => policy::opaque_audit(&root),
        "crash-audit" => policy::crash_audit(&root),
        "reexec-audit" => policy::reexec_audit(&root),
        "lab-names" => labnames::check(&root),
        "private-paths" => privpaths::check(&root),
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
        "kernel-index" => kernel_index::run(&root, args[1..].iter().any(|a| a == "--check")),
        "bench" if args.iter().any(|a| a == "--compare") => bench::compare(&root, &args[1..]),
        "bench" => bench::run(&root, &args[1..]),
        "fuzz" => fuzz::run(&root, &args[1..]),
        "fuzz-check" => fuzz::check(&root),
        "test" => test_all(&root, &args[1..]),
        "ctest" => ctest(&root),
        "nextest" => nextest::run(&std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()), &root, &args[1..]),
        "affected" => return affected::main(&root, &args[1..]),
        "bench-rows" => benchrows::run(&root, &args[1..]),
        "sweep" => sweep::run(&root, &args[1..]),
        "so" => so::run_so(&root),
        "repro" => repro::run(&root, &args[1..]),
        "native" => native_examples::run_native(&root),
        "rust-dylib" => rust_dylib::run_rust_dylib(&root, args.get(1).map(String::as_str)),
        "plants" => return plants::main(&root, &args[1..]),
        "classed-xref" => classed::run(&root, &args[1..]),
        "jcc-check" => jcc::run(&root, &args[1..]),
        _ => {
            println!("usage: cargo xtask <stubs [--force] | trace [--untested [--out F]] | static | clippy | status [--schedule] | symgap | symcov [--tsv <file | compat-plan | register [--check]>] | arch-audit [--list] | arch-check | purity | unsafe-audit | csys-audit | env-audit | opaque-audit | crash-audit | csys-gen [--check] | abi-gen [--check] | unicode-gen [--check] | locale-gen [--check] | native-gen [--check] | compat-gen [--check] | math-gen [--check] | mathcmp FUNC FMT [opts] (FMT: f32|float|f, f64|double|d, f80|ld|long-double|l, f128|_Float128|q; whole names only) | remez [--check] FIT... | tables [--check] | kernel-index [--check] | records-gen [--check] | records (check | fmt [--check] | new <decision|compat|saturation> <id> [--section S] [--series S] [--story S]) | bench [--runs N] [--only <bench>,...] [--rows <bench>/<row>,... | --rows-file F | --affected <base>] [--build-only | --no-build] | bench-rows [--base <ref> | --range <a>..<b> | --files <p>,...] [--out F] | sweep (run --out <dir> [--only a,b] | compare --new <class>=<tsv>... (--old <class>=<tsv>... | --old-table <md>) [--label L]) [--compare [--runs N] [--v3] [--sorted] [--counters] [--tunables V] [--only a,b | <cbench>...]] (flags may stand anywhere after bench; an unknown --flag is an error; --tunables V, or else a non-empty GLIBC_TUNABLES in the caller's environment, sets it for both sides; the two differing is an error) | fuzz <target> --time <minutes> | fuzz-check | so | test | ctest | nextest <args> | affected (--list | --run) [--base <ref>] [--files <path>...] [--only <names>] [--tier direct|wide|full] [--with-exports] | native | rust-dylib [<abi>] | repro [--release] [--dir <base>]>");
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

/// `cargo xtask arch-check` (ARCH-RUST-CHECK, A-2): `cargo check` for aarch64 over the crates that build without
/// csys (the check set in xtask/src/arch.rs).
fn arch_check(root: &Path) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    for args in arch::cross_check_args() {
        run(Command::new(&cargo).current_dir(root).args(&args))?;
    }
    println!("arch-check: ok ({} check-set crates, {})", arch::CHECK_ALL_TARGETS.len() + arch::CHECK_LIB_ONLY.len(), arch::CROSS_TARGET);
    Ok(())
}

fn arch_tools() -> arch::Tools {
    arch::Tools { rust_files: policy::rust_files, strip: policy::strip }
}

/// `cargo xtask static`: the gate's static checks (the first steps of `test`: arch-audit (ARCH-SEAMS), purity, unsafe-audit, csys-audit,
/// env-audit, opaque-audit, crash-audit, k5 (ffi.rs against liella_csys.h), align, trace, fuzz-check, compat-probes) in one command, all of them run (a failure does not hide the next), so they can be run
/// on Zen 1 host before a long gate. `~/bin/static-checks` on the VMs is the same list.
fn static_checks(root: &Path) -> Result<(), String> {
    type Check = fn(&Path) -> Result<(), String>;
    let checks: [(&str, Check); 28] =
        [("align", align::static_check), ("arch-audit", |r| arch::audit(r, false, &arch_tools())), ("remap", remap::static_check), ("classed-xref", classed::static_check), ("jcc-check", jcc::static_check), ("k5", csys::machine_decls_match), ("purity", policy::purity), ("unsafe-audit", policy::unsafe_audit), ("csys-audit", policy::csys_audit), ("env-audit", policy::env_audit), ("opaque-audit", policy::opaque_audit), ("crash-audit", policy::crash_audit), ("fork-child-alloc", forkalloc::fork_child_alloc), ("fs-order", fs_order::fs_order), ("trace", trace::run), ("fuzz-check", fuzz::check), ("machine-exports", affected::machine_exports_check), ("compat-gen", |r| compat::run(r, true)), ("compat-plan", compat_plan::check), ("compat-probes", compatprobe::check), ("records", records::check), ("records-gen", records::fresh_check), ("register", register::check), ("compat-ids", compat_ids::check), ("lab-names", labnames::check), ("private-paths", privpaths::check), ("reexec-audit", policy::reexec_audit), ("tables-fresh", |r| math::run_tables(r, &["--check".to_string()]))];
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

/// `cargo xtask test`, the full gate. Every step prints `step: <name> <seconds>s` when it ends and the run ends with
/// the summary, slowest first, also when a step failed (`steptime.rs`, XTASK-TEST-STEP-TIMES).
fn test_all(root: &Path, extra: &[String]) -> Result<(), String> {
    let mut t = steptime::Steps::default();
    let r = test_steps(root, extra, &mut t);
    t.print_summary();
    r
}

fn test_steps(root: &Path, extra: &[String], t: &mut steptime::Steps) -> Result<(), String> {
    t.time("purity", || policy::purity(root))?;
    t.time("unsafe-audit", || policy::unsafe_audit(root))?;
    t.time("csys-audit", || policy::csys_audit(root))?;
    t.time("env-audit", || policy::env_audit(root))?;
    t.time("opaque-audit", || policy::opaque_audit(root))?;
    t.time("crash-audit", || policy::crash_audit(root))?;
    t.time("reexec-audit", || policy::reexec_audit(root))?;
    t.time("trace", || trace::run(root))?;
    // The fuzz harnesses still compile against the libraries (HYGIENE-1).
    t.time("fuzz-check", || fuzz::check(root))?;
    t.time("csys-gen-check", || csys::run(root, true))?;
    t.time("abi-gen-check", || abi::run(root, true))?;
    t.time("unicode-gen-check", || unicode::run(root, true))?;
    t.time("locale-gen-check", || langinfo::run(root, true))?;
    t.time("native-gen-check", || native::run(root, true))?;
    t.time("compat-gen-check", || compat::run(root, true))?;
    t.time("math-gen-check", || math::run(root, true))?;
    // The generated coefficients still follow from their fit files (D-10.12). The remez binary prints a
    // `step: remez/<fit>` line per fit. The fits run in parallel and replay unchanged verdicts (D-10.58), so their
    // times overlap: they name the slow fit, they do not add up to this step's time.
    let mut fits: Vec<String> = std::fs::read_dir(root.join("crates/math/fits")).map_err(|e| e.to_string())?.flatten().map(|e| e.path().display().to_string()).filter(|p| p.ends_with(".fit")).collect();
    fits.sort();
    fits.insert(0, "--check".into());
    t.time("remez-check", || math::run_remez(root, &fits))?;
    t.time("tables-check", || math::run_tables(root, &["--check".to_string()]))?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    // The products first: tests of what only a process Liella started can show run C
    // conformance programs against them.
    t.time("products", || so::run_so(root))?;
    workspace_tests(root, &cargo, None, extra, t, "tests")?;
    // Again with the exports interposed on the test processes (D-4.13): the harness then runs
    // on Liella's own `malloc` and `memcpy`, so a recursion in their paths crashes the run. Not
    // with `runtime`: a test process must keep the host C library's start and exit.
    workspace_tests(root, &cargo, Some("liella-boundary/export,liella-boundary/selftest"), extra, t, "tests-interposed")?;
    t.time("conformance", || ctest(root))?;
    // The native Rust interface's examples, on the products built above (D-X.10).
    t.time("native-examples", || native_examples::run_native(root))?;
    // The Rust dylib of the current Rust ABI version and its program (D-X.10).
    t.time("rust-dylib", || rust_dylib::run_rust_dylib(root, None))?;
    t.time("symgap", || symgap::run(root))?;
    // The status table and the schedule report: a report only, not failing the gate, until the
    // owners of the flagged work are settled (implementation-plan.md §4.3).
    t.time("status", || status::run(root, false))
}

/// The workspace's tests: under nextest when it is available (`nextest.rs`; the doctests then
/// with `cargo test --doc`, which nextest does not run), else one `cargo test --workspace`.
/// `extra` goes to whichever runs the tests.
/// `name` is the steps' name: `<name>-nextest` and `<name>-doc` (`<name>-cargo-test` without nextest).
fn workspace_tests(root: &Path, cargo: &str, features: Option<&str>, extra: &[String], t: &mut steptime::Steps, name: &str) -> Result<(), String> {
    let feat: Vec<String> = features.map(|f| vec!["--features".to_string(), f.to_string()]).unwrap_or_default();
    if !nextest::available(cargo) {
        return t.time(&format!("{name}-cargo-test"), || run(nextest::poison(&mut Command::new(cargo)).current_dir(root).args(["test", "--workspace"]).args(&feat).args(extra)));
    }
    let mut args = vec!["--workspace".to_string()];
    args.extend(feat.iter().cloned());
    // The exports-interposed pass leaves out the pure-Rust sweeps (`nextest::PASS0_ONLY`, #889)
    // and runs shard 0 only of the heavy math products sweeps (`nextest::PASS1_SHARD0`, D-X.36).
    // Binaries without liella-boundary code are the same in both passes and run once (CI-GATE-DIET).
    if features.is_some() {
        let once = if nextest::pass1_filter(extra, &[]).is_some() { t.time(&format!("{name}-unchanged-scan"), || nextest::unchanged_binaries(cargo, root, &feat)) } else { Vec::new() };
        args.extend(nextest::pass1_filter(extra, &once).unwrap_or_default());
    }
    args.extend(extra.iter().cloned());
    // Every failure of both steps is reported before the gate stops.
    let tests = t.time(&format!("{name}-nextest"), || nextest::run(cargo, root, &args));
    let docs = t.time(&format!("{name}-doc"), || run(nextest::poison(&mut Command::new(cargo)).current_dir(root).args(["test", "--workspace", "--doc"]).args(&feat)));
    tests.and(docs)
}

fn ctest(root: &Path) -> Result<(), String> {
    run(Command::new("sh").current_dir(root).arg("conformance/run.sh"))
}
