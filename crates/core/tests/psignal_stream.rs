// Spec §20.8 (`psignal`, `psiginfo`): the diagnostic goes through the `stderr` stream; cases (a)-(d) of
// PSIGNAL-STDERR-STREAM, probed on glibc 2.39. The cases run as one self-checking C program
// (conformance/c/s20_psignal_stream.c) against the products, because they need a real descriptor 2.

//! `psignal` and `psiginfo` against the `stderr` stream (§20.8).
#![allow(clippy::unwrap_used, missing_docs)]

fn conformance(names: &[&str]) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// §20.8: (a) psignal's text follows the pending buffered `stderr` text and precedes later text; (b) a
/// wide-oriented `stderr` stays wide and receives it; (c) after `fclose(stderr)` nothing is written;
/// (d) a closed descriptor 2 sets the stream's error flag and `errno` is EBADF; `psiginfo` instead writes
/// to descriptor 2 directly (before pending text, even after `fclose`, no flag). `perror`, `warn`, `err`
/// follow the same four cases.
// no spec clause: §20.8 gives the text only; the stream behaviour is what glibc 2.39 was probed to do.
#[test]
fn psignal_stderr_stream_cases() {
    conformance(&["s20_psignal_stream"]);
}
