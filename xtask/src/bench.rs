//! `cargo xtask bench`: run the benchmark examples in release mode and check the budgets in
//! `bench/budgets.tsv` (rule P6). A phase cannot exit with a budget regression. Benchmarks with
//! `ratio_host` rows also run once built against the host C library, and those rows are checked
//! against its times from the same run.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

/// A C benchmark is `bench/c/<name>.c`: what only a process Liella started can measure (the
/// thread block, for one). It is linked statically against the products like a conformance
/// program. What it is linked against: the products, or the host C library (for the
/// comparison of `compare`), dynamically or statically.
#[derive(Clone, Copy, PartialEq)]
enum Build {
    Liella,
    /// Measurement only (MEASURE-1): the products' archive built with `-Ctarget-cpu=x86-64-v3`
    /// (`target/liella-v3/libc.a`), an upper bound on what CPU multiversioning could buy.
    LiellaV3,
    /// Measurement only (MEASURE-1): the Liella build linked with `--sort-section=name`: the same
    /// code in another order, to size the code-layout effect on a row.
    LiellaSorted,
    HostDynamic,
    HostStatic,
}

/// Set by `compare --no-build`: `c_build` returns the existing programs instead of compiling.
static COMPARE_NO_BUILD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Builds `bench/c/<name>.c` into `target/liella/bench` and returns the program's path. The
/// host builds use the same compiler flags as the products' build (`-fno-builtin` included), so
/// both sides run identical code.
/// The C benches that call libm (M6-0 `cbench_libm`): Liella's `libm.a` goes before `libc.a`, the
/// host builds add `-lm`. The archive is the one `cargo xtask so` stages (D-10.7), like `xtask mathcmp`.
fn uses_libm(name: &str) -> bool {
    name == "cbench_libm"
}

fn c_build(root: &Path, name: &str, build: Build) -> Result<std::path::PathBuf, String> {
    let dir = root.join("target/liella/bench");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if COMPARE_NO_BUILD.load(std::sync::atomic::Ordering::Relaxed) {
        // `compare --no-build`: measure the programs a `--build-only` run left (timing hosts have no compiler).
        let exe = match build {
            Build::Liella => dir.join(name),
            Build::LiellaV3 => dir.join(format!("{name}.v3")),
            Build::LiellaSorted => dir.join(format!("{name}.sorted")),
            Build::HostDynamic => dir.join(format!("{name}.host-dynamic")),
            Build::HostStatic => dir.join(format!("{name}.host-static")),
        };
        return if exe.exists() { Ok(exe) } else { Err(format!("{name}: --no-build: {} does not exist (run --build-only first)", exe.display())) };
    }
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let mut cmd = Command::new(cc);
    cmd.args(["-std=c17", "-O2", "-fno-builtin"]);
    let exe = match build {
        Build::Liella | Build::LiellaV3 | Build::LiellaSorted => {
            let lib = if build == Build::LiellaV3 { "target/liella-v3/libc.a" } else { "target/liella/libc.a" };
            cmd.args(["-static", "-nostdlib"]);
            if build == Build::LiellaSorted {
                cmd.arg("-Wl,--sort-section=name");
            }
            cmd.arg(root.join("target/liella/liella-crt1.o"))
                .arg("-I")
                .arg(root.join("include"))
                .arg(root.join(format!("bench/c/{name}.c")));
            if uses_libm(name) {
                cmd.arg(root.join("target/liella/libm.a"));
            }
            cmd.arg(root.join(lib)).arg("-lgcc");
            match build {
                Build::LiellaV3 => dir.join(format!("{name}.v3")),
                Build::LiellaSorted => dir.join(format!("{name}.sorted")),
                _ => dir.join(name),
            }
        }
        Build::HostDynamic | Build::HostStatic => {
            if build == Build::HostStatic {
                cmd.arg("-static");
            }
            cmd.arg(root.join(format!("bench/c/{name}.c")));
            if uses_libm(name) {
                cmd.arg("-lm");
            }
            dir.join(format!("{name}.host-{}", if build == Build::HostStatic { "static" } else { "dynamic" }))
        }
    };
    let built = cmd.arg("-o").arg(&exe).output().map_err(|e| format!("{name}: {e}"))?;
    if !built.status.success() {
        return Err(format!("{name} does not build:
{}", String::from_utf8_lossy(&built.stderr)));
    }
    Ok(exe)
}

/// The host-library build of `bench/c/<name>.c` (dynamically linked), rebuilt only when the
/// source is newer than the program (the host C library and compiler are the machine's).
fn host_build_cached(root: &Path, name: &str) -> Result<std::path::PathBuf, String> {
    let exe = root.join(format!("target/liella/bench/{name}.host-dynamic"));
    let src = root.join(format!("bench/c/{name}.c"));
    match (mtime(&exe), mtime(&src)) {
        (Some(e), Some(s)) if e >= s => Ok(exe),
        _ if NO_BUILD.load(std::sync::atomic::Ordering::Relaxed) => Err(format!("bench --no-build: {} is missing or older than bench/c/{name}.c; run `cargo xtask bench --build-only` first", exe.display())),
        _ => c_build(root, name, Build::HostDynamic),
    }
}

/// How `run` uses the build: `Normal` builds then measures (the merge bench, unchanged);
/// `BuildOnly` builds everything the measuring run needs and stops (run it under the shared queue,
/// outside the exclusive timing lock); `NoBuild` measures only and FAILS if a program is missing or
/// older than its inputs (run it under the timing lock).
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    BuildOnly,
    NoBuild,
}

static NO_BUILD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn mtime(p: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// The newest modification time of the files with these extensions below `dir`. `tests/` directories are skipped:
/// integration tests are never inputs of the library or of a bench program, and counting them made `--no-build`
/// refuse fresh products after a test-only change (2026-10-08: the baselines of e20ade9 and ccfca68 failed so: cargo
/// rightly kept `bench_kernel`, the check saw a newer test file).
fn newest_below(dir: &Path, exts: &[&str]) -> Option<std::time::SystemTime> {
    let mut best = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "target" && n != ".git" && n != "tests") {
                    stack.push(p);
                }
            } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.contains(&x)) {
                best = best.max(mtime(&p));
            }
        }
    }
    best
}

/// `--no-build` check: `exe` exists and is not older than `newest` (the newest of its inputs).
fn require_fresh(exe: &Path, newest: Option<std::time::SystemTime>, what: &str) -> Result<(), String> {
    match (mtime(exe), newest) {
        (Some(e), Some(n)) if e >= n => Ok(()),
        (None, _) => Err(format!("bench --no-build: {} is missing ({what}); run `cargo xtask bench --build-only` first", exe.display())),
        _ => Err(format!("bench --no-build: {} is older than its inputs ({what}); run `cargo xtask bench --build-only` again", exe.display())),
    }
}

/// BENCH_YIELD_CMD hook: runs the command (`sh -c`) and returns whether it yielded the machine (exit status 10).
/// `None`/empty (the variable unset) is a no-op; any other exit status, or a failure to start, is "did not yield"
/// (reported, never fatal: the bench goes on).
fn yield_point(cmd: Option<&str>) -> bool {
    let Some(cmd) = cmd.filter(|c| !c.trim().is_empty()) else { return false };
    match Command::new("sh").args(["-c", cmd]).status() {
        Ok(st) => match st.code() {
            Some(10) => true,
            Some(0) => false,
            other => {
                eprintln!("bench: BENCH_YIELD_CMD exited with {other:?}: not treated as a yield");
                false
            }
        },
        Err(e) => {
            eprintln!("bench: BENCH_YIELD_CMD could not run: {e}");
            false
        }
    }
}

/// Seconds of idle before the first measured round (thermal / frequency / cache settle after
/// another task's build): `BENCH_SETTLE_SECS`, default 12.
fn settle_secs(var: Option<&str>) -> u64 {
    var.and_then(|v| v.trim().parse().ok()).unwrap_or(12)
}

/// The whole environment of a C benchmark (BENCH-FIXED-ENV): fixed names, values and therefore lengths, on
/// both sides. `PATH` is for the rows that spawn `/bin/true` or search for a command, `HOME` for the
/// wordexp tilde rows; the harness adds `CBENCH_ROWS` / `CBENCH_COUNTERS` after this list. No `LANG`,
/// `LC_*`, `LANGUAGE` or `TZ`: the rows that need them set them themselves.
const BENCH_ENV: [(&str, &str); 2] = [("PATH", "/usr/local/bin:/usr/bin:/bin"), ("HOME", "/home/cbench")];

/// The command running a C benchmark: the name-service lookups run with a private /dev/shm
/// (see `private_shm`).
pub(crate) fn c_command(name: &str, exe: &Path) -> Command {
    stress_command(name, exe, STRESS.get().map_or(&[][..], Vec::as_slice), TUNABLES.get().map(String::as_str))
}

/// The stress profiles of `bench --stress` (STRESS-ENV-AUDIT): advisory runs that put the benchmark in a
/// large, realistic environment instead of the small fixed one, to find costs that only stay low because
/// the test environment is small. Each is fixed and reproducible, needs no root and touches no system file:
/// - `env`: 2,000 extra variables of 132 bytes each (about 258 KiB) named `AAA_STRESS_nnnn`, which sort
///   before every fixed name, so a lookup of `HOME`, `PATH` or a missing name walks all of them;
/// - `path`: a `PATH` of 200 directories, the 197 first of them missing, the real three last;
/// - `fds`: 10,000 open descriptors (3 to 10002, all `/dev/null`, soft `RLIMIT_NOFILE` 16384), so the
///   benchmark's own descriptors are numbered above 10,000 (bash opens them; it may reorder the variables).
///
/// - `deep` (STRESS-ENV-AUDIT-2): `CBENCH_STRESS_DEPTH=64`: cbench_fstree builds its tree 64 directories
///   (32-byte names, about 2 KiB of path) below its scratch directory;
/// - `threads`: `CBENCH_STRESS_THREADS=64` (or the caller's `CBENCH_STRESS_THREADS`, a positive number): that many idle
///   threads, started before main (`bench/c/cbench_stress.h`);
/// - `names`: /etc/passwd and /etc/group with 50,000 users and groups appended (`stressNNNNN`, `stressgNNNNN`,
///   each group with two members), and `CBENCH_STRESS_USER` / `CBENCH_STRESS_GROUP` naming the last ones for
///   the `*_named` rows of cbench_netdb;
/// - `locale`: the host's locale archive with 500 more locale names added (`st_SNNN.UTF-8`, all copies of
///   en_US.UTF-8, so the data is stored once and only the name tables grow).
///
/// `names` and `locale` replace system files only inside an unprivileged user and mount namespace (see
/// `namespace_command`; no sudo, nothing outside the benchmark's own mount namespace changes); the generated
/// files are kept in `STRESS_DIR` and reused. They reach every benchmark, the name-service ones included, and
/// do not combine with `fds`. The other profiles are not applied to `cbench_netdb` and `cbench_nscache`: their
/// `private_shm` route rebuilds the environment.
pub(crate) const STRESS_PROFILES: [&str; 7] = ["env", "path", "fds", "names", "locale", "deep", "threads"];
static STRESS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
/// The absolute `STRESS_DIR` of this run (set with `--stress`).
static STRESS_AT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

const STRESS_VARS: usize = 2000;
const STRESS_PATH_DIRS: usize = 200;
const STRESS_FDS: usize = 10_000;
const STRESS_NAMES: usize = 50_000;
const STRESS_LOCALES: usize = 500;
const STRESS_DEPTH: usize = 64;
const STRESS_THREADS: usize = 64;
/// Where `names` and `locale` keep their generated files (under the tree's target/, rebuilt when missing).
const STRESS_DIR: &str = "target/liella/bench/stress";
const STRESS_ARCHIVE: &str = "/usr/lib/locale/locale-archive";

/// `id -u` / `id -g` of the caller ("" when `id` fails: the namespace command then fails visibly).
fn own_id(flag: &str) -> String {
    Command::new("id").arg(flag).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// The `names` / `locale` route: `unshare -r -m` makes the caller root of a new user namespace with a private
/// mount namespace (unprivileged; no sudo); there the generated files are bind-mounted over the system ones
/// (`binds`: source, target; source `tmpfs` mounts a private tmpfs, as `private_shm` does for /dev/shm). A
/// nested `unshare -U` then maps the caller's own uid and gid back, so the benchmark runs as the caller, with
/// no capabilities, and sees the same files on the Liella and the glibc side. Mounts made in the namespace end
/// with it; the host's files are only read.
fn namespace_command(binds: &[(String, String)], exe: &Path) -> Command {
    let script = r#"while [ "$1" != -- ]; do if [ "$1" = tmpfs ]; then mount -t tmpfs -o mode=1777 tmpfs "$2"; else mount --bind "$1" "$2"; fi || exit 96; shift 2; done; shift; u=$1; g=$2; shift 2; exec unshare -U --map-user="$u" --map-group="$g" env -u PWD -u OLDPWD -u SHLVL -u _ "$@""#;
    let mut c = Command::new("unshare");
    c.args(["-r", "-m", "sh", "-c", script, "sh"]);
    for (s, d) in binds {
        c.arg(s).arg(d);
    }
    c.arg("--").arg(own_id("-u")).arg(own_id("-g")).arg(exe);
    c
}

/// The bind mounts of the `names` / `locale` profiles (None: neither is on).
fn stress_binds(dir: &Path, shm: bool, names: bool, locale: bool) -> Option<Vec<(String, String)>> {
    let mut b = Vec::new();
    let at = |f: &str| dir.join(f).to_string_lossy().into_owned();
    if names {
        b.push((at("passwd"), "/etc/passwd".to_string()));
        b.push((at("group"), "/etc/group".to_string()));
    }
    if locale {
        b.push((at("locale-archive"), STRESS_ARCHIVE.to_string()));
    }
    if b.is_empty() {
        return None;
    }
    if shm {
        b.push(("tmpfs".to_string(), "/dev/shm".to_string()));
    }
    Some(b)
}

/// The host's file with `n` generated lines appended (the system entries keep their places: root stays first).
fn appended(host: &str, n: usize, line: impl Fn(usize) -> String) -> Result<String, String> {
    let mut t = fs::read_to_string(host).map_err(|e| format!("--stress names: {host}: {e}"))?;
    if !t.is_empty() && !t.ends_with('\n') {
        t.push('\n');
    }
    for k in 0..n {
        t.push_str(&line(k));
        t.push('\n');
    }
    Ok(t)
}

/// Build the generated files of the `names` and `locale` profiles in `dir` (kept when present).
fn prepare_stress(dir: &Path, list: &[String]) -> Result<(), String> {
    let has = |p: &str| list.iter().any(|s| s == p);
    fs::create_dir_all(dir).map_err(|e| format!("--stress: {}: {e}", dir.display()))?;
    let write = |name: &str, text: &str| -> Result<(), String> {
        let tmp = dir.join(format!("{name}.tmp"));
        fs::write(&tmp, text).and_then(|_| fs::rename(&tmp, dir.join(name))).map_err(|e| format!("--stress: {name}: {e}"))
    };
    if has("names") && !(dir.join("passwd").is_file() && dir.join("group").is_file()) {
        let n = STRESS_NAMES;
        write("passwd", &appended("/etc/passwd", n, |k| format!("stress{k:05}:x:{}:{}:Stress user {k:05}:/home/stress{k:05}:/bin/sh", 200_000 + k, 200_000 + k))?)?;
        write("group", &appended("/etc/group", n, |k| format!("stressg{k:05}:x:{}:stress{k:05},stress{:05}", 200_000 + k, (k + 1) % n))?)?;
    }
    if has("locale") && !dir.join("locale-archive").is_file() {
        build_locale_archive(dir)?;
    }
    Ok(())
}

/// The host archive plus `STRESS_LOCALES` names, all of them en_US.UTF-8 compiled once by the host's localedef
/// (the names are directories of symbolic links to that one compiled locale).
fn build_locale_archive(dir: &Path) -> Result<(), String> {
    let work = dir.join("locale-build");
    let _ = fs::remove_dir_all(&work);
    let arch = work.join("root/usr/lib/locale");
    fs::create_dir_all(&arch).map_err(|e| format!("--stress locale: {e}"))?;
    fs::copy(STRESS_ARCHIVE, arch.join("locale-archive")).map_err(|e| format!("--stress locale: {STRESS_ARCHIVE}: {e}"))?;
    let en = work.join("en_US.UTF-8");
    // A path with a slash is an output directory; -c writes it despite warnings (exit 1).
    let st = Command::new("localedef").args(["-c", "-i", "en_US", "-f", "UTF-8", "--no-archive"]).arg(&en).status().map_err(|e| format!("--stress locale: localedef: {e}"))?;
    if st.code().is_none_or(|c| c > 1) || !en.join("LC_CTYPE").is_file() {
        return Err(format!("--stress locale: localedef -i en_US -f UTF-8 failed ({st})"));
    }
    let parts: Vec<_> = fs::read_dir(&en).map_err(|e| format!("--stress locale: {e}"))?.filter_map(|e| e.ok().map(|e| e.file_name())).collect();
    let mut names = Vec::new();
    for k in 0..STRESS_LOCALES {
        let d = work.join(format!("st_S{k:03}.UTF-8"));
        fs::create_dir(&d).map_err(|e| format!("--stress locale: {e}"))?;
        for p in &parts {
            std::os::unix::fs::symlink(en.join(p), d.join(p)).map_err(|e| format!("--stress locale: {e}"))?;
        }
        names.push(d);
    }
    let st = Command::new("localedef").arg(format!("--prefix={}", work.join("root").display())).arg("--add-to-archive").args(&names).status().map_err(|e| format!("--stress locale: localedef: {e}"))?;
    if !st.success() {
        return Err(format!("--stress locale: localedef --add-to-archive failed ({st})"));
    }
    fs::rename(arch.join("locale-archive"), dir.join("locale-archive")).map_err(|e| format!("--stress locale: {e}"))?;
    let _ = fs::remove_dir_all(&work);
    Ok(())
}

/// The `threads` profile's thread count: [`STRESS_THREADS`], or `CBENCH_STRESS_THREADS` from the
/// caller's environment when it holds a positive number (STDIO-MT-FPUTS-2: a cost curve over the
/// thread count; the benchmark's own environment is cleared, so the caller's value never reached it).
fn stress_threads() -> usize {
    std::env::var("CBENCH_STRESS_THREADS").ok().and_then(|v| v.parse().ok()).filter(|&n| n > 0).unwrap_or(STRESS_THREADS)
}

fn stress_command(name: &str, exe: &Path, stress: &[String], tunables: Option<&str>) -> Command {
    stress_command_in(STRESS_AT.get().map_or(Path::new(STRESS_DIR), |p| p.as_path()), name, exe, stress, tunables)
}

/// `tunables` is the `--tunables` value, passed in explicitly (not read from the process-global `TUNABLES` here),
/// so a test builds a command from its own arguments and no other test can change the result.
fn stress_command_in(dir: &Path, name: &str, exe: &Path, stress: &[String], tunables: Option<&str>) -> Command {
    let shm = matches!(name, "cbench_netdb" | "cbench_nscache");
    let has = |p: &str| stress.iter().any(|s| s == p);
    let on = |p: &str| !shm && has(p);
    let binds = stress_binds(dir, shm, has("names"), has("locale"));
    let mut c = if let Some(b) = &binds {
        namespace_command(b, exe)
    } else if shm {
        private_shm(exe)
    } else if on("fds") {
        let mut c = Command::new("bash");
        let script = format!("ulimit -n 16384 || exit 97; for ((i = 3; i < {}; i++)); do eval \"exec $i</dev/null\" || exit 98; done; exec env -u PWD -u OLDPWD -u SHLVL -u _ \"$@\"", STRESS_FDS + 3);
        c.args(["-c", &script, "bash"]).arg(exe);
        c
    } else {
        Command::new(exe)
    };
    // BENCH-FIXED-ENV: the benchmark sees only `BENCH_ENV`, never the caller's environment (build-tree
    // paths in it moved the environment-sensitive instruction counts with the directory a job ran from).
    // The same command runs the Liella and the host (glibc) side, so the comparison stays fair.
    c.env_clear();
    c.envs(BENCH_ENV);
    // BENCH-TUNABLES: the one caller setting that is passed on, and only through `--tunables <value>`
    // (never from the caller's own GLIBC_TUNABLES): both sides read it, so a forced dispatch level is the same.
    if let Some(t) = tunables {
        c.env("GLIBC_TUNABLES", t);
    }
    if on("env") {
        // 15 + 1 + 116 = 132 bytes per entry.
        let value = "x".repeat(116);
        for k in 0..STRESS_VARS {
            c.env(format!("AAA_STRESS_{k:04}"), &value);
        }
    }
    if on("path") {
        let mut dirs: Vec<String> = (0..STRESS_PATH_DIRS - 3).map(|k| format!("/nonexistent/stress/dir{k:03}")).collect();
        dirs.push(BENCH_ENV[0].1.to_string());
        c.env("PATH", dirs.join(":"));
    }
    if on("deep") {
        c.env("CBENCH_STRESS_DEPTH", STRESS_DEPTH.to_string());
    }
    if on("threads") {
        c.env("CBENCH_STRESS_THREADS", stress_threads().to_string());
    }
    if has("names") {
        c.env("CBENCH_STRESS_USER", format!("stress{:05}", STRESS_NAMES - 1));
        c.env("CBENCH_STRESS_GROUP", format!("stressg{:05}", STRESS_NAMES - 1));
    }
    // A targeted bench (BENCH-AFF): the binary times only these rows (`bench/c/cbench_rows.h`).
    if let Some(list) = ROW_ENV.get().and_then(|m| m.get(name)) {
        c.env("CBENCH_ROWS", list);
    }
    c
}

/// `--tunables <value>` (BENCH-TUNABLES): the `GLIBC_TUNABLES` value both sides of the comparison run with.
static TUNABLES: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Per C benchmark, the `CBENCH_ROWS` list of a targeted bench (absent: the binary runs every row).
static ROW_ENV: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();

/// Where `--build-only --affected` leaves the row selection for the measuring `--no-build --affected` run (the
/// remote-build route copies target/liella to the timing host).
const AFFECTED_ROWS: &str = "target/liella/bench/affected-rows.tsv";

/// The `CBENCH_ROWS` list per C binary: the kept rows, plus the base of every kept `ratio_to` row (often an
/// output row with no budget line, so it is not in `kept`). Binaries that run every row get no list.
fn row_env(per: &mut BTreeMap<String, (usize, Vec<String>)>, kept: &[Vec<&str>]) -> BTreeMap<String, String> {
    for x in kept {
        let e = &mut per.entry(x[0].to_string()).or_default().1;
        e.push(x[1].to_string());
        // A `ratio_to` row needs its base measured in the same run (often an output row with no budget line).
        if x.get(2) == Some(&"ratio_to") {
            if let Some((_, base)) = x.get(3).and_then(|b| b.split_once('x')) {
                if !e.iter().any(|r| r == base) {
                    e.push(base.to_string());
                }
            }
        }
    }
    per.iter().filter(|(b, (n, k))| b.starts_with("cbench_") && !k.is_empty() && k.len() < *n).map(|(b, (_, k))| (b.clone(), k.join(","))).collect()
}

/// A targeted bench: keep the budget rows of `sel` (plus every row of a Rust bench with a selected row: those
/// binaries cannot skip rows), set the C binaries' row lists, and say what runs. `strict` (`--rows`,
/// `--rows-file`): every name must be a row (a typo is an error); `--affected` intersects with `--only`.
fn select_rows<'a>(rows: Vec<Vec<&'a str>>, sel: &std::collections::BTreeSet<(String, String)>, strict: bool) -> Result<Vec<Vec<&'a str>>, String> {
    for (b, r) in sel.iter().filter(|_| strict) {
        if !rows.iter().any(|x| x.first() == Some(&b.as_str()) && x.get(1) == Some(&r.as_str())) {
            return Err(format!("bench: row `{b}/{r}` is not a row of bench/budgets.tsv (or not in --only)"));
        }
    }
    let rust: std::collections::BTreeSet<String> = sel.iter().filter(|(b, _)| !b.starts_with("cbench_")).map(|(b, _)| b.clone()).collect();
    let total = rows.len();
    let mut per: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
    for x in &rows {
        per.entry(x[0].to_string()).or_default().0 += 1;
    }
    let kept: Vec<Vec<&str>> = rows.into_iter().filter(|x| rust.contains(x[0]) || sel.contains(&(x[0].to_string(), x[1].to_string()))).collect();
    let env = row_env(&mut per, &kept);
    let bins = per.values().filter(|(_, k)| !k.is_empty()).count();
    println!("bench: TARGETED BENCH (not a merge bench): {} of {total} rows in {bins} binaries; the full `cargo xtask bench` stays the merge bench", kept.len());
    let _ = ROW_ENV.set(env);
    Ok(kept)
}

/// The `name<TAB>ns` lines of a benchmark's output, in order.
pub(crate) fn parse_rows(bench: &str, stdout: &[u8]) -> Result<Vec<(String, f64)>, String> {
    let mut rows = Vec::new();
    for line in String::from_utf8_lossy(stdout).lines() {
        if let Some((name, ns)) = line.split_once('\t') {
            let v: f64 = ns.trim().parse().map_err(|_| format!("{bench}: bad line `{line}`"))?;
            rows.push((name.to_string(), v));
        }
    }
    Ok(rows)
}

/// The median, minimum and maximum of `v` (not empty).
pub(crate) fn stats(v: &[f64]) -> (f64, f64, f64) {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let mid = s.len() / 2;
    let median = if s.len() % 2 == 1 { s[mid] } else { (s[mid - 1] + s[mid]) / 2.0 };
    (median, s[0], s[s.len() - 1])
}

/// The first line of a command's output, or `?`.
fn first_line(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(str::to_string))
        .unwrap_or_else(|| "?".into())
}

/// `--only a,b`: keep the budget rows of the named benchmarks (every name must have rows, so a typo
/// is an error, not an empty run that "passes"). A `ratio_to` row names a row of its own benchmark,
/// so a benchmark's rows are always kept whole.
fn select_only<'a>(rows: Vec<Vec<&'a str>>, names: &[String]) -> Result<Vec<Vec<&'a str>>, String> {
    if names.is_empty() {
        return Err("bench: --only needs at least one benchmark name".into());
    }
    for n in names {
        if !rows.iter().any(|r| r.first() == Some(&n.as_str())) {
            let mut known: Vec<&str> = rows.iter().filter_map(|r| r.first().copied()).collect();
            known.dedup();
            return Err(format!("bench: --only: `{n}` has no rows in bench/budgets.tsv (known: {})", known.join(", ")));
        }
    }
    Ok(rows.into_iter().filter(|r| r.first().is_some_and(|b| names.iter().any(|n| n == b))).collect())
}

/// A positional benchmark name: one starting with `-` is a mistyped flag, never a file name.
fn positional_name(n: &str) -> Result<String, String> {
    if n.starts_with('-') {
        return Err(format!("unknown option {n}: benchmark names are positional (`--compare cbench_opt`) or `--only a,b`"));
    }
    Ok(n.to_string())
}

/// The value of `--only`: a comma list of benchmark names (the same meaning as in a normal run).
fn only_list(v: Option<&str>) -> Result<Vec<String>, String> {
    let v = v.ok_or("--only needs a comma list of bench names")?;
    let list = v.split(',').filter(|n| !n.is_empty()).map(positional_name).collect::<Result<Vec<_>, _>>()?;
    if list.is_empty() {
        return Err("--only needs a comma list of bench names".into());
    }
    Ok(list)
}

/// `cargo xtask bench --compare [--runs N] [--v3] [--sorted] [--counters] [--only a,b | <cbench>...]` (rule
/// P10 (9)): each named C benchmark (default `cbench_opt`) built against the products, the host C
/// library dynamically linked (the reference, as `benchmarks/progress.md` uses it) and statically
/// linked (the like-for-like pair of the static products), run alternately: one warm-up round
/// whose results are discarded, then N rounds (default 5). Prints, per row, the median, minimum
/// and maximum over the rounds of each build (each value is itself the program's own best of
/// rounds) and the ratio of the medians (Liella ÷ host, dynamic), as a Markdown table; writes the
/// same as TSV to `target/liella/bench/<name>.compare.tsv`. A host build that does not build or
/// fails (a static glibc cannot load everything) leaves its column empty. No budgets are checked.
///
/// Measurement columns (MEASURE-1; none of them is a product change): `--v3` adds the products'
/// archive built with `-Ctarget-cpu=x86-64-v3`; `--sorted` adds the Liella build linked with
/// `--sort-section=name` (the same code in another order: the code-layout effect); `--counters`
/// runs each build once more with `CBENCH_COUNTERS=1` (`bench/c/cbench_counters.h`: aggregate
/// hardware counters of the process per row, no sampling) and reports instructions, cycles, IPC,
/// branch and L1d misses per operation. All columns of all benchmarks go to
/// `target/liella/bench/measure.tsv`. Liella's `libc.so.6` is not measured: it needs Liella's
/// dynamic loader (phase 14); the host loader refuses it (it requires its own libc's private
/// start-up interface).
pub fn compare(root: &Path, args: &[String]) -> Result<(), String> {
    let mut runs = 5usize;
    let mut names: Vec<String> = Vec::new();
    let (mut v3, mut sorted, mut counters) = (false, false, false);
    let (mut build_only, mut no_build) = (false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--compare" => {}
            "--build-only" => build_only = true,
            "--no-build" => no_build = true,
            "--v3" => v3 = true,
            "--sorted" => sorted = true,
            "--counters" => counters = true,
            "--runs" => runs = it.next().and_then(|v| v.parse().ok()).filter(|&n| n > 0).ok_or("--runs needs a positive number")?,
            "--tunables" => {
                let v = it.next().ok_or("--tunables needs a GLIBC_TUNABLES value (for example glibc.cpu.hwcaps=-AVX2)")?;
                let _ = TUNABLES.set(v.clone());
            }
            "--only" => names.extend(only_list(it.next().map(String::as_str))?),
            n => names.push(positional_name(n)?),
        }
    }
    if names.is_empty() {
        names.push("cbench_opt".into());
    }
    COMPARE_NO_BUILD.store(no_build, std::sync::atomic::Ordering::Relaxed);
    // Measurements always use the shipped profile (fat LTO), never the fast test profile.
    if !no_build {
        crate::so::run_so_as(root, crate::so::Profile::Release)?;
    }
    if v3 && !no_build {
        let out = root.join("target/liella-v3");
        fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        // Own product dir and output dir: a v3 archive never lands in target/product or target/liella,
        // whose `.profile` marker describes the baseline products.
        crate::so::build_archive(root, &root.join("target/product-v3"), &out.join("libc.a"), Some("-Ctarget-cpu=x86-64-v3"), crate::so::Profile::Release)?;
    }
    let cpu = cpu_model();
    println!("machine: {}, {cpu}, kernel {}, {} (host C library)", first_line("hostname", &[]), first_line("uname", &["-r"]), first_line("ldd", &["--version"]));
    println!("tunables: GLIBC_TUNABLES={} (both sides)", TUNABLES.get().map_or("(unset)", String::as_str));
    println!("method: {runs} rounds after a discarded warm-up round, the builds alternating; each value is the program's best of 7 rounds (cold rows: median of 31 processes)");
    // Index 0 is Liella (static), 1 the reference (glibc dynamic); the order is the column order.
    let mut builds = vec![(Build::Liella, "liella"), (Build::HostDynamic, "glibc_dyn"), (Build::HostStatic, "glibc_static")];
    if v3 {
        builds.push((Build::LiellaV3, "liella_v3"));
    }
    if sorted {
        builds.push((Build::LiellaSorted, "liella_sorted"));
    }
    let mut measure = measure_header();
    for name in &names {
        let mut exes: Vec<Option<std::path::PathBuf>> = Vec::new();
        for (b, _) in &builds {
            match c_build(root, name, *b) {
                Ok(e) => exes.push(Some(e)),
                Err(e) if *b == Build::HostStatic => {
                    eprintln!("{name}: no static host build: {e}");
                    exes.push(None);
                }
                Err(e) => return Err(e),
            }
        }
        if build_only {
            println!("{name}: built, not measured (--build-only)");
            continue;
        }
        let mut order: Vec<String> = Vec::new();
        let mut values: Vec<BTreeMap<String, Vec<f64>>> = vec![BTreeMap::new(); builds.len()];
        for round in 0..=runs {
            for (k, (b, _)) in builds.iter().enumerate() {
                let Some(exe) = exes[k].clone() else { continue };
                let out = c_command(name, &exe).output().map_err(|e| format!("{name}: {e}"))?;
                if !out.status.success() {
                    let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    if *b == Build::HostStatic {
                        eprintln!("{name}: the static host build fails ({why}); its column is left empty");
                        exes[k] = None;
                        values[k].clear();
                        continue;
                    }
                    return Err(format!("{} failed:\n{why}", exe.display()));
                }
                if round == 0 {
                    continue;
                }
                for (row, v) in parse_rows(name, &out.stdout)? {
                    if *b == Build::Liella && !order.contains(&row) {
                        order.push(row.clone());
                    }
                    values[k].entry(row).or_default().push(v);
                }
            }
        }
        let mut ctr: Vec<BTreeMap<String, [f64; 6]>> = vec![BTreeMap::new(); builds.len()];
        if counters {
            for (k, exe) in exes.iter().enumerate() {
                let Some(exe) = exe else { continue };
                let out = c_command(name, exe).env("CBENCH_COUNTERS", "1").output().map_err(|e| format!("{name}: {e}"))?;
                if out.status.success() {
                    ctr[k] = parse_counters(&out.stderr);
                }
            }
        }
        let cell = |k: usize, row: &str| values[k].get(row).filter(|v| !v.is_empty()).map(|v| stats(v));
        let mut tsv = String::from("row\thost_median\thost_min\thost_max\tliella_median\tliella_min\tliella_max\tratio\thost_static_median\thost_static_min\thost_static_max\n");
        println!("\n{name}: ns per operation, median (min-max) over {runs} rounds\n");
        if counters {
            println!("| Row | glibc (dynamic) | Liella | Ratio | glibc (static) | instr/op Liella | instr/op glibc | instr ratio |");
            println!("|---|---:|---:|---:|---:|---:|---:|---:|");
        } else {
            println!("| Row | glibc (dynamic) | Liella | Ratio | glibc (static) |");
            println!("|---|---:|---:|---:|---:|");
        }
        let fmt = |c: Option<(f64, f64, f64)>| c.map_or("-".to_string(), |(m, lo, hi)| format!("{m:.2} ({lo:.2}-{hi:.2})"));
        let num = |c: Option<(f64, f64, f64)>| c.map_or("\t\t".to_string(), |(m, lo, hi)| format!("{m:.2}\t{lo:.2}\t{hi:.2}"));
        let col = |b: Build| builds.iter().position(|(x, _)| *x == b);
        for row in &order {
            let (l, h, s) = (cell(0, row), cell(1, row), cell(2, row));
            let ratio = match (l, h) {
                (Some((lm, ..)), Some((hm, ..))) if hm > 0.0 => format!("{:.2}", lm / hm),
                _ => "-".into(),
            };
            if counters {
                println!("| `{row}` | {} | {} | {ratio} | {} | {} |", fmt(h), fmt(l), fmt(s), instr_cells(ctr[0].get(row), ctr[1].get(row)));
            } else {
                println!("| `{row}` | {} | {} | {ratio} | {} |", fmt(h), fmt(l), fmt(s));
            }
            tsv.push_str(&format!("{row}\t{}\t{}\t{ratio}\t{}\n", num(h), num(l), num(s)));
            // measure.tsv: ratios of medians to glibc dynamic, counter ratios on both pairings.
            let med = |k: Option<usize>| k.and_then(|k| cell(k, row)).map(|(m, ..)| m);
            let host = med(Some(1));
            let r = |k: Option<usize>| match (med(k), host) {
                (Some(x), Some(hm)) if hm > 0.0 => format!("{:.3}", x / hm),
                _ => "-".into(),
            };
            let c = |k: usize| ctr[k].get(row).copied();
            let instr = |k: usize| c(k).map(|v| v[0]).filter(|&v| v > 0.0);
            let ipc = |k: usize| c(k).filter(|v| v[0] > 0.0 && v[1] > 0.0).map(|v| v[0] / v[1]);
            let q = |a: Option<f64>, b: Option<f64>| match (a, b) {
                (Some(a), Some(b)) if b > 0.0 => format!("{:.3}", a / b),
                _ => "-".into(),
            };
            measure.push_str(&format!(
                "{name}\t{row}\t{}\t{}\tn/a_no_ldso\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                host.map_or("-".into(), |v| format!("{v:.2}")),
                r(Some(0)),
                r(col(Build::HostStatic)),
                r(col(Build::LiellaV3)),
                r(col(Build::LiellaSorted)),
                q(instr(0), col(Build::HostStatic).and_then(instr)),
                q(ipc(0), col(Build::HostStatic).and_then(ipc)),
                q(instr(0), instr(1)),
                q(ipc(0), ipc(1)),
            ));
            // Fixed schema (47 columns): a group for a build this run did not make is "-" throughout.
            for label in MEASURE_GROUPS {
                let Some(k) = builds.iter().position(|(_, l)| *l == label) else {
                    measure.push_str("\t-\t-\t-\t-\t-\t-\t-");
                    continue;
                };
                let f = |v: f64, p: usize| if v < 0.0 { "-".to_string() } else { format!("{v:.p$}") };
                measure.push_str(&format!("\t{}", med(Some(k)).map_or("-".into(), |v| format!("{v:.2}"))));
                match c(k) {
                    Some(v) => measure.push_str(&format!(
                        "\t{}\t{}\t{}\t{}\t{}\t{}",
                        f(v[0], 1),
                        f(v[1], 1),
                        ipc(k).map_or("-".into(), |x| format!("{x:.3}")),
                        f(v[2], 3),
                        f(v[3], 3),
                        f(v[4], 1)
                    )),
                    None => measure.push_str("\t-\t-\t-\t-\t-\t-"),
                }
            }
            measure.push('\n');
        }
        let path = root.join(format!("target/liella/bench/{name}.compare.tsv"));
        fs::write(&path, tsv).map_err(|e| format!("{}: {e}", path.display()))?;
        // After EACH bench (an overrun of a booking keeps what was measured); the file holds every bench so far.
        flush_measure(root, &measure)?;
    }
    let path = flush_measure(root, &measure)?;
    println!("\ncompare: all columns in {}", path.display());
    Ok(())
}

/// The build groups of `measure.tsv`, in column order (bench/parity-measure.tsv has the same 47 columns).
const MEASURE_GROUPS: [&str; 5] = ["liella", "glibc_dyn", "glibc_static", "liella_v3", "liella_sorted"];

/// The header of `measure.tsv`: 12 leading columns, then seven per build group; always the same, whatever flags ran.
fn measure_header() -> String {
    let mut h = String::from("bench\trow\tglibc_dyn_ns\tliella_static_ratio\tliella_dyn_ratio\tglibc_static_ratio\tv3_ratio\tsorted_ratio\tinstr_ratio_ss\tipc_ratio_ss\tinstr_ratio_sd\tipc_ratio_sd");
    for l in MEASURE_GROUPS {
        h.push_str(&format!("\t{l}_ns\t{l}_instr\t{l}_cycles\t{l}_ipc\t{l}_brmiss\t{l}_l1dmiss\t{l}_instr_user"));
    }
    h.push('\n');
    h
}

/// Writes the cumulative `measure.tsv` of a compare run (header plus the rows of every bench finished so far).
fn flush_measure(root: &Path, measure: &str) -> Result<std::path::PathBuf, String> {
    let dir = root.join("target/liella/bench");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join("measure.tsv");
    fs::write(&path, measure).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// The C benches with a `ratio_host` row anywhere in bench/budgets.tsv: those have a host build to count
/// against. Computed on the WHOLE table, before `--only`/`--rows`/`--affected` filter it.
fn host_capable_benches<'a>(rows: &[Vec<&'a str>]) -> std::collections::BTreeSet<&'a str> {
    rows.iter().filter(|r| r.get(2) == Some(&"ratio_host")).filter_map(|r| r.first().copied()).filter(|b| b.starts_with("cbench_")).collect()
}

/// The benches of a counted run: every selected C bench (in run order, once) that has a host build. A bench
/// selected only through a `max_ns` or `ratio_to` row still counts (its rows are counted on both sides).
fn counted_benches<'a>(benches: &[&'a str], host_capable: &std::collections::BTreeSet<&str>) -> Vec<&'a str> {
    let mut seen = std::collections::BTreeSet::new();
    benches.iter().copied().filter(|b| host_capable.contains(b) && seen.insert(*b)).collect()
}

/// The row filter in force, for messages: `--only`, `--rows`/`--rows-file`, `--affected`, or the full bench.
fn filter_desc(only: Option<&[String]>, rows: Option<&std::collections::BTreeSet<(String, String)>>, affected: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(o) = only {
        parts.push(format!("--only {}", o.join(",")));
    }
    if let Some(base) = affected {
        parts.push(format!("--affected {base}"));
    } else if let Some(r) = rows {
        parts.push(format!("--rows {}", r.iter().map(|(b, r)| format!("{b}/{r}")).collect::<Vec<_>>().join(",")));
    }
    if parts.is_empty() { "the full bench".into() } else { parts.join(" ") }
}

/// A counters table with no row is an error naming the filter, never a silent empty table (MKSTEMP-LONG-2 lost a
/// run to one): only C benches with a host build (a `ratio_host` row in bench/budgets.tsv) are counted.
fn check_counted(rows: usize, filter: &str) -> Result<(), String> {
    if rows > 0 {
        return Ok(());
    }
    Err(format!("bench --counters: the counters table is empty for {filter}: no selected row of a C bench with a host build (a `ratio_host` row in bench/budgets.tsv) was counted on both sides"))
}

/// The `ctr<TAB>row<TAB>instructions<TAB>cycles<TAB>branch-misses<TAB>L1d-misses<TAB>user-instructions`
/// lines (a trailing `uops` field, `-1` when the PMU lacks it) of a counted run (`bench/c/cbench_counters.h`), per row; other lines are ignored.
pub(crate) fn parse_counters(stderr: &[u8]) -> BTreeMap<String, [f64; 6]> {
    let mut m = BTreeMap::new();
    for line in String::from_utf8_lossy(stderr).lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if (f.len() == 7 || f.len() == 8) && f[0] == "ctr" {
            let mut v = [-1.0f64; 6];
            for (i, x) in f[2..].iter().enumerate() {
                v[i] = x.trim().parse().unwrap_or(-1.0);
            }
            m.insert(f[1].to_string(), v);
        }
    }
    m
}

/// The three `--counters` summary cells for one row: instructions per operation for Liella and for glibc
/// (dynamic), and their ratio (Liella / glibc); `-` where a side has no counted value (TOOLS-2).
fn instr_cells(liella: Option<&[f64; 6]>, glibc: Option<&[f64; 6]>) -> String {
    let get = |v: Option<&[f64; 6]>| v.map(|v| v[0]).filter(|&x| x > 0.0);
    let (l, g) = (get(liella), get(glibc));
    let f = |x: Option<f64>| x.map_or("-".to_string(), |x| format!("{x:.1}"));
    let ratio = match (l, g) {
        (Some(l), Some(g)) => format!("{:.2}", l / g),
        _ => "-".into(),
    };
    format!("{} | {} | {ratio}", f(l), f(g))
}

/// Median counters of one row, the spread over the counted runs, and the median ns/op of those runs.
type CountedRow = ([f64; 6], f64, f64);

/// One counted run (`CBENCH_COUNTERS=1`) of the Liella build and of the host C library per benchmark, outside
/// the timed rounds, and a compact table per row: Liella/glibc ratios of ns (the timing rounds' median, when
/// given), cycles, uops (issued) and instructions; `-` where the PMU lacks the event (TOOLS-5, MEASUREMENT RULE:
/// time decides, counts explain). Also `target/liella/bench/counters.tsv` (per-op values of both sides).
fn counted_table(root: &Path, names: &[&str], no_build: bool, ns_ratio: &BTreeMap<(String, String), f64>) -> Result<Counted, String> {
    let mut instr: BTreeMap<(String, String), f64> = BTreeMap::new();
    let mut cyc: BTreeMap<(String, String), CycleCell> = BTreeMap::new();
    let mut printed = 0usize;
    NO_BUILD.store(no_build, std::sync::atomic::Ordering::Relaxed);
    let mut tsv = String::from("bench\trow\tns_ratio\tcycles_ratio\tuops_ratio\tinstr_ratio\tliella_cycles\tglibc_cycles\tliella_uops\tglibc_uops\tliella_instr\tglibc_instr\tspread\tsuspect\n");
    println!("\ncounters per operation (median of 3 counted runs per side; ratio = Liella / glibc dynamic; `-` = event not available):");
    println!("{:<14} {:<26} {:>7} {:>7} {:>7} {:>7}  {:>9} {:>9} {:>9}", "bench", "row", "ns", "cycles", "uops", "instr", "cyc/op L", "cyc/op G", "spread L/G");
    let q = |a: f64, b: f64| if a > 0.0 && b > 0.0 { Some(a / b) } else { None };
    let cell = |x: Option<f64>| x.map_or("-".to_string(), |v| format!("{v:.3}"));
    let abs = |v: f64| if v > 0.0 { format!("{v:.1}") } else { "-".to_string() };
    for b in names {
        let host = host_build_cached(root, b)?;
        let liella = if no_build { root.join(format!("target/liella/bench/{b}")) } else { c_build(root, b, Build::Liella)? };
        // TOOLS-6: three counted runs per side; per row the MEDIAN of every column, a spread column, and sanity flags.
        let run3 = |exe: &Path| -> Result<BTreeMap<String, CountedRow>, String> {
            let mut runs = Vec::new();
            for _ in 0..3 {
                let out = c_command(b, exe).env("CBENCH_COUNTERS", "1").output().map_err(|e| format!("{b}: {e}"))?;
                if !out.status.success() {
                    return Err(format!("{b} (counted run) failed"));
                }
                runs.push((parse_counters(&out.stderr), parse_rows(b, &out.stdout).unwrap_or_default().into_iter().collect::<BTreeMap<_, _>>()));
            }
            let mut res = BTreeMap::new();
            for row in runs[0].0.keys() {
                let all: Vec<&[f64; 6]> = runs.iter().filter_map(|r| r.0.get(row)).collect();
                if all.len() < 2 {
                    continue;
                }
                let mut med = [-1.0f64; 6];
                for (k, m) in med.iter_mut().enumerate() {
                    let v: Vec<f64> = all.iter().map(|x| x[k]).filter(|&x| x >= 0.0).collect();
                    if v.len() == all.len() {
                        *m = stats(&v).0;
                    }
                }
                let spread = |k: usize| {
                    let v: Vec<f64> = all.iter().map(|x| x[k]).collect();
                    let (m, lo, hi) = stats(&v);
                    if m > 0.0 { (hi - lo) / m } else { 0.0 }
                };
                let ns: Vec<f64> = runs.iter().filter_map(|r| r.1.get(row).copied()).collect();
                res.insert(row.clone(), (med, spread(1).max(spread(0)), if ns.is_empty() { 0.0 } else { stats(&ns).0 }));
            }
            Ok(res)
        };
        let (l, g) = (run3(&liella)?, run3(&host)?);
        for (row, (lv, lsp, lns)) in &l {
            if lv[0] > 0.0 {
                instr.insert((b.to_string(), row.clone()), lv[0]);
            }
            let Some((gv, gsp, gns)) = g.get(row) else { continue };
            let ns = ns_ratio.get(&(b.to_string(), row.clone())).copied();
            let (cy, up, ins) = (q(lv[1], gv[1]), q(lv[5], gv[5]), q(lv[0], gv[0]));
            // FALLBACK-CYCLES: the clock each side ran at in its counted runs (cycles per ns = GHz).
            cyc.insert((b.to_string(), row.clone()), CycleCell { ratio: cy, ghz_l: q(lv[1], *lns), ghz_g: q(gv[1], *gns) });
            // Sanity (TOOLS-6): instr/op < 1, IPC > 6, or implied clock (cycles/ns of the counted run) outside 0.5-5 GHz
            // = the value is not believed. Judge by ns; short rows: trust the instruction column. DVFS moves the clock
            // between runs, so no tighter check than this is sound.
            let mut flags = String::new();
            for (tag, v, n) in [("L", lv, *lns), ("G", gv, *gns)] {
                if v[0] >= 0.0 && v[0] < 1.0 {
                    flags.push_str(&format!(" {tag}:instr<1"));
                }
                if v[0] > 0.0 && v[1] > 0.0 && v[0] / v[1] > 6.0 {
                    flags.push_str(&format!(" {tag}:IPC>6"));
                }
                if v[1] > 0.0 && n > 0.0 && !(0.5..=5.0).contains(&(v[1] / n)) {
                    flags.push_str(&format!(" {tag}:GHz={:.2}", v[1] / n));
                }
            }
            let sp = format!("{:.0}%/{:.0}%", lsp * 100.0, gsp * 100.0);
            printed += 1;
            println!("{b:<14} {row:<26} {:>7} {:>7} {:>7} {:>7}  {:>9} {:>9} {:>9}{}", cell(ns), cell(cy), cell(up), cell(ins), abs(lv[1]), abs(gv[1]), sp, if flags.is_empty() { String::new() } else { format!("  SUSPECT:{flags}") });
            tsv.push_str(&format!("{b}\t{row}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{sp}\t{}\n", cell(ns), cell(cy), cell(up), cell(ins), abs(lv[1]), abs(gv[1]), abs(lv[5]), abs(gv[5]), abs(lv[0]), abs(gv[0]), flags.trim()));
        }
    }
    let dir = root.join("target/liella/bench");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("counters.tsv"), tsv).map_err(|e| e.to_string())?;
    Ok(Counted { instr, cyc, rows: printed })
}

/// What the counted runs measured: instructions per call of Liella (the instruction limits) and, per row, the
/// cycles ratio and the clock of each side (FALLBACK-CYCLES).
struct Counted {
    /// How many rows the table printed (both sides counted).
    rows: usize,
    instr: BTreeMap<(String, String), f64>,
    cyc: BTreeMap<(String, String), CycleCell>,
}

/// One row's cycles: Liella/glibc ratio of the median cycles per call (user+kernel, `PERF_COUNT_HW_CPU_CYCLES`
/// without `exclude_kernel`), and each side's GHz (cycles / ns of its counted runs). `None` = no counter.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct CycleCell {
    ratio: Option<f64>,
    ghz_l: Option<f64>,
    ghz_g: Option<f64>,
}

/// FALLBACK-CYCLES (owner ruling 2026-10-10): CPU models whose clock varies run to run (low-power Ivy Bridge:
/// 1.5-2.5 GHz for both libraries alike) decide their ratio_host rows on CYCLES, not ns. One line per CPU model
/// in this file (`cpu<TAB>rationale`); the cycle limits are in `CYCLE_LIMITS` (the host_limits.tsv format).
const CYCLE_CPUS: &str = "bench/cycle_cpus.tsv";
const CYCLE_LIMITS: &str = "bench/host_cycle_limits.tsv";

fn parse_cycle_cpus(text: &str) -> Vec<String> {
    text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).filter_map(|l| l.split('\t').next()).map(|c| c.trim().to_string()).collect()
}

/// The decision of one ratio_host row on a cycles CPU: on the cycles ratio when there is one and a cycle limit
/// for the row; otherwise on the ns median and the ns limit (so a missing counter or a row without a cycle limit
/// is still gated as before, and says so). Returns (metric, limit shown, verdict, failure, known regression).
fn decide_cycles(key: (&str, &str), ns: (f64, f64), ns_lim: Option<&Limit>, cell: Option<&CycleCell>, cyc_lim: Option<&Limit>) -> (String, Option<f64>, String, Option<String>, Option<String>) {
    let (bench, name) = key;
    let (med, worst) = ns;
    let (metric, value, lim) = match (cell.and_then(|c| c.ratio), cyc_lim) {
        (Some(c), Some(l)) => ("cycles", c, Some(l)),
        (None, Some(_)) => ("ns (cycles counter missing)", med, ns_lim),
        (_, None) => ("ns (no cycle limit)", med, ns_lim),
    };
    let Some(l) = lim else {
        return (metric.to_string(), None, "not gated".to_string(), None, None);
    };
    if value <= l.limit {
        return (metric.to_string(), Some(l.limit), "ok".to_string(), None, None);
    }
    let what = if metric == "cycles" { format!("cycles ratio {value:.3} exceeds the cycle limit {:.3} (ns median {med:.3}, advisory)", l.limit) } else { format!("median ratio {value:.3} exceeds the limit {:.3} (worst {worst:.2}; decided on {metric})", l.limit) };
    match l.status {
        Status::Gate => (metric.to_string(), Some(l.limit), "FAIL".to_string(), Some(format!("{bench}/{name}: {what} ({})", l.why)), None),
        Status::KnownRegression => (metric.to_string(), Some(l.limit), "FAIL (known regression, PERF-REGRESS)".to_string(), None, Some(format!("{bench}/{name}: {what}: known regression, PERF-REGRESS ({})", l.why))),
    }
}

fn private_shm(exe: &Path) -> Command {
    let id = |flag: &str| Command::new("id").arg(flag).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let sudo = Command::new("sudo").args(["-n", "true"]).status().is_ok_and(|s| s.success());
    let setpriv = Command::new("setpriv").arg("--version").output().is_ok_and(|o| o.status.success());
    match (sudo && setpriv, id("-u"), id("-g")) {
        (true, Some(u), Some(g)) => {
            let mut c = Command::new("sudo");
            c.args(["-n", "unshare", "-m", "sh", "-c", "u=$1; g=$2; P=$3; H=$4; shift 4; mount -t tmpfs -o mode=1777 tmpfs /dev/shm && exec setpriv --reuid=\"$u\" --regid=\"$g\" --init-groups env -i PATH=\"$P\" HOME=\"$H\" ${GLIBC_TUNABLES:+GLIBC_TUNABLES=\"$GLIBC_TUNABLES\"} ${CBENCH_ROWS:+CBENCH_ROWS=\"$CBENCH_ROWS\"} ${CBENCH_COUNTERS:+CBENCH_COUNTERS=\"$CBENCH_COUNTERS\"} \"$@\"", "sh"]).arg(u).arg(g).arg(BENCH_ENV[0].1).arg(BENCH_ENV[1].1).arg(exe);
            c
        }
        _ => Command::new(exe),
    }
}

/// The CPU model string (`model name` in `/proc/cpuinfo`), which keys the `ratio_host` limits in
/// `bench/host_limits.tsv`; `?` when it cannot be read.
fn cpu_model() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|t| cpu_model_of(&t))
        .unwrap_or_else(|| "?".into())
}

/// The first `model name` line of a `/proc/cpuinfo` text.
fn cpu_model_of(cpuinfo: &str) -> Option<String> {
    cpuinfo.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split_once(':')).map(|(_, v)| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// ICOUNT-LIMITS: the file of instruction-per-call limits (see `check_instr_limits`).
const INSTR_LIMITS: &str = "bench/instr_limits.tsv";

/// One line of `bench/instr_limits.tsv`: the most instructions per call Liella may execute in one row (the
/// median of the counted runs, `bench/c/cbench_counters.h`).
#[derive(Debug, PartialEq)]
struct InstrLimit {
    cpu: String,
    bench: String,
    row: String,
    max: f64,
}

impl InstrLimit {
    /// `*` applies to every CPU; otherwise the `model name` must match exactly.
    fn applies(&self, cpu: &str) -> bool {
        self.cpu == "*" || self.cpu == cpu
    }
}

/// Parses `bench/instr_limits.tsv` (columns `cpu bench row max_instr rationale`; `#` and blank lines ignored).
fn parse_instr_limits(text: &str) -> Result<Vec<InstrLimit>, String> {
    let mut v = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 4 {
            return Err(format!("{INSTR_LIMITS}: expected at least 4 columns: `{line}`"));
        }
        let max = f[3].trim().parse::<f64>().ok().filter(|m| *m > 0.0).ok_or(format!("{INSTR_LIMITS}: {}/{}: bad limit `{}`", f[1], f[2], f[3]))?;
        v.push(InstrLimit { cpu: f[0].to_string(), bench: f[1].to_string(), row: f[2].to_string(), max });
    }
    Ok(v)
}

/// The failures of the instruction limits for this CPU: a limited row (among the `selected` rows of a bench that
/// was counted) whose Liella instructions per call exceed the limit, or that has no counted value at all. Instruction
/// counts are near-deterministic, unlike the time of a one-offset row (code placement moves it +-10%).
fn check_instr_limits(limits: &[InstrLimit], cpu: &str, instr: &BTreeMap<(String, String), f64>, selected: &std::collections::BTreeSet<(String, String)>, counted: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for l in limits.iter().filter(|l| l.applies(cpu) && counted.contains(&l.bench.as_str()) && selected.contains(&(l.bench.clone(), l.row.clone()))) {
        match instr.get(&(l.bench.clone(), l.row.clone())) {
            Some(&v) if v > l.max => out.push(format!("{}/{}: {v:.1} instructions per call exceed the limit {} ({INSTR_LIMITS}; INSTR-REGRESS)", l.bench, l.row, l.max)),
            Some(_) => {}
            None => out.push(format!("{}/{}: no counted instruction value, the limit {} could not be checked ({INSTR_LIMITS})", l.bench, l.row, l.max)),
        }
    }
    out
}

/// How a `ratio_host` limit is enforced: `gate` is an ordinary regression limit; `known_regression`
/// is a limit the row is known to exceed (a confirmed slowdown, kept at its earlier bound): the
/// failure is printed, labelled, and still fails the run, so it cannot be overlooked.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Status {
    Gate,
    KnownRegression,
}

/// One line of `bench/host_limits.tsv`: the limit on the median Liella/glibc ratio of one row on
/// one CPU model.
#[derive(Clone, Debug, PartialEq)]
struct Limit {
    cpu: String,
    bench: String,
    row: String,
    limit: f64,
    status: Status,
    why: String,
}

/// Parses `bench/host_limits.tsv` (columns `cpu bench row limit status rationale`; `#` lines and
/// blank lines are skipped).
fn parse_limits(text: &str) -> Result<Vec<Limit>, String> {
    let mut v = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let [cpu, bench, row, limit, status, why] = f.as_slice() else {
            return Err(format!("host_limits.tsv: expected 6 columns: `{line}`"));
        };
        let limit = limit.parse::<f64>().map_err(|e| format!("host_limits.tsv: {bench}/{row}: bad limit `{limit}`: {e}"))?;
        let status = match *status {
            "gate" => Status::Gate,
            "known_regression" => Status::KnownRegression,
            s => return Err(format!("host_limits.tsv: {bench}/{row}: unknown status `{s}`")),
        };
        v.push(Limit { cpu: cpu.to_string(), bench: bench.to_string(), row: row.to_string(), limit, status, why: why.to_string() });
    }
    Ok(v)
}

/// The limit of `bench`/`row` for the CPU model `cpu` (exact string match), if there is one.
fn limit_for<'a>(limits: &'a [Limit], cpu: &str, bench: &str, row: &str) -> Option<&'a Limit> {
    limits.iter().find(|l| l.cpu == cpu && l.bench == bench && l.row == row)
}

/// The gate statistic of one row: the same-run median Liella/glibc ratio (the ratio of the two
/// builds' medians over the rounds, as `--compare` prints it), and the worst ratio (slowest
/// Liella round / fastest glibc round), which is a diagnostic only. `None` without data or with
/// a zero glibc time.
fn ratio_stats(liella: &[f64], host: &[f64]) -> Option<(f64, f64)> {
    if liella.is_empty() || host.is_empty() {
        return None;
    }
    let (lm, _, lmax) = stats(liella);
    let (hm, hmin, _) = stats(host);
    if hm <= 0.0 || hmin <= 0.0 {
        return None;
    }
    Some((lm / hm, lmax / hmin))
}

/// Rounds every `ratio_host` benchmark runs before the adaptive rule looks at it (BENCH-TP).
const ROUNDS_FIRST: usize = 3;

/// The adaptive-rounds rule (BENCH-TP): can rounds 4 and 5 change what this row reports? The gate
/// statistic is median(Liella) / median(glibc). The median of 5 values, 3 of which are known, lies
/// between the minimum and maximum of those 3, so with 3 rounds in hand the final statistic lies in
/// [lmin / hmax, lmax / hmin] whatever the last two rounds bring. The row is settled when that
/// whole interval is on one side of every line its verdict depends on: the regression limit (if
/// the CPU has one) and the parity line 1.00. Settled rows cannot change their gate verdict or
/// their side of parity; only the printed median and the informational 1.10 target flag may differ
/// in the last digits. `parity` false (`BENCH_SETTLE_PARITY=0`) drops the parity line: only the
/// limit has to be settled, and rows near 1.00 are left to the back-to-back rerun they get anyway.
fn row_settled(liella: &[f64], host: &[f64], limit: Option<f64>, parity: bool) -> bool {
    if liella.len() < ROUNDS_FIRST || host.len() < ROUNDS_FIRST {
        return false;
    }
    let (_, lmin, lmax) = stats(liella);
    let (_, hmin, hmax) = stats(host);
    if hmin <= 0.0 || hmax <= 0.0 {
        return false;
    }
    let (lo, hi) = (lmin / hmax, lmax / hmin);
    [parity.then_some(1.0), limit].into_iter().flatten().all(|line| hi <= line || lo > line)
}

type Rounds = BTreeMap<(String, String), Vec<f64>>;

/// Every `ratio_host` row of benchmark `b` is settled (see `row_settled`).
fn bench_settled(b: &str, rows: &[Vec<&str>], limits: &[Limit], cpu: &str, parity: bool, lr: &Rounds, hr: &Rounds) -> bool {
    rows.iter().filter(|r| r.first() == Some(&b) && r.get(2) == Some(&"ratio_host")).all(|r| {
        let key = (b.to_string(), r[1].to_string());
        let limit = limit_for(limits, cpu, b, r[1]).map(|l| l.limit);
        row_settled(lr.get(&key).map(Vec::as_slice).unwrap_or(&[]), hr.get(&key).map(Vec::as_slice).unwrap_or(&[]), limit, parity)
    })
}

/// The target for `ratio_host` rows (owner, 2026-10-05): within about 10% of glibc. A miss is
/// informational; the regression limit is the gate.
const TARGET: f64 = 1.10;

/// `cargo xtask bench [--runs N]`. Benchmarks with `ratio_host` rows run N rounds (default 5)
/// after a discarded warm-up round, the Liella and host-library builds alternating; the gate
/// statistic of such a row is the median Liella/glibc ratio over the rounds, checked against the
/// limit for this machine's CPU model in `bench/host_limits.tsv` (a CPU without entries is
/// measured and reported, not gated). The `bound` of a `ratio_host` row in `budgets.tsv` is the
/// target, which is reported but never fails the run.
///
/// Adaptive rounds (BENCH-TP): without `--runs` and without `BENCH_ROUNDS_FIXED=1` a benchmark
/// first runs `ROUNDS_FIRST` rounds; only if one of its rows is unsettled (`row_settled`) does it
/// run up to the full 5. `--runs N` or `BENCH_ROUNDS_FIXED=1` runs exactly N rounds as before.
/// Every round's values are written to `target/liella/bench/rounds.tsv` for replaying the rule.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut runs = 5usize;
    let mut fixed = std::env::var("BENCH_ROUNDS_FIXED").is_ok_and(|v| !v.is_empty() && v != "0");
    let mut only: Option<Vec<String>> = None;
    let mut rows_sel: Option<std::collections::BTreeSet<(String, String)>> = None;
    let mut affected: Option<String> = None;
    let mut ab_from: Option<std::path::PathBuf> = None;
    let mut mode = Mode::Normal;
    let (mut counters, mut counters_only) = (false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--counters" => counters = true,
            "--counters-only" => {
                counters = true;
                counters_only = true;
            }
            "--runs" => {
                runs = it.next().and_then(|v| v.parse().ok()).filter(|&n| n > 0).ok_or("--runs needs a positive number")?;
                fixed = true;
            }
            "--ab-from" => ab_from = Some(std::path::PathBuf::from(it.next().ok_or("--ab-from needs the other tree (A; this tree is B)")?)),
            "--build-only" => mode = Mode::BuildOnly,
            "--no-build" => mode = Mode::NoBuild,
            "--only" => only = Some(it.next().ok_or("--only needs a comma list of bench names")?.split(',').filter(|n| !n.is_empty()).map(str::to_string).collect()),
            "--rows" => {
                let list = it.next().ok_or("--rows needs a comma list of <bench>/<row>")?;
                let mut set = std::collections::BTreeSet::new();
                for x in list.split(',').filter(|x| !x.is_empty()) {
                    let (b, r) = x.split_once('/').ok_or(format!("--rows: `{x}` is not <bench>/<row>"))?;
                    set.insert((b.to_string(), r.to_string()));
                }
                rows_sel = Some(set);
            }
            "--rows-file" => {
                let f = it.next().ok_or("--rows-file needs a file (`cargo xtask bench-rows --out`)")?;
                let text = fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
                rows_sel = crate::benchrows::parse_rows_file(&text);
                if rows_sel.is_none() {
                    println!("bench: {f} selects every row (FULL)");
                }
            }
            "--stress" => {
                // STRESS-ENV-AUDIT: advisory, never gating (see `stress_command`).
                let list: Vec<String> = it.next().ok_or("--stress needs a comma list of profiles (env,path,fds,names,locale,deep,threads or all)")?.split(',').filter(|p| !p.is_empty()).map(str::to_string).collect();
                let list = if list.iter().any(|p| p == "all") { STRESS_PROFILES.iter().map(|p| p.to_string()).collect() } else { list };
                if let Some(bad) = list.iter().find(|p| !STRESS_PROFILES.contains(&p.as_str())) {
                    return Err(format!("--stress: unknown profile `{bad}` (known: {}, all)", STRESS_PROFILES.join(",")));
                }
                println!("bench: stress profiles {} (advisory: limits are set for the fixed environment)", list.join(","));
                let dir = root.join(STRESS_DIR);
                prepare_stress(&dir, &list)?;
                let _ = STRESS_AT.set(fs::canonicalize(&dir).unwrap_or(dir));
                let _ = STRESS.set(list);
            }
            "--affected" => affected = Some(it.next().ok_or("--affected needs a base ref (ignored in a tree without git: the .changed list)")?.clone()),
            "--tunables" => {
                let v = it.next().ok_or("--tunables needs a GLIBC_TUNABLES value (for example glibc.cpu.hwcaps=-AVX2)")?;
                let _ = TUNABLES.set(v.clone());
            }
            other => return Err(format!("bench: unknown argument `{other}`")),
        }
    }
    let table = fs::read_to_string(root.join("bench/budgets.tsv")).map_err(|e| format!("bench/budgets.tsv: {e}"))?;
    let mut rows: Vec<Vec<&str>> = table.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).map(|l| l.split('\t').collect()).collect();
    // XTASK-BENCH-ROWS-COUNTERS: which C benches have a host build is a property of the whole table, not of
    // the selected rows (`--rows b/<max_ns row>` used to drop `b` from the counted runs: an empty table).
    let host_capable = host_capable_benches(&rows);
    let filter = filter_desc(only.as_deref(), rows_sel.as_ref(), affected.as_deref());
    if let Some(names) = &only {
        rows = select_only(rows, names)?;
        println!("bench: PARTIAL BENCH (not a merge bench): only {}; the full `cargo xtask bench` stays the merge bench", names.join(", "));
    }
    if let Some(base) = &affected {
        // BENCH-AFF: the build phase selects (and leaves the file for the measuring phase, which may run on
        // another host without the sources' history); a normal run selects itself.
        let path = root.join(AFFECTED_ROWS);
        let text = if mode == Mode::NoBuild {
            fs::read_to_string(&path).map_err(|e| format!("{}: {e} (run `cargo xtask bench --build-only --affected <base>` first)", path.display()))?
        } else {
            let files = crate::benchrows::changed(root, base, None, None)?;
            let (_, text, plan) = crate::benchrows::plan_for(root, &files)?;
            for w in &plan.full {
                println!("bench: --affected: FULL: {w}");
            }
            fs::create_dir_all(path.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
            fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
            text
        };
        rows_sel = crate::benchrows::parse_rows_file(&text);
        if rows_sel.is_none() {
            println!("bench: --affected selects every row (FULL; see {AFFECTED_ROWS})");
        }
    }
    if let Some(sel) = &rows_sel {
        rows = select_rows(rows, sel, affected.is_none())?;
    }
    let limits_text = fs::read_to_string(root.join("bench/host_limits.tsv")).map_err(|e| format!("bench/host_limits.tsv: {e}"))?;
    let limits = parse_limits(&limits_text)?;
    let cpu = cpu_model();
    let cycle_cpu = fs::read_to_string(root.join(CYCLE_CPUS)).map(|t| parse_cycle_cpus(&t).contains(&cpu)).unwrap_or(false);
    let cycle_limits = if cycle_cpu { parse_limits(&fs::read_to_string(root.join(CYCLE_LIMITS)).unwrap_or_default())? } else { Vec::new() };
    let gated = limits.iter().any(|l| l.cpu == cpu) || cycle_limits.iter().any(|l| l.cpu == cpu);
    if cycle_cpu {
        println!("bench: this CPU is in {CYCLE_CPUS}: ratio_host rows are DECIDED ON CYCLES (user+kernel; limits {CYCLE_LIMITS}); ns ratios and GHz are printed as advisory");
    }
    let instr_limits = parse_instr_limits(&fs::read_to_string(root.join(INSTR_LIMITS)).map_err(|e| format!("{INSTR_LIMITS}: {e}"))?)?;
    println!("machine: {}, {cpu}, kernel {}, {} (host C library)", first_line("hostname", &[]), first_line("uname", &["-r"]), first_line("ldd", &["--version"]));
    println!("tunables: GLIBC_TUNABLES={} (both sides)", TUNABLES.get().map_or("(unset)", String::as_str));
    if gated {
        println!("bench: ratio_host rows are gated by the limits for this CPU in bench/host_limits.tsv");
    } else {
        println!("bench: NO ratio_host limits for the CPU `{cpu}` in bench/host_limits.tsv: its ratio_host rows are measured and reported, NOT gated");
    }
    let mut benches: Vec<&str> = rows.iter().filter_map(|r| r.first().copied()).collect();
    benches.dedup();
    if only.is_some() || !std::env::var("BENCH_REPEAT_LISTED").is_ok_and(|v| !v.is_empty() && v != "0") {
        // budgets.tsv lists some benchmarks in separate runs of rows (cbench_locale three times). Each
        // benchmark runs once, all its rows measured together (BENCH-TP: the repeats were 48% of a full
        // bench's lock time and only added rounds to some rows). BENCH_REPEAT_LISTED=1 restores the repeats.
        let mut seen = std::collections::BTreeSet::new();
        benches.retain(|b| seen.insert(*b));
    }
    let mut results: BTreeMap<(String, String), f64> = BTreeMap::new();
    NO_BUILD.store(mode == Mode::NoBuild, std::sync::atomic::Ordering::Relaxed);
    if benches.iter().any(|b| b.starts_with("cbench_")) {
        if mode == Mode::NoBuild {
            let newest = [newest_below(&root.join("crates"), &["rs", "S"]), newest_below(&root.join("csys"), &["rs", "c", "S", "h"])].into_iter().max().flatten();
            require_fresh(&root.join("target/liella/libc.a"), newest, "libc.a vs the crates/ and csys/ sources")?;
            // Performance numbers come from the shipped configuration only (fat LTO, one codegen unit).
            if crate::so::built_profile(root) != Some(crate::so::Profile::Release) {
                return Err("target/liella holds products built with the fast test profile (or of unknown profile), which are never benchmarked: run `cargo xtask bench --build-only` (or `cargo xtask so` with LIELLA_FULL_PROFILE=1) first".into());
            }
        } else {
            crate::so::run_so_as(root, crate::so::Profile::Release)?;
        }
    }
    let host_benches: Vec<&str> = {
        let mut v: Vec<&str> = rows.iter().filter(|r| r.get(2) == Some(&"ratio_host")).filter_map(|r| r.first().copied()).collect();
        v.dedup();
        v
    };
    // Per row, the values of every round: the Liella build and the host C library (same run).
    let mut liella_rounds: BTreeMap<(String, String), Vec<f64>> = BTreeMap::new();
    let mut host_rounds: BTreeMap<(String, String), Vec<f64>> = BTreeMap::new();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    if mode == Mode::BuildOnly {
        for b in &benches {
            if host_benches.contains(b) || counted_benches(&benches, &host_capable).contains(b) {
                host_build_cached(root, b)?;
            }
            if b.starts_with("cbench_") {
                c_build(root, b, Build::Liella)?;
            } else {
                let st = Command::new(&cargo).current_dir(root).args(["build", "-q", "--release", "-p", "liella-boundary", "--example", b]).status().map_err(|e| format!("{b}: {e}"))?;
                if !st.success() {
                    return Err(format!("{b} does not build"));
                }
            }
        }
        println!("bench: build-only: {} benchmarks built; measure with `cargo xtask bench --no-build` (timing lock)", benches.len());
        return Ok(());
    }
    if mode == Mode::NoBuild {
        let rs_new = newest_below(&root.join("crates"), &["rs"]);
        for b in &benches {
            if b.starts_with("cbench_") {
                require_fresh(&root.join(format!("target/liella/bench/{b}")), mtime(&root.join(format!("bench/c/{b}.c"))).max(mtime(&root.join("target/liella/libc.a"))), "Liella build of the bench")?;
            } else {
                require_fresh(&root.join(format!("target/release/examples/{b}")), rs_new, "Rust example vs the crates/ sources")?;
            }
        }
    }
    if let Some(dir_a) = &ab_from {
        // AB-BENCH: A1 B A2 B per round, one slot; --runs N rounds (default 5); no host C library, no budgets.
        if mode != Mode::NoBuild {
            return Err("bench: --ab-from measures prebuilt trees: use it with --no-build (both trees built with `--build-only`)".into());
        }
        let settle = settle_secs(std::env::var("BENCH_SETTLE_SECS").ok().as_deref());
        return crate::abbench::run_ab(root, dir_a, &benches, runs, settle);
    }
    if counters_only {
        // TOOLS-5: no timing rounds; the counted runs only (the baseline job runs this right after its timing run).
        let names = counted_benches(&benches, &host_capable);
        let c = counted_table(root, &names, mode == Mode::NoBuild, &BTreeMap::new())?;
        return check_counted(c.rows, &filter);
    }
    let settle = settle_secs(std::env::var("BENCH_SETTLE_SECS").ok().as_deref());
    if settle > 0 {
        println!("bench: settling {settle} s idle before the first round (BENCH_SETTLE_SECS)");
        std::thread::sleep(std::time::Duration::from_secs(settle));
    }
    let adaptive = !fixed && runs > ROUNDS_FIRST;
    let settle_parity = std::env::var("BENCH_SETTLE_PARITY").map_or(true, |v| v != "0");
    let mut rounds_run: BTreeMap<String, usize> = BTreeMap::new();
    let mut bench_secs: BTreeMap<String, f64> = BTreeMap::new();
    let started = std::time::Instant::now();
    for (i, b) in benches.iter().enumerate() {
        // TOOLS-9 (BENCH_YIELD): between two bench programs a long run lets a waiting priority job go first
        // (`slots --yield`, run as $BENCH_YIELD_CMD). Exit 10 = it yielded (the machine was used meanwhile): settle again.
        if i > 0 && yield_point(std::env::var("BENCH_YIELD_CMD").ok().as_deref()) && settle > 0 {
            println!("bench: yielded to a priority job; settling {settle} s idle again");
            std::thread::sleep(std::time::Duration::from_secs(settle));
        }
        let bench_started = std::time::Instant::now();
        let ratio = host_benches.contains(b);
        let host_exe = if ratio { Some(host_build_cached(root, b)?) } else { None };
        let liella_exe = if b.starts_with("cbench_") { Some(if mode == Mode::NoBuild { root.join(format!("target/liella/bench/{b}")) } else { c_build(root, b, Build::Liella)? }) } else { None };
        let mut upto = if !ratio { 0 } else if adaptive { ROUNDS_FIRST } else { runs };
        let mut round = 0usize;
        loop {
            if round > upto {
                if ratio && adaptive && upto < runs && !bench_settled(b, &rows, &limits, &cpu, settle_parity, &liella_rounds, &host_rounds) {
                    upto = runs;
                } else {
                    break;
                }
            }
            if let Some(h) = &host_exe {
                let out = c_command(b, h).output().map_err(|e| format!("{b} (host): {e}"))?;
                if !out.status.success() {
                    return Err(format!("{b} (host) failed:\n{}", String::from_utf8_lossy(&out.stderr)));
                }
                if round > 0 {
                    for (name, v) in parse_rows(b, &out.stdout)? {
                        host_rounds.entry((b.to_string(), name)).or_default().push(v);
                    }
                }
            }
            let out = match &liella_exe {
                Some(e) => c_command(b, e).output().map_err(|e| format!("{b}: {e}"))?,
                None if mode == Mode::NoBuild => Command::new(root.join(format!("target/release/examples/{b}"))).current_dir(root).output().map_err(|e| format!("{b}: {e}"))?,
                None => Command::new(&cargo)
                    .current_dir(root)
                    .args(["run", "-q", "--release", "-p", "liella-boundary", "--example", b])
                    .output()
                    .map_err(|e| format!("{b}: {e}"))?,
            };
            if !out.status.success() {
                return Err(format!("{b} failed:\n{}", String::from_utf8_lossy(&out.stderr)));
            }
            if round > 0 || !ratio {
                for (name, v) in parse_rows(b, &out.stdout)? {
                    liella_rounds.entry((b.to_string(), name)).or_default().push(v);
                }
            }
            round += 1;
        }
        rounds_run.insert(b.to_string(), upto);
        bench_secs.insert(b.to_string(), bench_started.elapsed().as_secs_f64());
    }
    println!(
        "bench: {} benchmarks, {} rounds for the ratio_host ones, took {:.0} s",
        benches.len(),
        if adaptive { format!("{ROUNDS_FIRST} to {runs} (adaptive)") } else { runs.to_string() },
        started.elapsed().as_secs_f64()
    );
    if adaptive {
        let settled = rows.iter().filter(|r| r.get(2) == Some(&"ratio_host") && rounds_run.get(r[0]) == Some(&ROUNDS_FIRST)).count();
        let full = rows.iter().filter(|r| r.get(2) == Some(&"ratio_host") && rounds_run.get(r[0]) == Some(&runs)).count();
        println!("rounds: {settled} rows settled at {ROUNDS_FIRST}, {full} ran {runs}");
    } else {
        println!("rounds: fixed {runs} (BENCH_ROUNDS_FIXED or --runs)");
    }
    // The value a max_ns / ratio_to row uses is the median over the rounds.
    for ((b, name), v) in &liella_rounds {
        let (median, ..) = stats(v);
        println!("{b:<14} {name:<26} {median:>10.2} ns/op");
        results.insert((b.clone(), name.clone()), median);
    }
    let mut failures = Vec::new();
    let mut known = Vec::new();
    let mut target_misses = 0usize;
    let mut ungated = 0usize;
    let mut ratio_rows = 0usize;
    let mut pending: Vec<(String, String, f64, f64)> = Vec::new();
    let mut out_tsv = String::from("cpu\tbench\trow\thost_median\tliella_median\tmedian_ratio\tworst_ratio\tlimit\tstatus\n");
    println!("\nratio_host rows (median Liella/glibc ratio over the rounds run; worst = slowest Liella / fastest glibc, diagnostic only; target {TARGET}):");
    for r in &rows {
        let [bench, name, rule, bound, ..] = r.as_slice() else {
            return Err(format!("budgets.tsv: short row {r:?}"));
        };
        let key = (bench.to_string(), name.to_string());
        let got = *results.get(&key).ok_or(format!("{bench}/{name}: not measured"))?;
        let (limit, what) = match *rule {
            "max_ns" => (bound.parse::<f64>().map_err(|e| e.to_string())?, format!("{bound} ns")),
            "ratio_to" => {
                let (factor, other) = bound.split_once('x').ok_or(format!("{bench}/{name}: bad ratio `{bound}`"))?;
                let base = *results.get(&(bench.to_string(), other.to_string())).ok_or(format!("{bench}/{other}: not measured"))?;
                (factor.parse::<f64>().map_err(|e| e.to_string())? * base, format!("{factor} x {other} = {:.2} ns", factor.parse::<f64>().unwrap_or(0.0) * base))
            }
            "ratio_host" => {
                ratio_rows += 1;
                let target = bound.parse::<f64>().map_err(|e| format!("{bench}/{name}: {e}"))?;
                let (lv, hv) = (liella_rounds.get(&key).map(Vec::as_slice).unwrap_or(&[]), host_rounds.get(&key).map(Vec::as_slice).unwrap_or(&[]));
                let (med, worst) = ratio_stats(lv, hv).ok_or(format!("{bench}/{name}: not measured with the host C library"))?;
                let lim = limit_for(&limits, &cpu, bench, name);
                let verdict = match lim {
                    // FALLBACK-CYCLES: decided after the counted run, on cycles; the ns verdict is advisory here.
                    _ if cycle_cpu => {
                        pending.push((bench.to_string(), name.to_string(), med, worst));
                        "advisory (decided on cycles below)".to_string()
                    }
                    None => {
                        ungated += 1;
                        "not gated".to_string()
                    }
                    Some(l) if med > l.limit => match l.status {
                        Status::Gate => {
                            failures.push(format!("{bench}/{name}: median ratio {med:.3} exceeds the limit {:.3} (worst {worst:.2}; {})", l.limit, l.why));
                            "FAIL".to_string()
                        }
                        Status::KnownRegression => {
                            known.push(format!("{bench}/{name}: median ratio {med:.3} exceeds the limit {:.3} (worst {worst:.2}): known regression, PERF-REGRESS ({})", l.limit, l.why));
                            "FAIL (known regression, PERF-REGRESS)".to_string()
                        }
                    },
                    Some(_) => "ok".to_string(),
                };
                let miss = med > target;
                target_misses += usize::from(miss);
                println!(
                    "  {bench}/{name}: median {med:.3} worst {worst:.2} limit {} {verdict}{}",
                    lim.map_or("-".to_string(), |l| format!("{:.3}", l.limit)),
                    if miss { format!(" (target {target} missed)") } else { String::new() }
                );
                out_tsv.push_str(&format!(
                    "{cpu}\t{bench}\t{name}\t{:.2}\t{:.2}\t{med:.3}\t{worst:.3}\t{}\t{verdict}\n",
                    stats(hv).0,
                    stats(lv).0,
                    lim.map_or("-".to_string(), |l| format!("{:.3}", l.limit))
                ));
                continue;
            }
            other => return Err(format!("{bench}/{name}: unknown rule `{other}`")),
        };
        if got > limit {
            failures.push(format!("{bench}/{name}: {got:.2} ns exceeds {what}"));
        }
    }
    let dir = root.join("target/liella/bench");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("ratio_host.tsv");
    fs::write(&path, out_tsv).map_err(|e| format!("{}: {e}", path.display()))?;
    // Every round of every ratio_host row, so the adaptive rule can be replayed on real data.
    let mut rounds_tsv = String::from("bench\trow\tround\tliella_ns\thost_ns\n");
    for (key, lv) in &liella_rounds {
        if let Some(hv) = host_rounds.get(key) {
            for (i, (l, h)) in lv.iter().zip(hv).enumerate() {
                rounds_tsv.push_str(&format!("{}\t{}\t{}\t{l}\t{h}\n", key.0, key.1, i + 1));
            }
        }
    }
    let times: String = benches.iter().map(|b| format!("{b}	{}	{:.1}
", rounds_run.get(*b).copied().unwrap_or(0), bench_secs.get(*b).copied().unwrap_or(0.0))).collect();
    let tpath = dir.join("bench_times.tsv");
    fs::write(&tpath, format!("bench	rounds	seconds
{times}")).map_err(|e| format!("{}: {e}", tpath.display()))?;
    let path = dir.join("rounds.tsv");
    fs::write(&path, rounds_tsv).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "\nbench: {ratio_rows} ratio_host rows, {target_misses} above the target {TARGET} (informational), {ungated} not gated{}",
        if gated { "" } else { " (no limits for this CPU)" }
    );
    // ICOUNT-LIMITS: rows with an instruction limit for this CPU are gated on the counted run, so that run happens
    // whether or not --counters was given, and a counted run that fails is then a bench failure.
    let selected: std::collections::BTreeSet<(String, String)> = rows.iter().filter(|r| r.len() > 1).map(|r| (r[0].to_string(), r[1].to_string())).collect();
    let instr_gated = instr_limits.iter().any(|l| l.applies(&cpu) && selected.contains(&(l.bench.clone(), l.row.clone())));
    let mut cells: BTreeMap<(String, String), CycleCell> = BTreeMap::new();
    if counters || instr_gated || !pending.is_empty() {
        let names = counted_benches(&benches, &host_capable);
        let ns: BTreeMap<(String, String), f64> = liella_rounds
            .iter()
            .filter_map(|(k, lv)| host_rounds.get(k).and_then(|hv| ratio_stats(lv, hv)).map(|(m, _)| (k.clone(), m)))
            .collect();
        match counted_table(root, &names, mode == Mode::NoBuild, &ns) {
            Ok(c) => {
                if counters {
                    if let Err(e) = check_counted(c.rows, &filter) {
                        failures.push(e);
                    }
                }
                failures.extend(check_instr_limits(&instr_limits, &cpu, &c.instr, &selected, &names));
                cells = c.cyc;
            }
            Err(e) if instr_gated => failures.push(format!("instruction limits ({INSTR_LIMITS}) could not be checked: counters table failed: {e}")),
            Err(e) if !pending.is_empty() => println!("bench: counters table failed: {e}; the cycles CPU's rows fall back to their ns limits"),
            Err(e) => println!("bench: counters table failed (not a bench failure): {e}"),
        }
    }
    if !pending.is_empty() {
        println!("\nratio_host decisions on CYCLES ({CYCLE_CPUS}; cycles = user+kernel; ns ratio and GHz advisory, TIME STAYS WATCHED):");
        println!("  {:<44} {:>7} {:>7} {:>7}  {:>5} {:>5}  {:<26} verdict", "row", "cycles", "limit", "ns", "GHz L", "GHz G", "decided on");
        let mut cyc_ungated = 0usize;
        let mut dec = String::from("bench\trow\tmetric\tns_ratio\tcycles_ratio\tghz_l\tghz_g\tlimit\tverdict\n");
        let f = |x: Option<f64>, p: usize| x.map_or("-".to_string(), |v| format!("{v:.p$}"));
        for (bench, name, med, worst) in &pending {
            let key = (bench.clone(), name.clone());
            let cell = cells.get(&key);
            let (metric, lim, verdict, fail, kn) = decide_cycles((bench, name), (*med, *worst), limit_for(&limits, &cpu, bench, name), cell, limit_for(&cycle_limits, &cpu, bench, name));
            if verdict == "not gated" {
                cyc_ungated += 1;
            }
            failures.extend(fail);
            known.extend(kn);
            let c = cell.copied().unwrap_or_default();
            println!("  {:<44} {:>7} {:>7} {med:>7.3}  {:>5} {:>5}  {metric:<26} {verdict}", format!("{bench}/{name}"), f(c.ratio, 3), f(lim, 3), f(c.ghz_l, 2), f(c.ghz_g, 2));
            dec.push_str(&format!("{bench}\t{name}\t{metric}\t{med:.3}\t{}\t{}\t{}\t{}\t{verdict}\n", f(c.ratio, 3), f(c.ghz_l, 3), f(c.ghz_g, 3), f(lim, 3)));
        }
        println!("bench: {} rows decided on the cycles CPU's limits, {cyc_ungated} not gated (no cycle or ns limit)", pending.len() - cyc_ungated);
        let path = root.join("target/liella/bench/decision.tsv");
        fs::write(&path, dec).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    if failures.is_empty() && known.is_empty() {
        println!("bench: all {} budgets met{}", rows.len(), if only.is_some() { " (PARTIAL BENCH: the selected benchmarks only; not a merge bench)" } else { "" });
        return Ok(());
    }
    let mut msg = String::from("bench: budget regressions:");
    for f in &failures {
        msg.push_str(&format!("\n  {f}"));
    }
    for k in &known {
        msg.push_str(&format!("\n  {k}"));
    }
    Err(msg)
}

#[cfg(test)]
mod tests {
    // BENCH-TUNABLES: no spec clause (developer tooling). `--tunables` reaches the benchmark's environment and the
    // caller's own GLIBC_TUNABLES does not.
    #[test]
    fn tunables_option_reaches_the_command_environment() {
        let env_of = |c: &std::process::Command| c.get_envs().find(|(k, _)| *k == "GLIBC_TUNABLES").and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()));
        let c = super::stress_command("cbench_opt", std::path::Path::new("/bin/true"), &[], Some("glibc.cpu.hwcaps=-AVX2"));
        assert_eq!(env_of(&c).as_deref(), Some("glibc.cpu.hwcaps=-AVX2"));
    }

    use super::*;

    // TEST-BENCH-TUNABLES-LEAK: no spec clause (developer tooling). Tests build commands with an explicit
    // `tunables` argument (None here) and never read or set the process-global `TUNABLES`, so no test order
    // or parallel run changes what another test sees.
    fn hermetic_stress(name: &str, exe: &Path, stress: &[String]) -> Command {
        stress_command(name, exe, stress, None)
    }

    fn hermetic_stress_in(dir: &Path, name: &str, exe: &Path, stress: &[String]) -> Command {
        stress_command_in(dir, name, exe, stress, None)
    }

    const CPUINFO: &str = "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel name\t: Intel(R) Xeon(R) CPU E5-2697 v3 @ 2.60GHz\nmodel name\t: other\n";

    #[test]
    fn staleness_ignores_integration_tests() {
        let root = std::env::temp_dir().join(format!("xtask-newest-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (src, tests) = (root.join("c/src"), root.join("c/tests"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&tests).unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        let lib = src.join("lib.rs");
        fs::write(&lib, "").unwrap();
        fs::File::options().write(true).open(&lib).unwrap().set_modified(old).unwrap();
        fs::write(tests.join("t.rs"), "").unwrap();
        assert_eq!(newest_below(&root, &["rs"]), Some(old), "a newer file under tests/ must not count");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn counted_benches_follow_the_whole_table_not_the_selected_rows() {
        let table = "cbench_fstree\tstat_x\tratio_host\t1.5\ncbench_fstree\tmkstemp_unlink_close\tmax_ns\t150000\ncbench_aio\taio_x\tmax_ns\t9\nstrlen\ts\tratio_host\t1\n";
        let all: Vec<Vec<&str>> = table.lines().map(|l| l.split('\t').collect()).collect();
        let cap = host_capable_benches(&all);
        // `--rows cbench_fstree/mkstemp_unlink_close` keeps only the max_ns row: the bench is still counted.
        let sel: std::collections::BTreeSet<(String, String)> = [("cbench_fstree".to_string(), "mkstemp_unlink_close".to_string())].into();
        let kept: Vec<&Vec<&str>> = all.iter().filter(|x| sel.contains(&(x[0].to_string(), x[1].to_string()))).collect();
        let benches: Vec<&str> = kept.iter().map(|r| r[0]).collect();
        assert_eq!(counted_benches(&benches, &cap), ["cbench_fstree"]);
        // A max_ns-only C bench and a Rust bench have no host build: not counted (check_counted names the filter).
        assert!(counted_benches(&["cbench_aio", "strlen", "cbench_fstree", "cbench_fstree"], &cap) == ["cbench_fstree"]);
        let f = filter_desc(None, Some(&sel), None);
        assert_eq!(f, "--rows cbench_fstree/mkstemp_unlink_close");
        let e = check_counted(0, &f).unwrap_err();
        assert!(e.contains("empty") && e.contains("--rows cbench_fstree/mkstemp_unlink_close"), "{e}");
        assert_eq!(check_counted(1, &f), Ok(()));
        assert_eq!(filter_desc(Some(&["cbench_fstree".to_string()]), None, None), "--only cbench_fstree");
        assert_eq!(filter_desc(None, None, None), "the full bench");
    }

    #[test]
    fn counters_lines_parse_and_others_are_ignored() {
        let m = parse_counters(b"ctr	strlen_16	12.5	4.000	0.0010	-1.0000	12.000
not a line
ctr	short	1
");
        assert_eq!(m.len(), 1);
        assert_eq!(m["strlen_16"], [12.5, 4.0, 0.001, -1.0, 12.0, -1.0]);
        let m = parse_counters(b"ctr	r	1	2	3	4	5	6.5
");
        assert_eq!(m["r"], [1.0, 2.0, 3.0, 4.0, 5.0, 6.5], "the uops field is the eighth");
    }

    #[test]
    fn instr_cells_show_both_sides_and_the_ratio() {
        let l = [30.0, 1.0, 0.0, 0.0, 0.0, -1.0];
        let g = [24.0, 1.0, 0.0, 0.0, 0.0, -1.0];
        assert_eq!(instr_cells(Some(&l), Some(&g)), "30.0 | 24.0 | 1.25");
        assert_eq!(instr_cells(Some(&l), None), "30.0 | - | -");
        assert_eq!(instr_cells(None, Some(&[-1.0, 0.0, 0.0, 0.0, 0.0, -1.0])), "- | - | -");
    }

    #[test]
    fn cpu_model_is_the_first_model_name() {
        assert_eq!(cpu_model_of(CPUINFO).as_deref(), Some("Intel(R) Xeon(R) CPU E5-2697 v3 @ 2.60GHz"));
        assert_eq!(cpu_model_of("processor\t: 0\n"), None);
        assert_eq!(cpu_model_of("model name\t: \n"), None);
    }

    const LIMITS: &str = "# comment\n\nCPU A\tb1\tr1\t1.5\tgate\twhy a\nCPU B\tb1\tr1\t2.5\tknown_regression\twhy b\nCPU A\tb1\tr2\t3\tgate\t\n";

    #[test]
    fn limits_are_keyed_by_cpu_model() {
        let l = parse_limits(LIMITS).unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(limit_for(&l, "CPU A", "b1", "r1").map(|x| x.limit), Some(1.5));
        assert_eq!(limit_for(&l, "CPU B", "b1", "r1").map(|x| (x.limit, x.status)), Some((2.5, Status::KnownRegression)));
        // A different CPU, a different row or a different benchmark has no limit.
        assert!(limit_for(&l, "CPU C", "b1", "r1").is_none());
        assert!(limit_for(&l, "CPU B", "b1", "r2").is_none());
        assert!(limit_for(&l, "CPU A", "b2", "r1").is_none());
        // The host name plays no part: only the exact CPU string matches.
        assert!(limit_for(&l, "cpu a", "b1", "r1").is_none());
    }

    #[test]
    fn instruction_limits_fail_over_limit_and_missing_rows_only() {
        let lim = parse_instr_limits("# c\n\nCPU A\tb1\tr1\t20\twhy\n*\tb1\tr2\t30\n*\tb2\tr3\t5\n").unwrap();
        let sel: std::collections::BTreeSet<(String, String)> = [("b1", "r1"), ("b1", "r2"), ("b2", "r3")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let mut m = BTreeMap::new();
        m.insert(("b1".to_string(), "r1".to_string()), 20.0);
        m.insert(("b1".to_string(), "r2".to_string()), 31.0);
        let f = check_instr_limits(&lim, "CPU A", &m, &sel, &["b1"]);
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].starts_with("b1/r2") && f[0].contains("INSTR-REGRESS"));
        // another CPU: the CPU-specific line does not apply; b2 was not counted; a row with no value fails
        assert!(check_instr_limits(&lim, "CPU B", &BTreeMap::new(), &sel, &["b1"]).iter().all(|x| x.starts_with("b1/r2") && x.contains("no counted")));
        assert_eq!(check_instr_limits(&lim, "CPU A", &BTreeMap::new(), &sel, &["b1", "b2"]).len(), 3);
        assert!(parse_instr_limits("*\tb\tr\tx\n").is_err() && parse_instr_limits("*\tb\tr\n").is_err());
    }

    #[test]
    fn bad_limit_lines_are_errors() {
        assert!(parse_limits("CPU\tb\tr\t1.0\tgate\n").is_err());
        assert!(parse_limits("CPU\tb\tr\tx\tgate\twhy\n").is_err());
        assert!(parse_limits("CPU\tb\tr\t1.0\tmaybe\twhy\n").is_err());
    }

    #[test]
    fn c_benchmarks_see_only_the_fixed_environment() {
        // BENCH-FIXED-ENV: whatever the calling job's environment holds (this test runs under cargo, so it holds
        // CARGO_* and build-tree paths), the benchmark process gets exactly the fixed list.
        let out = hermetic_stress("cbench_env_probe", Path::new("/usr/bin/env"), &[]).output().expect("run env");
        let mut got: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect();
        got.sort();
        assert_eq!(got, ["HOME=/home/cbench", "PATH=/usr/local/bin:/usr/bin:/bin"]);
    }

    #[test]
    fn stress_profiles_build_the_documented_environment() {
        // STRESS-ENV-AUDIT: `env` adds 2,000 variables that sort before the fixed names, `path` makes PATH 200
        // directories with the real ones last, `fds` leaves 10,000 descriptors open.
        let sel = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let out = hermetic_stress("cbench_env_probe", Path::new("/usr/bin/env"), &sel(&["env", "path"])).output().expect("run env");
        let lines: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect();
        assert_eq!(lines.len(), STRESS_VARS + 2);
        assert!(lines[0].starts_with("AAA_STRESS_0000=") && lines[0].len() == 132);
        let path = lines.iter().find_map(|l| l.strip_prefix("PATH=")).expect("PATH");
        assert_eq!(path.split(':').count(), STRESS_PATH_DIRS);
        assert!(path.ends_with(":/usr/local/bin:/usr/bin:/bin"));
        assert!(lines.iter().any(|l| l == "HOME=/home/cbench"));
        let out = hermetic_stress("cbench_fd_probe", Path::new("/bin/sh"), &sel(&["fds"])).args(["-c", "ls /proc/self/fd | wc -l"]).output().expect("run sh");
        let n: usize = String::from_utf8_lossy(&out.stdout).trim().parse().expect("fd count");
        // 0-2, the 10,000 stress descriptors, and the one `ls` reads /proc/self/fd through.
        assert!(n >= STRESS_FDS + 3, "{n} descriptors");
        // The bash that opens them passes on the fixed environment only (no PWD, SHLVL or _).
        let out = hermetic_stress("cbench_fd_probe", Path::new("/usr/bin/env"), &sel(&["fds"])).output().expect("run env");
        let mut got: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect();
        got.sort();
        assert_eq!(got, ["HOME=/home/cbench", "PATH=/usr/local/bin:/usr/bin:/bin"]);
    }

    #[test]
    fn stress_profiles_names_deep_threads() {
        // STRESS-ENV-AUDIT-2: `deep` and `threads` are one variable each for the C side to act on.
        let sel = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let out = hermetic_stress("cbench_fstree", Path::new("/usr/bin/env"), &sel(&["deep", "threads"])).output().expect("run env");
        let mut got: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect();
        got.sort();
        assert_eq!(got, ["CBENCH_STRESS_DEPTH=64", "CBENCH_STRESS_THREADS=64", "HOME=/home/cbench", "PATH=/usr/local/bin:/usr/bin:/bin"]);
        // `names`: 50,000 entries appended after the host's own, root keeps its place.
        let dir = std::env::temp_dir().join(format!("liella-stress-names-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        prepare_stress(&dir, &sel(&["names"])).expect("names files");
        let pw = fs::read_to_string(dir.join("passwd")).expect("passwd");
        let host = fs::read_to_string("/etc/passwd").expect("/etc/passwd");
        assert!(pw.starts_with(&host) && pw.lines().count() == host.lines().count() + STRESS_NAMES);
        assert!(pw.ends_with("stress49999:x:249999:249999:Stress user 49999:/home/stress49999:/bin/sh\n"));
        let gr = fs::read_to_string(dir.join("group")).expect("group");
        assert!(gr.ends_with("stressg49999:x:249999:stress49999,stress00000\n"), "{}", &gr[gr.len() - 80..]);
        // The namespace route, where unprivileged user namespaces are allowed: the benchmark runs as the
        // caller and sees the generated files and its own /dev/shm.
        if !Command::new("unshare").args(["-r", "-m", "true"]).status().is_ok_and(|s| s.success()) {
            eprintln!("stress_profiles_names_deep_threads: unprivileged user namespaces unavailable here; namespace route NOT TESTED");
        } else {
            let out = hermetic_stress_in(&dir, "cbench_netdb", Path::new("/bin/sh"), &sel(&["names"])).args(["-c", "id -u; id -g; tail -n 1 /etc/passwd; tail -n 1 /etc/group; ls -A /dev/shm | wc -l"]).output().expect("run sh");
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let lines: Vec<&str> = text.lines().collect();
            assert!(out.status.success(), "{text} {}", String::from_utf8_lossy(&out.stderr));
            assert_eq!(&lines[..5], [own_id("-u").as_str(), own_id("-g").as_str(), "stress49999:x:249999:249999:Stress user 49999:/home/stress49999:/bin/sh", "stressg49999:x:249999:stress49999,stress00000", "0"]);
            // The environment the benchmark gets (a probe shell would add its own PWD, so `env` is the program).
            let out = hermetic_stress_in(&dir, "cbench_netdb", Path::new("/usr/bin/env"), &sel(&["names"])).output().expect("run env");
            let mut lines: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect();
            lines.sort();
            assert_eq!(lines, ["CBENCH_STRESS_GROUP=stressg49999", "CBENCH_STRESS_USER=stress49999", "HOME=/home/cbench", "PATH=/usr/local/bin:/usr/bin:/bin"]);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_ratio_to_base_without_a_budget_line_is_run_with_its_row() {
        // The base `outx` is an output row with no budget line: it must still reach CBENCH_ROWS.
        let rows = [vec!["cbench_a", "r", "ratio_to", "2xout"], vec!["cbench_a", "other", "max_ns", "5"]];
        let mut per: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
        per.insert("cbench_a".into(), (3, vec![])); // three rows in the binary
        let env = row_env(&mut per, &rows[..1]);
        assert_eq!(env.get("cbench_a").map(String::as_str), Some("r,out"));
    }

    #[test]
    fn compare_takes_only_as_a_name_list_and_rejects_dash_names() {
        // no spec clause: xtask argument parsing
        assert_eq!(only_list(Some("cbench_a,cbench_b")).unwrap(), vec!["cbench_a", "cbench_b"]);
        assert!(only_list(None).is_err() && only_list(Some(",")).is_err());
        assert!(only_list(Some("--runs")).is_err());
        assert_eq!(positional_name("cbench_opt").unwrap(), "cbench_opt");
        let err = positional_name("--only").unwrap_err();
        assert!(err.contains("--only") && err.contains("positional"), "{err}");
    }

    #[test]
    fn only_keeps_the_rows_of_the_named_benchmarks() {
        let rows = vec![vec!["cbench_a", "x", "max_ns", "5"], vec!["bench_b", "y", "max_ns", "5"], vec!["cbench_a", "z", "ratio_to", "2xx"]];
        let kept = select_only(rows.clone(), &["cbench_a".to_string()]).unwrap();
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().all(|r| r[0] == "cbench_a"));
        assert_eq!(select_only(rows.clone(), &["bench_b".into(), "cbench_a".into()]).unwrap().len(), 3);
        let err = select_only(rows.clone(), &["cbench_typo".to_string()]).unwrap_err();
        assert!(err.contains("cbench_typo") && err.contains("known: cbench_a, bench_b"), "{err}");
        assert!(select_only(rows, &[]).is_err());
    }

    #[test]
    fn settle_default_and_override() {
        assert_eq!(settle_secs(None), 12);
        assert_eq!(settle_secs(Some("0")), 0);
        assert_eq!(settle_secs(Some(" 20 ")), 20);
        assert_eq!(settle_secs(Some("junk")), 12);
    }

    #[test]
    fn the_yield_hook_is_a_no_op_unless_the_command_exits_10() {
        // TOOLS-9: unset/empty = no-op; exit 10 = yielded; every other status = not a yield (never fatal).
        assert!(!yield_point(None));
        assert!(!yield_point(Some("  ")));
        assert!(!yield_point(Some("exit 0")));
        assert!(!yield_point(Some("exit 1")));
        assert!(!yield_point(Some("exit 11")));
        assert!(yield_point(Some("exit 10")));
    }

    #[test]
    fn measure_header_is_the_47_column_table_schema() {
        let h = measure_header();
        assert_eq!(h.trim_end().split('\t').count(), 47);
        let table = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../bench/parity-measure.tsv")).unwrap();
        assert_eq!(table.lines().next().unwrap(), h.trim_end());
    }

    #[test]
    fn flush_measure_keeps_the_benches_finished_so_far() {
        let dir = std::env::temp_dir().join(format!("xtask-flush-measure-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut m = String::from("bench\trow\n");
        m.push_str("a\tr1\n");
        let p = flush_measure(&dir, &m).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "bench\trow\na\tr1\n");
        m.push_str("b\tr2\n");
        flush_measure(&dir, &m).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "bench\trow\na\tr1\nb\tr2\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_build_refuses_a_missing_or_stale_program() {
        let dir = std::env::temp_dir().join(format!("xtask-bench-fresh-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("prog");
        let now = std::time::SystemTime::now();
        assert!(require_fresh(&exe, Some(now), "t").unwrap_err().contains("missing"));
        fs::write(&exe, b"x").unwrap();
        assert!(require_fresh(&exe, Some(now + std::time::Duration::from_secs(60)), "t").unwrap_err().contains("older"));
        assert!(require_fresh(&exe, Some(now - std::time::Duration::from_secs(60)), "t").is_ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn adaptive_rule_settles_only_when_no_line_can_be_crossed() {
        // Ratio about 0.5, limit 1.2: whatever rounds 4 and 5 add, below parity and the limit.
        assert!(row_settled(&[5.0, 5.2, 5.1], &[10.0, 10.1, 10.0], Some(1.2), true));
        // Clearly above the limit: also settled (the verdict cannot change).
        assert!(row_settled(&[15.0, 15.2, 15.1], &[10.0, 10.1, 10.0], Some(1.2), true));
        // The interval [lmin/hmax, lmax/hmin] straddles parity: unsettled.
        assert!(!row_settled(&[9.8, 10.2, 10.0], &[10.0, 10.0, 10.0], Some(1.5), true));
        // Below parity but the interval reaches the limit: unsettled.
        assert!(!row_settled(&[8.0, 11.0, 9.0], &[10.0, 10.0, 10.0], Some(1.05), true));
        // Without a limit only parity counts.
        assert!(row_settled(&[11.0, 12.0, 11.5], &[10.0, 10.0, 10.0], None, true));
        // Dropping the parity line settles a row that straddles 1.00 but is far from its limit.
        assert!(row_settled(&[9.8, 10.2, 10.0], &[10.0, 10.0, 10.0], Some(1.5), false));
        assert!(!row_settled(&[9.8, 10.2, 10.0], &[10.0, 10.0, 10.0], Some(1.5), true));
        // Fewer than three rounds, or a zero glibc time: never settled.
        assert!(!row_settled(&[5.0, 5.0], &[10.0, 10.0], None, true));
        assert!(!row_settled(&[5.0, 5.0, 5.0], &[0.0, 10.0, 10.0], None, true));
    }

    #[test]
    fn five_round_median_stays_inside_the_three_round_interval() {
        // The property the rule relies on, checked exhaustively on a small grid: for every pair of
        // extra rounds the 5-round median ratio is inside [lmin/hmax, lmax/hmin] of the first three.
        let (l3, h3) = ([10.0, 11.0, 13.0], [9.0, 10.0, 10.5]);
        let (lo, hi) = (10.0 / 10.5, 13.0 / 9.0);
        let grid = [1.0, 5.0, 10.0, 10.5, 11.0, 12.0, 20.0, 100.0];
        for &a in &grid {
            for &b in &grid {
                for &c in &grid {
                    for &d in &grid {
                        let (mut l, mut h) = (l3.to_vec(), h3.to_vec());
                        l.extend([a, b]);
                        h.extend([c, d]);
                        let m = ratio_stats(&l, &h).unwrap().0;
                        assert!(m >= lo - 1e-12 && m <= hi + 1e-12, "{a} {b} {c} {d}: {m}");
                    }
                }
            }
        }
    }

    #[test]
    fn gate_statistic_is_the_median_ratio() {
        // Liella 10, 11, 30 (median 11, one slow outlier); glibc 9, 10, 10 (median 10).
        let (median, worst) = ratio_stats(&[10.0, 30.0, 11.0], &[10.0, 9.0, 10.0]).unwrap();
        assert!((median - 1.1).abs() < 1e-12);
        // The worst ratio (diagnostic) is slowest Liella / fastest glibc.
        assert!((worst - 30.0 / 9.0).abs() < 1e-12);
        // An even number of rounds averages the middle pair.
        let (median, _) = ratio_stats(&[10.0, 20.0], &[10.0, 10.0]).unwrap();
        assert!((median - 1.5).abs() < 1e-12);
        assert!(ratio_stats(&[], &[1.0]).is_none());
        assert!(ratio_stats(&[1.0], &[0.0]).is_none());
    }

    #[test]
    fn cycle_cpus_are_listed_one_per_line() {
        let t = "# cpu\trationale\nIntel(R) Xeon(R) CPU E3-1265L V2 @ 2.50GHz\tclock bimodal\n\nCPU X\n";
        assert_eq!(parse_cycle_cpus(t), vec!["Intel(R) Xeon(R) CPU E3-1265L V2 @ 2.50GHz".to_string(), "CPU X".to_string()]);
    }

    #[test]
    fn fallback_rows_are_decided_on_cycles_with_ns_as_the_fallback() {
        let ns = parse_limits("C\tb\tr\t1.2\tgate\tns 1.25 x median\n").unwrap();
        let cy = parse_limits("C\tb\tr\t1.1\tgate\tcycles 1.25 x median\nC\tb\tk\t1.0\tknown_regression\tslow\n").unwrap();
        let cell = |r: Option<f64>| CycleCell { ratio: r, ghz_l: Some(1.5), ghz_g: Some(2.4) };
        let (nl, cl) = (limit_for(&ns, "C", "b", "r"), limit_for(&cy, "C", "b", "r"));
        // Canned: a clock drop on Liella's side made ns 1.38x, cycles are 1.02x: passes on cycles (time drag advisory).
        let (m, lim, v, fail, _) = decide_cycles(("b", "r"), (1.38, 1.40), nl, Some(&cell(Some(1.02))), cl);
        assert_eq!((m.as_str(), lim, v.as_str(), fail), ("cycles", Some(1.1), "ok", None));
        // Cycles over their limit fail even when ns looks fine (the other side ran slower).
        let (m, _, v, fail, _) = decide_cycles(("b", "r"), (0.85, 0.9), nl, Some(&cell(Some(1.15))), cl);
        assert_eq!((m.as_str(), v.as_str()), ("cycles", "FAIL"));
        assert!(fail.unwrap().contains("cycles ratio 1.150 exceeds the cycle limit 1.100"));
        // A missing cycles counter falls back to the ns limit and says so: 1.3 > 1.2 fails, 1.1 passes.
        let (m, lim, v, fail, _) = decide_cycles(("b", "r"), (1.3, 1.3), nl, Some(&cell(None)), cl);
        assert_eq!((m.as_str(), lim, v.as_str()), ("ns (cycles counter missing)", Some(1.2), "FAIL"));
        assert!(fail.unwrap().contains("decided on ns (cycles counter missing)"));
        let (_, _, v, _, _) = decide_cycles(("b", "r"), (1.1, 1.1), nl, None, cl);
        assert_eq!(v, "ok");
        // No cycle limit for the row: the ns limit decides; no limit at all: not gated.
        let (m, _, v, _, _) = decide_cycles(("b", "r"), (1.3, 1.3), nl, Some(&cell(Some(0.9))), None);
        assert_eq!((m.as_str(), v.as_str()), ("ns (no cycle limit)", "FAIL"));
        let (_, lim, v, fail, _) = decide_cycles(("b", "z"), (9.0, 9.0), None, Some(&cell(Some(9.0))), None);
        assert_eq!((lim, v.as_str(), fail), (None, "not gated", None));
        // A known regression on cycles is reported as known, not as an ordinary failure.
        let (_, _, v, fail, kn) = decide_cycles(("b", "k"), (1.0, 1.0), None, Some(&cell(Some(1.05))), limit_for(&cy, "C", "b", "k"));
        assert_eq!((v.as_str(), fail), ("FAIL (known regression, PERF-REGRESS)", None));
        assert!(kn.unwrap().contains("PERF-REGRESS"));
    }
}
