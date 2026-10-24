//! The no-`/proc` lane (D-X.12, OD-5): no function may crash or give a wrong answer where `/proc`
//! is absent; it fails with an error or uses the fallback its decision row records. Each test runs
//! its body in a copy of this test binary started in a private user and mount namespace with an
//! empty tmpfs over `/proc` and `/sys` (`unshare -r -m`; no root, no host state changed), and
//! compares the calls of the exported functions of the modules that read `/proc` (`pthread`,
//! `spawn`, `dispatch`, `io_compat`, `fs_compat`, `term`, `sys`, `cred_compat`, `accounting`,
//! `system`, `pthread_compat`) with the D-X.12 fallbacks. Where a value depends on the host (the
//! page size, the link limit of the working tree's file system), the parent computes it with
//! `/proc` and passes it in. The products-level comparison with the reference is
//! `noproc_products.rs`. Where user namespaces are disabled every test says so and passes.
//! Not covered here: the name-service cache's boot ID (private to `nscache`, reached only through
//! a cache region; `nscache_products.rs`).
#![no_builtins]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::as_conversions, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

#[path = "support/skip_note.rs"]
mod skip_note;

use std::ffi::CString;
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use liella_abi::ctypes::{c_char, c_void};
use liella_abi::layout::{pthread_attr_t, pthread_t};
use liella_boundary::errno;
use liella_boundary::posix::s01_startup as s01;
use liella_boundary::posix::s11_lowio as lo;
use liella_boundary::posix::s13_filesystem as fs;
use liella_boundary::posix::s17_users as us;
use liella_boundary::posix::s18_processes as pr;
use liella_boundary::posix::s19_terminals as tm;
use liella_boundary::posix::s22_threads as th;
use liella_boundary::posix::s24_system as sy;
use liella_boundary::posix::s25_ipc as ipc;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;

const CASE: &str = "LIELLA_NO_PROC_CASE";
const ENOENT: i32 = 2;
const EBADF: i32 = 9;
const ENOTTY: i32 = 25;
const ENOSYS: i32 = 38;
const EOPNOTSUPP: i32 = 95;
const AT_PAGESZ: u64 = 6;

/// Whether unprivileged user and mount namespaces work here (asked once).
fn userns() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| Command::new("unshare").args(["-r", "-m", "sh", "-c", "mount -t tmpfs none /proc && mount -t tmpfs none /sys"]).output().is_ok_and(|o| o.status.success()))
}

/// Run `body` without `/proc` and `/sys`: in the copy started for `test`, check that both are
/// empty, run it and exit; here, start that copy (with `env`) and require its success. The body
/// returns what was wrong, if anything.
fn without_proc(test: &str, env: &[(&str, String)], body: impl FnOnce() -> Result<(), String>) {
    if let Ok(case) = std::env::var(CASE) {
        if case != test {
            return;
        }
        let empty = |d: &str| std::fs::read_dir(d).is_ok_and(|mut e| e.next().is_none());
        let r = if empty("/proc") && empty("/sys") { body() } else { Err("/proc or /sys is not an empty tmpfs: the lane would test nothing".into()) };
        match r {
            Ok(()) => println!("no-proc-lane {test}: ok"),
            Err(e) => println!("no-proc-lane {test}: FAIL {e}"),
        }
        std::process::exit(0);
    }
    if !userns() {
        skip_note::skipped(&format!("skipped: {test}: no unprivileged user and mount namespaces here (`unshare -r -m` with a tmpfs over /proc failed); the no-/proc lane did not run"));
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let o = Command::new("unshare")
        .args(["-r", "-m", "sh", "-c", "mount -t tmpfs none /proc && mount -t tmpfs none /sys && exec \"$0\" \"$@\""])
        .arg(exe)
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(CASE, test)
        .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success() && out.contains(&format!("no-proc-lane {test}: ok")), "{test} without /proc:\n{out}{}", String::from_utf8_lossy(&o.stderr));
}

/// `want == got`, else a message naming `what`.
fn check<T: PartialEq + std::fmt::Debug>(what: &str, got: T, want: T) -> Result<(), String> {
    if got == want { Ok(()) } else { Err(format!("{what}: got {got:?}, want {want:?}")) }
}

/// A C string's text in `buf`.
fn text(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The processors the affinity mask allows.
fn affinity_count() -> i64 {
    let mut mask = [0u8; 128];
    assert_eq!(sy::sched_getaffinity(0, mask.len(), mask.as_mut_ptr().cast()), 0, "sched_getaffinity");
    mask.iter().map(|b| i64::from(b.count_ones())).sum()
}

#[test]
fn pthread_getattr_np_initial_thread() {
    // s22_2_c_4, D-X.12: the stack of the initial (or a host-made) thread comes from
    // /proc/self/maps; without it the read's ENOENT, as glibc (it was ENOMEM).
    let mut a = std::mem::MaybeUninit::<pthread_attr_t>::zeroed();
    if std::env::var(CASE).is_err() {
        // With /proc the same call succeeds, so the lane's ENOENT is the fallback, not a refusal.
        assert_eq!(th::pthread_getattr_np(th::pthread_self(), a.as_mut_ptr()), 0, "with /proc");
        th::pthread_attr_destroy(a.as_mut_ptr());
    }
    without_proc("pthread_getattr_np_initial_thread", &[], || {
        let r = th::pthread_getattr_np(th::pthread_self(), a.as_mut_ptr());
        th::pthread_attr_destroy(a.as_mut_ptr());
        check("pthread_getattr_np(self)", r, ENOENT)
    });
}

#[test]
fn pidfd_getpid_without_proc() {
    // s18_5_c_4, D-18.7, D-X.12: an open pidfd cannot be asked without /proc (ENOSYS); a
    // descriptor that is not open is EBADF.
    without_proc("pidfd_getpid_without_proc", &[], || {
        let fd = pr::pidfd_open(pr::getpid(), 0);
        check("pidfd_open", fd >= 0, true)?;
        let r = (pr::pidfd_getpid(fd), errno::get());
        lo::close(fd);
        check("pidfd_getpid(open pidfd)", r, (-1, ENOSYS))?;
        check("pidfd_getpid(closed)", (pr::pidfd_getpid(fd), errno::get()), (-1, EBADF))?;
        check("pidfd_getpid(-1)", (pr::pidfd_getpid(-1), errno::get()), (-1, EBADF))
    });
}

#[test]
fn processor_counts_without_proc() {
    // s24_4_c_4, D-X.12: without /sys and /proc both counts are the affinity mask's, at least 1
    // (as glibc); sysconf answers the same.
    without_proc("processor_counts_without_proc", &[], || {
        let n = affinity_count().max(1);
        check("get_nprocs", i64::from(sy::get_nprocs()), n)?;
        check("get_nprocs_conf", i64::from(sy::get_nprocs_conf()), n)?;
        check("sysconf(_SC_NPROCESSORS_ONLN)", sy::sysconf(84), n)?;
        check("sysconf(_SC_NPROCESSORS_CONF)", sy::sysconf(83), n)
    });
}

#[test]
fn ngroups_max_without_proc() {
    // s24_4_c_1_2, D-X.12: /proc/sys/kernel/ngroups_max unreadable is the kernel's fixed 65536.
    without_proc("ngroups_max_without_proc", &[], || check("sysconf(_SC_NGROUPS_MAX)", sy::sysconf(3), 65536));
}

#[test]
fn link_max_without_proc() {
    // s24_4_c_2, D-X.12: on the ext2/3/4 family (one magic number) ext4 is told apart only by
    // /proc/self/mountinfo; without it 32000, the family's smallest limit. Elsewhere the answer
    // comes from the magic number alone and must not change.
    let dir = env!("CARGO_MANIFEST_DIR");
    let mut env = Vec::new();
    if std::env::var(CASE).is_err() {
        let c = CString::new(dir).unwrap();
        let ext = Command::new("stat").args(["-f", "-c", "%t", dir]).output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "ef53").unwrap_or(false);
        env.push(("LIELLA_NPL_LINK_MAX", if ext { "32000".to_string() } else { sy::pathconf(c.as_ptr(), 0).to_string() }));
    }
    without_proc("link_max_without_proc", &env, || {
        let want: i64 = std::env::var("LIELLA_NPL_LINK_MAX").unwrap().parse().unwrap();
        let c = CString::new(dir).unwrap();
        check("pathconf(_PC_LINK_MAX)", sy::pathconf(c.as_ptr(), 0), want)
    });
}

#[test]
fn getloadavg_without_proc() {
    // s24_6_c_3, D-X.12: /proc/loadavg unreadable is an error (-1), never invented averages.
    without_proc("getloadavg_without_proc", &[], || {
        let mut v = [7.5f64; 3];
        check("getloadavg", sy::getloadavg(v.as_mut_ptr(), 3), -1)?;
        check("averages untouched", v, [7.5; 3])
    });
}

static GO: AtomicBool = AtomicBool::new(false);

extern "C" fn waiter(_: *mut c_void) -> *mut c_void {
    while !GO.load(Ordering::Acquire) {
        std::thread::yield_now();
    }
    std::ptr::null_mut()
}

#[test]
fn thread_names_without_proc() {
    // D-X.12 (pthread_setname_np/getname_np: no clause ID, §22 thread names): another thread's
    // name lives in /proc/self/task/<tid>/comm, so without /proc both are ENOENT (as glibc); the
    // calling thread's own name uses prctl and still works.
    without_proc("thread_names_without_proc", &[], || {
        let mut t = pthread_t(0);
        let start: extern "C" fn(*mut c_void) -> *mut c_void = waiter;
        check("pthread_create", th::pthread_create(Some(&mut t), std::ptr::null(), start as usize, std::ptr::null_mut()), 0)?;
        let mut buf = [0 as c_char; 16];
        let name = CString::new("lane").unwrap();
        let got = (th::pthread_getname_np(t, buf.as_mut_ptr(), buf.len()), th::pthread_setname_np(t, name.as_ptr()));
        let own = (th::pthread_setname_np(th::pthread_self(), name.as_ptr()), th::pthread_getname_np(th::pthread_self(), buf.as_mut_ptr(), buf.len()));
        GO.store(true, Ordering::Release);
        th::pthread_join(t, None);
        check("another thread's getname/setname", got, (ENOENT, ENOENT))?;
        check("own setname/getname", (own, text(&buf)), ((0, 0), "lane".to_string()))
    });
}

/// A pty pair from `openpty`, and the slave's name from `ptsname_r`.
fn pty() -> Result<(i32, i32, String), String> {
    let (mut m, mut s) = (-1, -1);
    check("openpty", tm::openpty(Some(&mut m), Some(&mut s), std::ptr::null_mut(), None, None), 0)?;
    let mut b = [0 as c_char; 64];
    check("ptsname_r", tm::ptsname_r(m, b.as_mut_ptr(), b.len()), 0)?;
    Ok((m, s, text(&b)))
}

#[test]
fn ttyname_without_proc() {
    // s19_3_c_0, D-19.3, D-X.12: without /proc/self/fd the name the device number gives, checked
    // to be this very terminal; a descriptor that is not a terminal is still ENOTTY.
    without_proc("ttyname_without_proc", &[], || {
        let (m, s, name) = pty()?;
        let mut b = [0 as c_char; 64];
        let r = tm::ttyname_r(s, b.as_mut_ptr(), b.len());
        let mut p = [-1; 2];
        check("pipe", ipc::pipe(Some(&mut p)), 0)?;
        let r2 = tm::ttyname_r(p[0], b.as_mut_ptr().wrapping_add(32), 32);
        for fd in [m, s, p[0], p[1]] {
            lo::close(fd);
        }
        check("ttyname_r(slave)", (r, text(&b)), (0, name))?;
        check("ttyname_r(pipe)", r2, ENOTTY)
    });
}

#[test]
fn tty_line_without_proc() {
    // D-X.12 (no spec clause: the accounting functions' terminal line, liella_legacy::accounting::
    // tty_line, behind getlogin, ttyslot and login): the device number's name without /proc.
    without_proc("tty_line_without_proc", &[], || {
        let (m, s, name) = pty()?;
        let mut out = [0u8; 256];
        let got = liella_legacy::accounting::tty_line(&CSys, Fd::new(s), &mut out).map(|n| String::from_utf8_lossy(&out[..n]).into_owned());
        lo::close(m);
        lo::close(s);
        check("tty_line(slave)", got, Some(name.strip_prefix("/dev/").unwrap_or(&name).to_string()))
    });
}

#[test]
fn fexecve_without_proc() {
    // s18_3_c_4, D-X.12: fexecve runs the open file through execveat(AT_EMPTY_PATH), with no
    // /proc/self/fd path, so an ELF program still runs (here /bin/sh, exit status 7).
    without_proc("fexecve_without_proc", &[], || {
        let args: Vec<CString> = ["sh", "-c", "exit 7"].iter().map(|a| CString::new(*a).unwrap()).collect();
        let mut argv: Vec<*mut c_char> = args.iter().map(|a| a.as_ptr().cast_mut()).collect();
        argv.push(std::ptr::null_mut());
        let envp: [*mut c_char; 1] = [std::ptr::null_mut()];
        let file = std::fs::File::open("/bin/sh").map_err(|e| format!("open /bin/sh: {e}"))?;
        let fd = std::os::fd::AsRawFd::as_raw_fd(&file);
        let pid = pr::fork();
        if pid == 0 {
            pr::fexecve(fd, argv.as_ptr(), envp.as_ptr());
            s01::_exit(100 + errno::get());
        }
        let mut status = 0;
        check("waitpid", pr::waitpid(pid, Some(&mut status), 0), pid)?;
        check("exit status (100 + errno: fexecve failed)", (status >> 8) & 0xff, 7)
    });
}

#[test]
fn group_member_without_proc() {
    // s17_1_c_0, D-X.12: up to 64 supplementary groups are asked with getgroups, so membership
    // needs no /proc: each listed group and the effective group are members, another is not.
    without_proc("group_member_without_proc", &[], || {
        let mut g = [0u32; 64];
        let n = us::getgroups(64, g.as_mut_ptr());
        check("getgroups (at most 64 here)", (0..=64).contains(&n), true)?;
        for &gid in g[..n as usize].iter().chain([us::getegid()].iter()) {
            check(&format!("group_member({gid})"), us::group_member(gid), 1)?;
        }
        let other = (1u32..).map(|k| 0x7fff_0000 + k).find(|x| !g[..n as usize].contains(x) && *x != us::getegid()).unwrap();
        check("group_member(not a group)", us::group_member(other), 0)
    });
}

#[test]
fn closefrom_without_proc() {
    // s11_2_c_0, D-X.12: closefrom closes with close_range; /proc/self/fd is only its fallback
    // where close_range fails, so without /proc every descriptor from `lo` up is closed.
    without_proc("closefrom_without_proc", &[], || {
        let mut p = [-1; 2];
        check("pipe", ipc::pipe(Some(&mut p)), 0)?;
        check("dup2", (lo::dup2(p[0], 700), lo::dup2(p[1], 701), lo::dup2(p[0], 950)), (700, 701, 950))?;
        lo::closefrom(701);
        let open = |fd: i32| lo::dup2(fd, fd) == fd;
        let seen = (open(700), open(701), open(950));
        for fd in [p[0], p[1], 700] {
            lo::close(fd);
        }
        check("open after closefrom(701): 700, 701, 950", seen, (true, false, false))
    });
}

#[test]
fn lchmod_without_proc() {
    // s13_7_c_1, D-13.9, D-X.12: Linux 6.6 and later change a file's own mode with fchmodat2
    // (no /proc); before 6.6 the /proc fallback cannot work and the answer is EOPNOTSUPP. A
    // symlink has no mode: EOPNOTSUPP on every kernel, and its target is untouched.
    let mut env = Vec::new();
    if std::env::var(CASE).is_err() {
        let r = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
        let mut v = r.trim().split(['.', '-']).map(|p| p.parse::<u32>().unwrap_or(0));
        env.push(("LIELLA_NPL_FCHMODAT2", ((v.next().unwrap_or(0), v.next().unwrap_or(0)) >= (6, 6)).to_string()));
    }
    without_proc("lchmod_without_proc", &env, || {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("no_proc_lane_lchmod_{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let (f, l) = (dir.join("f"), dir.join("l"));
        std::fs::write(&f, b"x").map_err(|e| e.to_string())?;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
        std::os::unix::fs::symlink("f", &l).map_err(|e| e.to_string())?;
        let cf = CString::new(f.to_str().unwrap()).unwrap();
        let cl = CString::new(l.to_str().unwrap()).unwrap();
        let file = (fs::lchmod(cf.as_ptr(), 0o600), errno::get());
        let mode_f = std::fs::metadata(&f).map(|m| m.permissions().mode() & 0o777).unwrap_or(0);
        let link = (fs::lchmod(cl.as_ptr(), 0o640), errno::get());
        let mode_after = std::fs::metadata(&f).map(|m| m.permissions().mode() & 0o777).unwrap_or(0);
        let _ = std::fs::remove_dir_all(&dir);
        if std::env::var("LIELLA_NPL_FCHMODAT2").unwrap() == "true" {
            check("lchmod(file) (fchmodat2)", (file.0, mode_f), (0, 0o600))?;
        } else {
            check("lchmod(file) (before 6.6)", (file, mode_f), ((-1, EOPNOTSUPP), 0o644))?;
        }
        check("lchmod(symlink)", link, (-1, EOPNOTSUPP))?;
        check("target untouched by the symlink's lchmod", mode_after, mode_f)
    });
}

#[test]
fn getauxval_hosted_without_proc() {
    // s1_5_c_0, D-X.12: a process the library did not start (this test harness) reads the
    // auxiliary vector from /proc/self/auxv; without it the entry is not found (0, ENOENT),
    // never a wrong value. (A program Liella starts keeps the vector from its entry.)
    let mut env = Vec::new();
    if std::env::var(CASE).is_err() {
        env.push(("LIELLA_NPL_PAGESZ", s01::getauxval(AT_PAGESZ).to_string()));
    }
    without_proc("getauxval_hosted_without_proc", &env, || {
        let want: u64 = std::env::var("LIELLA_NPL_PAGESZ").unwrap().parse().unwrap();
        lo::close(-1); // errno = EBADF, so a stale ENOENT cannot pass
        let got = (s01::getauxval(AT_PAGESZ), errno::get());
        if got.0 == want && want != 0 { Ok(()) } else { check("getauxval(AT_PAGESZ)", got, (0, ENOENT)) }
    });
}

#[test]
fn null_fold_guard_without_proc() {
    // D-4.22, D-X.12 (no spec clause: an internal dispatch choice): NULL folding in the copy
    // entries needs vm.mmap_min_addr of at least 64 KiB; unreadable (no /proc) is refused and
    // the explicit checks stay.
    without_proc("null_fold_guard_without_proc", &[], || {
        let on = liella_boundary::selftest::set_null_fold(true);
        liella_boundary::selftest::set_null_fold(false);
        check("set_null_fold(true) without /proc", on, false)
    });
}
