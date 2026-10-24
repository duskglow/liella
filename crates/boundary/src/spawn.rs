//! Program execution and process creation for C callers (spec §18.3–§18.8; D-18.1 to D-18.5):
//! the exec forms, spawning, the shell interfaces (`system`, `popen`), `daemon`, the waits and
//! `pidfd_getpid`. The decisions are `liella::process`'s; this module supplies the C caller's
//! arrays, the process-wide state and the system calls on addresses (`crate::raw`).
//!
//! **Spawning** (D-18.3). Spawn is the library's one way to start a program (D-18.1): the
//! caller's signals are all blocked across the creation, and the child is created with
//! `clone3(CLONE_VM | CLONE_VFORK | CLONE_CLEAR_SIGHAND)` (`raw::spawn_clone_vm`). It shares the
//! caller's memory, so the kernel copies no page tables (the cost of `fork`, which grows with the
//! caller), runs on a private stack (128 KiB mapped for the call), and the caller is suspended
//! until the child executes its program or exits. The child must not touch what the caller
//! relies on, and does not: `child_main` runs `liella::process::run_child` over the plain
//! system calls (no allocation, no lock, no stdio, no thread-local write, no signal handler:
//! handlers are cleared at creation and everything is blocked), only reads the context on the
//! caller's stack, and on a failure stores the error in a word of that context and exits with
//! 127. No fork handler or cancellation runs in it (POSIX). The caller then restores its mask
//! and unmaps the stack; a stored error is the call's result, and the child is reaped at once,
//! so none is left behind. No descriptor is involved, so no file action can collide with the
//! error channel. A pidfd that `pidfd_spawn` asked for is closed when the spawn fails. Where
//! `clone3` is refused (`ENOSYS`) the child instead gets a copy-on-write copy of the memory
//! (`CLONE_VFORK` alone) and reports through a shared anonymous word. User-visible `vfork`
//! (D-18.1) shares the memory too.
//!
//! **`system`** (D-18.5) ignores `SIGINT` and `SIGQUIT` for as long as any thread is in it: the
//! first caller saves the dispositions and ignores the signals, the last one out restores
//! them. Each caller blocks `SIGCHLD` in its own thread only. The shell gets the caller's
//! original mask, and `SIGINT`/`SIGQUIT` at their default action unless they were ignored
//! before. A shell that cannot be executed is status 127 (`_exit(127)`'s word); `errno` is left
//! as it was. Cancellation while it waits kills and reaps the shell, then acts.
//!
//! **`popen`** streams are recorded (stream, PID, descriptor) in a list of heap nodes behind
//! one lock, held for the whole of `popen`, so a child never inherits another popen stream's
//! pipe (POSIX): its file actions close them. `pclose` of a stream not in the list closes it
//! and fails with `ECHILD` (there is no child whose status it could report).

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering::{Acquire, Relaxed, Release, SeqCst}};

use liella::mem::TryVec;
use liella::process::{self as lp, Child, FileAction, Program, SpawnAttr};
use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::mem::PAGE_SIZE;
use liella_kernel::ops::FcntlInt;
use liella_kernel::path::KStr;
use liella_kernel::sys::{Disposition, Sys};
use liella_kernel::uapi::{KSigAction, SigSet, MAP_ANONYMOUS, MAP_PRIVATE, MAP_SHARED, MAP_STACK, O_CLOEXEC, O_RDONLY, O_RDWR, PROT_READ, PROT_WRITE, RLIMIT_NOFILE, SIGCHLD, SIGINT, SIGKILL, SIGQUIT};
use liella_legacy::spawn_compat::CFileActions;
use liella_rt::futex::Scope;
use liella_rt::sync::RawMutex;

use crate::raw::{self, Spawned};
use crate::sys::{CSys, CP};
use crate::{caller_alloc, conv, ffi, heap, process, stdio, strz, tb};

const SIG_BLOCK: u32 = 0;
const SIG_SETMASK: u32 = 2;
/// The shell (§18.3 fallback, §18.7), by absolute path whatever `PATH` says.
const SH: &[u8] = b"/bin/sh\0";
/// The stack of a spawn child that shares the caller's memory (D-18.3): address space until
/// touched; `run_child` needs a few KiB (a `PATH_MAX` candidate buffer and the call frames).
const CHILD_STACK: usize = 128 * 1024;

/// One of this module's terminated literals.
fn kstr(b: &'static [u8]) -> KStr<'static> {
    match KStr::from_nul_terminated(b) {
        Ok(k) => k,
        Err(e) => crate::ret::fatal(e),
    }
}

fn addr_of(b: &[u8]) -> usize {
    b.as_ptr().expose_provenance()
}

/// The current `environ` array.
pub fn environ() -> usize {
    ffi::environ.load(Acquire)
}

/// The caller's `PATH` (`None` when unset).
fn caller_path() -> Option<&'static [u8]> {
    process::getenv(b"PATH").map(strz::env_value)
}

// ------------------------------------------------------------------------------ exec (§18.3)

/// `execve(path, argv, envp)`: the error (it returns only on failure).
pub fn execve(path: KResult<KStr<'_>>, argv: usize, envp: usize) -> Errno {
    match path {
        Ok(p) => raw::execve(p, argv, envp),
        Err(e) => e,
    }
}

/// The `p` forms (`execvp`, `execvpe`, `execlp`): search the caller's `PATH` (never `envp`'s,
/// exec(3)), run a file the kernel refuses with `ENOEXEC` with the shell. The error.
pub fn execvpe(name: KResult<KStr<'_>>, argv: usize, envp: usize) -> Errno {
    let name = match name {
        Ok(n) => n,
        Err(e) => return e,
    };
    let mut exec = |p: KStr<'_>| raw::execve(p, argv, envp);
    let mut shell = |p: KStr<'_>| shell_fallback(p, argv, envp);
    lp::search(name, caller_path(), &mut exec, Some(&mut shell))
}

/// Entries of the caller's `argv` before its NULL.
fn count(argv: usize) -> usize {
    let mut n = 0usize;
    while conv::exec_array_entry(argv, n) != 0 {
        n = n.saturating_add(1);
    }
    n
}

/// Run `file` with the shell: `/bin/sh file argv[1]...` (the caller's `argv[0]` is dropped).
fn shell_fallback(file: KStr<'_>, argv: usize, envp: usize) -> Errno {
    let n = count(argv);
    let mut words = Words::new();
    let pushed = words.push(addr_of(SH)).and_then(|()| words.push(addr_of(file.with_nul()))).and_then(|()| (1..n).try_for_each(|i| words.push(conv::exec_array_entry(argv, i)))).and_then(|()| words.push(0));
    match pushed {
        Ok(()) => raw::execve(kstr(SH), words.addr(), envp),
        Err(e) => e,
    }
}

/// A NULL-terminated pointer array being built: on the stack up to 512 entries (the exec
/// forms are async-signal-safe there, exec(3)), on the heap beyond.
struct Words {
    small: [usize; 512],
    n: usize,
    big: TryVec<usize>,
}

impl Words {
    fn new() -> Words {
        Words { small: [0; 512], n: 0, big: TryVec::new() }
    }

    fn push(&mut self, w: usize) -> KResult<()> {
        if self.big.is_empty() {
            if let Some(slot) = self.small.get_mut(self.n) {
                *slot = w;
                self.n = self.n.saturating_add(1);
                return Ok(());
            }
            self.big.extend_from_slice(&self.small)?;
        }
        self.big.push(w)
    }

    fn addr(&self) -> usize {
        if self.big.is_empty() { self.small.as_ptr().expose_provenance() } else { self.big.as_ptr().expose_provenance() }
    }
}

/// Which `l` form: `execl`, `execle` (the environment follows the list's NULL), `execlp`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ListForm {
    /// `execl`.
    Plain,
    /// `execle`.
    Env,
    /// `execlp`.
    Search,
}

/// The `l` forms: `arg0` and the rest of the variadic list (read from the `va_list` at `ap`
/// up to its NULL) become the argument array. The error.
pub fn execl(form: ListForm, path: KResult<KStr<'_>>, arg0: usize, ap: usize) -> Errno {
    let mut words = Words::new();
    let mut a = arg0;
    loop {
        if let Err(e) = words.push(a) {
            return e;
        }
        if a == 0 {
            break;
        }
        // One argument per call, up to the list's NULL (the obligation of `liella_va_long`).
        a = usize::from_ne_bytes(ffi::liella_va_long(ap).to_ne_bytes());
    }
    match form {
        ListForm::Plain => execve(path, words.addr(), environ()),
        ListForm::Env => execve(path, words.addr(), usize::from_ne_bytes(ffi::liella_va_long(ap).to_ne_bytes())),
        ListForm::Search => execvpe(path, words.addr(), environ()),
    }
}

/// `fexecve(fd, argv, envp)`: `execveat(fd, "", AT_EMPTY_PATH)`. A negative descriptor or a
/// NULL array is `EINVAL` (fexecve(3)).
pub fn fexecve(fd: i32, argv: usize, envp: usize) -> Errno {
    if fd < 0 || argv == 0 || envp == 0 {
        return Errno::EINVAL;
    }
    let flags = i32::try_from(liella_kernel::uapi::AT_EMPTY_PATH).unwrap_or(0);
    raw::execveat(fd, kstr(b"\0"), argv, envp, flags)
}

// ------------------------------------------------------------------------------ spawn (§18.4)

/// A spawn to make.
pub struct Request<'a> {
    /// The program.
    pub program: Program<'a>,
    /// The encoded file actions.
    pub actions: &'a [u8],
    /// The attributes (`None`: no attributes object).
    pub attr: Option<SpawnAttr>,
    /// The caller's `argv` array.
    pub argv: usize,
    /// The caller's `envp` array.
    pub envp: usize,
    /// Return a pidfd for the child (`pidfd_spawn`).
    pub pidfd: bool,
}

/// Where a spawn failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// No child was created.
    Create(Errno),
    /// The child failed before its program ran (it has been reaped).
    Child(Errno),
}

impl Failure {
    /// The error number.
    pub fn errno(self) -> Errno {
        match self {
            Failure::Create(e) | Failure::Child(e) => e,
        }
    }
}

/// What a spawn child needs, on the suspended caller's stack (see the module documentation).
pub(crate) struct ChildCtx<'a> {
    child: Child<'a>,
    argv: usize,
    envp: usize,
    /// The child's error number, 0 while there is none: the child writes it, the caller reads it
    /// once it has resumed.
    error: &'a core::sync::atomic::AtomicU32,
}

/// What the spawn child does (the attributes, the file actions, the mask, the exec); it ends the
/// child either way. Nothing here allocates, locks, writes thread-local state or touches stdio:
/// it runs `liella::process::run_child` over the plain system calls (`CSys`) and the context.
pub(crate) fn child_main(ctx: &ChildCtx<'_>) -> ! {
    let (argv, envp) = (ctx.argv, ctx.envp);
    let e = lp::run_child(&CSys, &ctx.child, &mut |p: KStr<'_>| raw::execve(p, argv, envp));
    ctx.error.store(u32::try_from(e.get()).unwrap_or(1), SeqCst);
    CSys.exit_group(127)
}

/// Spawn (see the module documentation): the child's PID and, for `pidfd`, its pidfd.
pub fn spawn(req: &Request<'_>) -> Result<(i32, Option<Fd>), Failure> {
    let attr = req.attr.unwrap_or_default();
    let old = CSys.sigprocmask(SIG_SETMASK, Some(&SigSet(u64::MAX))).map_err(Failure::Create)?;
    let mask = if attr.flags & lp::SPAWN_SETSIGMASK != 0 { attr.sigmask } else { old };
    let cgroup = (attr.flags & lp::SPAWN_SETCGROUP != 0).then_some(attr.cgroup);
    let r = match spawn_shared(req, attr, mask, cgroup) {
        Err(Failure::Create(Errno::ENOSYS)) => spawn_copy(req, attr, mask, cgroup),
        r => r,
    };
    let _ = CSys.sigprocmask(SIG_SETMASK, Some(&old));
    r.and_then(|(pid, fd, error)| settle(pid, fd, error))
}

/// The child shares the caller's memory (`CLONE_VM`) and runs on a stack of its own; its error
/// word is a local of ours. `ENOSYS` (no `clone3`) is for [`spawn_copy`] to answer.
fn spawn_shared(req: &Request<'_>, attr: SpawnAttr, mask: SigSet, cgroup: Option<i32>) -> Result<(i32, i32, u32), Failure> {
    let stack = CSys.mmap(0, CHILD_STACK, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_STACK, None, 0).map_err(Failure::Create)?;
    let error = core::sync::atomic::AtomicU32::new(0);
    let child = Child { attr, actions: req.actions, mask, reset_handlers: false, program: req.program };
    let ctx = ChildCtx { child, argv: req.argv, envp: req.envp, error: &error };
    let created = raw::spawn_clone_vm(req.pidfd, cgroup, stack.addr(), stack.len(), conv::spawn_entry, core::ptr::from_ref(&ctx).expose_provenance());
    // The caller resumed: the child has executed its program or exited, and left the stack.
    let _ = CSys.munmap(stack);
    let (pid, fd) = created.map_err(Failure::Create)?;
    Ok((pid, fd, error.load(SeqCst)))
}

/// The child has a copy-on-write copy of the caller's memory (no `CLONE_VM`; the `ENOSYS` fallback),
/// and reports its error through a shared anonymous word.
fn spawn_copy(req: &Request<'_>, attr: SpawnAttr, mask: SigSet, cgroup: Option<i32>) -> Result<(i32, i32, u32), Failure> {
    let region = CSys.mmap(0, PAGE_SIZE, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, None, 0).map_err(Failure::Create)?;
    let r = match CSys.region_words(&region).and_then(|w| w.first()) {
        Some(word) => spawn_copy_with(req, attr, mask, cgroup, word),
        None => Err(Failure::Create(Errno::ENOMEM)),
    };
    let _ = CSys.munmap(region);
    r
}

fn spawn_copy_with(req: &Request<'_>, attr: SpawnAttr, mask: SigSet, cgroup: Option<i32>, word: &core::sync::atomic::AtomicU32) -> Result<(i32, i32, u32), Failure> {
    word.store(0, SeqCst);
    let created = raw::spawn_clone(req.pidfd, cgroup);
    if let Ok(Spawned::Child(cleared)) = created {
        let child = Child { attr, actions: req.actions, mask, reset_handlers: !cleared, program: req.program };
        child_main(&ChildCtx { child, argv: req.argv, envp: req.envp, error: word });
    }
    match created {
        Ok(Spawned::Parent(pid, fd)) => Ok((pid, fd, word.load(SeqCst))),
        Ok(Spawned::Child(_)) => Err(Failure::Create(Errno::EINVAL)),
        Err(e) => Err(Failure::Create(e)),
    }
}

/// The caller's side once the child has executed or exited: its PID and pidfd, or its error
/// (the child is reaped and a pidfd closed then).
fn settle(pid: i32, fd: i32, e: u32) -> Result<(i32, Option<Fd>), Failure> {
    let pidfd = (fd >= 0).then(|| Fd::new(fd));
    if e == 0 {
        return Ok((pid, pidfd));
    }
    if let Some(fd) = pidfd {
        let _ = CSys.close(fd);
    }
    reap(pid);
    Err(Failure::Child(i32::try_from(e).ok().and_then(Errno::new).unwrap_or(Errno::EINVAL)))
}

/// Wait for a child that has exited or is about to (a spawn child that reported an error, a
/// shell killed at cancellation), ignoring signals; `ECHILD` (the system reaped it) is fine.
fn reap(pid: i32) {
    while CSys.wait4(pid, 0, None) == Err(Errno::EINTR) {}
}

/// `posix_spawn`/`posix_spawnp`/`pidfd_spawn(p)` for a C caller: the actions of `fa`, the
/// attributes of `attr`. The PID (or pidfd) and an error number.
pub fn spawn_c(program: Program<'_>, fa: Option<&CFileActions>, attr: Option<SpawnAttr>, argv: usize, envp: usize, pidfd: bool) -> KResult<i32> {
    let actions = fa.map_or(Ok(&[][..]), action_bytes)?;
    let r = spawn(&Request { program, actions, attr, argv, envp, pidfd }).map_err(Failure::errno)?;
    Ok(match r {
        (_, Some(fd)) => fd.raw(),
        (pid, None) => pid,
    })
}

/// `posix_spawnp`'s program: `name` searched in the caller's `PATH`.
pub fn search_program(name: KStr<'_>) -> Program<'_> {
    Program::Search { name, path: caller_path() }
}

// ------------------------------------------------------------------------------ file actions (§18.4)

/// A spawn object or value argument (`posix_spawnattr_t *`, `posix_spawn_file_actions_t *`, a
/// set or parameter to store): NULL is `EINVAL`, the error number POSIX gives for an invalid
/// object.
pub fn object<T>(o: Option<T>) -> KResult<T> {
    o.ok_or(Errno::EINVAL)
}

/// The encoded actions of a file-actions object.
fn action_bytes(fa: &CFileActions) -> KResult<&'static [u8]> {
    match fa.block() {
        (0, _) | (_, 0) => Ok(&[]),
        (addr, len) => conv::own_bytes(addr, len).map(|b| &*b).ok_or(Errno::EINVAL),
    }
}

/// A descriptor an action names: `EBADF` when negative or not below the descriptor limit
/// (`RLIMIT_NOFILE`, posix_spawn_file_actions_addopen(3p)), checked when the action is added.
fn check_fd(fd: i32) -> KResult<()> {
    let limit = CSys.prlimit(0, RLIMIT_NOFILE, None).map_or(u64::MAX, |l| l.cur);
    match u64::try_from(fd) {
        Ok(v) if v < limit => Ok(()),
        _ => Err(Errno::EBADF),
    }
}

/// `posix_spawn_file_actions_init`.
pub fn actions_init(fa: Option<&mut CFileActions>) -> KResult<()> {
    *object(fa)? = CFileActions::new();
    Ok(())
}

/// `posix_spawn_file_actions_destroy`: the list's block is released.
pub fn actions_destroy(fa: Option<&mut CFileActions>) -> KResult<()> {
    let fa = object(fa)?;
    // The block came from `caller_alloc` (`add_action`) and is not used afterwards.
    caller_alloc::release(fa.block().0);
    *fa = CFileActions::new();
    Ok(())
}

/// Append `action` to the list (`posix_spawn_file_actions_add*`): every descriptor it names is
/// checked first ([`check_fd`]); `ENOMEM` leaves the list as it was.
pub fn add_action(fa: Option<&mut CFileActions>, action: FileAction<'_>) -> KResult<()> {
    let fa = object(fa)?;
    match action {
        FileAction::Open { fd, .. } | FileAction::Close(fd) | FileAction::Fchdir(fd) | FileAction::Closefrom(fd) | FileAction::Tcsetpgrp(fd) => check_fd(fd)?,
        FileAction::Dup2(fd, newfd) => {
            check_fd(fd)?;
            check_fd(newfd)?;
        }
        FileAction::Chdir(_) => {}
    }
    let mut list = TryVec::from_slice(action_bytes(fa)?)?;
    lp::encode(&action, &mut list)?;
    let block = caller_alloc::dup_bytes(&list)?;
    // The old block came from `caller_alloc` and nothing refers to it any more.
    caller_alloc::release(fa.block().0);
    fa.set_block(block, list.len());
    Ok(())
}

// ------------------------------------------------------------------------------ waiting (§18.6)

/// `wait4(pid, status, options, usage)` (and `wait`, `waitpid`, `wait3`): a cancellation point.
/// `*status` is written only when a child is reported (`WNOHANG` with none leaves it).
pub fn wait4(pid: i32, status: Option<&mut i32>, options: i32, usage: Option<&mut liella_kernel::uapi::Rusage>) -> KResult<i32> {
    let (p, st) = CP.wait4(pid, conv::bits(options), usage)?;
    if p > 0 {
        if let Some(s) = status {
            *s = st;
        }
    }
    Ok(p)
}

/// `waitid(idtype, id, info, options)`: a cancellation point.
pub fn waitid(idtype: i32, id: u32, info: Option<&mut liella_kernel::uapi::SigInfo>, options: i32) -> KResult<()> {
    let mut local = liella_kernel::uapi::SigInfo::default();
    CP.waitid(conv::bits(idtype), i32::from_ne_bytes(id.to_ne_bytes()), info.unwrap_or(&mut local), conv::bits(options), None)
}

// ------------------------------------------------------------------------------ pidfd (§18.5)

/// `pidfd_getpid(fd)`: the `Pid:` line of `/proc/self/fdinfo/<fd>`. `EBADF` for a descriptor
/// that is not open or not a pidfd, `ESRCH` once the process has been reaped, `EREMOTE` when it
/// is outside the caller's PID namespace; `ENOSYS` for an open descriptor without `/proc`
/// (OD-5: the kernel has no other way to tell before Linux 6.13).
pub fn pidfd_getpid(fd: i32) -> KResult<i32> {
    let n = u32::try_from(fd).map_err(|_| Errno::EBADF)?;
    let mut path = [0u8; 40];
    let prefix = b"/proc/self/fdinfo/";
    let mut len = prefix.len();
    path.get_mut(..len).ok_or(Errno::EBADF)?.copy_from_slice(prefix);
    let mut digits = [0u8; 10];
    let mut k = digits.len();
    let mut v = n;
    loop {
        k = k.saturating_sub(1);
        if let Some(d) = digits.get_mut(k) {
            *d = b'0'.saturating_add(u8::try_from(v % 10).unwrap_or(0));
        }
        v /= 10;
        if v == 0 {
            break;
        }
    }
    for &d in digits.get(k..).unwrap_or(&[]) {
        if let Some(slot) = path.get_mut(len) {
            *slot = d;
        }
        len = len.saturating_add(1);
    }
    let p = KStr::from_nul_terminated(path.get(..=len).ok_or(Errno::EBADF)?)?;
    // ENOENT: the descriptor is not open, or there is no `/proc` (OD-5); an open descriptor
    // then cannot be asked, which is ENOSYS rather than a wrong EBADF.
    let file = CSys.openat(Fd::CWD, p, O_RDONLY | O_CLOEXEC, 0).map_err(|e| match e {
        Errno::ENOENT if CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0).is_ok() => Errno::ENOSYS,
        Errno::ENOENT => Errno::EBADF,
        e => e,
    })?;
    let mut text = [0u8; 2048];
    let mut got = 0usize;
    let read = loop {
        match text.get_mut(got..).map(|rest| CSys.read(file, rest)) {
            Some(Ok(0)) | None => break Ok(()),
            Some(Ok(k)) => got = got.saturating_add(k),
            Some(Err(Errno::EINTR)) => {}
            Some(Err(e)) => break Err(e),
        }
        if got >= text.len() {
            break Ok(());
        }
    };
    let _ = CSys.close(file);
    read?;
    match lp::fdinfo_pid(text.get(..got).unwrap_or(&[])) {
        None => Err(Errno::EBADF),
        Some(-1) => Err(Errno::ESRCH),
        Some(0) => Err(Errno::EREMOTE),
        Some(pid) => i32::try_from(pid).map_err(|_| Errno::EBADF),
    }
}

// ------------------------------------------------------------------------------ system (§18.7)

/// Serializes `system`'s shared state.
static SHELL_LOCK: RawMutex = RawMutex::new();
/// Threads inside `system`.
static SHELL_USERS: AtomicUsize = AtomicUsize::new(0);
/// The saved `SIGINT` and `SIGQUIT` actions (handler, flags, restorer, mask each).
static SHELL_SAVED: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

fn save(i: usize, a: &KSigAction) {
    let word = |v: usize| u64::try_from(v).unwrap_or(0);
    let w = [word(a.handler), a.flags, word(a.restorer), a.mask.0];
    for (slot, v) in SHELL_SAVED.iter().skip(i.saturating_mul(4)).zip(w) {
        slot.store(v, Relaxed);
    }
}

fn saved(i: usize) -> KSigAction {
    let w: [u64; 4] = core::array::from_fn(|k| SHELL_SAVED.get(i.saturating_mul(4).saturating_add(k)).map_or(0, |s| s.load(Relaxed)));
    let addr = |v: u64| usize::try_from(v).unwrap_or(0);
    KSigAction { handler: addr(w[0]), flags: w[1], restorer: addr(w[2]), mask: SigSet(w[3]) }
}

/// Enter `system`: the first caller ignores `SIGINT` and `SIGQUIT`. The original actions.
fn shell_enter() -> (KSigAction, KSigAction) {
    SHELL_LOCK.lock_raw(&CSys, Scope::Private);
    if SHELL_USERS.load(Relaxed) == 0 {
        for (i, sig) in [SIGINT, SIGQUIT].into_iter().enumerate() {
            if let Ok(old) = CSys.sigaction_set(sig, Disposition::Ignore, 0, SigSet::empty()) {
                save(i, &old);
            }
        }
    }
    SHELL_USERS.fetch_add(1, Relaxed);
    let r = (saved(0), saved(1));
    SHELL_LOCK.unlock(&CSys, Scope::Private);
    r
}

/// Leave `system`: the last caller restores `SIGINT` and `SIGQUIT`.
fn shell_leave() {
    SHELL_LOCK.lock_raw(&CSys, Scope::Private);
    if SHELL_USERS.fetch_sub(1, Relaxed) == 1 {
        for (i, sig) in [SIGINT, SIGQUIT].into_iter().enumerate() {
            let _ = raw::restore_action(sig, &saved(i));
        }
    }
    SHELL_LOCK.unlock(&CSys, Scope::Private);
}

/// `sh -c cmd` as a C argument array (on the caller's stack).
fn shell_argv(cmd: usize) -> [usize; 4] {
    [addr_of(b"sh\0"), addr_of(b"-c\0"), cmd, 0]
}

/// `system(cmd)` with `cmd` the address of the command text: the shell's wait status (127's
/// when the shell could not be executed), or an error.
pub fn system(cmd: usize) -> KResult<i32> {
    if cmd == 0 {
        // Whether a shell can run at all: the status of `exit 0`. No error is reported.
        return Ok(i32::from(run_shell(addr_of(b"exit 0\0")) == Ok(0)));
    }
    run_shell(cmd)
}

fn run_shell(cmd: usize) -> KResult<i32> {
    let (int, quit) = shell_enter();
    let r = match CSys.sigprocmask(SIG_BLOCK, Some(&SigSet::empty().with(SIGCHLD))) {
        Ok(old) => {
            let mut sigdefault = SigSet::empty();
            for (sig, a) in [(SIGINT, &int), (SIGQUIT, &quit)] {
                if a.handler != 1 {
                    sigdefault = sigdefault.with(sig);
                }
            }
            let attr = SpawnAttr { flags: lp::SPAWN_SETSIGMASK | lp::SPAWN_SETSIGDEF, sigmask: old, sigdefault, ..SpawnAttr::default() };
            let argv = shell_argv(cmd);
            let req = Request { program: Program::Path(kstr(SH)), actions: &[], attr: Some(attr), argv: argv.as_ptr().expose_provenance(), envp: environ(), pidfd: false };
            let r = match spawn(&req) {
                Ok((pid, _)) => wait_shell(pid),
                Err(Failure::Child(_)) => Ok(127 << 8),
                Err(Failure::Create(e)) => Err(e),
            };
            let _ = CSys.sigprocmask(SIG_SETMASK, Some(&old));
            r
        }
        Err(e) => Err(e),
    };
    shell_leave();
    r
}

/// Wait for `system`'s shell (a cancellation point: a cancelled caller kills and reaps it).
fn wait_shell(pid: i32) -> KResult<i32> {
    loop {
        match CP.wait4(pid, 0, None) {
            Ok((_, status)) => return Ok(status),
            Err(Errno::EINTR) => {}
            Err(Errno::ECANCELED) => {
                let _ = CSys.kill(pid, SIGKILL);
                reap(pid);
                return Err(Errno::ECANCELED);
            }
            Err(e) => return Err(e),
        }
    }
}

// ------------------------------------------------------------------------------ popen, pclose (§18.7)

/// The popen list: nodes of four words (stream, PID, descriptor, next).
static POPEN_LOCK: RawMutex = RawMutex::new();
static POPEN_FIRST: AtomicUsize = AtomicUsize::new(0);

fn node(addr: usize) -> &'static [AtomicUsize] {
    tb::words64(addr, 4)
}

fn field(addr: usize, i: usize) -> usize {
    node(addr).get(i).map_or(0, |w| w.load(Relaxed))
}

/// `popen(cmd, mode)` with `cmd` the address of the command text: the stream.
pub fn popen(cmd: usize, mode: KResult<&[u8]>) -> KResult<usize> {
    let m = lp::popen_mode(mode?)?;
    if cmd == 0 {
        return Err(Errno::EFAULT);
    }
    let entry = heap::alloc_words(4).ok_or(Errno::ENOMEM)?;
    POPEN_LOCK.lock_raw(&CSys, Scope::Private);
    let r = popen_locked(cmd, m, entry);
    POPEN_LOCK.unlock(&CSys, Scope::Private);
    if r.is_err() {
        heap::free_words(entry);
    }
    r
}

fn popen_locked(cmd: usize, m: lp::PopenMode, entry: usize) -> KResult<usize> {
    let [rd, wr] = CSys.pipe2(O_CLOEXEC)?;
    let (mine, theirs, target) = if m.read { (rd, wr, 1) } else { (wr, rd, 0) };
    let close_both = || {
        let _ = CSys.close(rd);
        let _ = CSys.close(wr);
    };
    // The child: the other popen streams' descriptors closed, then its end of the pipe as
    // standard input or output (on itself, `dup2` clears close-on-exec, §18.4).
    let mut actions = TryVec::new();
    let mut a = POPEN_FIRST.load(Acquire);
    let mut built = Ok(());
    while a != 0 && built.is_ok() {
        let fd = i32::try_from(field(a, 2)).unwrap_or(-1);
        if fd >= 0 && fd != theirs.raw() {
            built = lp::encode(&FileAction::Close(fd), &mut actions);
        }
        a = field(a, 3);
    }
    let built = built.and_then(|()| lp::encode(&FileAction::Dup2(theirs.raw(), target), &mut actions));
    if let Err(e) = built {
        close_both();
        return Err(e);
    }
    let argv = shell_argv(cmd);
    let req = Request { program: Program::Path(kstr(SH)), actions: &actions, attr: None, argv: argv.as_ptr().expose_provenance(), envp: environ(), pidfd: false };
    let pid = match spawn(&req) {
        Ok((pid, _)) => pid,
        Err(f) => {
            close_both();
            return Err(f.errno());
        }
    };
    let _ = CSys.close(theirs);
    if !m.cloexec {
        let _ = CSys.fcntl_int(mine, FcntlInt::SetFd, 0);
    }
    let fp = match stdio::fdopen(mine.raw(), Ok(if m.read { b"r" } else { b"w" })) {
        Ok(fp) => fp,
        Err(e) => {
            let _ = CSys.close(mine);
            reap(pid);
            return Err(e);
        }
    };
    let words = node(entry);
    let pid_word = usize::try_from(pid).unwrap_or(0);
    let fd_word = usize::try_from(mine.raw()).unwrap_or(0);
    for (slot, v) in words.iter().zip([fp, pid_word, fd_word, POPEN_FIRST.load(Acquire)]) {
        slot.store(v, Relaxed);
    }
    POPEN_FIRST.store(entry, Release);
    Ok(fp)
}

/// `pclose(fp)`: the stream is closed and its command waited for; its wait status. A stream
/// `popen` did not return is closed, and the call fails with `ECHILD`.
pub fn pclose(fp: usize) -> KResult<i32> {
    POPEN_LOCK.lock_raw(&CSys, Scope::Private);
    let mut prev = 0usize;
    let mut a = POPEN_FIRST.load(Acquire);
    while a != 0 && field(a, 0) != fp {
        prev = a;
        a = field(a, 3);
    }
    if a != 0 {
        let next = field(a, 3);
        match prev {
            0 => POPEN_FIRST.store(next, Release),
            p => {
                if let Some(w) = node(p).get(3) {
                    w.store(next, Release);
                }
            }
        }
    }
    POPEN_LOCK.unlock(&CSys, Scope::Private);
    let _ = stdio::fclose(fp);
    if a == 0 {
        return Err(Errno::ECHILD);
    }
    let pid = i32::try_from(field(a, 1)).unwrap_or(-1);
    heap::free_words(a);
    loop {
        match CSys.wait4(pid, 0, None) {
            Ok((_, status)) => return Ok(status),
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
}

// ------------------------------------------------------------------------------ wordexp (§14.10)

/// `wordexp`'s command substitution (D-14.5): run `/bin/sh -c command` (`command` ends with its
/// NUL) with standard output captured into `out`, standard error sent to `/dev/null` unless
/// `show_err`, and wait for it; its exit status is not an error. The child gets the caller's
/// environment and none of the popen streams' pipes (as `popen`'s children).
pub fn capture_shell(command: &[u8], show_err: bool, out: &mut TryVec<u8>) -> KResult<()> {
    if command.last() != Some(&0) {
        return Err(Errno::EINVAL);
    }
    let [rd, wr] = CSys.pipe2(O_CLOEXEC)?;
    POPEN_LOCK.lock_raw(&CSys, Scope::Private);
    let spawned = capture_spawn(command, show_err, wr);
    POPEN_LOCK.unlock(&CSys, Scope::Private);
    let _ = CSys.close(wr);
    let pid = match spawned {
        Ok(pid) => pid,
        Err(e) => {
            let _ = CSys.close(rd);
            return Err(e);
        }
    };
    let mut buf = [0u8; 4096];
    let mut r = Ok(());
    loop {
        match CSys.read(rd, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Err(e) = out.extend_from_slice(buf.get(..n).unwrap_or(&[])) {
                    r = Err(e);
                    break;
                }
            }
            Err(Errno::EINTR) => {}
            Err(e) => {
                r = Err(e);
                break;
            }
        }
    }
    let _ = CSys.close(rd);
    reap(pid);
    r
}

fn capture_spawn(command: &[u8], show_err: bool, wr: Fd) -> KResult<i32> {
    let mut actions = TryVec::new();
    let mut a = POPEN_FIRST.load(Acquire);
    while a != 0 {
        if let Ok(fd) = i32::try_from(field(a, 2)) {
            lp::encode(&FileAction::Close(fd), &mut actions)?;
        }
        a = field(a, 3);
    }
    lp::encode(&FileAction::Dup2(wr.raw(), 1), &mut actions)?;
    if !show_err {
        lp::encode(&FileAction::Open { fd: 2, path: kstr(b"/dev/null\0"), flags: liella_kernel::uapi::O_WRONLY, mode: 0 }, &mut actions)?;
    }
    // `argv[0]` is the shell's path, which its messages then name.
    let argv = [addr_of(SH), addr_of(b"-c\0"), addr_of(command), 0];
    let req = Request { program: Program::Path(kstr(SH)), actions: &actions, attr: None, argv: argv.as_ptr().expose_provenance(), envp: environ(), pidfd: false };
    spawn(&req).map(|(pid, _)| pid).map_err(Failure::errno)
}

// ------------------------------------------------------------------------------ fork (D-18.6)

/// `fork`'s lock handling (D-18.6): take (`true`) or release `popen`'s list lock, then
/// `system`'s; both are taken before the stdio list and the heap, which `popen` uses while it
/// holds its own.
pub(crate) fn fork_locks(take: bool) {
    if take {
        POPEN_LOCK.lock_raw(&CSys, Scope::Private);
        SHELL_LOCK.lock_raw(&CSys, Scope::Private);
    } else {
        SHELL_LOCK.unlock(&CSys, Scope::Private);
        POPEN_LOCK.unlock(&CSys, Scope::Private);
    }
}

/// In a `fork` or `_Fork` child (its only thread): both locks free; a `system` call that
/// another thread of the parent was in does not exist here, so its count is reset and, if it
/// was counted, `SIGINT` and `SIGQUIT` get the dispositions it saved. The popen list stays:
/// its streams were copied, and `pclose` of one fails with `ECHILD` (not this process's child).
pub(crate) fn after_fork_child() {
    POPEN_LOCK.reset();
    SHELL_LOCK.reset();
    if SHELL_USERS.swap(0, Relaxed) != 0 {
        for (i, sig) in [SIGINT, SIGQUIT].into_iter().enumerate() {
            let _ = raw::restore_action(sig, &saved(i));
        }
    }
}

/// For the boundary's tests (`selftest`): hold both locks until another thread waits for the
/// popen lock (or `ms` milliseconds pass), with `held` set once they are taken; whether a
/// waiter was seen.
#[cfg(feature = "selftest")]
pub(crate) fn hold_locks_until_contended(held: &core::sync::atomic::AtomicBool, ms: u64) -> bool {
    fork_locks(true);
    held.store(true, Release);
    let mut seen = false;
    for _ in 0..ms {
        if POPEN_LOCK.is_contended() {
            seen = true;
            break;
        }
        let _ = CSys.clock_nanosleep(1, 0, &liella_kernel::uapi::Timespec { sec: 0, nsec: 1_000_000 }, None);
    }
    fork_locks(false);
    seen
}

// ------------------------------------------------------------------------------ daemon (§18.8)

/// `daemon(nochdir, noclose)`: fork; the parent ends with `_exit(0)` (no handlers, no stdio
/// flush, daemon(3)); the child starts a session, changes to `/` unless `nochdir`, and puts
/// `/dev/null` on descriptors 0–2 unless `noclose`. Errors are `fork`'s and `setsid`'s; the
/// directory change and the redirection are made when possible.
pub fn daemon(nochdir: bool, noclose: bool) -> KResult<()> {
    if crate::fork::fork()? != 0 {
        CSys.exit_group(0);
    }
    CSys.setsid()?;
    if !nochdir {
        let _ = CSys.chdir(kstr(b"/\0"));
    }
    if !noclose {
        if let Ok(null) = CSys.openat(Fd::CWD, kstr(b"/dev/null\0"), O_RDWR, 0) {
            for fd in 0..3 {
                if fd != null.raw() {
                    let _ = CSys.dup2(null, Fd::new(fd));
                }
            }
            if null.raw() > 2 {
                let _ = CSys.close(null);
            }
        }
    }
    Ok(())
}
