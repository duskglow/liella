//! The diagnostics for a bad `host.conf` line or `RESOLV_*` value (D-16.13), end to end in the
//! products against the reference. No spec clause: §16 is silent on them. `tests/oracle/hostconf_diag.c`
//! is built against the host C library and statically against Liella's products and run with
//! `RESOLV_HOST_CONF` naming a scratch file (the system's `hosts` and `nsswitch.conf` answer
//! `localhost`; the diagnostics do not depend on the answer). Both libraries must print the same
//! standard output and standard error: the text, once per process, at the first host-name lookup
//! and not for a numeric `getaddrinfo`. Needs the products (`cargo xtask so`) and a C compiler;
//! without them the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/timeout.rs"]
mod timeout;
#[path = "support/oracle.rs"]
mod oracle;
use std::path::{Path, PathBuf};
use std::process::Command;

/// (host.conf text, extra environment, mode, whether the reference must print something).
type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str, bool);

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle/hostconf_diag.c");
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1"]).arg(&src);
    }
    oracle::build(cmd, out)
}

/// Standard output and standard error of the program in `mode`, with `RESOLV_HOST_CONF` set to
/// `conf` and the extra variables `env`.
fn run(exe: &Path, mode: &str, conf: &Path, env: &[(&str, &str)]) -> (String, String) {
    let mut cmd = Command::new("timeout");
    cmd.arg(timeout::secs(60)).arg(exe).arg(mode).env_clear().env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin");
    cmd.env("RESOLV_HOST_CONF", conf);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "run failed: {}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}

/// Trace: no spec clause (D-16.13). Record: each malformed line and bad `RESOLV_*` value prints
/// the reference's text on standard error once, at the first host-name lookup; a numeric
/// `getaddrinfo` and a good file print nothing; `on`/`off` are read as a prefix.
#[test]
fn diagnostics_match_the_reference() {
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    let out = r.join("target/test-oracles");
    std::fs::create_dir_all(&out).unwrap();
    let (host, ours) = (out.join("hostconf_diag_host"), out.join("hostconf_diag_liella"));
    assert!(build(&host, false), "hostconf_diag.c does not build against the host C library");
    assert!(build(&ours, true), "hostconf_diag.c does not link against the products");
    let dir = std::env::temp_dir().join(format!("hostconf-diag-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let conf = dir.join("host.conf");
    // (host.conf text, extra environment, mode, whether the reference must print something)
    let cases: &[Case] = &[
        ("multi on\nreorder off\ntrim a.b\norder hosts,bind\n", &[], "name", false),
        ("", &[], "name", false),
        ("nospoof on\n", &[], "name", true),
        ("spoof warn\nspoofalert on\n", &[], "name", true),
        ("  foo bar baz  \n# c\n\nmulti=on\n", &[], "name", true),
        ("multi\n", &[], "name", true),
        ("multi maybe\n", &[], "name", true),
        ("multi # c\nreorder\n", &[], "name", true),
        ("reorder yes\n", &[], "name", true),
        ("multi on x\n", &[], "name", true),
        ("multi onx\n", &[], "name", true),
        ("multi on,off\n", &[], "name", true),
        ("multi on#c\n", &[], "name", false),
        ("multi on # c\n", &[], "name", false),
        ("MULTI ON\nReOrDeR OfF\nTRIM x y\nORDER z\n", &[], "name", false),
        ("foo\r\nmulti on\r\n", &[], "name", true),
        ("multi on x  \n", &[], "name", true),
        ("bad1\nmulti no\nbad2", &[], "name", true),
        ("multi on\n", &[("RESOLV_MULTI", "maybe")], "name", true),
        ("multi on\n", &[("RESOLV_MULTI", "")], "name", true),
        ("multi on\n", &[("RESOLV_MULTI", " on")], "name", true),
        ("multi on\n", &[("RESOLV_MULTI", "onx"), ("RESOLV_REORDER", "on x")], "name", false),
        ("multi on\n", &[("RESOLV_MULTI", "on,off"), ("RESOLV_REORDER", "off")], "name", false),
        ("multi on\n", &[("RESOLV_REORDER", "perhaps")], "name", true),
        ("foo\n", &[("RESOLV_MULTI", "x"), ("RESOLV_REORDER", "y")], "name", true),
        ("multi on\n", &[("RESOLV_OVERRIDE_TRIM_DOMAINS", "x;y"), ("RESOLV_ADD_TRIM_DOMAINS", "z"), ("RESOLV_SPOOF_CHECK", "z")], "name", false),
        ("foo\n", &[("RESOLV_MULTI", "x")], "numeric", false),
        ("foo\n", &[], "numeric-name", true),
        ("multi on\n", &[], "numeric", false),
    ];
    for (i, (text, env, mode, prints)) in cases.iter().enumerate() {
        std::fs::write(&conf, text).unwrap();
        let reference = run(&host, mode, &conf, env);
        assert_eq!(!reference.1.is_empty(), *prints, "case {i} {text:?} {env:?}: the reference's standard error: {:?}", reference.1);
        assert_eq!(run(&ours, mode, &conf, env), reference, "case {i} {text:?} {env:?} {mode}: Liella differs from the reference");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Trace: no spec clause (D-16.13). Record: the problems found in a text, with their line numbers.
#[test]
fn problems_found_in_a_text() {
    use liella::names::hostconf::{problems, Problem};
    let mut got = Vec::new();
    problems(b"multi on\n  foo bar \n# c\nmulti x y\nreorder off z\ntrim q\norder r", |n, p| got.push((n, p)));
    assert_eq!(got, [(2, Problem::BadCommand(b"foo bar ")), (4, Problem::Expected(b"x y")), (5, Problem::Garbage(b"z"))]);
    assert_eq!(liella::names::hostconf::on_off(b"OFFx"), Some(false));
    assert_eq!(liella::names::hostconf::on_off(b"o"), None);
}
