//! The process-wide state of spec §1 and §2.4–§2.5 behind the C interface (IP-9, IP-13; D-1.2,
//! D-1.3, D-1.5): the exit-handler registries, the environment's bookkeeping, the auxiliary
//! vector, `exit`, and the fatal diagnostics.
//!
//! In a process Liella started, the runtime (`crate::runtime`) fills in the auxiliary vector and
//! `environ`. In a test process the host C library started, `environ` and the program names are
//! the host's objects (`crate::ffi`), and the auxiliary vector is read from `/proc/self/auxv`.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella_kernel::auxv::Auxv;
use liella_kernel::errno::{Errno, KResult};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::AT_SECURE;
use liella_legacy::env_table;
use liella_legacy::environ::{self, EnvState};
use liella_legacy::exit_handlers::{self, Entry, Registry};
use liella_legacy::mb_compat::ReadAt;
use liella_legacy::search_c::CWords;
use liella_rt::fatal;
use liella_rt::sync::RawMutex;

use crate::conv;
use crate::ffi;
use crate::heap::{KernelPages, HEAP};
use crate::sys::CSys;
use crate::tb;

/// `atexit`, `on_exit` and `__cxa_atexit` registrations.
pub static EXIT_HANDLERS: Registry = Registry::new();
/// `at_quick_exit` and `__cxa_at_quick_exit` registrations.
pub static QUICK_HANDLERS: Registry = Registry::new();

static ENV_LOCK: RawMutex = RawMutex::new();
static ENV_OWNED: AtomicUsize = AtomicUsize::new(0);
static ENV_CAP: AtomicUsize = AtomicUsize::new(0);

/// The initial auxiliary vector (address and words), set by the runtime; 0 in test processes.
pub(crate) static AUXV_ADDR: AtomicUsize = AtomicUsize::new(0);
pub(crate) static AUXV_WORDS: AtomicUsize = AtomicUsize::new(0);

fn w(x: usize) -> u64 {
    u64::try_from(x).unwrap_or(u64::MAX)
}

// ------------------------------------------------------------------------------ environment

fn env() -> EnvState<'static> {
    EnvState { environ: &ffi::environ, owned: &ENV_OWNED, cap: &ENV_CAP }
}

/// `getenv(name)`: the address of the value. The C layer scans the array in one call (PJ-8;
/// the semantics of `environ::getenv_bounded`); Rust re-reads the entry it reported as far as
/// `name` and the `=` (`settle_getenv`).
pub fn getenv(name: &[u8]) -> Option<usize> {
    let p = conv::env_find(name)?;
    if cfg!(feature = "selftest") {
        settle_getenv(name, p)
    } else {
        // ENV-SCAN: the scanner's answer is an entry whose bytes it compared one by one as it
        // read them (`name`, then `=`), so it is a correct answer for a call racing a writer
        // (D-4.14 as amended: the same ruling as `strlen`). The selftest build re-reads it.
        Some(p.saturating_add(name.len()).saturating_add(1))
    }
}

/// `getenv` for `strz::getenv_c`: the value address of the entry for the `len` bytes at `name`
/// (readable: the caller measured the string), 0 for none. The scanner's answer as it is; the
/// selftest build never comes here (it takes [`getenv`], which re-reads the entry).
pub(crate) fn getenv_raw(name: *const u8, len: usize) -> usize {
    // ENVIRON-TABLE-CONTRACT: one load and a branch while the contract is off (the default).
    if TABLE_ON.load(Relaxed)
        && let Some(a) = table_getenv(name.addr(), len)
    {
        return a;
    }
    conv::env_find_raw(name, len).map_or(0, |p| p.saturating_add(len).saturating_add(1))
}

// ------------------------------------------------------------- the environment contract (D-1.12)

/// Whether the environment contract is on (`liella_environ_contract`; off by default). While it
/// is, `getenv` answers from the table (`liella_legacy::env_table`), which this library's
/// environment writers keep current and `liella_environ_reload` rebuilds.
static TABLE_ON: AtomicBool = AtomicBool::new(false);
static TABLE_SEQ: AtomicU64 = AtomicU64::new(0);
static TABLE_N: AtomicUsize = AtomicUsize::new(0);
static TABLE_ENV: AtomicUsize = AtomicUsize::new(0);
/// The slots: their address and number (a power of two; 0 until the contract is first turned
/// on). Published before the sequence word turns even; an allocation is never freed, so a
/// lookup still probing an old one reads valid memory.
static TABLE_ADDR: AtomicUsize = AtomicUsize::new(0);
static TABLE_LEN: AtomicUsize = AtomicUsize::new(0);

fn table() -> env_table::Store<'static> {
    env_table::Store { seq: &TABLE_SEQ, n: &TABLE_N, env: &TABLE_ENV }
}

fn table_slots() -> &'static [AtomicUsize] {
    conv::env_table_words(TABLE_ADDR.load(Relaxed), TABLE_LEN.load(Relaxed))
}

/// The table's answer for the `len` bytes at `name` (`getenv_raw`'s: the value address, 0 for
/// none), or `None` (scan: not built, a writer racing, or `environ` not the table's array).
/// Takes no lock and allocates nothing.
#[cold]
#[inline(never)]
fn table_getenv(name: usize, len: usize) -> Option<usize> {
    let mut text = conv::env_text();
    let name = text.read_at(name, len)?;
    let h = env_table::hash(name);
    match table().lookup(table_slots, ffi::environ.load(Acquire), name, h, &conv::env_text(), &mut conv::env_text()) {
        env_table::Answer::Found(_, p) => p.checked_add(len)?.checked_add(1),
        env_table::Answer::Absent => Some(0),
        env_table::Answer::Unknown => None,
    }
}

/// Rebuild the table for the current `environ`, growing its slots if they are too few (writers'
/// lock held). Without memory the table stays unusable and lookups scan.
#[cold]
#[inline(never)]
fn table_rebuild() {
    let t = table();
    t.invalidate();
    let env = ffi::environ.load(Acquire);
    let mem = conv::env_text();
    let Some(need) = env_table::slots_for(environ::count(env, &mem)) else { return };
    if TABLE_LEN.load(Relaxed) < need {
        let Some(bytes) = need.checked_mul(size_of::<usize>()) else { return };
        match crate::heap::rust_alloc_zeroed(bytes, size_of::<usize>()) {
            0 => return,
            a => {
                TABLE_ADDR.store(a, Relaxed);
                TABLE_LEN.store(need, Relaxed);
            }
        }
    }
    t.build(table_slots(), env, &mem);
}

/// After a writer call (writers' lock held) that found `environ` at `before` and replaced the
/// entry `replaced` (`Ok(Some)`), or none: keep the table current.
#[cold]
#[inline(never)]
fn table_written(before: usize, r: Result<Option<usize>, ()>) {
    let ok = match r {
        Ok(Some(_)) => before == TABLE_ENV.load(Relaxed) && table().built(),
        Ok(None) => before == TABLE_ENV.load(Relaxed) && table().appended(table_slots(), ffi::environ.load(Acquire), &conv::env_text()),
        Err(()) => false,
    };
    if !ok {
        table_rebuild();
    }
}

/// `liella_environ_contract(on)`: turn the contract on (building the table) or off; the previous
/// state (1: on).
pub fn environ_contract(on: bool) -> bool {
    let _g = ENV_LOCK.lock(&CSys);
    let was = TABLE_ON.load(Relaxed);
    if on {
        if !was {
            table_rebuild();
        }
        TABLE_ON.store(true, Release);
    } else {
        TABLE_ON.store(false, Release);
        table().invalidate();
    }
    was
}

/// `liella_environ_reload()`: rebuild the table after a change to `environ` made without this
/// library's writers (nothing while the contract is off).
pub fn environ_reload() {
    let _g = ENV_LOCK.lock(&CSys);
    if TABLE_ON.load(Relaxed) {
        table_rebuild();
    }
}

/// Check the entry `p` the C layer reported for `name` (D-4.21): an entry that no longer starts
/// with `name=` was changed by a writer during the call (an entry's bytes, or `environ` itself),
/// so the array is searched once more by `environ::getenv_bounded`, whose own reads decide the
/// answer. No answer is impossible: any address may be an entry after a writer's change.
#[inline]
pub(crate) fn settle_getenv(name: &[u8], p: usize) -> Option<usize> {
    let end = name.len().checked_add(1)?;
    if conv::env_text().read_at(p, end).is_some_and(|t| environ::matches(t, name)) {
        return p.checked_add(end);
    }
    rescan_getenv(name)
}

/// `settle_getenv`'s rescan.
#[cold]
#[inline(never)]
fn rescan_getenv(name: &[u8]) -> Option<usize> {
    crate::strz::note_rescan();
    environ::getenv_bounded(&env(), name, &conv::env_text(), &mut conv::env_text())
}

/// The value address of each of `names` (`getenv`'s answer for each; at most 64) found in one
/// walk over `environ` (`environ::getenv_many`, whose own reads decide the answers).
pub fn getenv_many(names: &[&[u8]], out: &mut [Option<usize>]) {
    environ::getenv_many(&env(), names, out, &conv::env_text(), &mut conv::env_text());
}

/// `secure_getenv(name)`: `getenv`, except in secure mode.
pub fn secure_getenv(name: &[u8]) -> Option<usize> {
    if secure() { None } else { getenv(name) }
}

/// Moves after every call of this library's environment writers (`setenv`, `unsetenv`,
/// `putenv`, `clearenv`): an [`EnvMemo`] remembered before it may be out of date.
static ENV_VALUES: AtomicUsize = AtomicUsize::new(1);
/// The index of the entry the last writer call replaced in place (`setenv` or `putenv` of a name
/// already set: the first entry for it, by that call's own scan); `usize::MAX` after any other
/// call (an entry added or removed, `clearenv`, a failure). Stored before [`ENV_VALUES`] moves.
static ENV_REPLACED: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Run `f` on the environment with the writers' lock held (MT-Unsafe `env`: readers are not
/// excluded, as in C), then publish for [`EnvMemo`] which entry it replaced (`replaced`).
fn with_env<T>(f: impl FnOnce(&EnvState<'static>, &mut conv::CText) -> KResult<T>, replaced: impl Fn(&T) -> Option<usize>) -> KResult<T> {
    let _g = ENV_LOCK.lock(&CSys);
    let mut mem = conv::env_text();
    // The contract only changes under this lock, so it holds for the whole call.
    let contract = TABLE_ON.load(Relaxed);
    let before = if contract { ffi::environ.load(Acquire) } else { 0 };
    let r = f(&env(), &mut mem);
    if contract {
        table_written(before, r.as_ref().map(&replaced).map_err(|_| ()));
    }
    ENV_REPLACED.store(r.as_ref().ok().and_then(replaced).unwrap_or(usize::MAX), Relaxed);
    // Writers are serialised by the lock, so a plain store is enough.
    ENV_VALUES.store(ENV_VALUES.load(Relaxed).wrapping_add(1), Release);
    r
}

/// One environment variable's lookup remembered across calls (GETTEXT-ENV-SCAN, ruling #504):
/// `gettext` asks for `LANGUAGE` at every call, and a scan costs the whole environment (11,000
/// instructions with 2,000 entries). Words: [`ENV_VALUES`] and `environ` when it
/// was remembered, the slot index and the entry address (0: no entry).
///
/// An answer is reused only for the same `environ`, and either the same [`ENV_VALUES`] (no
/// writer call since) or one writer call since that replaced the remembered slot itself (`putenv` or
/// `setenv` of the same name: that call's own scan found no earlier entry for it). A found
/// entry's slot is read again and its name compared again (`conv::env_find_from`: one scanner
/// call that compares the slot's entry first, and the hit only when that entry is its answer;
/// GETTEXT-MEMO-HIT-COST, so a hit costs no more than a short scan), so a slot the program
/// stored a new pointer into, and a `putenv` string it edited in place, are seen.
/// Anything else scans. Not seen until this library's next environment writer call (or a change
/// of `environ`): a name the program made appear by a direct store into another slot, or by
/// renaming a `putenv` string in place, and a duplicate it stored before the remembered slot
/// (docs/differences-from-glibc.md). Callers serialise their calls (the gettext lock).
pub struct EnvMemo([AtomicUsize; 4]);

impl EnvMemo {
    /// Nothing remembered: the first lookup scans (nothing is done at startup).
    pub const fn new() -> Self {
        Self([const { AtomicUsize::new(0) }; 4])
    }

    /// `getenv(name)`'s answer (the value's address).
    #[inline]
    pub fn getenv(&self, name: &[u8]) -> Option<usize> {
        let values = ENV_VALUES.load(Acquire);
        let env = ffi::environ.load(Acquire);
        let [v, e, slot, entry] = &self.0;
        if env != 0 && e.load(Relaxed) == env {
            let (remembered, seen) = (entry.load(Relaxed), v.load(Relaxed));
            let found = remembered != 0;
            let at = slot.load(Relaxed);
            let fresh = seen == values;
            if !found && fresh {
                return None;
            }
            // The fresh hit (GETTEXT-MEMO-HIT-COST-2): no writer call since the slot was remembered,
            // so the scanner's answer from that slot is the hit when it is the remembered entry
            // itself (one compare with a word already loaded; a pointer the program stored into
            // the slot since is another address and scans). After one writer call that replaced
            // the slot, the slot's new pointer is read and remembered.
            if found
                && (fresh || (seen.wrapping_add(1) == values && ENV_REPLACED.load(Relaxed) == at))
                && let Some(p) = conv::env_find_from(env, at, name)
                && (if fresh { p == remembered } else { self.replaced(env, at, p, values) })
                && (!cfg!(feature = "selftest") || environ::entry_is(p, name, &conv::env_text()))
            {
                // `p` is an entry holding `name` and `=` (the scanner compared them), so this
                // stays inside that string.
                return Some(p.wrapping_add(name.len()).wrapping_add(1));
            }
        }
        self.scan(name, values, env)
    }

    /// The hit after one writer call that replaced the remembered slot itself: the scanner's
    /// answer `p` is that slot's new entry. Remembered for the fresh hits that follow.
    fn replaced(&self, env: usize, at: usize, p: usize, values: usize) -> bool {
        let [v, _, _, entry] = &self.0;
        if p != conv::env_text().word(env, at) {
            return false;
        }
        entry.store(p, Relaxed);
        v.store(values, Relaxed);
        true
    }

    /// The scan: [`getenv`], then the found entry's index (pointer compares only).
    #[cold]
    #[inline(never)]
    fn scan(&self, name: &[u8], values: usize, env: usize) -> Option<usize> {
        let found = getenv(name);
        let [v, e, slot, entry] = &self.0;
        let mem = conv::env_text();
        let at = match found.and_then(|a| a.checked_sub(name.len())?.checked_sub(1)) {
            Some(p) if env != 0 => (0..).map(|i| (i, mem.word(env, i))).take_while(|&(_, w)| w != 0).find(|&(_, w)| w == p),
            _ => Some((0, 0)),
        };
        // An entry no longer in the array (a writer raced the scan) is not remembered.
        let (i, p, values) = at.map_or((0, 0, 0), |(i, p)| (i, p, values));
        slot.store(i, Relaxed);
        entry.store(p, Relaxed);
        e.store(env, Relaxed);
        v.store(values, Relaxed);
        found
    }
}

impl Default for EnvMemo {
    fn default() -> Self {
        Self::new()
    }
}

/// `setenv(name, value, overwrite)`.
pub fn setenv(name: &[u8], value: &[u8], overwrite: i32) -> KResult<()> {
    with_env(|st, mem| environ::setenv_at(st, name, value, overwrite != 0, mem), |i| *i).map(|_| ())
}

/// `unsetenv(name)`.
pub fn unsetenv(name: &[u8]) -> KResult<()> {
    with_env(|st, mem| environ::unsetenv(st, name, mem), |()| None)
}

/// The value of `name` copied into `out` if it fits whole: its length (the native interface,
/// D-X.10). The copy is made with the writers' lock held, so no concurrent `setenv` can replace
/// the entry while it is read.
pub fn getenv_into(name: &[u8], out: &mut [u8]) -> Option<usize> {
    let _g = ENV_LOCK.lock(&CSys);
    getenv(name).map(|a| conv::copy_out(crate::strz::env_value(a), out))
}

/// The number of entries in the environment.
pub fn env_count() -> usize {
    environ::count(ffi::environ.load(Acquire), &conv::env_text())
}

/// Entry `i` (`NAME=VALUE`) copied into `out` if it fits whole: its length; `None` past the
/// last entry. Under the writers' lock, as [`getenv_into`].
pub fn env_entry_into(i: usize, out: &mut [u8]) -> Option<usize> {
    let _g = ENV_LOCK.lock(&CSys);
    let mem = conv::env_text();
    environ::entry(ffi::environ.load(Acquire), i, &mem).map(|p| conv::copy_out(mem.text(p), out))
}

// ------------------------------------------------------------------------------ arguments

/// The program's argument count and vector (`argc`, the address of `argv`), recorded by the
/// runtime at startup. A process the host C library started (a test process) has none.
pub(crate) static ARGC: AtomicUsize = AtomicUsize::new(0);
pub(crate) static ARGV: AtomicUsize = AtomicUsize::new(0);

/// The number of program arguments (`argc`; 0 in a process Liella did not start).
pub fn arg_count() -> usize {
    ARGC.load(Acquire)
}

/// Argument `i` copied into `out` if it fits whole: its length; `None` for `i >= argc`.
pub fn arg_into(i: usize, out: &mut [u8]) -> Option<usize> {
    let mem = conv::env_text();
    let p = (i < arg_count()).then(|| mem.word(ARGV.load(Acquire), i))?;
    Some(conv::copy_out(mem.text(p), out))
}

/// Argument `i` (`argv[i]`, `i < argc`) appended to `out`: `false` when there is none
/// (`wordexp`'s positional parameters, §14.10).
pub fn arg_bytes(i: usize, out: &mut liella::pattern::wordexp::ValBuf) -> KResult<bool> {
    if i >= arg_count() {
        return Ok(false);
    }
    let mem = conv::env_text();
    out.extend_from_slice(mem.text(mem.word(ARGV.load(Acquire), i)))?;
    Ok(true)
}

/// `putenv(string)`: `string` is the caller's own `NAME=VALUE` text.
pub fn putenv(string: usize) -> KResult<()> {
    if string == 0 {
        return Err(Errno::EINVAL);
    }
    with_env(|st, mem| environ::putenv_at(st, string, mem), |i| *i).map(|_| ())
}

/// `clearenv()`.
pub fn clearenv() -> KResult<()> {
    with_env(
        |st, mem| {
            environ::clearenv(st, mem);
            Ok(())
        },
        |()| None,
    )?;
    Ok(())
}

// ------------------------------------------------------------------------------ auxiliary vector

/// The value for `key` (D-1.5: absent is `None`, distinct from 0).
pub fn auxval(key: u64) -> Option<u64> {
    match AUXV_ADDR.load(Acquire) {
        0 => crate::sys::auxv_from_proc(key),
        a => Auxv::from_words(conv::startup_auxv(a, AUXV_WORDS.load(Acquire))).get(key),
    }
}

/// Whether `addr` lies in a read-only segment of the executable (D-12.4).
pub fn is_read_only(addr: usize) -> bool {
    let get = |k: u64| auxval(k).and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    let (phdr, phnum) = (get(liella_kernel::uapi::AT_PHDR), get(liella_kernel::uapi::AT_PHNUM));
    let words = conv::program_headers(phdr, phnum.saturating_mul(liella_rt::start::PHDR_WORDS));
    liella_rt::start::read_only(words, phnum, phdr, addr)
}

/// `getauxval(key)`: the value, or 0 with `ENOENT`.
pub fn getauxval(key: u64) -> (u64, Option<Errno>) {
    match auxval(key) {
        Some(v) => (v, None),
        None => (0, Some(Errno::ENOENT)),
    }
}

/// Secure-execution mode (`AT_SECURE`). Where the vector cannot be read (a hosted process whose
/// `/proc/self/auxv` is closed to it: a set-ID process is not dumpable, so the file is root's),
/// a process whose real and effective IDs differ is taken as secure (failing safe).
pub fn secure() -> bool {
    // The vector's answer never changes: it is read once (0: not yet, 1: no, 2: yes). The
    // fallback is asked every time (the IDs can change).
    match SECURE.load(Relaxed) {
        1 => return false,
        2 => return true,
        _ => {}
    }
    match auxval(AT_SECURE) {
        Some(v) => {
            SECURE.store(if v != 0 { 2 } else { 1 }, Relaxed);
            v != 0
        }
        None => CSys.getuid() != CSys.geteuid() || CSys.getgid() != CSys.getegid(),
    }
}

/// `AT_SECURE` as read once from the vector ([`secure`]).
static SECURE: AtomicU8 = AtomicU8::new(0);

// ------------------------------------------------------------------------------ exit

/// Register an entry; `false` without memory.
pub fn register(e: Entry) -> bool {
    EXIT_HANDLERS.register(&CSys, &KernelPages, e)
}

/// Register a quick-exit entry.
pub fn register_quick(e: Entry) -> bool {
    QUICK_HANDLERS.register(&CSys, &KernelPages, e)
}

/// The entry for a registration call.
pub fn entry(kind: u64, f: usize, arg: usize, dso: usize) -> Entry {
    [kind, w(f), w(arg), w(dso)]
}

/// `__cxa_thread_atexit_impl(f, obj, dso)` for the calling thread; `false` without memory.
pub fn thread_atexit(f: usize, obj: usize, dso: usize) -> bool {
    match HEAP.alloc(&CSys, &KernelPages, 32, 16) {
        Some(b) => {
            exit_handlers::thread_atexit(&tb::current().tls_dtors, &KernelPages, b.addr, f, obj, dso);
            true
        }
        None => false,
    }
}

/// Run the calling thread's C++ `thread_local` destructors.
pub fn run_thread_dtors() {
    exit_handlers::run_thread_dtors(&tb::current().tls_dtors, &KernelPages, &mut conv::CCalls, |n| {
        let _ = HEAP.free(&CSys, &KernelPages, n);
    });
}

/// `exit(status)` (spec §1.2): the calling thread's `thread_local` destructors (C++ requires
/// them before static destructors; ERRATA E9), then every exit entry, most recent first,
/// including those registered meanwhile (a static program's `.fini_array` and `_fini` are one
/// of them, registered at startup), then every stream's pending output is written (§12.1), then
/// the whole process ends.
pub fn exit(status: i32) -> ! {
    run_thread_dtors();
    exit_handlers::run_all(&EXIT_HANDLERS, &CSys, &KernelPages, status, &mut conv::CCalls);
    crate::stdio::flush_at_exit();
    CSys.exit_group(status)
}

/// `quick_exit(status)`: only the quick-exit entries, then `_Exit`.
pub fn quick_exit(status: i32) -> ! {
    exit_handlers::run_all(&QUICK_HANDLERS, &CSys, &KernelPages, status, &mut conv::CCalls);
    CSys.exit_group(status)
}

/// `__cxa_finalize(dso)`.
pub fn finalize(dso: usize) {
    exit_handlers::finalize(&EXIT_HANDLERS, &QUICK_HANDLERS, &CSys, &KernelPages, dso, &mut conv::CCalls);
}

/// `_exit`/`_Exit(status)`: end the process now.
pub fn exit_now(status: i32) -> ! {
    CSys.exit_group(status)
}

// ------------------------------------------------------------------------------ diagnostics

/// The program's short name (`__progname`), empty if unknown.
fn progname() -> &'static [u8] {
    conv::process_text(ffi::program_invocation_short_name.load(Acquire))
}

/// `__assert_fail` (spec §2.4): the message, then `abort`.
pub fn assert_fail(expr: &[u8], file: &[u8], line: u32, function: Option<&[u8]>) -> ! {
    fatal::assert_message(&CSys, progname(), expr, file, line, function);
    crate::ret::abort()
}

/// `__assert_perror_fail`: the error's text (`strerror`'s).
pub fn assert_perror_fail(errnum: i32, file: &[u8], line: u32, function: Option<&[u8]>) -> ! {
    let mut b = [0u8; 32];
    let text = match liella::diag::description(errnum) {
        Some(d) => d.as_bytes(),
        None => {
            let n = liella::diag::unknown(errnum, &mut b);
            b.get(..n).unwrap_or(&[])
        }
    };
    fatal::assert_perror_message(&CSys, progname(), text, file, line, function);
    crate::ret::abort()
}

/// A fortify failure with glibc's message: `*** <what> ***: terminated`, then abort.
pub fn fortify_fail(what: &[u8]) -> ! {
    fatal::fatal(&CSys, &[b"*** ", what, b" ***: terminated\n"], crate::ret::trap)
}

/// `__stack_chk_fail` (spec §2.5).
pub fn stack_chk_fail() -> ! {
    fatal::fatal(&CSys, &[b"*** stack smashing detected ***: terminated\n"], crate::ret::trap)
}

/// A panic inside the libm product (a bug): a fixed diagnostic, then abort. The product keeps
/// no location formatting or unwinder of its own (D-10.7).
pub fn libm_panicked() -> ! {
    fatal::fatal(&CSys, &[b"liella: internal error in libm\n"], crate::ret::trap)
}

/// A panic inside the library (a bug): the location, then abort (`liella_rt::panic`).
pub fn panicked(info: &core::panic::PanicInfo<'_>) -> ! {
    liella_rt::panic::panicked(&CSys, info, crate::ret::trap)
}

/// The auxiliary vector as `liella::system` reads it (`sysconf`'s page size, clock ticks and
/// signal-stack minimum).
pub struct ProcessAux;

impl liella::system::Aux for ProcessAux {
    fn aux(&self, key: u64) -> Option<u64> {
        auxval(key)
    }
}

/// Writes `bytes` to standard error, ignoring failures (`psiginfo`, which like glibc's writes to descriptor 2
/// behind the stream's back).
pub fn stderr(bytes: &[u8]) {
    let _ = liella_kernel::sys::Sys::write(&crate::sys::CSys, liella_kernel::Fd::new(2), bytes);
}

/// Take (`true`) or release the environment lock without a guard (`fork`, the AS-safety
/// harness).
pub fn hold_env_lock(take: bool) {
    if take {
        ENV_LOCK.lock_raw(&CSys, liella_rt::futex::Scope::Private);
    } else {
        ENV_LOCK.unlock(&CSys, liella_rt::futex::Scope::Private);
    }
}
