//! The exact and correctly rounded math functions end to end (spec §10.1, §10.2, §10.4, §10.6;
//! D-10.3): `tests/oracle/exact_diff.c` built against the host C library and statically against
//! Liella's products (`libm.a`, `libc.a`); every function of those families in every format, on
//! edge operands, under every rounding direction where it matters. The outputs (result bits,
//! raised flags, `errno`) must be identical line by line, except the recorded differences in
//! [`KNOWN`]. Needs the products (`cargo xtask so`) and a C compiler; without the compiler the
//! test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, missing_docs)]
#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

#[path = "../../core/tests/support/oracle.rs"]
mod oracle;

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/math/tests/oracle/exact_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        let p = r.join("target/liella");
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"])
            .arg(p.join("liella-crt1.o"))
            .arg(&src)
            .arg(p.join("libm.a"))
            .arg(p.join("libc.a"))
            .arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1", "-fno-builtin"]).arg(&src).arg("-lm");
    }
    oracle::build(cmd, out)
}

/// Lines that differ from the reference by decision: the start of the line, and why. A recorded
/// entry that no longer matches a differing line fails the test (docs/differences-from-glibc.md).
const KNOWN: [(&str, &str); 10] = [
    ("signbitf ", "`__signbit*` return 1 for a set sign bit; glibc returns the bit where its code finds it (8, 128, 512)"),
    ("signbitd ", "as above"),
    ("signbitl ", "as above"),
    ("signbitq ", "as above"),
    ("signbit-x87 ", "as above"),
    ("isnanl ", "`__isnanl` returns 1 for a NaN; glibc returns 65535"),
    ("isnan-x87 ", "as above"),
    ("scalbl N 7fff", "scalbl(NaN, −∞): the quieted NaN operand; glibc returns a NaN its x87 code computes (7ffff800...)"),
    ("canonicalize-x87 ", "pseudo-denormals are stored in their canonical encoding with 0 returned (spec §10.2, the manual); glibc returns 1"),
    ("finite-x87 ", "an x87 unnormal is not finite, as it is a NaN to isnan and fpclassify; glibc's finitel says it is finite"),
];

/// The other rounding modes of the `scalbl(NaN, −∞)` difference.
fn known(line: &str) -> Option<usize> {
    let scalbl_nan = ["scalbl U 7fff", "scalbl D 7fff", "scalbl Z 7fff"];
    if scalbl_nan.iter().any(|p| line.starts_with(p)) {
        return Some(7);
    }
    KNOWN.iter().position(|(p, _)| line.starts_with(p))
}

#[test]
fn exact_functions_equal_the_reference() {
    // Clauses: s10_1_c_1, s10_2_t_r16, s10_4_t_r1, s10_6_c_0 (D-10.10).
    let r = root();
    assert!(r.join("target/liella/libm.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("exact_diff_host"), dir.join("exact_diff_liella"));
    assert!(build(&host, false), "exact_diff.c does not build against the host C library");
    assert!(build(&ours, true), "exact_diff.c does not link against the products");
    let run = |exe: &Path| -> String {
        let o = Command::new(exe).env_clear().output().unwrap();
        assert!(o.status.success(), "{} failed: {}", exe.display(), String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap()
    };
    let (want, got) = (run(&host), run(&ours));
    let (want, got): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    assert_eq!(want.len(), got.len(), "different numbers of lines");
    assert!(want.len() > 100_000, "the reference printed only {} lines", want.len());
    let mut used = [false; KNOWN.len()];
    let mut errors = Vec::new();
    for (w, g) in want.iter().zip(&got) {
        if w == g {
            continue;
        }
        match known(w) {
            Some(i) => used[i] = true,
            None => errors.push(format!("reference: {w}\n   liella: {g}")),
        }
    }
    for (i, u) in used.iter().enumerate() {
        if !u && !KNOWN[i].1.starts_with("as above") {
            errors.push(format!("recorded difference no longer seen: {:?}", KNOWN[i].0));
        }
    }
    assert!(errors.is_empty(), "{} differences:\n{}", errors.len(), errors.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
}
