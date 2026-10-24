//! `system`, `pclose` and `popen` waiting for their child while a signal interrupts them
//! (spec §18.7). Each behaviour was settled by a black-box probe of the reference C library
//! (glibc 2.39, a handler installed without `SA_RESTART`):
//!
//! * `system` and `pclose` do NOT report `EINTR`: the handler runs, the wait is retried, and the
//!   call returns the child's wait status (the `errno` left behind is unspecified on success and
//!   is not pinned);
//! * a `system` interrupted that way leaves no child behind;
//! * `pclose` of a stream whose child the caller already reaped returns -1 with `ECHILD`;
//! * while `system` waits, `SIGCHLD` is blocked in the caller and `SIGINT`/`SIGQUIT` are ignored;
//!   afterwards the old handlers are back and `SIGCHLD` is unblocked, so a `SIGCHLD` handler runs
//!   once, after `system` returned; a `SIGINT` that was already ignored stays ignored;
//! * a `SIGINT` sent to the process while `system` waits is not delivered to the old handler;
//! * `system(NULL)` returns non-zero when a shell is available;
//! * two threads in `system` share one ignore count: the dispositions stay ignored until the
//!   last returns (`s18_7_t_0` pins the exact state; here a signal sent meanwhile is not seen).
//!
//! Process-wide state is changed only in a re-executed copy of the test binary (`alone`).
//! Every child is reaped; nothing is left running.
#![allow(clippy::unwrap_used, clippy::panic, clippy::as_conversions, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use std::ffi::CString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use liella_boundary::errno;
use liella_boundary::posix::s12_stdio as io;
use liella_boundary::posix::s18_processes as x;
use liella_boundary::posix::s20_signals as sg;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;

const ECHILD: i32 = 10;
const SIGINT: i32 = 2;
const SIGQUIT: i32 = 3;
const SIGUSR1: i32 = 10;
const SIGCHLD: i32 = 17;
const WNOHANG: i32 = 1;

const ALONE: &str = "LIELLA_EINTR_ALONE";
const BASE: &str = "LIELLA_EINTR_BASE";

static HITS: AtomicU32 = AtomicU32::new(0);
extern "C" fn count_signal(_sig: i32) {
    HITS.fetch_add(1, Ordering::SeqCst);
}
fn handler_addr() -> usize {
    count_signal as extern "C" fn(i32) as usize
}

fn cs(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn base() -> String {
    std::env::var(BASE).unwrap_or_else(|_| std::process::id().to_string())
}

/// A fresh scratch directory (shared by the test process and its re-executed copy).
fn scratch(tag: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("eintr-{}-{tag}", base()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Run `body` alone in a re-executed copy of this test binary; the copy exits 0 when `body`
/// returns true. Returns the copy's standard output (empty in a copy made for another case).
fn alone_ok(test: &str, body: impl FnOnce() -> bool) -> String {
    if let Ok(v) = std::env::var(ALONE) {
        if v == test {
            let ok = body();
            CSys.exit_group(if ok { 0 } else { 1 })
        }
        return String::new();
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args([test, "--exact", "--test-threads=1", "--nocapture"]).env(ALONE, test).env(BASE, base()).output().unwrap();
    let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
    if o.status.code() != Some(0) {
        text.push_str(&String::from_utf8_lossy(&o.stderr));
    }
    assert_eq!(o.status.code(), Some(0), "case {test} failed:\n{text}");
    text
}

fn tagged(out: &str, tag: &str) -> Vec<String> {
    out.lines().filter_map(|l| l.split_once(tag)).map(|(_, r)| r.to_string()).collect()
}

fn say(text: &str) {
    let _ = CSys.write(Fd::STDOUT, text.as_bytes());
}

fn bit(sig: i32) -> u64 {
    1u64 << (sig - 1)
}

fn status_field(out: &str, key: &str) -> u64 {
    let (_, v) = out.lines().find_map(|l| l.split_once(key)).unwrap_or_else(|| panic!("no {key} in {out}"));
    u64::from_str_radix(v.trim(), 16).unwrap()
}

/// Install `count_signal` for `sig` WITHOUT `SA_RESTART`.
fn catch(sig: i32) {
    let sa = liella_legacy::signal_compat::CSigaction::new(handler_addr(), liella_kernel::uapi::SigSet::empty(), 0);
    assert_eq!(sg::sigaction(sig, Some(&sa), None), 0);
}

/// The disposition of `sig`: "default", "ignored", "handler" or "other".
fn disp(sig: i32) -> &'static str {
    let mut sa = liella_legacy::signal_compat::CSigaction::new(0, liella_kernel::uapi::SigSet::empty(), 0);
    sg::sigaction(sig, None, Some(&mut sa));
    match sa.handler {
        0 => "default",
        1 => "ignored",
        h if h == handler_addr() => "handler",
        _ => "other",
    }
}

/// A FIFO the child shell blocks on until [`interrupt_then_release`] has sent its signal.
fn fifo(tag: &str) -> PathBuf {
    let f = scratch(tag).join("fifo");
    assert!(std::process::Command::new("mkfifo").arg(&f).status().unwrap().success());
    f
}

/// After a short pause, send `SIGUSR1` to thread `tid` of this process (the one inside the
/// call under test), then release the child blocked on `fifo`. Returns the helper to join.
fn interrupt_then_release(tid: i32, fifo: PathBuf) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(sg::tgkill(x::getpid(), tid, SIGUSR1), 0);
        drop(std::fs::OpenOptions::new().write(true).open(&fifo).unwrap());
    })
}

/// s18_7_c_1 (§18.7 Contract): `system` is not cut short by a signal handled without `SA_RESTART`: the
/// handler runs once, the wait is retried and the shell's status comes back; no child is left.
#[test]
fn eintr_system_retries_the_wait() {
    let f = fifo("sys");
    let out = alone_ok("eintr_system_retries_the_wait", move || {
        catch(SIGUSR1);
        let h = interrupt_then_release(x::gettid(), f.clone());
        errno::set_raw(0);
        let r = x::system(cs(&format!("cat '{}' > /dev/null; exit 3", f.display())).as_ptr());
        h.join().unwrap();
        let left = x::waitpid(-1, None, WNOHANG);
        say(&format!("R:{r:#x} {} left={left} {}\n", HITS.load(Ordering::SeqCst), errno::get() == 0 || left == -1));
        true
    });
    if !out.is_empty() {
        assert_eq!(tagged(&out, "R:"), vec![format!("{:#x} 1 left=-1 true", 3 << 8)], "{out}");
    }
}

/// s18_7_c_2 (§18.7 Contract): `pclose` is not cut short by such a signal either: it returns the command's
/// wait status once the command ended.
#[test]
fn eintr_pclose_retries_the_wait() {
    let f = fifo("pclose");
    let out = alone_ok("eintr_pclose_retries_the_wait", move || {
        catch(SIGUSR1);
        let fp = x::popen(cs(&format!("cat '{}' > /dev/null; exit 5", f.display())).as_ptr(), cs("r").as_ptr());
        assert!(!fp.is_null());
        let h = interrupt_then_release(x::gettid(), f.clone());
        let r = x::pclose(fp);
        h.join().unwrap();
        say(&format!("R:{r:#x} {}\n", HITS.load(Ordering::SeqCst)));
        true
    });
    if !out.is_empty() {
        assert_eq!(tagged(&out, "R:"), vec![format!("{:#x} 1", 5 << 8)], "{out}");
    }
}

/// s18_7_c_2 (§18.7 Contract): `pclose` of a stream whose command the caller already reaped closes the
/// stream and fails with `ECHILD`.
#[test]
fn pclose_after_the_caller_reaped() {
    let out = alone_ok("pclose_after_the_caller_reaped", || {
        let fp = x::popen(cs("exit 7").as_ptr(), cs("r").as_ptr());
        assert!(!fp.is_null());
        let reaped = x::waitpid(-1, None, 0);
        errno::set_raw(0);
        let r = x::pclose(fp);
        say(&format!("R:{} {r} {}\n", reaped > 0, errno::get()));
        true
    });
    if !out.is_empty() {
        assert_eq!(tagged(&out, "R:"), vec![format!("true -1 {ECHILD}")], "{out}");
    }
}

/// s18_7_c_1 (§18.7 Contract): while `system` waits `SIGCHLD` is blocked and `SIGINT`/`SIGQUIT` ignored in
/// the caller; afterwards the handlers are back and `SIGCHLD` is unblocked (its handler runs once,
/// then, for the child that ended meanwhile); a `SIGINT` that was already ignored stays ignored.
#[test]
fn system_signal_state_during_and_after() {
    let out = alone_ok("system_signal_state_during_and_after", || {
        catch(SIGINT);
        catch(SIGQUIT);
        catch(SIGCHLD);
        let script = format!("grep -E '^Sig(Blk|Ign):' /proc/{}/task/{}/status | sed 's/^/R:caller /'", x::getpid(), x::gettid());
        let r = x::system(cs(&script).as_ptr());
        let mut m = liella_abi::layout::sigset_t { __val: [0; 16] };
        sg::sigprocmask(0, None, Some(&mut m));
        say(&format!("R:after {r} int={} quit={} chld={} blocked={} hits={}\n", disp(SIGINT), disp(SIGQUIT), disp(SIGCHLD), sg::sigismember(Some(&m), SIGCHLD), HITS.load(Ordering::SeqCst)));
        sg::signal(SIGINT, 1);
        let r = x::system(cs("exit 0").as_ptr());
        say(&format!("R:ignored {r} {}\n", disp(SIGINT)));
        true
    });
    if out.is_empty() {
        return;
    }
    let caller = tagged(&out, "R:caller ").join("\n");
    assert_eq!(status_field(&caller, "SigBlk:") & bit(SIGCHLD), bit(SIGCHLD), "SIGCHLD blocked while it waits: {out}");
    let iq = bit(SIGINT) | bit(SIGQUIT);
    assert_eq!(status_field(&caller, "SigIgn:") & iq, iq, "SIGINT and SIGQUIT ignored while it waits: {out}");
    let r = tagged(&out, "R:");
    assert!(r.contains(&"after 0 int=handler quit=handler chld=handler blocked=0 hits=1".to_string()), "{out}");
    assert!(r.contains(&"ignored 0 ignored".to_string()), "{out}");
}

/// s18_7_c_1 (§18.7 Contract): `system(NULL)` reports that a shell is available (non-zero).
#[test]
fn system_null_reports_a_shell() {
    assert_ne!(x::system(core::ptr::null()), 0);
}

/// s18_7_t_0 (§18.7 Threads): with two threads inside `system`, a `SIGINT` sent to the process meanwhile
/// does not reach the caller's handler, and both handlers are back after the last returns.
#[test]
fn system_two_threads_share_the_ignore() {
    let out = alone_ok("system_two_threads_share_the_ignore", || {
        catch(SIGINT);
        catch(SIGQUIT);
        let long = std::thread::spawn(|| x::system(cs("sleep 1").as_ptr()));
        while disp(SIGINT) != "ignored" {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let short = x::system(cs("exit 6").as_ptr());
        let during = format!("{} {}", disp(SIGINT), disp(SIGQUIT));
        assert_eq!(sg::kill(x::getpid(), SIGINT), 0);
        let first = long.join().unwrap();
        say(&format!("R:{short:#x} {during} {first} {} {} hits={}\n", disp(SIGINT), disp(SIGQUIT), HITS.load(Ordering::SeqCst)));
        let _ = io::fflush(core::ptr::null_mut());
        true
    });
    if !out.is_empty() {
        assert_eq!(tagged(&out, "R:"), vec![format!("{:#x} ignored ignored 0 handler handler hits=0", 6 << 8)], "{out}");
    }
}
