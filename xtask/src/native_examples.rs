//! `cargo xtask native`: the native Rust interface's examples (D-X.10), built for both paths;
//! the static one is run and its output checked.
//!
//! 1. The products (`cargo xtask so`), if `target/liella` lacks them.
//! 2. `liella-native` on its own with each feature: `cargo clippy -p liella-native --all-targets
//!    --features <f> -- -D warnings` and `cargo test -p liella-native --lib --features <f>`.
//! 3. Two link helpers in `target/liella/native`:
//!    * `liella-cc`, the linker driver rustc is given: `cc` with `liella-crt1.o` first. The start
//!      object must lead the link: its `.eh_frame` label marks the start of the executable's
//!      call-frame information (D-2.7), and rustc places link arguments after its own objects.
//!    * `nounwind.o` from `crates/native/link/nounwind.c`: weak `_Unwind_Resume` and
//!      `rust_eh_personality` that abort. Rust's precompiled `core` and `alloc` reference them;
//!      a panic=abort program never calls them; the unwinder that would define them (libgcc_eh)
//!      needs `dl_iterate_phdr` and `_dl_find_object`, which arrive with phase 14.
//! 4. The static example, linked like a product (GNU ld; rust-lld would put the end label of
//!    `.eh_frame`, a section `.eh_frame.liella_end`, outside it):
//!
//!    ```text
//!    cargo rustc -p liella-native --example native_static --features static -- \
//!        -C linker=target/liella/native/liella-cc -C linker-features=-lld \
//!        -C link-arg=-nostartfiles -C link-arg=-nostdlib -C link-arg=target/liella/native/nounwind.o \
//!        -C relocation-model=static -C target-feature=+crt-static \
//!        -C link-arg=target/liella/libc.a -C link-arg=-lgcc
//!    ```
//!
//!    It is then run with an empty environment except `LIELLA_NATIVE_GREETING=hello` and a POSIX
//!    `TZ` rule, with a fresh directory and two arguments; its standard output must be exactly
//!    [`EXPECTED`], its standard error empty and its status 0. The executable must have no
//!    program interpreter and no dynamic section.
//! 5. The dynamic example, linked against Liella's `libc.so.6`:
//!
//!    ```text
//!    cargo rustc -p liella-native --example native_dynamic --features dynamic -- \
//!        -C linker=target/liella/native/liella-cc -C linker-features=-lld \
//!        -C link-arg=-nostartfiles -C link-arg=-nostdlib -C link-arg=target/liella/native/nounwind.o \
//!        -C link-arg=target/liella/libc.so.6 -C link-arg=-lgcc
//!    ```
//!
//!    `readelf` must show `libc.so.6` as its only `DT_NEEDED` entry and every native function
//!    except `liella_heap_usable_size` (no safe use) referenced at its node (`@LIELLA_NATIVE_1`),
//!    and no other `liella_` name. It is not run: that needs Liella's dynamic loader (phase 14).
//!    Its program interpreter is the host's, which would load the host's `libc.so.6` (no
//!    `RUNPATH` points at `target/liella`) and stop at the missing `LIELLA_NATIVE_1`, never
//!    Liella's library.
//!
//! 6. The proof that `liella_secure_zero` is never optimised away (D-X.11): the boundary's
//!    example `secure_zero_codegen` is built with the release profile (fat LTO, one codegen
//!    unit), not position-independent so that calls name their targets:
//!
//!    ```text
//!    cargo rustc --release -p liella-boundary --example secure_zero_codegen -- -C relocation-model=static
//!    ```
//!
//!    and disassembled (`objdump -d -C`, and `objdump -s -j .got` to resolve calls through the
//!    GOT). Three functions zero a dead 4096-byte local buffer. In the two controls (`fill(0)`,
//!    and `bytes::wipe` without the barrier) the zeroing must be gone: no `memset` call, no
//!    `rep stos`. In `dead_secure_zero`, which calls `liella_secure_zero` inlined, a zeroing of
//!    0x1000 bytes must remain, followed by the call to `liella_machine_barrier`.
//!
//! The examples are `#![no_std]` `#![no_main]` programs: a `std` program cannot link against
//! Liella yet. Statically, `std` and libgcc_eh need `getcwd`, `realpath`, `dl_iterate_phdr`
//! and `_dl_find_object` from `libc.a`, which does not define them, and `libc.a`'s strong
//! `rust_eh_personality` (csys/src/adapter/runtime.c) collides with `std`'s; dynamically,
//! `std` needs `getcwd`, `realpath` and `dl_iterate_phdr` from `libc.so.6`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The static example's standard output, exactly.
const EXPECTED: &str = "\
native example: linkage static
ok heap: 1000 words (sum 499500); 100 lines aligned to 64 after growth
ok secure: 28-byte secret grown to 5028 bytes (capacity 64 to 8224, moved by the heap), truncated intact; zeroed buffer of 32; stack buffer of 32 zeroed
ok args: 4 (alpha beta)
ok env: LIELLA_NATIVE_GREETING=hello; set, overwrite, unset; 2 entries
ok file: 25 bytes written, read back, renamed, removed
ok clock: realtime after 2026-01-01, monotonic non-decreasing
ok tz: 1782907200 is 2026-07-01 14:00:00 CEST (+7200, dst 1, weekday 3, yearday 181); 2026-01-15 13:00:00 CET is 1768478400; with TZ=JST-9 2026-07-01 21:00:00 JST
ok clean api: 1782907200 is 2026-07-01 12:00:00 UTC
ok kernel types: ENOENT(2) is the facade's Errno
ok locale: LC_ALL=C, LC_CTYPE=C, decimal point \".\"
ok process: pid is positive
ok nscache: enabled and idle, 6 categories; switched off and on, invalidated
ok aio: worker threads chosen before the first request
ok getpass: default 1048576 bytes; set to 4096 and back
native example: 14 of 14 steps passed
";

/// The time zone the static example runs in (a POSIX rule: no zone database needed).
const TZ: &str = "CET-1CEST,M3.5.0,M10.5.0/3";

/// The native function no example references: it takes any address, so the facade has no safe
/// use for it.
pub(crate) const UNUSED: &[&str] = &["liella_heap_usable_size"];

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("cannot run {cmd:?}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{cmd:?} failed: {status}")) }
}

fn output(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("cannot run {cmd:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{cmd:?} failed: {}", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `s` quoted for `sh`.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Build `example` with `feature` and the link arguments `link`: the executable.
fn build(root: &Path, example: &str, feature: &str, link: &[String]) -> Result<PathBuf, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    run(Command::new(&cargo).current_dir(root).args(["rustc", "-q", "-p", "liella-native", "--example", example, "--features", feature, "--"]).args(link))?;
    Ok(root.join("target/debug/examples").join(example))
}

/// The static example: run it and compare its output; check it has no interpreter.
fn check_static(root: &Path, exe: &Path, dir: &Path) -> Result<(), String> {
    let work = dir.join("run");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let out = Command::new(exe)
        .current_dir(root)
        .arg(&work)
        .args(["alpha", "beta"])
        .env_clear()
        .env("LIELLA_NATIVE_GREETING", "hello")
        .env("TZ", TZ)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", exe.display()))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let mut errors = Vec::new();
    if out.status.code() != Some(0) {
        errors.push(format!("native_static: status {}, expected 0", out.status));
    }
    if stdout != EXPECTED {
        errors.push(format!("native_static: unexpected output\n--- got\n{stdout}--- expected\n{EXPECTED}---"));
    }
    if !stderr.is_empty() {
        errors.push(format!("native_static: standard error is not empty:\n{stderr}"));
    }
    let left: Vec<_> = fs::read_dir(&work).map_err(|e| e.to_string())?.flatten().map(|e| e.file_name()).collect();
    if !left.is_empty() {
        errors.push(format!("native_static: left {left:?} in {}", work.display()));
    }
    let _ = fs::remove_dir_all(&work);
    let headers = output(Command::new("readelf").args(["-lW"]).arg(exe))?;
    if headers.contains("INTERP") || headers.contains("DYNAMIC") {
        errors.push("native_static: has a program interpreter or a dynamic section; a static link has neither".to_string());
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
}

/// The dynamic example: its needed libraries and native references. The number of native
/// symbols and the program interpreter.
fn check_dynamic(root: &Path, exe: &Path) -> Result<(usize, String), String> {
    let mut errors = Vec::new();
    let dynamic = output(Command::new("readelf").args(["-dW"]).arg(exe))?;
    let needed: Vec<&str> = dynamic.lines().filter(|l| l.contains("(NEEDED)")).filter_map(|l| l.split('[').nth(1)?.split(']').next()).collect();
    if needed != ["libc.so.6"] {
        errors.push(format!("native_dynamic: needs {needed:?}, expected only libc.so.6"));
    }
    let syms = output(Command::new("readelf").args(["--dyn-syms", "-W"]).arg(exe))?;
    let mut refs: BTreeMap<String, String> = BTreeMap::new();
    for line in syms.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if let (Some(&"UND"), Some(sym)) = (f.get(6), f.get(7)) {
            if let Some((name, ver)) = sym.split_once('@') {
                if name.starts_with("liella_") {
                    refs.insert(name.to_string(), ver.trim_start_matches('@').to_string());
                }
            } else if sym.starts_with("liella_") {
                refs.insert((*sym).to_string(), String::new());
            }
        }
    }
    let want: BTreeMap<String, String> = crate::native::symbols(root)?.into_iter().filter(|(s, _)| !UNUSED.contains(&s.as_str())).collect();
    for (sym, node) in &want {
        match refs.get(sym) {
            Some(v) if v == node => {}
            Some(v) => errors.push(format!("native_dynamic: references {sym}@{v}, expected {sym}@{node}")),
            None => errors.push(format!("native_dynamic: does not reference {sym}")),
        }
    }
    for sym in refs.keys().filter(|s| !want.contains_key(*s)) {
        errors.push(format!("native_dynamic: references {sym}, which is not a native function it should use"));
    }
    let headers = output(Command::new("readelf").args(["-lW"]).arg(exe))?;
    let interp = headers.lines().find_map(|l| l.split("Requesting program interpreter: ").nth(1)).map(|s| s.trim_end_matches(']').to_string()).unwrap_or_default();
    if interp.is_empty() {
        errors.push("native_dynamic: no program interpreter".to_string());
    }
    if errors.is_empty() { Ok((refs.len(), interp)) } else { Err(errors.join("\n")) }
}

/// The functions of an `objdump -d -C` listing: demangled name -> (address, instruction lines).
fn functions(asm: &str) -> BTreeMap<String, (u64, Vec<String>)> {
    let mut out = BTreeMap::new();
    let mut current: Option<(String, u64, Vec<String>)> = None;
    for line in asm.lines() {
        // A header: `000000000020fdc0 <name>:`.
        let header = line.strip_suffix(">:").and_then(|l| l.split_once(" <")).and_then(|(a, n)| Some((u64::from_str_radix(a.trim(), 16).ok()?, n)));
        if let Some((addr, name)) = header {
            if let Some((n, a, body)) = current.take() {
                out.insert(n, (a, body));
            }
            current = Some((name.to_string(), addr, Vec::new()));
        } else if let Some((_, _, body)) = current.as_mut() {
            if line.contains(":\t") {
                body.push(line.to_string());
            }
        }
    }
    if let Some((n, a, body)) = current {
        out.insert(n, (a, body));
    }
    out
}

/// The 8-byte words of an `objdump -s -j .got` listing, by address.
fn got_words(dump: &str) -> BTreeMap<u64, u64> {
    let mut bytes = BTreeMap::new();
    for line in dump.lines() {
        let mut fields = line.split_whitespace();
        let Some(addr) = fields.next().and_then(|a| u64::from_str_radix(a, 16).ok()) else { continue };
        let mut at = addr;
        // Up to four groups of eight hex digits; the ASCII column after them is not hex.
        for group in fields.take(4).take_while(|g| g.len() == 8 && g.bytes().all(|b| b.is_ascii_hexdigit())) {
            for i in (0..8).step_by(2) {
                if let Ok(b) = u8::from_str_radix(&group[i..i + 2], 16) {
                    bytes.insert(at, b);
                }
                at += 1;
            }
        }
    }
    let mut words = BTreeMap::new();
    for &a in bytes.keys() {
        let word: Option<Vec<u8>> = (0..8).map(|i| bytes.get(&(a + i)).copied()).collect();
        if let Some(w) = word.and_then(|w| <[u8; 8]>::try_from(w).ok()) {
            words.insert(a, u64::from_le_bytes(w));
        }
    }
    words
}

/// Step 6: the proof that `liella_secure_zero`'s zeroing survives where a plain one is
/// removed (D-X.11; the module documentation). A summary of what was seen.
fn check_secure_zero(root: &Path) -> Result<String, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    run(Command::new(&cargo).current_dir(root).args(["rustc", "-q", "--release", "-p", "liella-boundary", "--example", "secure_zero_codegen", "--", "-C", "relocation-model=static"]))?;
    let exe = root.join("target/release/examples/secure_zero_codegen");
    let funcs = functions(&output(Command::new("objdump").args(["-d", "--no-show-raw-insn", "-C"]).arg(&exe))?);
    let got = got_words(&output(Command::new("objdump").args(["-s", "-j", ".got"]).arg(&exe))?);
    // The barrier's address; absent if nothing calls it (then nothing is linked in).
    let barrier = funcs.get("liella_machine_barrier").map(|f| f.0);
    let body = |name: &str| -> Result<&Vec<String>, String> {
        funcs.iter().find(|(n, _)| n.ends_with(&format!("::{name}"))).map(|(_, (_, b))| b).ok_or(format!("secure_zero_codegen: no function {name} (inlined or folded?)"))
    };
    let is_call = |l: &str| l.contains("\tcall ") || l.contains("\tjmp ");
    let zeroes = |l: &str| l.contains("rep stos") || (is_call(l) && l.contains("<memset"));
    // A call to the barrier: direct, or through a GOT slot holding its address.
    let to_barrier = |l: &str| {
        is_call(l)
            && (l.contains("<liella_machine_barrier>")
                || (l.contains("(%rip)") && l.split('#').nth(1).and_then(|t| t.split_whitespace().next()).and_then(|a| u64::from_str_radix(a, 16).ok()).and_then(|slot| got.get(&slot)).is_some_and(|&a| Some(a) == barrier)))
    };
    let mut errors = Vec::new();
    for control in ["dead_fill", "dead_wipe_without_barrier"] {
        let b = body(control)?;
        if b.iter().any(|l| zeroes(l)) {
            errors.push(format!("secure_zero_codegen: {control} still zeroes its dead buffer: this build does not show that dead zeroing is removed"));
        }
        if b.iter().any(|l| to_barrier(l)) {
            errors.push(format!("secure_zero_codegen: {control} calls the barrier"));
        }
    }
    let b = body("dead_secure_zero")?;
    let zero_at = b.iter().position(|l| zeroes(l));
    let barrier_at = b.iter().position(|l| to_barrier(l));
    match (zero_at, barrier_at) {
        (Some(z), Some(c)) if z < c => {
            if !b.iter().take(z).any(|l| l.contains("$0x1000,")) {
                errors.push("secure_zero_codegen: dead_secure_zero zeroes, but not visibly 0x1000 bytes".to_string());
            }
        }
        (None, _) => errors.push("secure_zero_codegen: dead_secure_zero's zeroing was optimised away".to_string()),
        (_, None) => errors.push("secure_zero_codegen: dead_secure_zero does not call liella_machine_barrier".to_string()),
        _ => errors.push("secure_zero_codegen: dead_secure_zero calls the barrier before zeroing".to_string()),
    }
    if errors.is_empty() {
        Ok("liella_secure_zero under fat LTO: the 4096-byte zeroing of a dead buffer stays, before the barrier call, where fill(0) and an unbarriered wipe are removed".to_string())
    } else {
        Err(errors.join("\n"))
    }
}

/// Step 1: the products, built if `target/liella` lacks one. Their paths: `libc.a`,
/// `libc.so.6`, `liella-crt1.o`.
pub(crate) fn products(root: &Path) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let liella = root.join("target/liella");
    let (libc_a, libc_so, crt1) = (liella.join("libc.a"), liella.join("libc.so.6"), liella.join("liella-crt1.o"));
    if !(libc_a.is_file() && libc_so.is_file() && crt1.is_file()) {
        crate::so::run_so(root)?;
    }
    Ok((libc_a, libc_so, crt1))
}

/// Step 3: the link helpers in `target/liella/native` (also used by `cargo xtask rust-dylib`):
/// the directory, the driver `liella-cc` and `nounwind.o`.
pub(crate) fn link_helpers(root: &Path) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let crt1 = root.join("target/liella/liella-crt1.o");
    let dir = root.join("target/liella/native");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let driver = dir.join("liella-cc");
    let script = format!(
        "#!/bin/sh\n# @generated by `cargo xtask native`: the C compiler driver with Liella's start object first.\nexec {} {} \"$@\"\n",
        quote(&cc),
        quote(&crt1.to_string_lossy())
    );
    fs::write(&driver, script).map_err(|e| format!("{}: {e}", driver.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&driver, fs::Permissions::from_mode(0o755)).map_err(|e| format!("{}: {e}", driver.display()))?;
    }
    let nounwind = dir.join("nounwind.o");
    run(Command::new(&cc).args(["-c", "-O2", "-fno-asynchronous-unwind-tables"]).arg(root.join("crates/native/link/nounwind.c")).arg("-o").arg(&nounwind))?;
    Ok((dir, driver, nounwind))
}

pub fn run_native(root: &Path) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let (libc_a, libc_so, _) = products(root)?;

    // 2. The crate on its own, with each feature.
    for feature in ["static", "dynamic"] {
        run(Command::new(&cargo).current_dir(root).args(["clippy", "-q", "-p", "liella-native", "--all-targets", "--features", feature, "--", "-D", "warnings"]))?;
        run(Command::new(&cargo).current_dir(root).args(["test", "-q", "-p", "liella-native", "--lib", "--features", feature]))?;
    }

    // 3. The link helpers.
    let (dir, driver, nounwind) = link_helpers(root)?;

    let arg = |a: &str| vec!["-C".to_string(), a.to_string()];
    let link_arg = |p: &Path| arg(&format!("link-arg={}", p.display()));
    let mut common = Vec::new();
    common.extend(arg(&format!("linker={}", driver.display())));
    common.extend(arg("linker-features=-lld"));
    common.extend(arg("link-arg=-nostartfiles"));
    common.extend(arg("link-arg=-nostdlib"));
    common.extend(link_arg(&nounwind));

    // 4. The static example.
    let mut link = common.clone();
    link.extend(arg("relocation-model=static"));
    link.extend(arg("target-feature=+crt-static"));
    link.extend(link_arg(&libc_a));
    link.extend(arg("link-arg=-lgcc"));
    let exe = build(root, "native_static", "static", &link)?;
    check_static(root, &exe, &dir)?;

    // 5. The dynamic example.
    let mut link = common;
    link.extend(link_arg(&libc_so));
    link.extend(arg("link-arg=-lgcc"));
    let exe = build(root, "native_dynamic", "dynamic", &link)?;
    let (count, interp) = check_dynamic(root, &exe)?;

    // 6. The zeroing proof.
    let proof = check_secure_zero(root)?;

    let steps = EXPECTED.lines().filter(|l| l.starts_with("ok ")).count();
    println!("native: liella-native clean with each feature; static example ran ({steps} steps, output as expected); dynamic example linked (NEEDED libc.so.6, {count} native symbols at their nodes), not run: it needs Liella's ld.so (phase 14), its interpreter is {interp}; {proof}");
    Ok(())
}
