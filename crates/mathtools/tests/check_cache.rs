//! The `remez --check` verdict cache and parallel driver (D-10.58): the hash against FIPS 180-4
//! test vectors, the key against each of its inputs, and the binary's exit status, order and
//! cache behaviour on small fits.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]
#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use liella_mathtools::check_cache::{CACHE_DIR_ENV, KeyInputs, NO_CACHE_ENV, Sha256, check_key, hex, sha256};

#[test]
fn sha256_fips_vectors() {
    // no spec clause: test tooling of the math oracle (D-10.58); vectors from FIPS 180-4's examples.
    assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    let two_blocks = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    assert_eq!(hex(&sha256(two_blocks)), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
    // A million 'a's fed in uneven pieces: the incremental path agrees with the one-shot one.
    let million = vec![b'a'; 1_000_000];
    let mut s = Sha256::new();
    for piece in million.chunks(997) {
        s.update(piece);
    }
    assert_eq!(hex(&s.finish()), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    assert_eq!(hex(&sha256(&million)), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
}

#[test]
fn key_changes_with_every_input() {
    // no spec clause: test tooling of the math oracle (D-10.58).
    let image = vec![("/x/remez".to_string(), [1u8; 32]), ("/lib/libm.so.6".to_string(), [2u8; 32])];
    let image_exe = vec![("/x/remez".to_string(), [3u8; 32]), ("/lib/libm.so.6".to_string(), [2u8; 32])];
    let image_lib = vec![("/x/remez".to_string(), [1u8; 32]), ("/lib/libm.so.6".to_string(), [4u8; 32])];
    let image_path = vec![("/y/remez".to_string(), [1u8; 32]), ("/lib/libm.so.6".to_string(), [2u8; 32])];
    let base = KeyInputs {
        image: &image,
        mode: "--check",
        fit_arg: "crates/math/fits/a.fit",
        fit_display: "crates/math/fits/a.fit",
        fit: b"name = a\n",
        coeff_path: "crates/math/src/coeffs/a.rs",
        coeff: Some(b"pub const A: f64 = 1.0;\n"),
    };
    assert_eq!(check_key(&base), check_key(&base.clone()), "the key is deterministic");
    let variants: Vec<(&str, KeyInputs)> = vec![
        ("executable bytes", KeyInputs { image: &image_exe, ..base.clone() }),
        ("shared object bytes", KeyInputs { image: &image_lib, ..base.clone() }),
        ("image path", KeyInputs { image: &image_path, ..base.clone() }),
        ("image empty", KeyInputs { image: &[], ..base.clone() }),
        ("mode", KeyInputs { mode: "", ..base.clone() }),
        ("fit argument", KeyInputs { fit_arg: "./crates/math/fits/a.fit", ..base.clone() }),
        ("fit display", KeyInputs { fit_display: "a.fit", ..base.clone() }),
        ("fit bytes", KeyInputs { fit: b"name = b\n", ..base.clone() }),
        ("coefficient path", KeyInputs { coeff_path: "crates/math/src/coeffs/b.rs", ..base.clone() }),
        ("coefficient bytes", KeyInputs { coeff: Some(b"pub const A: f64 = 2.0;\n"), ..base.clone() }),
        ("coefficient empty", KeyInputs { coeff: Some(b""), ..base.clone() }),
        ("coefficient absent", KeyInputs { coeff: None, ..base.clone() }),
        // Field boundaries: moving bytes from one field to the next is a different key.
        ("boundary", KeyInputs { fit_arg: "crates/math/fits/a.fitc", fit_display: "rates/math/fits/a.fit", ..base.clone() }),
    ];
    let mut seen = vec![("base", check_key(&base))];
    for (what, v) in &variants {
        let k = check_key(v);
        assert_eq!(k.len(), 64, "{what}: a 256-bit hex key");
        for (other, ko) in &seen {
            assert_ne!(&k, ko, "changing the {what} must change the key (same as {other})");
        }
        seen.push((what, k));
    }
}

/// A scratch directory for one test, removed first so every run starts cold.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("liella-remez-cache-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A small fit (sin on a short interval, degree 5) whose output is an absolute path in `dir`.
fn small_fit(dir: &Path, name: &str) -> (PathBuf, PathBuf) {
    let out = dir.join(format!("{name}.rs"));
    let fit = dir.join(format!("{name}.fit"));
    let text = format!(
        "name = {name}\ntarget = sin\ninterval = -1/8, 1/8\npowers = odd 1..5\nerror = relative\nformat = f64\nprecision = 128\npoints = 64\noutput = {}\n",
        out.display()
    );
    std::fs::write(&fit, text).unwrap();
    (fit, out)
}

fn remez(args: &[&Path], check: bool, cache: &Path, no_cache: bool) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_remez"));
    if check {
        c.arg("--check");
    }
    c.args(args).env(CACHE_DIR_ENV, cache).env_remove(NO_CACHE_ENV);
    if no_cache {
        c.env(NO_CACHE_ENV, "1");
    }
    c.output().unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The `up to date` lines of a run, in order.
fn verdicts(o: &Output) -> Vec<String> {
    text(&o.stdout).lines().filter(|l| l.contains(": up to date")).map(|l| l.trim().to_string()).collect()
}

/// The fit names of a run's `step: remez/<fit> <seconds>s` lines, in order (the format xtask/src/steptime.rs
/// reads); a malformed step line panics.
fn steps(o: &Output) -> Vec<String> {
    let out = text(&o.stdout);
    out.lines()
        .filter_map(|l| l.strip_prefix("step: remez/"))
        .map(|rest| {
            let (name, secs) = rest.split_once(' ').unwrap_or_else(|| panic!("step line without a time: {rest}"));
            // `<seconds>` has one decimal, then `s`.
            let num = secs.strip_suffix('s').unwrap_or_else(|| panic!("step time without `s`: {rest}"));
            let one_decimal = num.split_once('.').is_some_and(|(_, d)| d.len() == 1);
            assert!(one_decimal && num.parse::<f64>().is_ok_and(|n| n >= 0.0), "step time: {rest}");
            name.to_string()
        })
        .collect()
}

/// Each fit's `up to date` line is followed by its own step line (output and step line stay together).
fn verdict_then_step(o: &Output, names: &[&str]) {
    let lines: Vec<String> = text(&o.stdout).lines().map(str::to_string).collect();
    for name in names {
        let v = lines.iter().position(|l| l.contains(&format!("{name}.rs: up to date"))).unwrap_or_else(|| panic!("no verdict for {name}: {lines:?}"));
        assert!(lines.get(v + 1).is_some_and(|l| l.starts_with(&format!("step: remez/{name} "))), "step line of {name} does not follow its output: {lines:?}");
    }
}

fn stamps(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|r| r.count()).unwrap_or(0)
}

#[test]
fn parallel_check_order_failure_and_cache() {
    // no spec clause: test tooling of the math oracle (D-10.58).
    let d = scratch("parallel");
    let cache = d.join("cache");
    let fits: Vec<(PathBuf, PathBuf)> = ["fa", "fb", "fc"].iter().map(|n| small_fit(&d, n)).collect();
    let fit_paths: Vec<&Path> = fits.iter().map(|f| f.0.as_path()).collect();

    // Generating mode writes every output and never touches the cache.
    let g = remez(&fit_paths, false, &cache, false);
    assert!(g.status.success(), "generate failed: {}", text(&g.stderr));
    assert_eq!(stamps(&cache), 0, "the generating mode wrote a stamp");

    // Cold: every fit runs, passes, and is stamped; the output is in command-line order.
    let cold = remez(&fit_paths, true, &cache, false);
    assert!(cold.status.success(), "cold check failed: {}", text(&cold.stderr));
    let v = verdicts(&cold);
    assert_eq!(v.len(), 3, "cold verdicts: {v:?}");
    for (line, (_, out)) in v.iter().zip(&fits) {
        assert!(line.starts_with(&format!("{}: up to date", out.display())), "order: {v:?}");
        assert!(!line.contains("cached"), "cold run replayed a verdict: {line}");
    }
    assert_eq!(stamps(&cache), 3);
    // One step line per fit (XTASK-TEST-STEP-TIMES), in command-line order, right after that fit's output.
    let names = ["fa", "fb", "fc"];
    assert_eq!(steps(&cold), names, "cold step lines");
    verdict_then_step(&cold, &names);

    // Warm: the same verdicts, replayed, with the same reports.
    let warm = remez(&fit_paths, true, &cache, false);
    assert!(warm.status.success(), "warm check failed: {}", text(&warm.stderr));
    let w = verdicts(&warm);
    assert_eq!(w.len(), 3, "warm verdicts: {w:?}");
    for (line, (_, out)) in w.iter().zip(&fits) {
        assert!(line.starts_with(&format!("{}: up to date (cached ", out.display())), "warm: {w:?}");
    }
    // A replayed verdict still prints its step line.
    assert_eq!(steps(&warm), names, "warm step lines");
    verdict_then_step(&warm, &names);
    // The reports, without the verdict and step lines (the step times differ between runs).
    let strip = |o: &Output| text(&o.stdout).lines().filter(|l| !l.contains(": up to date") && !l.starts_with("step: ")).collect::<Vec<_>>().join("\n");
    assert_eq!(strip(&cold), strip(&warm), "the replayed reports differ from the computed ones");

    // The cache off gives the cold result.
    let off = remez(&fit_paths, true, &cache, true);
    assert!(off.status.success(), "uncached check failed: {}", text(&off.stderr));
    assert_eq!(verdicts(&off), v);

    // The last fit's coefficient file changes: its key changes, it is fitted again and fails, and the
    // run fails although every other fit passes from the cache. Nothing is stamped for it.
    let last_out = &fits[2].1;
    let mut changed = std::fs::read_to_string(last_out).unwrap();
    changed.push_str("// edited\n");
    std::fs::write(last_out, changed).unwrap();
    let bad = remez(&fit_paths, true, &cache, false);
    assert!(!bad.status.success(), "a failing last fit passed: {}", text(&bad.stdout));
    let err = text(&bad.stderr);
    assert!(err.contains(&format!("{}: out of date", last_out.display())), "stderr: {err}");
    assert_eq!(verdicts(&bad).len(), 2, "{:?}", verdicts(&bad));
    // A failing fit is timed and named too.
    assert_eq!(steps(&bad), names, "step lines with a failing fit");
    assert_eq!(stamps(&cache), 3, "a failing fit wrote a stamp");

    // Failure of the first fit with the last one passing: still a failure.
    let first_out = &fits[0].1;
    std::fs::write(first_out, "").unwrap();
    std::fs::write(last_out, "").unwrap();
    let g2 = remez(&fit_paths[2..], false, &cache, false);
    assert!(g2.status.success());
    let bad_first = remez(&fit_paths, true, &cache, true);
    assert!(!bad_first.status.success(), "a failing first fit passed");
    assert!(text(&bad_first.stderr).contains(&format!("{}: out of date", first_out.display())));

    let _ = std::fs::remove_dir_all(&d);
}
