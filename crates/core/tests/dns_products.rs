//! The DNS stub resolver, the `dns` source of the hosts database, IDNA and `getaddrinfo_a` end to
//! end (spec §16.1–16.4; D-16.3, D-16.4, D-16.8): two probes, each built against the host C
//! library and statically against Liella's products, and run by its script in private mount and
//! network namespaces where only `lo` is up (as root through `sudo -n`; each probe starts its own
//! DNS servers on loopback addresses there, so nothing reaches a network):
//!
//! * `tests/oracle/dns_diff.c` with `dns_run.sh` (the behaviour record `spec/behaviour/dns.md`);
//! * `tests/oracle/resolv_diff.c` with `resolv_run.sh` (the options, several servers, timeouts,
//!   rotation, EDNS, single-request, sortlist, the environment, `HOSTALIASES`, reloading, the
//!   search order, IDNA, `getaddrinfo_a`, and the message and text functions).
//!
//! The outputs must be identical line by line, except the recorded differences, each keyed by
//! its section (the text after `== `), the reference's exact line and its occurrence in the
//! section: [`KNOWN`] (a line that differs), [`ADDED`] (a line only Liella prints) and
//! [`DROPPED`] (a line only the reference prints). A recorded difference that no longer shows
//! fails the test. A disagreement is checked by running both probes again (up to [`ATTEMPTS`]
//! runs): the outputs include server logs and timed waits that a loaded machine can stretch, so
//! only a disagreement seen in every run fails. Needs the products (`cargo xtask so`), a C compiler and passwordless `sudo`;
//! without the latter two the test says so and passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/timeout.rs"]
mod timeout;
#[path = "support/oracle.rs"]
mod oracle;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(probe: &str, out: &Path, liella: bool) -> bool {
    let r = root();
    let src = r.join("crates/core/tests/oracle").join(probe);
    let mut cmd = Command::new("cc");
    if liella {
        cmd.args(["-std=gnu17", "-O1", "-w", "-static", "-nostdlib", "-fno-builtin"]).arg(r.join("target/liella/liella-crt1.o")).arg(&src).arg(r.join("target/liella/libc.a")).arg("-lgcc");
    } else {
        cmd.args(["-std=gnu17", "-O1", "-w"]).arg(&src).args(["-lresolv", "-lpthread"]);
    }
    oracle::build(cmd, out)
}

/// A recorded difference: the section, the reference's line, its occurrence (from 0) among the
/// equal lines of the section, and why.
type Entry = (&'static str, &'static str, usize, &'static str);

const H_ERRNO_SUCCESS: &str = "a successful res_search leaves h_errno alone, as res_query does (spec §16.4 Failure: h_errno reports failures; dns.md lists the reference's HOST_NOT_FOUND after success as an artifact)";
const TCP_REFUSED: &str = "REFUSED is TRY_AGAIN / EAI_AGAIN over TCP as over UDP (the server refuses, another may answer; dns.md lists the transport-dependent result as an artifact)";
const TCP_WRONG_QUESTION: &str = "a response to another question is not an answer, over TCP as over UDP (spec §16.4 Guarantees: the question must match): no response, TRY_AGAIN / EAI_AGAIN; the reference accepts it over TCP";

/// `dns_diff.c`: lines that differ by decision.
const DNS_KNOWN: [Entry; 7] = [
    ("scenario search", "res_search [host] type=1: 68 h_errno=1 rcode=0 aa=1 tc=0 ad=0 an=1", 0, H_ERRNO_SUCCESS),
    ("scenario search", "res_search [x.y] type=1: 66 h_errno=1 rcode=0 aa=1 tc=0 ad=0 an=1", 0, H_ERRNO_SUCCESS),
    ("scenario search", "res_search [x.y.] type=1: 40 h_errno=1 rcode=0 aa=1 tc=0 ad=0 an=1", 0, H_ERRNO_SUCCESS),
    ("scenario usevc", "gethostbyname: NULL h_errno=3", 0, TCP_REFUSED),
    ("scenario usevc", "gai [refused.test] fam=0 flags=2: -2 (Name or service not known)", 0, TCP_REFUSED),
    ("scenario usevc", "gethostbyname: NULL h_errno=4", 1, TCP_WRONG_QUESTION),
    ("scenario usevc", "gai [wrongq.test] fam=0 flags=2: -5 (No address associated with hostname)", 0, TCP_WRONG_QUESTION),
];

/// `resolv_diff.c`: lines that differ by decision.
const RESOLV_KNOWN: [Entry; 6] = [
    (
        "offline",
        "loc [90 S 180 W 42849672.95m 90000000m 90000000m 90000000m]: 16 bin=004848486cb0270059604e00ffffffff ntoa=[90 00 00.000 S 180 00 00.000 W -100000.-01m 4000000.00m 4000000.00m 4000000.00m] same-buffer=1",
        0,
        "LOC sizes of up to 9e9 cm and the whole 32-bit altitude range as RFC 1876 defines them (the reference wraps 90000000m at 32 bits and prints the highest altitudes as -100000.-01m)",
    ),
    ("scenario envspace", "  search: [] [l1.test] [l2.test] defdname=[]", 0, "LOCALDOMAIN is a white-space separated list (resolv.conf(5)): white space before the first domain makes no empty domain"),
    ("scenario envempty", "  search: [] defdname=[]", 0, "an empty LOCALDOMAIN is an empty search list, not a list of one empty domain (the names asked are the same)"),
    ("scenario idn", "gai idn [ل٠۰.test] fam=2 flags=42: -2", 0, "UTS #46 CheckBidi applies RFC 5893 rule 4: an Arabic-Indic and an Extended Arabic-Indic digit in one right-to-left label is not a valid label (EAI_IDN_ENCODE); the reference accepts it"),
    ("scenario gaia", "gai_suspend after done: -103", 0, "gai_suspend returns 0 when a listed request has completed (getaddrinfo_a(3)); the reference reports EAI_ALLDONE for one completed before the call"),
    ("scenario gaia", "in progress: 0 suspend=-11 gai_error=-100", 0, "gai_suspend's timeout is EAI_AGAIN (getaddrinfo_a(3)); the reference reports EAI_SYSTEM"),
];

/// Lines only Liella prints.
const RESOLV_ADDED: [Entry; 0] = [];

/// Lines only the reference prints.
const RESOLV_DROPPED: [Entry; 1] = [("scenario idn", "  q srv1 udp xn--ghb0dsx.test A rd=1 ad=0 cd=0 an=0 ns=0 ar=0 opt=none qlen=34", 0, "the name of the bidi line above is refused before any query")];

/// The lines of an output with their section and their occurrence in it.
fn sections(text: &str) -> Vec<(String, String, usize)> {
    let mut sec = String::new();
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    let mut out = Vec::new();
    for l in text.lines() {
        if let Some(s) = l.strip_prefix("== ") {
            sec = s.to_string();
        }
        let n = seen.entry((sec.clone(), l.to_string())).or_insert(0);
        out.push((sec.clone(), l.to_string(), *n));
        *n += 1;
    }
    out
}

fn find(list: &[Entry], sec: &str, line: &str, n: usize) -> Option<usize> {
    list.iter().position(|e| e.0 == sec && e.1 == line && e.2 == n)
}

/// `text` without the lines recorded in `list` (each must be found once).
fn without(text: &str, list: &[Entry], who: &str) -> Result<Vec<(String, String, usize)>, String> {
    let mut seen = vec![false; list.len()];
    let mut out = Vec::new();
    for (sec, l, n) in sections(text) {
        match find(list, &sec, &l, n) {
            Some(i) => {
                seen[i] = true;
                eprintln!("recorded line only {who} prints ({}):\n  {l}", list[i].3);
            }
            None => out.push((sec, l, n)),
        }
    }
    match seen.iter().position(|s| !s) {
        Some(i) => Err(format!("recorded line no longer printed by {who}: {:?}", list[i])),
        None => Ok(out),
    }
}

/// Whether Liella's output `got` equals the reference's `want`, but for the recorded
/// differences; the first discrepancy otherwise.
fn check(want: &str, got: &str, known: &[Entry], added: &[Entry], dropped: &[Entry]) -> Result<(), String> {
    let want = without(want, dropped, "the reference")?;
    let got = without(got, added, "Liella")?;
    let mut diffs = 0;
    let mut seen = vec![false; known.len()];
    for ((ws, w, wn), (_, g, _)) in want.iter().zip(got.iter()) {
        if let Some(i) = find(known, ws, w, *wn) {
            if w != g {
                seen[i] = true;
                eprintln!("recorded difference ({}):\n  glibc  {w}\n  liella {g}", known[i].3);
            }
            continue;
        }
        if w != g {
            diffs += 1;
            if diffs <= 40 {
                eprintln!("[{ws}]\n  glibc  {w}\n  liella {g}");
            }
        }
    }
    let lines = |v: &[(String, String, usize)]| v.iter().map(|x| x.1.as_str()).collect::<Vec<_>>().join("\n");
    if want.len() != got.len() {
        return Err(format!("line counts differ ({} and {}):\n--- glibc\n{}\n--- liella\n{}", want.len(), got.len(), lines(&want), lines(&got)));
    }
    if diffs != 0 {
        return Err(format!("{diffs} lines differ from the reference"));
    }
    match seen.iter().position(|s| !s) {
        Some(i) => Err(format!("recorded difference no longer seen: {:?}", known[i])),
        None => Ok(()),
    }
}

/// Runs of both probes when they disagree: the probes print what their own DNS servers logged
/// and how long the timed waits took, which a loaded machine stretches (a server process not
/// scheduled within the 1 s timeout makes the resolver ask again). A disagreement fails only if
/// it is there in every run.
const ATTEMPTS: usize = 3;

fn compare(probe: &str, script: &str, known: &[Entry], added: &[Entry], dropped: &[Entry]) {
    let r = root();
    assert!(r.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so`");
    if skip_note::missing("cc") {
        skip_note::skipped("skipped: no C compiler");
        return;
    }
    if !Command::new("sudo").args(["-n", "true"]).status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no passwordless sudo (the lookups need private mount and network namespaces)");
        return;
    }
    let dir = r.join("target/test-oracles");
    std::fs::create_dir_all(&dir).unwrap();
    let stem = probe.trim_end_matches(".c");
    let (host, ours) = (dir.join(format!("{stem}_host")), dir.join(format!("{stem}_liella")));
    assert!(build(probe, &host, false), "{probe} does not build against the host C library");
    assert!(build(probe, &ours, true), "{probe} does not link against the products");
    let script = r.join("crates/core/tests/oracle").join(script);
    let wrap = root().join("crates/core/tests/oracle/nscache_mode.sh");
    let run = |exe: &Path, scratch: &str, mode: Option<&str>| -> String {
        let mut cmd = Command::new("timeout");
        cmd.arg(timeout::secs(900));
        if let Some(m) = mode {
            cmd.arg("sh").arg(&wrap).arg(m);
        }
        let o = cmd.arg("sh").arg(&script).arg(exe).arg(dir.join(scratch)).env_clear().env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin").output().unwrap();
        assert!(o.status.success(), "{} failed: {:?}\n{}", exe.display(), o.status, String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let mut reference = run(&host, &format!("{stem}-host"), None);
    // Liella with the name-service cache on and off (D-16.10), each in a private /dev/shm (`nscache_mode.sh`).
    for mode in ["on", "off"] {
        for attempt in 1..=ATTEMPTS {
            eprintln!("name-service cache {mode}, run {attempt}");
            let got = run(&ours, &format!("{stem}-liella-{mode}"), Some(mode));
            match check(&reference, &got, known, added, dropped) {
                Ok(()) => break,
                Err(e) if attempt < ATTEMPTS => {
                    eprintln!("run {attempt} disagrees; both probes run again (a loaded machine stretches the timed waits):\n{e}");
                    reference = run(&host, &format!("{stem}-host"), None);
                }
                Err(e) => panic!("{probe}, name-service cache {mode}: {e}"),
            }
        }
    }
}

/// The two tests run their DNS servers and timeouts one at a time: together they load the
/// machine enough to stretch the timed waits the probes print.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn dns_equals_the_reference() {
    // Clauses: s16_4_c_0, s16_4_c_1, s16_4_c_1_1, s16_4_c_1_2, s16_4_c_1_3, s16_4_c_1_4, s16_4_c_2, s16_4_c_3,
    // s16_4_c_4, s16_4_g_1, s16_4_g_2, s16_4_g_3, s16_4_g_4, s16_4_f_0, s16_1_c_0, s16_1_c_1, s16_1_c_2,
    // s16_1_g_1, s16_2_c_1, s16_2_c_2, s16_2_c_2_2, s16_2_c_2_4, s16_2_c_2_6, s16_2_c_4, s16_2_f_0,
    // s16_2_g_0, s16_3_c_0, s16_3_f_0 (D-16.3, D-16.4).
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    compare("dns_diff.c", "dns_run.sh", &DNS_KNOWN, &[], &[]);
}

#[test]
fn resolv_equals_the_reference() {
    // Clauses: s16_4_c_1, s16_4_c_1_1, s16_4_c_1_2, s16_4_c_1_3, s16_4_c_1_4, s16_4_c_2, s16_4_c_3, s16_4_c_4,
    // s16_4_g_1, s16_4_g_2, s16_4_g_3, s16_4_g_4, s16_4_f_0, s16_2_c_5, s16_2_c_2_6, s16_2_c_1, s16_2_c_4,
    // s16_2_f_0, s16_1_c_1, s16_3_c_0, s16_3_f_0 (D-16.3, D-16.4, D-16.8).
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    compare("resolv_diff.c", "resolv_run.sh", &RESOLV_KNOWN, &RESOLV_ADDED, &RESOLV_DROPPED);
}
