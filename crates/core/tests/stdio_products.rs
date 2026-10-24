//! Stdio end to end (spec §12; D-12.1, D-12.11): `tests/oracle/stdio_diff.c` built against the
//! host C library and statically against Liella's products, run in a scratch directory; the
//! outputs must be identical line by line, except the recorded differences in [`KNOWN`].
//! Needs the products (`cargo xtask so`) and a C compiler; without the latter the test says
//! so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/oracle.rs"]
mod oracle;

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/stdio_diff.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

/// Lines that differ from the reference by decision: the start of the line, the locale it is
/// under (`*` any), and why. A recorded line that no longer differs fails the test.
const KNOWN: [(&str, &str, &str); 2] = [
    // With a caller buffer smaller than 128 bytes, the reference library writes a block that
    // finds no room in it straight through; Liella keeps it until the buffer fills (§12.1).
    ("user_buffer ", "buffering", "small caller buffers"),
    // `fdopen(fd, "a")` on a descriptor without O_APPEND, at offset 2: POSIX sets the stream's
    // position from the descriptor's offset (2, Liella; owner ruling 2026-10-04, D-12.16); the
    // reference also moves the descriptor to the end when it adds O_APPEND itself (6), but not
    // when the descriptor already had it (0 stays 0). Both set O_APPEND on the descriptor.
    ("fdopen off2 a ", "append", "fdopen position of a descriptor given O_APPEND"),
];
#[test]
fn stdio_behaviour_equals_the_reference() {
    // Clauses: s12_1_c_0, s12_1_c_1, s12_1_c_2, s12_1_c_3, s12_1_c_4, s12_1_c2_0, s12_1_g_1, s12_1_g_2, s12_2_c_1,
    // s12_2_c_1_1, s12_2_c_1_2, s12_2_c_1_3, s12_2_c_2, s12_2_c_3, s12_2_c_4, s12_2_c_5, s12_2_f_0, s12_3_c_1,
    // s12_3_c_2, s12_3_c_3, s12_4_c_1, s12_4_c_2, s12_4_c_3, s12_4_c_3_1, s12_4_c_3_2, s12_4_c_3_3, s12_4_c_4,
    // s12_4_c_5, s12_4_c_6, s12_4_f_0, s12_5_c_1, s12_5_c_2, s12_5_c_3, s12_5_c_4, s12_5_c_5, s12_5_c_6,
    // s12_5_c_7, s12_5_c_8, s12_5_c_9, s12_5_c_10, s12_5a_c_1, s12_5a_c_2, s12_5a_g_0, s12_8_c_1, s12_8_c_2,
    // s12_8_c_3, s12_8_c_4, s12_8_c_5, s12_8_c_6, s12_8_f_0, s12_12_c_2, s12_12_c_3, s12_13_c_0
    // (D-12.1, D-12.11).
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let (host, ours) = (dir.join("stdio_diff_host"), dir.join("stdio_diff_liella"));
    // Without a C compiler the test cannot run; with one, the program must build.
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    assert!(build(&host, false), "stdio_diff.c does not build against the host C library");
    assert!(build(&ours, true), "stdio_diff.c does not link against the products");
    let run = |exe: &Path| -> String {
        let scratch = r.join("target/test-oracles").join(format!("stdio-scratch-{}", exe.file_name().unwrap().to_string_lossy()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let o = Command::new(exe).arg(&scratch).env_clear().stdin(std::process::Stdio::null()).output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}", exe.display(), o.status);
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let (want, got) = (run(&host), run(&ours));
    let (mut diffs, mut locale, mut seen) = (0, String::new(), [false; KNOWN.len()]);
    for (k, (w, g)) in want.lines().zip(got.lines()).enumerate() {
        if let Some(rest) = w.strip_prefix("== ") {
            locale = rest.to_string();
        }
        let known = KNOWN.iter().position(|(start, l, _)| (*l == "*" || *l == locale) && w.starts_with(start));
        if let Some(i) = known {
            if w != g {
                seen[i] = true;
                eprintln!("recorded difference ({}), {locale}:\n  glibc  {w}\n  liella {g}", KNOWN[i].2);
            }
            continue;
        }
        if w != g {
            diffs += 1;
            if diffs <= 40 {
                eprintln!("line {} ({locale}):\n  glibc  {}\n  liella {}", k + 1, &w[..w.len().min(400)], &g[..g.len().min(400)]);
            }
        }
    }
    assert_eq!(want.lines().count(), got.lines().count(), "line counts differ");
    assert_eq!(diffs, 0, "lines that differ from the reference");
    for (i, s) in seen.iter().enumerate() {
        assert!(s, "recorded difference no longer seen: {:?}", KNOWN[i]);
    }
}
