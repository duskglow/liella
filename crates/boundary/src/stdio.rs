//! Stdio glue (spec §12; D-12.11): `FILE` objects, the standard streams, and the library
//! services `liella_legacy::stdio_compat` runs on.
//!
//! A stream the library opens lives in a [`Slot`]: its `FILE` words (so the `FILE *` is the
//! slot's address), its lock and its record. Slots are leaked, never freed: a closed stream's
//! slot is recycled for the next one, so every address the library ever handed out stays
//! readable. The standard streams' `FILE` objects are the exported `_IO_2_1_*_` data; their
//! lock and record live in a slot taken at first use.

use core::sync::atomic::{AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella::error::{Errno, KResult};
use liella::mem::{TryBox, TryVec};
use liella::text::Encoding;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_legacy::stdio_compat::{self as sc, word, Env, Ext, Kind, StreamLock, FILE_WORDS};
use liella_rt::futex::Scope;
use liella_rt::sync::RawMutex;

use crate::sys::CSys;
use crate::{caller_alloc, conv, data, ffi, locale, tb, thread};

/// One stream's memory.
#[repr(C)]
struct Slot {
    file: [AtomicUsize; FILE_WORDS],
    lock: StreamLock,
    /// The seal: the address of the `FILE` this record serves, xor the pointer guard (D-12.19).
    seal: AtomicUsize,
    ext: Ext,
    /// The buffer used when one cannot be allocated (T22).
    spare: [u8; sc::SPARE],
}

/// Every process allocates a slot per standard stream it uses: keep it in the heap class it has
/// shared since before D-12.21 (classes above 512 bytes step by 128). A slot of 880 bytes (the
/// plan cache inline) cost each process ~2.7k instructions and two page faults (printf-loop,
/// PRINTF-STAR-2); re-measure a process start before raising this.
const _: () = assert!(core::mem::size_of::<Slot>() <= 768);

/// Where a slot's spare buffer lies relative to its record.
const SPARE_FROM_EXT: usize = core::mem::offset_of!(Slot, spare) - core::mem::offset_of!(Slot, ext);
/// Where a slot's seal and lock lie before its record.
const SEAL_BEFORE_EXT: usize = core::mem::offset_of!(Slot, ext) - core::mem::offset_of!(Slot, seal);
const LOCK_BEFORE_EXT: usize = core::mem::offset_of!(Slot, ext) - core::mem::offset_of!(Slot, lock);
const _: () = assert!(SEAL_BEFORE_EXT == 8 && core::mem::offset_of!(Slot, ext) % 8 == 0);

/// The seal of a record serving the `FILE` at `fp` (D-12.19): unforgeable without the
/// process's pointer guard, and different for every `FILE`.
#[inline(always)]
fn seal_of(fp: usize) -> usize {
    fp ^ tb::read::<tb::PointerGuard>()
}

/// Bind the record at `ext` (in a slot) to the `FILE` at `fp`.
fn seal(ext: usize, fp: usize) {
    if let Some(w) = tb::c_word(ext.wrapping_sub(SEAL_BEFORE_EXT)) {
        w.store(seal_of(fp), Release);
    }
}

/// The library's side of the stream operations.
pub struct Glue;

fn addr_of<T>(r: &T) -> usize {
    core::ptr::from_ref(r).expose_provenance()
}

impl Env for Glue {
    type S = CSys;

    #[inline(always)]
    fn find(&self, s: &[u8], c: u8) -> Option<usize> {
        crate::strz::find_in(s, c)
    }

    /// GETDELIM-FUSE: with AVX2 and at least 32 bytes to scan, one pass ([`crate::hot::delim_copy`]);
    /// else with at least 16, the SSE2 build ([`crate::hot::delim_copy_sse2`], PERF-GETLINE-SHORT);
    /// else [`sc::find_copy`]'s two passes.
    #[inline(always)]
    fn find_copy(&self, s: &[u8], d: &mut [u8], c: u8) -> Option<usize> {
        let lim = d.len().checked_sub(1)?.min(s.len());
        if lim >= 32 && crate::dispatch::avx2() {
            return crate::hot::delim_copy(s, d, c);
        }
        if lim >= 16 {
            return crate::hot::delim_copy_sse2(s, d, c);
        }
        sc::find_copy(self, s, d, c)
    }

    fn sys(&self) -> &CSys {
        &CSys
    }

    fn tid(&self) -> u32 {
        u32::try_from(thread::own_tid()).unwrap_or(0)
    }

    fn bytes(&self, addr: usize, len: usize) -> &'static mut [u8] {
        tb::stream_bytes(addr, len)
    }

    fn load_word(&self, addr: usize) -> usize {
        tb::c_word(addr).map_or(0, |w| w.load(Relaxed))
    }

    fn store_word(&self, addr: usize, v: usize) {
        if let Some(w) = tb::c_word(addr) {
            w.store(v, Relaxed);
        }
    }

    fn malloc(&self, n: usize) -> usize {
        caller_alloc::malloc(n).expose_provenance()
    }

    fn spare(&self, ext: usize) -> (usize, usize) {
        // Every record lives in a slot (`new_slot`), its spare buffer after it.
        (ext.saturating_add(SPARE_FROM_EXT), sc::SPARE)
    }

    fn realloc(&self, addr: usize, n: usize) -> usize {
        caller_alloc::realloc(core::ptr::with_exposed_provenance_mut(addr), n).expose_provenance()
    }

    fn free(&self, addr: usize) {
        caller_alloc::free(core::ptr::with_exposed_provenance_mut(addr));
    }

    fn call3(&self, f: usize, a: usize, b: usize, c: usize) -> isize {
        let w = |v: usize| i64::from_ne_bytes(v.to_ne_bytes());
        isize::try_from(ffi::liella_machine_call3(f, w(a), w(b), w(c))).unwrap_or(-1)
    }

    fn list_head(&self) -> &AtomicUsize {
        &data::_IO_list_all
    }

    fn file_at(&self, addr: usize) -> &'static [AtomicUsize] {
        tb::file_words(addr).unwrap_or(&[])
    }

    /// D-12.19: `ext` must be 8-aligned with the seal before it naming `fp`. The seal lies in
    /// the word before the record; a crafted `ext` is only read (a bad address faults, as any
    /// wild `FILE *` may), never written, before it passes.
    #[inline(always)]
    fn record(&self, fp: usize, ext: usize) -> bool {
        if ext <= SEAL_BEFORE_EXT || ext % 8 != 0 {
            return false;
        }
        tb::c_word(ext.wrapping_sub(SEAL_BEFORE_EXT)).is_some_and(|seal| seal.load(Relaxed) == seal_of(fp))
    }

    fn lock_of(&self, ext: usize) -> &'static StreamLock {
        tb::stream_lock_at(ext.wrapping_sub(LOCK_BEFORE_EXT))
    }

    fn ext_at(&self, addr: usize) -> &'static mut Ext {
        tb::stream_ext_at(addr)
    }
}

// ------------------------------------------------------------------------------ slots

/// Closed slots, linked through their `_chain` word.
static FREE: AtomicUsize = AtomicUsize::new(0);
/// Serializes the free list and the standard streams' set-up.
static SLOTS: RawMutex = RawMutex::new();

/// A slot for a new stream, its `FILE` words zeroed and pointing at its lock and record: the
/// slot's address (the `FILE *`).
fn new_slot() -> KResult<usize> {
    SLOTS.lock_raw(&CSys, Scope::Private);
    let reused = FREE.load(Acquire);
    if reused != 0 {
        let next = Glue.file_at(reused).get(word::CHAIN).map_or(0, |w| w.load(Relaxed));
        FREE.store(next, Release);
    }
    SLOTS.unlock(&CSys, Scope::Private);
    let fp = if reused != 0 {
        reused
    } else {
        let slot: &'static mut Slot = TryBox::new(Slot { file: [const { AtomicUsize::new(0) }; FILE_WORDS], lock: StreamLock::new(), seal: AtomicUsize::new(0), ext: Ext::default(), spare: [0; sc::SPARE] })?.leak();
        let fp = addr_of(&*slot);
        if let (Some(l), Some(e)) = (slot.file.get(word::LOCK), slot.file.get(word::EXT)) {
            l.store(addr_of(&slot.lock), Relaxed);
            e.store(addr_of(&slot.ext), Relaxed);
        }
        fp
    };
    let file = Glue.file_at(fp);
    // Every word but `_lock` and the record's: three runs of plain stores, no per-word test
    // (STDIO-3: the per-word test was most of `new_slot`'s ~370 instructions in fopen).
    const { assert!(word::LOCK < word::EXT) };
    let zero = |words: &[AtomicUsize]| words.iter().for_each(|w| w.store(0, Relaxed));
    zero(file.get(..word::LOCK).unwrap_or(&[]));
    zero(file.get(word::LOCK + 1..word::EXT).unwrap_or(&[]));
    zero(file.get(word::EXT + 1..).unwrap_or(&[]));
    seal(file.get(word::EXT).map_or(0, |w| w.load(Relaxed)), fp);
    Ok(fp)
}

/// Return a closed stream's slot.
fn recycle(fp: usize) {
    SLOTS.lock_raw(&CSys, Scope::Private);
    if let Some(w) = Glue.file_at(fp).get(word::CHAIN) {
        w.store(FREE.load(Acquire), Relaxed);
    }
    FREE.store(fp, Release);
    SLOTS.unlock(&CSys, Scope::Private);
}

/// The descriptor of a standard stream's `FILE` (`None` for any other).
fn standard_fd(fp: usize) -> Option<i32> {
    [(addr_of(&data::_IO_2_1_stdin_), 0), (addr_of(&data::_IO_2_1_stdout_), 1), (addr_of(&data::_IO_2_1_stderr_), 2)].iter().find(|(a, _)| *a == fp).map(|(_, fd)| *fd)
}

/// Set up a standard stream at first use (once, whichever thread comes first). Its slot comes
/// from the heap, or with the heap exhausted from the library's reserve (`heap`, T22), so a
/// program out of memory can still report (D-12.17).
fn set_up_standard(fp: usize, file: &'static [AtomicUsize], fd: i32) -> KResult<()> {
    SLOTS.lock_raw(&CSys, Scope::Private);
    let done = sc::parts(&Glue, file).is_some();
    SLOTS.unlock(&CSys, Scope::Private);
    if done {
        return Ok(());
    }
    let slot = new_slot()?;
    let slot_file = Glue.file_at(slot);
    let (lock, ext) = (slot_file.get(word::LOCK).map_or(0, |w| w.load(Relaxed)), slot_file.get(word::EXT).map_or(0, |w| w.load(Relaxed)));
    SLOTS.lock_raw(&CSys, Scope::Private);
    let raced = sc::parts(&Glue, file).is_some();
    if !raced {
        seal(ext, fp);
        sc::setup_standard(&Glue, file, tb::stream_ext_at(ext), fd);
        if let (Some(l), Some(e)) = (file.get(word::LOCK), file.get(word::EXT)) {
            l.store(lock, Relaxed);
            e.store(ext, Release);
        }
    }
    SLOTS.unlock(&CSys, Scope::Private);
    if raced {
        recycle(slot);
    } else {
        sc::link(&Glue, fp, file);
    }
    Ok(())
}

/// A stream's words, lock and record address (a standard stream is set up at first use).
/// Inlined into [`with`] and the open/close paths (STDIO-4: the call was ~10 of its ~44
/// instructions per operation); a standard stream's first use is out of line.
#[inline(always)]
fn parts(fp: usize) -> KResult<(&'static [AtomicUsize], &'static StreamLock, usize)> {
    let file = tb::file_words(fp).ok_or(Errno::EINVAL)?;
    if let Some((lock, ext)) = sc::parts(&Glue, file) {
        return Ok((file, lock, ext));
    }
    parts_first_use(fp, file)
}

/// [`parts`] of a stream not set up: a standard stream is set up now, anything else is `EBADF`.
#[cold]
#[inline(never)]
fn parts_first_use(fp: usize, file: &'static [AtomicUsize]) -> KResult<(&'static [AtomicUsize], &'static StreamLock, usize)> {
    let fd = standard_fd(fp).ok_or(Errno::EBADF)?;
    set_up_standard(fp, file, fd)?;
    sc::parts(&Glue, file).map(|(l, e)| (file, l, e)).ok_or(Errno::EBADF)
}

/// Whether an operation takes the stream's lock (the plain forms) or the caller holds it (the
/// `_unlocked` forms).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Locking {
    /// The operation locks, unless the caller took over locking (`__fsetlocking`).
    Locked,
    /// The caller holds the lock.
    Unlocked,
}

/// Run `f` on the stream `fp`, with its lock held as `how` says.
pub fn with<T>(fp: usize, how: Locking, f: impl FnOnce(&Glue, &'static [AtomicUsize], &mut Ext) -> T) -> KResult<T> {
    let (file, lock, ext) = parts(fp)?;
    // The only thread needs no stream lock (`thread::alone`): its own `flockfile` is recursive.
    // STDIO-6: the thread id only when the lock is taken.
    let take = how == Locking::Locked && !sc::by_caller(file) && !crate::thread::alone();
    let me = if take { Glue.tid() } else { 0 };
    if take {
        lock.lock(&CSys, me);
    }
    let r = f(&Glue, file, tb::stream_ext_at(ext));
    if take {
        lock.unlock(&CSys, me);
    }
    Ok(r)
}

/// The front path of byte output (P5, before [`with`]'s general plumbing): a stream already set
/// up, byte-oriented, with room in its open write window gets `data` there, under its lock as
/// [`with`] decides; whether it applied. Anything else (a standard stream's first use, an
/// undecided or wide stream, a closed or full window: line-buffered, unbuffered, reading, at a
/// flush) returns false and the caller goes [`with`]'s way, unchanged.
/// Inlined into each export, so a single byte is one store; the locked case is out of line.
/// P5b (D-12.19): an open published window is the readiness test (the window-closed
/// invariant), then the record's seal; no orientation, role or set-up checks are left here.
/// The answer is [`Front::Locked`] when the stream lock must be taken: the caller then goes out
/// of line to [`put_front_locked`], so the inlined part needs no frame and every other case
/// ends in one store or one jump.
#[inline(always)]
fn put_front(fp: usize, how: Locking, data: &[u8]) -> Front {
    put_front_ext(fp, how, data).0
}

/// [`put_front`] with the record it accepted (meaningful with [`Front::Locked`]): the locked
/// paths take the lock in that record without reading and checking it again (PUTC-FRONT).
#[inline(always)]
fn put_front_ext(fp: usize, how: Locking, data: &[u8]) -> (Front, usize) {
    let Some(file) = tb::file_words(fp) else { return (Front::General, 0) };
    let Some(p) = sc::window_room(file, data.len()) else { return (Front::General, 0) };
    let ext = sc::ext_word(file);
    if !Glue.record(fp, ext) {
        return (Front::General, 0);
    }
    // `alone` first: in a process with one thread the flags word is not loaded at all.
    if how == Locking::Unlocked || crate::thread::alone() || sc::by_caller(file) {
        let done = sc::put_front(&Glue, file, tb::stream_ext_at(ext), data, p);
        return (if done { Front::Done } else { Front::General }, ext);
    }
    (Front::Locked, ext)
}

/// What [`put_front`] did.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Front {
    /// The bytes are in the window.
    Done,
    /// Try again under the stream's lock ([`put_front_locked`]).
    Locked,
    /// The general path.
    General,
}

/// [`put_front`], then [`put_front_locked`] when the lock is needed: whether the bytes went in.
#[inline(always)]
fn front_done(fp: usize, how: Locking, data: &[u8]) -> bool {
    match put_front_ext(fp, how, data) {
        (Front::Done, _) => true,
        (Front::Locked, ext) => put_front_locked_at(fp, ext, data),
        (Front::General, _) => false,
    }
}

/// [`put_front`] under the stream's lock (another thread may use the stream): the record read
/// and checked, then [`front_locked`].
#[inline(never)]
fn put_front_locked(fp: usize, data: &[u8]) -> bool {
    let Some(file) = tb::file_words(fp) else { return false };
    let ext = sc::ext_word(file);
    Glue.record(fp, ext) && front_locked(fp, ext, data)
}

/// [`front_locked`] out of line, for a record [`put_front_ext`] accepted.
#[inline(never)]
fn put_front_locked_at(fp: usize, ext: usize, data: &[u8]) -> bool {
    front_locked(fp, ext, data)
}

/// The front path under the lock of `ext` (a record [`Glue::record`] accepted for `fp`): the
/// uncontended form [`front_lock_fast`], else [`front_under_lock`] out of line.
#[inline(always)]
fn front_locked(fp: usize, ext: usize, data: &[u8]) -> bool {
    front_lock_fast(fp, ext, data).unwrap_or_else(|| front_locked_full(fp, ext, data))
}

/// [`front_under_lock`] out of line: the lock held by another thread, or no thread block TID.
#[cold]
#[inline(never)]
fn front_locked_full(fp: usize, ext: usize, data: &[u8]) -> bool {
    front_under_lock(fp, ext, data)
}

/// STDIO-MT-FPUTS: the front path under an uncontended lock, as [`putc_lock_c`] does it (the
/// thread block's TID, [`StreamLock::lock_fast`], the window, the copy, [`StreamLock::unlock_held`];
/// the wake out of line): whether the bytes went in, or `None` with nothing taken when the
/// lock is another thread's or the TID is not the thread block's (then [`front_under_lock`]).
/// Once a second thread existed, `fputs` of 32 bytes went from its leaf through `fputs_slow`
/// and [`put_front_locked`] (the record and window checked again, the generic lock and
/// unlock): 126 -> 242 instructions per call, glibc 148 -> 153.
#[inline(always)]
fn front_lock_fast(fp: usize, ext: usize, data: &[u8]) -> Option<bool> {
    match front_lock_try(fp, ext, data) {
        Held::Done => Some(true),
        Held::Missed => Some(false),
        Held::Wake(done) => {
            front_wake(Glue.lock_of(ext));
            Some(done)
        }
        Held::Full => None,
    }
}

/// What [`front_lock_try`] did.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Held {
    /// The bytes are in the window and the lock is released.
    Done,
    /// The front path did not apply (a closed or full window, or no record words); the lock
    /// is released: the general path, which takes the lock again.
    Missed,
    /// The lock is released and another thread waits for it: wake it ([`StreamLock::wake`]),
    /// then go on as [`Held::Done`] (true) or [`Held::Missed`] (false).
    Wake(bool),
    /// Nothing taken: the lock is another thread's, or the thread block holds no TID
    /// ([`front_under_lock`]).
    Full,
}

/// STDIO-LOCKED-PATH: the uncontended locked front path with no call in it (the TID, the lock,
/// the window, the copy, the release; the copy is inline when the caller has bounded `data` to
/// 1 to [`sc::FRONT_INLINE`] bytes, see [`front_bounded`]). Every case it
/// cannot finish is an answer, so the caller ends each of them in one tail call and its locked
/// leaf needs no stack frame (the `fputs` locked leaf had six saved registers and a frame
/// because of its calls to the wake, the copy and the fallbacks between the lock and the
/// return).
#[inline(always)]
fn front_lock_try(fp: usize, ext: usize, data: &[u8]) -> Held {
    let Some(file) = tb::file_words(fp) else { return Held::Missed };
    let Some(me) = thread::own_tid_fast() else { return Held::Full };
    let lock = Glue.lock_of(ext);
    if !lock.lock_fast(me) {
        return Held::Full;
    }
    // The short form for data within the leaves' bound (inline, no call); longer data as before.
    let put = |p| {
        let e = tb::stream_ext_at(ext);
        if front_bounded(data) { sc::put_front_short(&Glue, file, e, data, p) } else { sc::put_front(&Glue, file, e, data, p) }
    };
    let done = sc::window_room(file, data.len()).is_some_and(put);
    if lock.unlock_held() {
        return Held::Wake(done);
    }
    if done { Held::Done } else { Held::Missed }
}

/// Whether `data` is within the leaves' bound (1 to [`sc::FRONT_INLINE`] bytes). The locked
/// leaves test it again before [`front_lock_try`]: they are called only with such data, and the
/// test lets the compiler drop the copy's `memcpy` call (and with it the stack frame).
#[inline(always)]
fn front_bounded(data: &[u8]) -> bool {
    data.len().wrapping_sub(1) < sc::FRONT_INLINE
}

/// [`front_lock_fast`]'s unlock freed a lock another thread waits for: wake it.
#[cold]
#[inline(never)]
fn front_wake(lock: &StreamLock) {
    lock.wake(&CSys);
}

/// The front path under the lock of `ext`, a record [`Glue::record`] accepted for `fp` (the
/// record stays the stream's while the `FILE` is open: `freopen` sets it up again in place):
/// the window and the buffer are read again under the lock. Inlined into the out-of-line locked
/// paths (PUTC-FRONT: [`putc_lock_c`] stores one byte without a call or a copy dispatch).
#[inline(always)]
fn front_under_lock(fp: usize, ext: usize, data: &[u8]) -> bool {
    let Some(file) = tb::file_words(fp) else { return false };
    let lock = Glue.lock_of(ext);
    let me = Glue.tid();
    lock.lock(&CSys, me);
    let done = sc::window_room(file, data.len()).is_some_and(|p| sc::put_front(&Glue, file, tb::stream_ext_at(ext), data, p));
    lock.unlock(&CSys, me);
    done
}

/// The current `stdin`, `stdout`, `stderr` (the variables; programs may assign them).
pub fn stdin() -> usize {
    data::stdin.load(Relaxed).expose_provenance()
}

/// See [`stdin`].
pub fn stdout() -> usize {
    data::stdout.load(Relaxed).expose_provenance()
}

/// See [`stdin`].
pub fn stderr() -> usize {
    data::stderr.load(Relaxed).expose_provenance()
}

// ------------------------------------------------------------------------------ open, close

/// A new stream on `kind`: its `FILE *`.
fn open_stream(kind: Kind, mode: &sc::Mode, fd: i32) -> KResult<usize> {
    let fp = new_slot()?;
    let (file, _, ext) = parts(fp)?;
    sc::setup(&Glue, file, tb::stream_ext_at(ext), kind, mode, fd);
    sc::link(&Glue, fp, file);
    Ok(fp)
}

/// A write-only append stream (`a`, which POSIX opens "for writing at end-of-file") starts at
/// the end of the file, as of opening; `a+` reads from the start (D-12.16). A descriptor that
/// cannot seek keeps its position.
fn start_at_end(fd: Fd, m: &sc::Mode) {
    if m.append && !m.read {
        let _ = CSys.lseek(fd, 0, liella_kernel::uapi::SEEK_END);
    }
}

/// `fopen(path, mode)`.
pub fn fopen(path: KResult<KStr<'_>>, mode: KResult<&[u8]>) -> KResult<usize> {
    let mode = mode?;
    let m = sc::parse_mode(mode)?;
    // `,ccs=CHARSET`: wide-oriented on that charset (an unknown one fails, as `iconv_open`).
    let codec = match sc::mode_charset(mode) {
        Some(name) => Some(crate::iconv::codec_named(name).ok_or(Errno::EINVAL)?),
        None => None,
    };
    let fd = CSys.openat(Fd::CWD, path?, sc::open_flags(&m), 0o666)?;
    start_at_end(fd, &m);
    let fp = open_stream(Kind::Fd(fd.raw()), &m, fd.raw()).inspect_err(|_| {
        let _ = CSys.close(fd);
    })?;
    if let Some(c) = codec {
        with(fp, Locking::Locked, |_, file, ext| sc::set_wide(file, ext, sc::Codec::charset(c)))?;
    }
    Ok(fp)
}

/// `fdopen(fd, mode)`: the mode must fit the descriptor's access mode. `a`/`a+` set `O_APPEND`
/// on the descriptor when it lacks it (every write then lands at the end atomically, through the
/// stream or any descriptor sharing the open file description) and keep its offset, which
/// becomes the stream's position (POSIX fdopen; D-12.16, owner ruling 2026-10-04).
pub fn fdopen(fd: i32, mode: KResult<&[u8]>) -> KResult<usize> {
    use liella_kernel::ops::FcntlInt;
    use liella_kernel::uapi::{O_APPEND, O_RDONLY, O_RDWR, O_WRONLY};
    let m = sc::parse_mode(mode?)?;
    let flags = CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFl, 0)?;
    let acc = u32::try_from(flags).map_err(|_| Errno::EBADF)? & (O_RDONLY | O_WRONLY | O_RDWR);
    if (m.read && acc == O_WRONLY) || (m.write && acc == O_RDONLY) {
        return Err(Errno::EINVAL);
    }
    if m.append && u32::try_from(flags).unwrap_or(0) & O_APPEND == 0 {
        CSys.fcntl_int(Fd::new(fd), FcntlInt::SetFl, flags | i64::from(O_APPEND))?;
    }
    open_stream(Kind::Fd(fd), &m, fd)
}

/// `fclose(fp)`: the stream leaves the list, is closed and its slot recycled (a standard
/// stream's `FILE` stays, closed).
pub fn fclose(fp: usize) -> KResult<()> {
    let (file, lock, ext) = parts(fp)?;
    sc::unlink(&Glue, fp);
    let me = Glue.tid();
    lock.lock(&CSys, me);
    let r = sc::close(&Glue, file, tb::stream_ext_at(ext));
    lock.unlock(&CSys, me);
    if standard_fd(fp).is_none() {
        recycle(fp);
    }
    r
}

/// `fcloseall`: close every stream. 0, or `EOF` if a close failed.
pub fn fcloseall() -> KResult<()> {
    let mut result = Ok(());
    sc::close_every(&Glue, &mut |fp| {
        if let Err(e) = fclose(fp) {
            result = Err(e);
        }
    });
    result
}

/// `freopen(path, mode, fp)`: the stream's backing is closed (errors ignored) and the same
/// `FILE` reopened on `path`, on the same descriptor number when the old backing had one
/// (`dup3`). A NULL path reopens the same descriptor with the new mode.
pub fn freopen(path: KResult<Option<KStr<'_>>>, mode: KResult<&[u8]>, fp: usize) -> KResult<usize> {
    let m = sc::parse_mode(mode?)?;
    let path = path?;
    let (file, lock, ext_addr) = parts(fp)?;
    let me = Glue.tid();
    lock.lock(&CSys, me);
    let ext = tb::stream_ext_at(ext_addr);
    let old_fd = match ext.kind {
        Kind::Fd(fd) => Some(fd),
        _ => None,
    };
    let result = (|| {
        let fd = match path {
            Some(p) => {
                let _ = sc::flush(&Glue, file, ext);
                let new = CSys.openat(Fd::CWD, p, sc::open_flags(&m), 0o666)?;
                let fd = match old_fd {
                    Some(o) if o != new.raw() => {
                        let cloexec = if m.cloexec { liella_kernel::uapi::O_CLOEXEC } else { 0 };
                        let r = CSys.dup3(new, Fd::new(o), cloexec);
                        let _ = CSys.close(new);
                        r?.raw()
                    }
                    _ => new.raw(),
                };
                // The old backing is replaced: a descriptor of another number is closed.
                if let Some(o) = old_fd.filter(|&o| o != fd) {
                    let _ = CSys.close(Fd::new(o));
                }
                fd
            }
            None => {
                // §12.2: a NULL path changes only what `fcntl` can: the access mode must fit
                // the descriptor's (`EBADF`, as POSIX names it), `O_APPEND` and close-on-exec
                // follow the mode, and nothing is reopened or truncated.
                use liella_kernel::ops::FcntlInt;
                use liella_kernel::uapi::{O_APPEND, O_RDONLY, O_RDWR, O_WRONLY};
                let o = old_fd.ok_or(Errno::EBADF)?;
                let flags = CSys.fcntl_int(Fd::new(o), FcntlInt::GetFl, 0)?;
                let f = u32::try_from(flags).map_err(|_| Errno::EBADF)?;
                let acc = f & (O_RDONLY | O_WRONLY | O_RDWR);
                if (m.read && acc == O_WRONLY) || (m.write && acc == O_RDONLY) {
                    return Err(Errno::EBADF);
                }
                let _ = sc::flush(&Glue, file, ext);
                let want = if m.append { f | O_APPEND } else { f & !O_APPEND };
                if want != f {
                    CSys.fcntl_int(Fd::new(o), FcntlInt::SetFl, i64::from(want))?;
                }
                if m.cloexec {
                    CSys.fcntl_int(Fd::new(o), FcntlInt::SetFd, 1)?;
                }
                o
            }
        };
        // As if the file were opened again with the new mode (POSIX), with or without a path.
        start_at_end(Fd::new(fd), &m);
        if ext.buf_owned && ext.buf != 0 {
            Glue.free(ext.buf);
        }
        sc::setup(&Glue, file, ext, Kind::Fd(fd), &m, fd);
        Ok(fp)
    })();
    lock.unlock(&CSys, me);
    result
}

/// `fileno(fp)`: the descriptor, or `EBADF` for a stream without one.
pub fn fileno(fp: usize, how: Locking) -> KResult<i32> {
    with(fp, how, |_, _, ext| match ext.kind {
        Kind::Fd(fd) => Ok(fd),
        _ => Err(Errno::EBADF),
    })?
}

/// `fflush(fp)`; `fflush(NULL)` flushes every stream.
pub fn fflush(fp: usize, how: Locking) -> KResult<()> {
    if fp == 0 {
        return sc::flush_all(&Glue);
    }
    with(fp, how, sc::flush)?
}

/// At `exit`: every stream's pending output is written (§12.1).
pub fn flush_at_exit() {
    let _ = sc::flush_all(&Glue);
}

/// In a `fork` child: stream locks held by threads that are gone become free.
pub fn after_fork_in_child() {
    sc::reset_locks_in_child(&Glue);
}

// ------------------------------------------------------------------------------ operations

/// `fgetc` (and `__uflow` unlocked): the next byte, `None` at end of input.
pub fn getc(fp: usize, how: Locking) -> KResult<Option<u8>> {
    match read_front(fp, how) {
        Ok(b) => Ok(Some(b)),
        Err(f) => getc_past_front(fp, how, f),
    }
}

/// [`getc`] with its C result (the byte, or `EOF` with `errno` set; the exports' form): the
/// read front path, else one tail call to [`getc_slow`] (STDIO-2).
#[inline(always)]
pub fn getc_c(fp: usize, how: Locking) -> core::ffi::c_int {
    match read_front_ext(fp, how) {
        Ok(b) => core::ffi::c_int::from(b),
        // FILE-LOCK-3: `fp` passed `file_words` in the front path; inlined here, this second
        // look folds into it, and the words go on as one fixed-size reference (no re-check).
        Err((ReadFront::Locked, ext)) => match file_array(fp) {
            Some(file) => getc_lock_c(fp, how, file, ext),
            None => getc_slow_general(fp, how),
        },
        // PERF-GETC-PUTC-2 (trim d): the general case is its own tail call with no `ReadFront`
        // argument, so no constant for it is set up on the hot path (it was hoisted to entry).
        Err((ReadFront::General, _)) => getc_slow_general(fp, how),
        Err((f, _)) => getc_slow(fp, how, f),
    }
}

/// A stream's words as one fixed-size reference ([`tb::file_words`]; the length is a constant,
/// so the conversion is free): passed to an out-of-line path, the stream need not be checked
/// again there (FILE-LOCK-3).
#[inline(always)]
fn file_array(fp: usize) -> Option<&'static [AtomicUsize; FILE_WORDS]> {
    tb::file_words(fp).and_then(|w| w.try_into().ok())
}

/// [`getc_c`] when the stream's lock must be taken (FILE-LOCK-2): the lock taken without waiting
/// ([`StreamLock::lock_fast`], the TID from the thread block), a byte in the open read window,
/// the lock released; every call it could need (a TID by system call, waiting for the lock, waking
/// a waiter, the slow path) is a tail call, so this path has no stack frame. Anything else is
/// [`getc_lock_full`]. FILE-LOCK-3: `file` and `ext` are what [`read_front_ext`] accepted (the
/// words checked, the record sealed for `fp`), so neither is checked again here.
#[inline(never)]
fn getc_lock_c(fp: usize, how: Locking, file: &'static [AtomicUsize; FILE_WORDS], ext: usize) -> core::ffi::c_int {
    let Some(me) = thread::own_tid_fast() else { return getc_lock_full(fp, how, ext) };
    let lock = Glue.lock_of(ext);
    if !lock.lock_fast(me) {
        return getc_lock_full(fp, how, ext);
    }
    let got = sc::read_room(file).and_then(|p| sc::get_front(&Glue, file, tb::stream_ext_at(ext), p));
    if lock.unlock_held() {
        return getc_wake(lock, fp, how, got);
    }
    match got {
        Some(b) => core::ffi::c_int::from(b),
        None => getc_slow(fp, how, ReadFront::Locked),
    }
}

/// [`getc_lock_c`] after its unlock freed a lock another thread waits for: wake it, then the result.
#[cold]
#[inline(never)]
fn getc_wake(lock: &StreamLock, fp: usize, how: Locking, got: Option<u8>) -> core::ffi::c_int {
    lock.wake(&CSys);
    match got {
        Some(b) => core::ffi::c_int::from(b),
        None => getc_slow(fp, how, ReadFront::Locked),
    }
}

/// [`getc_lock_c`]'s full form: the lock taken however it must be (waiting for it), a byte in the
/// open read window under it, everything read again under it (PUTC-FRONT); anything else (an
/// empty window, a refill, the engine) is [`getc_slow`], which takes the lock again in
/// [`getc_locked`].
#[cold]
#[inline(never)]
fn getc_lock_full(fp: usize, how: Locking, ext: usize) -> core::ffi::c_int {
    if let Some(file) = tb::file_words(fp) {
        let lock = Glue.lock_of(ext);
        let me = Glue.tid();
        lock.lock(&CSys, me);
        let got = sc::read_room(file).and_then(|p| sc::get_front(&Glue, file, tb::stream_ext_at(ext), p));
        lock.unlock(&CSys, me);
        if let Some(b) = got {
            return core::ffi::c_int::from(b);
        }
    }
    getc_slow(fp, how, ReadFront::Locked)
}

/// [`getc_c`] past its front path.
#[cold]
#[inline(never)]
fn getc_slow(fp: usize, how: Locking, f: ReadFront) -> core::ffi::c_int {
    crate::ret::byte_or_eof(getc_past_front(fp, how, f))
}

/// [`getc_slow`] for [`ReadFront::General`]: the engine's path ([`getc_general`]).
#[cold]
#[inline(never)]
fn getc_slow_general(fp: usize, how: Locking) -> core::ffi::c_int {
    crate::ret::byte_or_eof(getc_general(fp, how))
}

/// [`getc`] past its front path: under the lock when needed ([`getc_locked`]); a sealed record
/// that needs no lock and has an empty window tries the refill path ([`sc::refill`], STDIO-6:
/// before [`with`]'s plumbing, the record already checked by [`read_front`]); else the engine.
#[inline(always)]
fn getc_past_front(fp: usize, how: Locking, f: ReadFront) -> KResult<Option<u8>> {
    match f {
        ReadFront::Locked => getc_locked(fp, how),
        ReadFront::Empty => getc_refill(fp, how),
        ReadFront::General => getc_general(fp, how),
    }
}

/// What [`read_front`] leaves to do.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadFront {
    /// Try again under the stream's lock ([`getc_locked`]).
    Locked,
    /// An empty published window on a stream whose words name a buffer: [`getc_refill`].
    Empty,
    /// The general path.
    General,
}

/// The read front path (STDIO-2, D-12.19), the mirror of [`put_front`]: an open published read
/// window on a sealed record, under the lock as [`with`] decides; the byte, or what to do next.
#[inline(always)]
fn read_front(fp: usize, how: Locking) -> Result<u8, ReadFront> {
    read_front_ext(fp, how).map_err(|(f, _)| f)
}

/// [`read_front`] with the record it accepted (meaningful with [`ReadFront::Locked`]: the lock
/// is taken in it without reading and checking it again, as [`put_front_ext`]).
#[inline(always)]
fn read_front_ext(fp: usize, how: Locking) -> Result<u8, (ReadFront, usize)> {
    let Some(file) = tb::file_words(fp) else { return Err((ReadFront::General, 0)) };
    let Some(p) = sc::read_room(file) else {
        // STDIO-6: no buffer published (a first read) goes straight to the engine.
        return Err((if sc::buf_hint(file) { ReadFront::Empty } else { ReadFront::General }, 0));
    };
    let ext = sc::ext_word(file);
    if !Glue.record(fp, ext) {
        return Err((ReadFront::General, 0));
    }
    if how == Locking::Unlocked || crate::thread::alone() || sc::by_caller(file) {
        return sc::get_front(&Glue, file, tb::stream_ext_at(ext), p).ok_or((ReadFront::General, 0));
    }
    Err((ReadFront::Locked, ext))
}

/// [`getc`] on an empty window (STDIO-6): on a sealed record that needs no lock, the refill
/// path ([`sc::refill`]) before [`with`]'s plumbing; under the lock [`getc_locked`]; else the
/// engine.
#[inline(never)]
fn getc_refill(fp: usize, how: Locking) -> KResult<Option<u8>> {
    let Some(file) = tb::file_words(fp) else { return getc_general(fp, how) };
    let ext = sc::ext_word(file);
    if !Glue.record(fp, ext) {
        return getc_general(fp, how);
    }
    if how == Locking::Unlocked || crate::thread::alone() || sc::by_caller(file) {
        if let Some(r) = sc::refill(&Glue, file, tb::stream_ext_at(ext)) {
            return r;
        }
        return getc_general(fp, how);
    }
    getc_locked(fp, how)
}

/// [`getc`] under the stream's lock (another thread may use the stream): the front path again
/// with everything read under it, else the general path.
#[inline(never)]
fn getc_locked(fp: usize, how: Locking) -> KResult<Option<u8>> {
    let front = (|| {
        let file = tb::file_words(fp)?;
        let (lock, ext) = sc::parts(&Glue, file)?;
        let me = Glue.tid();
        lock.lock(&CSys, me);
        let r = match sc::read_room(file) {
            Some(p) => sc::get_front(&Glue, file, tb::stream_ext_at(ext), p).map(|b| Ok(Some(b))),
            None => sc::refill(&Glue, file, tb::stream_ext_at(ext)),
        };
        lock.unlock(&CSys, me);
        r
    })();
    match front {
        Some(r) => r,
        None => getc_general(fp, how),
    }
}

/// [`getc`] past its front path: the engine's.
#[cold]
#[inline(never)]
fn getc_general(fp: usize, how: Locking) -> KResult<Option<u8>> {
    with(fp, how, sc::getc)?
}

/// `__underflow`: the next byte without consuming it.
pub fn peek(fp: usize) -> KResult<Option<u8>> {
    with(fp, Locking::Unlocked, |g, file, ext| {
        let c = sc::getc(g, file, ext)?;
        if let Some(b) = c {
            sc::ungetc(g, file, ext, b)?;
        }
        Ok(c)
    })?
}

fn low_byte(c: i32) -> u8 {
    c.to_ne_bytes()[0]
}

/// `fputc`: the byte written.
#[inline]
pub fn putc(fp: usize, how: Locking, c: i32) -> KResult<u8> {
    let b = low_byte(c);
    match put_front(fp, how, &[b]) {
        Front::Done => Ok(b),
        Front::Locked => putc_locked(fp, how, b),
        Front::General => putc_general(fp, how, b),
    }
}

/// [`putc`] with its C result (the byte, or `EOF` with `errno` set; the exports' form). Every
/// case but the front path's is one tail call to [`putc_slow`], so the inlined part needs no
/// frame and no saved register (STDIO-HOT).
#[inline(always)]
pub fn putc_c(fp: usize, how: Locking, c: i32) -> core::ffi::c_int {
    let b = low_byte(c);
    match put_front_ext(fp, how, &[b]) {
        (Front::Done, _) => core::ffi::c_int::from(b),
        (Front::Locked, ext) => putc_lock_c(fp, how, b, ext),
        (Front::General, _) => putc_slow(fp, how, b, Front::General),
    }
}

/// [`putc_c`] when the stream's lock must be taken (another thread exists): the front path
/// under the lock in one function, everything read again under it. FILE-LOCK-2: as
/// [`getc_lock_c`], the lock taken without waiting and every call a tail call (no stack frame);
/// anything else is [`putc_lock_full`]. PUTC-FRONT: the lock, the window and the one-byte store
/// without the layers of calls between them.
#[inline(never)]
fn putc_lock_c(fp: usize, how: Locking, b: u8, ext: usize) -> core::ffi::c_int {
    let Some(file) = tb::file_words(fp) else { return putc_slow(fp, how, b, Front::General) };
    let Some(me) = thread::own_tid_fast() else { return putc_lock_full(fp, how, b, ext) };
    let lock = Glue.lock_of(ext);
    if !lock.lock_fast(me) {
        return putc_lock_full(fp, how, b, ext);
    }
    let done = sc::window_room(file, 1).is_some_and(|p| sc::put_front(&Glue, file, tb::stream_ext_at(ext), &[b], p));
    if lock.unlock_held() {
        return putc_wake(lock, fp, how, b, done);
    }
    if done { core::ffi::c_int::from(b) } else { putc_slow(fp, how, b, Front::General) }
}

/// [`putc_lock_c`] after its unlock freed a lock another thread waits for: wake it, then the result.
#[cold]
#[inline(never)]
fn putc_wake(lock: &StreamLock, fp: usize, how: Locking, b: u8, done: bool) -> core::ffi::c_int {
    lock.wake(&CSys);
    if done { core::ffi::c_int::from(b) } else { putc_slow(fp, how, b, Front::General) }
}

/// [`putc_lock_c`]'s full form ([`front_under_lock`]: the lock taken however it must be); when
/// the front path does not apply, [`putc_slow`] goes the general way as [`putc_locked`] does.
#[cold]
#[inline(never)]
fn putc_lock_full(fp: usize, how: Locking, b: u8, ext: usize) -> core::ffi::c_int {
    if front_under_lock(fp, ext, &[b]) {
        return core::ffi::c_int::from(b);
    }
    putc_slow(fp, how, b, Front::General)
}

/// [`putc_c`] past its front path: the locked or the general way, as [`putc`] goes.
#[cold]
#[inline(never)]
fn putc_slow(fp: usize, how: Locking, b: u8, f: Front) -> core::ffi::c_int {
    crate::ret::byte_written(if f == Front::Locked { putc_locked(fp, how, b) } else { putc_general(fp, how, b) })
}

/// [`putc`] under the stream's lock: the front path again, else the general one.
#[inline(never)]
fn putc_locked(fp: usize, how: Locking, b: u8) -> KResult<u8> {
    if put_front_locked(fp, &[b]) {
        return Ok(b);
    }
    putc_general(fp, how, b)
}

/// [`putc`] past its front path.
#[cold]
#[inline(never)]
fn putc_general(fp: usize, how: Locking, b: u8) -> KResult<u8> {
    with(fp, how, |g, file, ext| sc::putc(g, file, ext, b).map(|()| b))?
}

/// `__overflow(fp, c)`: write `c`; with `EOF`, only flush (0).
pub fn overflow(fp: usize, c: i32) -> KResult<i32> {
    if c == -1 {
        return with(fp, Locking::Unlocked, sc::flush)?.map(|()| 0);
    }
    putc(fp, Locking::Unlocked, c).map(i32::from)
}

/// `ungetc(c, fp)`: `EOF` is refused.
pub fn ungetc(fp: usize, c: i32) -> KResult<u8> {
    if c == -1 {
        return Err(Errno::EINVAL);
    }
    let b = low_byte(c);
    with(fp, Locking::Locked, |g, file, ext| sc::ungetc(g, file, ext, b).map(|()| b))?
}

/// `fgets(buf, n, fp)`: the bytes stored, `None` at end of input before any byte.
pub fn fgets(buf: KResult<&mut [u8]>, fp: usize, how: Locking) -> KResult<Option<usize>> {
    let buf = buf?;
    if buf.is_empty() {
        return Err(Errno::EINVAL);
    }
    match window_front(fp, how, |file, ext, p| sc::gets_front(&Glue, file, ext, p, buf)) {
        Ok(n) => Ok(Some(n)),
        Err(f) => fgets_slow(buf, fp, how, f),
    }
}

/// [`fgets`] past its front path: the front path again under the stream's lock when another
/// thread may use the stream, else the engine's.
#[inline(never)]
fn fgets_slow(buf: &mut [u8], fp: usize, how: Locking, f: Front) -> KResult<Option<usize>> {
    if f == Front::Locked {
        if let Some(n) = window_locked(fp, |file, ext, p| sc::gets_front(&Glue, file, ext, p, buf)) {
            return Ok(Some(n));
        }
    }
    with(fp, how, |g, file, ext| sc::gets_into(g, file, ext, buf))?
}

/// The read front paths' entry (STDIO-2, D-12.19), as [`read_front`] decides for one byte: an
/// open published read window on a sealed record and the lock held or needed by none; then `f`
/// (the stream's words, its record, `_IO_read_ptr`) decides, `None` meaning the general path.
#[inline(always)]
fn window_front<R>(fp: usize, how: Locking, f: impl FnOnce(&'static [AtomicUsize], &Ext, usize) -> Option<R>) -> Result<R, Front> {
    let Some(file) = tb::file_words(fp) else { return Err(Front::General) };
    let Some(p) = sc::read_room(file) else { return Err(Front::General) };
    let ext = sc::ext_word(file);
    if !Glue.record(fp, ext) {
        return Err(Front::General);
    }
    if how == Locking::Unlocked || crate::thread::alone() || sc::by_caller(file) {
        return f(file, tb::stream_ext_at(ext), p).ok_or(Front::General);
    }
    Err(Front::Locked)
}

/// [`window_front`] under the stream's lock (another thread may use the stream): everything is
/// read again under it.
#[inline(always)]
fn window_locked<R>(fp: usize, f: impl FnOnce(&'static [AtomicUsize], &Ext, usize) -> Option<R>) -> Option<R> {
    let file = tb::file_words(fp)?;
    let (lock, ext) = sc::parts(&Glue, file)?;
    let me = Glue.tid();
    lock.lock(&CSys, me);
    let r = sc::read_room(file).and_then(|p| f(file, tb::stream_ext_at(ext), p));
    lock.unlock(&CSys, me);
    r
}

/// `__fgets_chk(s, size, n, fp)` (C0.5): `fgets` with `n` bytes into an object of `buf.len()`
/// (`min(n, size)`). The process terminates only when the line would not fit the object: the
/// buffer filled without a newline and another byte follows (`fgets` would have written past
/// the object); at the end of the input it fits.
pub fn fgets_chk(buf: KResult<&mut [u8]>, n: usize, fp: usize, how: Locking) -> KResult<Option<usize>> {
    let buf = buf?;
    let cap = buf.len();
    if cap == 0 {
        return Err(Errno::EINVAL);
    }
    with(fp, how, |g, file, ext| {
        let got = sc::gets_into(g, file, ext, buf)?;
        let filled = got.is_some_and(|k| k.saturating_add(1) == cap && k.checked_sub(1).and_then(|i| buf.get(i)) != Some(&b'\n'));
        if n > cap && filled && sc::getc(g, file, ext).ok().flatten().is_some() {
            crate::ret::chk_fail();
        }
        Ok(got)
    })?
}

/// `fputs(s, fp)`: a non-negative count (1, as the reference library returns). FPUTS-FUSE: the
/// front path as [`fwrite_elements`]'s leaf (D-12.19): a string of 1 to [`sc::FRONT_INLINE`]
/// bytes that fits the open window is the inline copy and nothing else; every other case is one
/// jump to [`fputs_slow`] (fputs_unlocked at 12 bytes: 81 of 126 instructions per call were the
/// out-of-line `fputs` with the locked path and the general call inlined into it).
#[inline(always)]
pub fn fputs(s: KResult<&[u8]>, fp: usize, how: Locking) -> KResult<usize> {
    let s = s?;
    if front_bounded(s) {
        return match put_front_ext(fp, how, s) {
            (Front::Done, _) => Ok(1),
            (Front::Locked, ext) => fputs_locked(s, fp, ext).map(|()| 1),
            (Front::General, _) => fputs_slow(s, fp, WriteFrom::General(how)),
        };
    }
    fputs_long(s, fp, how)
}

/// [`fputs`]'s leaf when the lock must be taken (STDIO-MT-FPUTS): the front path under the lock
/// in the record the leaf accepted, without reading and checking it again; the general path when
/// the front path does not apply. STDIO-LOCKED-PATH: [`front_lock_try`] and one tail call for
/// each case it does not finish, so the uncontended call has no stack frame. The answer is
/// `Ok(())` for [`fputs`]'s 1: a `KResult<usize>` is returned in memory, and a call that returns
/// one was not made a tail call (three saved registers and a frame).
#[inline(never)]
fn fputs_locked(s: &[u8], fp: usize, ext: usize) -> KResult<()> {
    if !front_bounded(s) {
        return fputs_lock_full(s, fp, ext);
    }
    match front_lock_try(fp, ext, s) {
        Held::Done => Ok(()),
        Held::Missed => fputs_general_unit(s, fp),
        Held::Wake(done) => fputs_wake(s, fp, ext, done),
        Held::Full => fputs_lock_full(s, fp, ext),
    }
}

/// [`fputs_locked`] after its release freed a lock another thread waits for: wake it, then the
/// answer ([`Held::Wake`]).
#[cold]
#[inline(never)]
fn fputs_wake(s: &[u8], fp: usize, ext: usize, done: bool) -> KResult<()> {
    front_wake(Glue.lock_of(ext));
    if done { Ok(()) } else { fputs_general_unit(s, fp) }
}

/// [`fputs_general`] with the lock for [`fputs_locked`]: its answer is always 1 when it is not
/// an error.
#[inline(never)]
fn fputs_general_unit(s: &[u8], fp: usize) -> KResult<()> {
    fputs_general(s, fp, Locking::Locked).map(|_| ())
}

/// [`fputs_locked`] when the lock could not be taken without waiting: [`front_under_lock`]
/// (the lock taken however it must be), else the general path.
#[cold]
#[inline(never)]
fn fputs_lock_full(s: &[u8], fp: usize, ext: usize) -> KResult<()> {
    if front_under_lock(fp, ext, s) {
        return Ok(());
    }
    fputs_general_unit(s, fp)
}

/// [`fputs`] of an empty text or one longer than its leaf: the whole front path, then the
/// general path, as before (its own entry, so a long text pays no [`WriteFrom`] dispatch).
#[inline(never)]
fn fputs_long(s: &[u8], fp: usize, how: Locking) -> KResult<usize> {
    if front_done(fp, how, s) {
        return Ok(1);
    }
    fputs_general(s, fp, how)
}

/// [`fputs`] past its leaf: the front path with the lock or the general path, as before.
#[inline(never)]
fn fputs_slow(s: &[u8], fp: usize, from: WriteFrom) -> KResult<usize> {
    let how = match from {
        WriteFrom::Locked if put_front_locked(fp, s) => return Ok(1),
        WriteFrom::Start(how) | WriteFrom::General(how) => how,
        WriteFrom::Locked => Locking::Locked,
        WriteFrom::Fail(e) => return Err(e),
    };
    fputs_general(s, fp, how)
}

/// [`fputs`] past its front path.
#[inline(never)]
fn fputs_general(s: &[u8], fp: usize, how: Locking) -> KResult<usize> {
    let t = with(fp, how, |g, file, ext| sc::write(g, file, ext, s))?;
    t.error.map_or(Ok(1), Err)
}

/// `puts(s)`: `s` and a newline on `stdout`; the bytes written. FPUTS-FUSE: when no lock is
/// needed (one thread, or the caller took over locking) and the open window holds `s` and the
/// newline, both go there ([`puts_front`]); otherwise, and for whatever the front path left, the
/// general path as before.
#[inline(always)]
pub fn puts(s: KResult<&[u8]>) -> KResult<usize> {
    let s = s?;
    let fp = stdout();
    match puts_front(fp, s) {
        2 => Ok(s.len().saturating_add(1)),
        done => puts_general(fp, s, done == 1),
    }
}

/// [`puts`]'s front path (D-12.19, the checks of [`put_front_ext`] for `s.len() + 1` bytes): 2 when
/// `s` and the newline are in the window, 1 when only `s` is (the newline is then the general
/// path's), 0 when nothing was written.
#[inline(always)]
fn puts_front(fp: usize, s: &[u8]) -> u8 {
    let Some(file) = tb::file_words(fp) else { return 0 };
    let Some(p) = sc::window_room(file, s.len().saturating_add(1)) else { return 0 };
    let ext = sc::ext_word(file);
    if !Glue.record(fp, ext) || !(crate::thread::alone() || sc::by_caller(file)) {
        return 0;
    }
    let x = tb::stream_ext_at(ext);
    if !sc::put_front(&Glue, file, x, s, p) {
        return 0;
    }
    if sc::put_front(&Glue, file, x, b"\n", p.wrapping_add(s.len())) { 2 } else { 1 }
}

/// [`puts`] past its front path: `s` (unless `s_done`) and the newline under the stream's lock.
#[inline(never)]
fn puts_general(fp: usize, s: &[u8], s_done: bool) -> KResult<usize> {
    with(fp, Locking::Locked, |g, file, ext| {
        if !s_done {
            if let Some(e) = sc::write(g, file, ext, s).error {
                return Err(e);
            }
        }
        sc::write(g, file, ext, b"\n").error.map_or(Ok(s.len().saturating_add(1)), Err)
    })?
}

/// `getdelim(&line, &n, delim, fp)`: the record's length, `None` at end of input. Inlined into
/// the exports (GETDELIM-3): the front path's result then stays in registers.
#[inline(always)]
pub fn getdelim(line: KResult<usize>, cap: KResult<usize>, delim: i32, fp: usize) -> KResult<Option<usize>> {
    let (line, cap) = (line?, cap?);
    let d = low_byte(delim);
    match delim_front(fp, line, cap, d) {
        Ok(n) => Ok(Some(n)),
        Err(Front::Locked) => delim_locked(fp, line, cap, d),
        Err(_) => getdelim_general(fp, line, cap, d),
    }
}

/// [`getdelim`] past its front path: the engine's.
#[inline(never)]
fn getdelim_general(fp: usize, line: usize, cap: usize, d: u8) -> KResult<Option<usize>> {
    with(fp, Locking::Locked, |g, file, ext| sc::getdelim(g, file, ext, line, cap, d))?
}

/// The `getdelim` front path (GETDELIM, D-12.19), the mirror of [`read_front`]: a whole record
/// in an open published read window on a sealed record that fits the caller's buffer.
#[inline(always)]
fn delim_front(fp: usize, line: usize, cap: usize, d: u8) -> Result<usize, Front> {
    let Some(file) = tb::file_words(fp) else { return Err(Front::General) };
    let Some(p) = sc::read_room(file) else { return Err(Front::General) };
    let ext = sc::ext_word(file);
    if !Glue.record(fp, ext) {
        return Err(Front::General);
    }
    if crate::thread::alone() || sc::by_caller(file) {
        return sc::getdelim_front(&Glue, file, tb::stream_ext_at(ext), p, line, cap, d).ok_or(Front::General);
    }
    Err(Front::Locked)
}

/// [`delim_front`] under the stream's lock (another thread may use the stream): everything is
/// read again under it, else the general path.
#[inline(never)]
fn delim_locked(fp: usize, line: usize, cap: usize, d: u8) -> KResult<Option<usize>> {
    let front = (|| {
        let file = tb::file_words(fp)?;
        let (lock, ext) = sc::parts(&Glue, file)?;
        let me = Glue.tid();
        lock.lock(&CSys, me);
        let n = sc::read_room(file).and_then(|p| sc::getdelim_front(&Glue, file, tb::stream_ext_at(ext), p, line, cap, d));
        lock.unlock(&CSys, me);
        n
    })();
    match front {
        Some(n) => Ok(Some(n)),
        None => getdelim_general(fp, line, cap, d),
    }
}

/// `gets(buf)` and `__gets_chk(buf, size)`: a line without its newline. Unbounded (`size`
/// `None`): the caller vouches for the room (spec 12.5, HIST). Bounded: a longer line aborts.
pub fn gets(buf: usize, size: Option<usize>) -> KResult<Option<usize>> {
    with(stdin(), Locking::Locked, |g, file, ext| {
        let mut n = 0usize;
        loop {
            match sc::getc(g, file, ext) {
                Ok(Some(b'\n')) => break,
                Ok(Some(c)) => {
                    if size.is_some_and(|s| n.saturating_add(1) >= s) {
                        crate::ret::chk_fail();
                    }
                    if let Some(slot) = tb::stream_bytes(buf.wrapping_add(n), 1).first_mut() {
                        *slot = c;
                    }
                    n = n.saturating_add(1);
                }
                Ok(None) if n == 0 => return Ok(None),
                Ok(None) => break,
                Err(e) => return Err(e),
            }
        }
        if let Some(slot) = tb::stream_bytes(buf.wrapping_add(n), 1).first_mut() {
            *slot = 0;
        }
        Ok(Some(n))
    })?
}

/// `fread`: the bytes moved, and the error that stopped it. Two entries before the general
/// plumbing, both on a sealed record whose lock is held or needed by none (as [`window_front`]
/// decides): the front path when the published read window holds a byte, and FREAD-BIG-2's
/// empty-window entry, which hands the stream straight to the engine's [`sc::read`] (the call
/// [`with`] would make, without its second `parts` and closure; for a request at least the
/// buffer's size that is [`sc::read`]'s direct path, the caller's buffer filled by the backing).
pub fn fread(out: KResult<&mut [u8]>, fp: usize, how: Locking) -> liella::stream::Transfer {
    let out = match out {
        Ok(out) => out,
        Err(e) => return liella::stream::Transfer { done: 0, error: Some(e) },
    };
    let Some(file) = tb::file_words(fp) else { return fread_slow(out, fp, how, Front::General) };
    let room = sc::read_room(file);
    let ext = sc::ext_word(file);
    if !Glue.record(fp, ext) {
        return fread_slow(out, fp, how, Front::General);
    }
    if how != Locking::Unlocked && !crate::thread::alone() && !sc::by_caller(file) {
        // With nothing in the window the locked front path cannot apply: straight to `with`.
        return fread_slow(out, fp, how, if room.is_some() { Front::Locked } else { Front::General });
    }
    match room {
        Some(p) => match sc::read_front(&Glue, file, tb::stream_ext_at(ext), p, out) {
            Some(n) => liella::stream::Transfer { done: n, error: None },
            None => fread_engine(out, file, ext),
        },
        None => fread_engine(out, file, ext),
    }
}

/// [`fread`]'s engine call for a sealed record `ext` of `file` whose lock is held or needed by
/// none: exactly what [`with`] runs then.
#[inline(never)]
fn fread_engine(out: &mut [u8], file: &'static [AtomicUsize], ext: usize) -> liella::stream::Transfer {
    sc::read(&Glue, file, tb::stream_ext_at(ext), out)
}

/// [`fread`] past its front path: as [`fgets_slow`].
#[inline(never)]
fn fread_slow(out: &mut [u8], fp: usize, how: Locking, f: Front) -> liella::stream::Transfer {
    if f == Front::Locked {
        if let Some(n) = window_locked(fp, |file, ext, p| sc::read_front(&Glue, file, ext, p, out)) {
            return liella::stream::Transfer { done: n, error: None };
        }
    }
    with(fp, how, |g, file, ext| sc::read(g, file, ext, out)).unwrap_or_else(|e| liella::stream::Transfer { done: 0, error: Some(e) })
}

/// `fwrite`: the bytes moved, and the error that stopped it. Always inlined: with the front
/// path's inline small copy a plain `#[inline]` stayed out of line (a call and a `Transfer`
/// through memory per small write, measured +2% instructions on uniq; WL-HOT-CUT).
#[inline(always)]
pub fn fwrite(data: KResult<&[u8]>, fp: usize, how: Locking) -> liella::stream::Transfer {
    match data {
        Ok(d) if front_done(fp, how, d) => liella::stream::Transfer { done: d.len(), error: None },
        _ => fwrite_general(data, fp, how),
    }
}

/// `fwrite` and `fwrite_unlocked` as the exports answer: the whole elements written (`size`, `n`
/// as the caller passed them, `data` their `size * n` bytes). FWRITE-FRONT: a write of 1 to
/// [`sc::FRONT_INLINE`] bytes that fits the open window is a leaf without a frame (the window,
/// the record's seal and the buffer bounds as [`put_front`] tests them, an inline copy, `n`);
/// everything else, and a front path that does not apply, is one jump to [`fwrite_slow`], which
/// answers exactly as before (sort -C: ~1M 46-byte lines, ~45 of 117 instructions per call
/// were the frame, the `memcpy` call and the `Transfer` between them).
#[inline(always)]
pub fn fwrite_elements(data: KResult<&[u8]>, size: usize, n: usize, fp: usize, how: Locking) -> usize {
    let d = match data {
        Ok(d) => d,
        Err(e) => return fwrite_slow(&[], size, n, fp, WriteFrom::Fail(e)),
    };
    if front_bounded(d) {
        return match put_front_ext(fp, how, d) {
            (Front::Done, _) => n,
            (Front::Locked, ext) => fwrite_locked(d, size, n, fp, ext),
            (Front::General, _) => fwrite_slow(d, size, n, fp, WriteFrom::General(how)),
        };
    }
    fwrite_slow(d, size, n, fp, WriteFrom::Start(how))
}

/// [`fwrite_elements`]'s leaf when the lock must be taken (STDIO-LOCKED-PATH): as
/// [`fputs_locked`], in the record the leaf accepted, with no stack frame on the uncontended
/// call; every other case is one jump.
#[inline(never)]
fn fwrite_locked(d: &[u8], size: usize, n: usize, fp: usize, ext: usize) -> usize {
    if !front_bounded(d) {
        return fwrite_slow(d, size, n, fp, WriteFrom::Locked);
    }
    match front_lock_try(fp, ext, d) {
        Held::Done => n,
        Held::Missed => fwrite_slow(d, size, n, fp, WriteFrom::General(Locking::Locked)),
        Held::Wake(true) => fwrite_wake(ext, n),
        Held::Wake(false) => fwrite_wake_general(d, size, n, fp, ext),
        Held::Full => fwrite_slow(d, size, n, fp, WriteFrom::Locked),
    }
}

/// [`fwrite_locked`] after its release freed a lock another thread waits for, the bytes written:
/// wake it; `n`.
#[cold]
#[inline(never)]
fn fwrite_wake(ext: usize, n: usize) -> usize {
    front_wake(Glue.lock_of(ext));
    n
}

/// [`fwrite_wake`] when the front path did not apply: wake, then the general path.
#[cold]
#[inline(never)]
fn fwrite_wake_general(d: &[u8], size: usize, n: usize, fp: usize, ext: usize) -> usize {
    front_wake(Glue.lock_of(ext));
    fwrite_slow(d, size, n, fp, WriteFrom::General(Locking::Locked))
}

/// Where [`fwrite_slow`] takes over from [`fwrite_elements`]. With the bytes as a plain slice,
/// all of [`fwrite_slow`]'s arguments travel in registers, so the leaf reaches it by a jump.
#[derive(Clone, Copy)]
enum WriteFrom {
    /// Nothing tried yet (an empty write or one too long for the leaf).
    Start(Locking),
    /// The front path did not apply: the general path.
    General(Locking),
    /// The front path needs the stream's lock ([`put_front_locked`]), then the general path.
    Locked,
    /// The arguments name no bytes (`size * n` overflows, or a NULL buffer): this error.
    Fail(Errno),
}

/// [`fwrite_elements`] past its leaf: the answer of the front path with the lock or of the
/// general path, as [`fwrite`] gives it.
#[inline(never)]
fn fwrite_slow(d: &[u8], size: usize, n: usize, fp: usize, from: WriteFrom) -> usize {
    let t = match from {
        WriteFrom::Start(how) => fwrite(Ok(d), fp, how),
        WriteFrom::General(how) => fwrite_general(Ok(d), fp, how),
        WriteFrom::Locked if put_front_locked(fp, d) => return n,
        WriteFrom::Locked => fwrite_general(Ok(d), fp, Locking::Locked),
        WriteFrom::Fail(e) => fwrite_general(Err(e), fp, Locking::Locked),
    };
    crate::ret::elements(t, size, n)
}

/// [`fwrite`] past its front path.
#[inline(never)]
fn fwrite_general(data: KResult<&[u8]>, fp: usize, how: Locking) -> liella::stream::Transfer {
    let fail = |e| liella::stream::Transfer { done: 0, error: Some(e) };
    match data {
        Ok(data) => with(fp, how, |g, file, ext| sc::write(g, file, ext, data)).unwrap_or_else(fail),
        Err(e) => fail(e),
    }
}

/// `getw`: a native `int`.
pub fn getw(fp: usize) -> KResult<Option<i32>> {
    let mut b = [0u8; 4];
    let t = with(fp, Locking::Locked, |g, file, ext| sc::read(g, file, ext, &mut b))?;
    match (t.done, t.error) {
        (4, _) => Ok(Some(i32::from_ne_bytes(b))),
        (_, Some(e)) => Err(e),
        _ => Ok(None),
    }
}

/// `putw`: a native `int`.
pub fn putw(w: i32, fp: usize) -> KResult<()> {
    let t = with(fp, Locking::Locked, |g, file, ext| sc::write(g, file, ext, &w.to_ne_bytes()))?;
    t.error.map_or(Ok(()), Err)
}

fn whence(w: i32) -> KResult<liella::stream::Whence> {
    match w {
        0 => Ok(liella::stream::Whence::Set),
        1 => Ok(liella::stream::Whence::Cur),
        2 => Ok(liella::stream::Whence::End),
        _ => Err(Errno::EINVAL),
    }
}

/// FTELL-BOUNDARY: the words and record of a stream an operation may use right away, with none
/// of [`with`]'s plumbing: set up (its record sealed to `fp`, as [`parts`] checks) and needing no
/// lock (the only thread, or the caller locks, as [`with`] decides; `alone` first, so a process
/// with one thread does not load the flags word). `None`: a standard stream's first use, a
/// bad `FILE`, or a lock to take; the caller goes [`with`]'s way.
#[inline(always)]
fn front_parts(fp: usize) -> Option<(&'static [AtomicUsize], &'static mut Ext)> {
    let file = tb::file_words(fp)?;
    let ext = sc::ext_word(file);
    if !Glue.record(fp, ext) || !(crate::thread::alone() || sc::by_caller(file)) {
        return None;
    }
    Some((file, tb::stream_ext_at(ext)))
}

/// `f` on the stream `fp` under its lock: through [`front_parts`] when no lock is needed, else
/// [`with`] (the same call, the same answer).
#[inline(always)]
fn with_locked<T>(fp: usize, f: impl FnOnce(&Glue, &'static [AtomicUsize], &mut Ext) -> T) -> KResult<T> {
    if let Some((file, ext)) = front_parts(fp) {
        return Ok(f(&Glue, file, ext));
    }
    with(fp, Locking::Locked, f)
}

/// The position as an `off_t`.
fn signed_offset(p: u64) -> KResult<i64> {
    i64::try_from(p).map_err(|_| Errno::EOVERFLOW)
}

/// `fseeko`.
pub fn fseek(fp: usize, offset: i64, from: i32) -> KResult<()> {
    let w = whence(from)?;
    with_locked(fp, |g, file, ext| sc::seek(g, file, ext, offset, w).map(|_| ()))?
}

/// `ftello`. FTELL-BOUNDARY: a stream that needs no lock and whose offset is known answers from
/// its window inline ([`sc::tell_front`]: a few loads, no call); anything else goes out of line.
#[inline(always)]
pub fn ftell(fp: usize) -> KResult<i64> {
    if let Some(p) = front_parts(fp).and_then(|(file, ext)| sc::tell_front(file, ext)) {
        if let Ok(v) = i64::try_from(p) {
            return Ok(v);
        }
    }
    ftell_general(fp)
}

#[inline(never)]
fn ftell_general(fp: usize) -> KResult<i64> {
    signed_offset(with_locked(fp, sc::tell)??)
}

/// `rewind`: to the start, indicators cleared, no error reported.
pub fn rewind(fp: usize) {
    let _ = with_locked(fp, sc::rewind);
}

/// `fgetpos`: the position and the conversion state (FTELL-BOUNDARY: as [`ftell`], the window's
/// answer inline when it needs no lock and no call).
#[inline(always)]
pub fn fgetpos(fp: usize) -> KResult<liella_abi::layout::fpos_t> {
    if let Some((p, mb)) = front_parts(fp).and_then(|(file, ext)| sc::getpos_front(file, ext)) {
        if let Ok(pos) = i64::try_from(p) {
            return Ok(fpos(pos, mb));
        }
    }
    fgetpos_general(fp)
}

#[inline(never)]
fn fgetpos_general(fp: usize) -> KResult<liella_abi::layout::fpos_t> {
    let (p, mb) = with_locked(fp, |g, file, ext| sc::tell(g, file, ext).map(|p| (p, ext.wide.decode.to_bytes())))??;
    Ok(fpos(signed_offset(p)?, mb))
}

/// An `fpos_t` of a position and a conversion state.
#[inline(always)]
fn fpos(pos: i64, mb: [u8; 8]) -> liella_abi::layout::fpos_t {
    let [c0, c1, c2, c3, v0, v1, v2, v3] = mb;
    liella_abi::layout::fpos_t {
        __pos: pos,
        __state: liella_abi::layout::fpos_t__state { __count: i32::from_ne_bytes([c0, c1, c2, c3]), __value: liella_abi::layout::fpos_t__state__value { __wch: u32::from_ne_bytes([v0, v1, v2, v3]) } },
    }
}

/// `fsetpos`: back to a position `fgetpos` gave.
pub fn fsetpos(fp: usize, pos: Option<&liella_abi::layout::fpos_t>) -> KResult<()> {
    let pos = pos.ok_or(Errno::EINVAL)?.__pos;
    fseek(fp, pos, 0)
}

/// `feof` / `ferror`: the indicator.
pub fn indicator(fp: usize, bit: usize) -> KResult<bool> {
    let file = tb::file_words(fp).ok_or(Errno::EINVAL)?;
    Ok(file.get(word::FLAGS).is_some_and(|w| w.load(Relaxed) & bit != 0))
}

/// `clearerr`.
pub fn clearerr(fp: usize) {
    if let Some(file) = tb::file_words(fp) {
        sc::clear_indicators(file);
    }
}

/// `fwide`.
pub fn fwide(fp: usize, mode: i32) -> KResult<i32> {
    let enc = encoding();
    with(fp, Locking::Locked, |_, file, ext| sc::fwide(file, ext, mode, enc))
}

/// The lock of a stream already set up, and the calling thread's TID from the thread block
/// (FILE-LOCK-3: the direct path of `flockfile` and friends, one record check and no call);
/// `None` sends the caller to its full path ([`parts`]: a standard stream's first use, a TID by
/// system call, a stream that is not one).
#[inline(always)]
fn lock_direct(fp: usize) -> Option<(&'static StreamLock, u32)> {
    let file = tb::file_words(fp)?;
    let ext = sc::ext_word(file);
    if !Glue.record(fp, ext) {
        return None;
    }
    Some((Glue.lock_of(ext), thread::own_tid_fast()?))
}

/// `flockfile`: taken without waiting on the direct path, else [`flockfile_full`].
pub fn flockfile(fp: usize) {
    if let Some((lock, me)) = lock_direct(fp) {
        if lock.lock_fast(me) {
            return;
        }
    }
    flockfile_full(fp);
}

/// [`flockfile`] the full way: the stream set up if it must be, the lock waited for.
#[cold]
#[inline(never)]
fn flockfile_full(fp: usize) {
    if let Ok((_, lock, _)) = parts(fp) {
        lock.lock(&CSys, Glue.tid());
    }
}

/// `ftrylockfile`: whether the lock was taken.
pub fn ftrylockfile(fp: usize) -> bool {
    match lock_direct(fp) {
        Some((lock, me)) => lock.try_lock(me),
        None => ftrylockfile_full(fp),
    }
}

/// [`ftrylockfile`] the full way (see [`flockfile_full`]).
#[cold]
#[inline(never)]
fn ftrylockfile_full(fp: usize) -> bool {
    parts(fp).is_ok_and(|(_, lock, _)| lock.try_lock(Glue.tid()))
}

/// `funlockfile`: one level released if the calling thread holds the lock.
pub fn funlockfile(fp: usize) {
    match lock_direct(fp) {
        Some((lock, me)) => lock.unlock(&CSys, me),
        None => funlockfile_full(fp),
    }
}

/// [`funlockfile`] the full way (see [`flockfile_full`]).
#[cold]
#[inline(never)]
fn funlockfile_full(fp: usize) {
    if let Ok((_, lock, _)) = parts(fp) {
        lock.unlock(&CSys, Glue.tid());
    }
}

/// `setvbuf(fp, buf, mode, size)`.
pub fn setvbuf(fp: usize, buf: usize, mode: i32, size: usize) -> KResult<()> {
    with(fp, Locking::Locked, |g, file, ext| sc::setvbuf(g, file, ext, buf, mode, size))?
}

/// The `<stdio_ext.h>` facts.
pub fn info(fp: usize) -> KResult<sc::Info> {
    with(fp, Locking::Unlocked, |_, file, ext| sc::info(file, ext))
}

/// `__fsetlocking`.
pub fn fsetlocking(fp: usize, how: i32) -> KResult<i32> {
    let (file, _, _) = parts(fp)?;
    Ok(sc::set_locking(file, how))
}

/// `__fpurge`.
pub fn fpurge(fp: usize) {
    let _ = with(fp, Locking::Locked, |_, file, ext| sc::purge(file, ext));
}

/// `_flushlbf`.
pub fn flushlbf() {
    sc::flush_line_buffered(&Glue, false);
}

// ------------------------------------------------------------------------------ §12.13

/// `_IO_list_lock` (`true`), `_IO_list_unlock` (`false`).
pub fn list_lock(take: bool) {
    sc::hold_list(&CSys, take);
}

/// `_IO_list_resetlock`.
pub fn list_resetlock() {
    sc::reset_list_lock();
}

/// `_IO_iter_begin`: the most recently opened stream (0: none).
pub fn iter_begin() -> usize {
    data::_IO_list_all.load(Acquire)
}

/// `_IO_iter_next(fp)`: the stream opened before `fp` (its `_chain`).
pub fn iter_next(fp: usize) -> usize {
    tb::file_words(fp).and_then(|f| f.get(word::CHAIN)).map_or(0, |w| w.load(Acquire))
}

/// `_IO_peekc_locked(fp)`: the next byte without consuming it, the stream locked.
pub fn peek_locked(fp: usize) -> KResult<Option<u8>> {
    with(fp, Locking::Locked, |g, file, ext| {
        let c = sc::getc(g, file, ext)?;
        if let Some(b) = c {
            sc::ungetc(g, file, ext, b)?;
        }
        Ok(c)
    })?
}

/// `_IO_padn(fp, pad, count)`: `count` copies of `pad` written (unlocked); the number written.
pub fn padn(fp: usize, pad: i32, count: isize) -> usize {
    let block = [pad.to_ne_bytes()[0]; 64];
    let mut left = usize::try_from(count).unwrap_or(0);
    let mut done = 0usize;
    while left > 0 {
        let n = left.min(block.len());
        let t = fwrite(Ok(block.get(..n).unwrap_or(&[])), fp, Locking::Unlocked);
        done = done.saturating_add(t.done);
        if t.done < n {
            break;
        }
        left = left.saturating_sub(n);
    }
    done
}

/// `_IO_seekoff(fp, offset, dir, mode)` and `_IO_seekpos(fp, pos, mode)`: the new position.
pub fn seekoff(fp: usize, offset: i64, from: i32) -> KResult<i64> {
    fseek(fp, offset, from)?;
    ftell(fp)
}

/// `_IO_getline_info(fp, buf, n, delim, extract, &eof)` and `_IO_getline` (`eof_var` 0: no
/// `eof` argument): the count stored, at most `n` bytes and no NUL. Reading stops at the
/// delimiter, which `extract` > 0 stores (and counts), 0 consumes, and < 0 leaves in the
/// stream; it also stops when the buffer is full, without reading further. `*eof` is 0, or
/// `EOF` once end of input (or an error) stopped the read. The stream is not locked, as
/// observed (the caller's contract). The delimiter compares by its low byte (D-12.15).
pub fn getline_info(fp: usize, buf: KResult<&mut [u8]>, delim: i32, extract: i32, eof_var: usize) -> usize {
    conv::store_int(eof_var, 4, 0);
    let Ok(buf) = buf else {
        return 0;
    };
    let d = low_byte(delim);
    let (n, ended) = with(fp, Locking::Unlocked, |g, file, ext| {
        let mut n = 0usize;
        for slot in buf.iter_mut() {
            let Ok(Some(c)) = sc::getc(g, file, ext) else {
                return (n, true);
            };
            let hit = c == d;
            if hit && extract < 0 {
                let _ = sc::ungetc(g, file, ext, c);
            }
            if !hit || extract > 0 {
                *slot = c;
                n = n.saturating_add(1);
            }
            if hit {
                break;
            }
        }
        (n, false)
    })
    .unwrap_or((0, true));
    if ended {
        conv::store_int(eof_var, 4, -1);
    }
    n
}


// ------------------------------------------------------------------------------ §12.3

/// `fmemopen(buf, size, mode)` (the `GLIBC_2.22` contract): a NULL `buf` is allocated, zeroed,
/// and freed at close.
pub fn fmemopen(buf: usize, size: usize, mode: KResult<&[u8]>) -> KResult<usize> {
    fmemopen_contract(buf, size, mode, false)
}

/// `fmemopen@GLIBC_2.2.5` (LG-2, probe-settled): size 0 fails with `EINVAL`; otherwise the old
/// buffer rules of [`sc::mem_backing`] (`old`).
pub fn fmemopen_2_2_5(buf: usize, size: usize, mode: KResult<&[u8]>) -> KResult<usize> {
    if size == 0 {
        return Err(Errno::EINVAL);
    }
    fmemopen_contract(buf, size, mode, true)
}

fn fmemopen_contract(buf: usize, size: usize, mode: KResult<&[u8]>, old: bool) -> KResult<usize> {
    let m = sc::parse_mode(mode?)?;
    let (addr, owned) = if buf == 0 {
        let a = caller_alloc::calloc(size.max(1), 1).expose_provenance();
        if a == 0 {
            return Err(Errno::ENOMEM);
        }
        (a, true)
    } else {
        (buf, false)
    };
    let mem = sc::mem_backing(&Glue, addr, size, &m, owned, old);
    open_stream(Kind::Mem(mem), &m, -1).inspect_err(|_| {
        if owned {
            Glue.free(addr);
        }
    })
}

/// `open_memstream(&ptr, &size)`: a write-only stream to a growing buffer, published through
/// the caller's variables at `fflush` and `fclose`.
pub fn open_memstream(ptr: KResult<usize>, size: KResult<usize>, wide: bool) -> KResult<usize> {
    let (ptr_var, size_var) = (ptr?, size?);
    let buf = caller_alloc::calloc(64, 1).expose_provenance();
    if buf == 0 {
        return Err(Errno::ENOMEM);
    }
    let g = sc::GrowStream { ptr_var, size_var, buf, cap: 64, len: 0, filled: 0, pos: 0, wide };
    let m = sc::Mode { write: true, first: b'w', ..sc::Mode::default() };
    let fp = open_stream(Kind::Grow(g), &m, -1).inspect_err(|_| Glue.free(buf))?;
    if wide {
        // `open_wmemstream`: wide-oriented from the start, on `wchar_t` units.
        with(fp, Locking::Locked, |_, file, ext| sc::set_wide(file, ext, sc::Codec::Units))?;
    }
    Ok(fp)
}

// ------------------------------------------------------------------------------ §12.10

/// The calling thread's locale encoding: a stream becoming wide-oriented converts with it.
fn encoding() -> Encoding {
    Encoding::of(locale::current())
}

/// `WEOF`.
const WEOF: u32 = u32::MAX;

/// `fgetwc`: the next wide character, `None` at end of input.
pub fn getwc(fp: usize, how: Locking) -> KResult<Option<u32>> {
    let enc = encoding();
    with(fp, how, |g, file, ext| sc::getwc(g, file, ext, enc))?
}

/// `__wunderflow`: the next wide character without consuming it.
pub fn peekwc(fp: usize) -> KResult<Option<u32>> {
    let enc = encoding();
    with(fp, Locking::Unlocked, |g, file, ext| {
        let c = sc::getwc(g, file, ext, enc)?;
        if let Some(w) = c {
            sc::ungetwc(file, ext, w, enc)?;
        }
        Ok(c)
    })?
}

/// `fputwc`: the character written.
pub fn putwc(fp: usize, how: Locking, wc: u32) -> KResult<u32> {
    let enc = encoding();
    with(fp, how, |g, file, ext| sc::putwc(g, file, ext, wc, enc).map(|()| wc))?
}

/// `__woverflow(fp, wc)`: write `wc`; with `WEOF`, only flush (0).
pub fn woverflow(fp: usize, wc: u32) -> KResult<u32> {
    if wc == WEOF {
        return with(fp, Locking::Unlocked, sc::flush)?.map(|()| 0);
    }
    putwc(fp, Locking::Unlocked, wc)
}

/// `ungetwc(wc, fp)`: `WEOF` is refused.
pub fn ungetwc(fp: usize, how: Locking, wc: u32) -> KResult<u32> {
    if wc == WEOF {
        return Err(Errno::EINVAL);
    }
    let enc = encoding();
    with(fp, how, |_, file, ext| sc::ungetwc(file, ext, wc, enc).map(|()| wc))?
}

/// `_IO_sungetc(fp)`: give back the last byte read (the stream is not locked, as for
/// `_IO_sungetwc`).
pub fn sungetc(fp: usize) -> KResult<u8> {
    with(fp, Locking::Unlocked, sc::unget_last)?
}

/// `_IO_sungetwc(fp)`: give back the last wide character read.
pub fn sungetwc(fp: usize) -> KResult<u32> {
    let enc = encoding();
    with(fp, Locking::Unlocked, |_, file, ext| sc::unget_last_wide(file, ext, enc))?
}

/// `fgetws(buf, n, fp)`: the characters stored, `None` at end of input before any.
pub fn fgetws(buf: KResult<&mut [i32]>, fp: usize, how: Locking) -> KResult<Option<usize>> {
    let buf = buf?;
    if buf.is_empty() {
        return Err(Errno::EINVAL);
    }
    let enc = encoding();
    with(fp, how, |g, file, ext| sc::getws_into(g, file, ext, buf, enc))?
}

/// `__fgetws_chk(ws, size, n, fp)` (C0.5): as [`fgets_chk`], in wide characters.
pub fn fgetws_chk(buf: KResult<&mut [i32]>, n: usize, fp: usize, how: Locking) -> KResult<Option<usize>> {
    let buf = buf?;
    let cap = buf.len();
    if cap == 0 {
        return Err(Errno::EINVAL);
    }
    let enc = encoding();
    with(fp, how, |g, file, ext| {
        let got = sc::getws_into(g, file, ext, buf, enc)?;
        let nl = i32::from(b'\n');
        let filled = got.is_some_and(|k| k.saturating_add(1) == cap && k.checked_sub(1).and_then(|i| buf.get(i)) != Some(&nl));
        if n > cap && filled && sc::getwc(g, file, ext, enc).ok().flatten().is_some() {
            crate::ret::chk_fail();
        }
        Ok(got)
    })?
}

/// `fputws(s, fp)`: a non-negative count on success.
pub fn fputws(s: KResult<&[i32]>, fp: usize, how: Locking) -> KResult<usize> {
    let s = s?;
    let enc = encoding();
    with(fp, how, |g, file, ext| {
        for &w in s {
            sc::putwc(g, file, ext, u32::from_ne_bytes(w.to_ne_bytes()), enc)?;
        }
        Ok(1)
    })?
}

/// `fopencookie(cookie, mode, functions)`.
pub fn fopencookie(cookie: usize, mode: KResult<&[u8]>, f: liella_abi::layout::cookie_io_functions_t) -> KResult<usize> {
    let m = sc::parse_mode(mode?)?;
    let c = sc::CookieStream { cookie, read: f.read, write: f.write, seek: f.seek, close: f.close };
    open_stream(Kind::Cookie(c), &m, -1)
}


// ------------------------------------------------------------------------------ §12.12

/// `tmpfile()`: an update stream on a new file in `P_tmpdir` that no name refers to: an
/// `O_TMPFILE` file, or (where the file system lacks it) a file created exclusively under a
/// random name and unlinked at once.
pub fn tmpfile() -> KResult<usize> {
    use liella_kernel::uapi::{O_CLOEXEC, O_CREAT, O_EXCL, O_RDWR, O_TMPFILE};
    let dir = KStr::from_nul_terminated(b"/tmp\0")?;
    let fd = match CSys.openat(Fd::CWD, dir, O_TMPFILE | O_RDWR | O_EXCL, 0o600) {
        Ok(fd) => fd,
        Err(_) => {
            let mut name = *b"/tmp/tmpfXXXXXXXXXXXX\0";
            let mut made = Err(Errno::EEXIST);
            for _ in 0..100 {
                let mut r = [0u8; 12];
                let _ = CSys.getrandom(&mut r, liella_kernel::uapi::GRND_NONBLOCK);
                const ALPHABET: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
                for (slot, b) in name.iter_mut().skip(9).zip(r) {
                    *slot = ALPHABET.get(usize::from(b).checked_rem(ALPHABET.len()).unwrap_or(0)).copied().unwrap_or(b'x');
                }
                let path = KStr::from_nul_terminated(&name)?;
                made = CSys.openat(Fd::CWD, path, O_CREAT | O_EXCL | O_RDWR | O_CLOEXEC, 0o600);
                if let Ok(fd) = made {
                    let _ = CSys.unlinkat(Fd::CWD, path, 0);
                    made = Ok(fd);
                    break;
                }
                if made != Err(Errno::EEXIST) {
                    break;
                }
            }
            made?
        }
    };
    let m = sc::Mode { read: true, write: true, first: b'w', ..sc::Mode::default() };
    open_stream(Kind::Fd(fd.raw()), &m, fd.raw()).inspect_err(|_| {
        let _ = CSys.close(fd);
    })
}

/// `P_tmpdir`: `tmpnam`'s directory and `tempnam`'s last choice.
const P_TMPDIR: &[u8] = b"/tmp";
/// `L_tmpnam`: the room a `tmpnam` buffer has (`/tmp/file` and six characters fit easily).
const L_TMPNAM: usize = 20;
/// `TMP_MAX`: the number of distinct names (62³), and so the most a call tries.
const TMP_MAX: u32 = 238_328;
/// The characters of a generated name.
const NAME_CHARS: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
/// The prefix of a name with none given (and of every `tmpnam` name).
const DEFAULT_PREFIX: &[u8] = b"file";
/// The most of a `tempnam` prefix that is used.
const PREFIX_MAX: usize = 5;

/// `text` and a terminator, for the kernel.
fn terminated(text: &[u8]) -> KResult<TryVec<u8>> {
    let mut v = TryVec::with_capacity(text.len().saturating_add(1))?;
    v.extend_from_slice(text)?;
    v.push(0)?;
    Ok(v)
}

/// Whether `path` names a directory (following symbolic links). Any failure is "no".
fn is_dir(path: &[u8]) -> bool {
    use liella_kernel::uapi::{S_IFDIR, S_IFMT};
    !path.is_empty()
        && terminated(path).is_ok_and(|p| {
            KStr::from_nul_terminated(p.as_slice()).and_then(|k| CSys.fstatat(Fd::CWD, k, 0)).is_ok_and(|st| st.mode & S_IFMT == S_IFDIR)
        })
}

/// 64 unpredictable bits: from `getrandom`, or (where it gives nothing at once) the clocks
/// mixed with `salt`.
fn random_bits(salt: u64) -> u64 {
    use liella_kernel::uapi::{CLOCK_MONOTONIC, CLOCK_REALTIME, GRND_NONBLOCK};
    let mut r = [0u8; 8];
    if CSys.getrandom(&mut r, GRND_NONBLOCK) == Ok(r.len()) {
        return u64::from_ne_bytes(r);
    }
    let ns = |clock| CSys.clock_gettime(clock).map_or(0, |t| u64::from_ne_bytes(t.sec.to_ne_bytes()).wrapping_mul(1_000_000_000).wrapping_add(u64::from_ne_bytes(t.nsec.to_ne_bytes())));
    // A 64-bit finaliser spreads the clock bits over the whole word.
    let mut x = ns(CLOCK_REALTIME) ^ ns(CLOCK_MONOTONIC).rotate_left(32) ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// A name `dir/` + `prefix` + six characters of [`NAME_CHARS`], terminated, that does not exist
/// now (spec §12.12). Trailing slashes of `dir` are dropped, but never its first byte. A name
/// that exists (as anything, a dangling symbolic link included) is replaced by another, up to
/// `TMP_MAX` tries (then `EEXIST`); any other failure to look a name up is the error.
fn fresh_name(dir: &[u8], prefix: &[u8]) -> KResult<TryVec<u8>> {
    use liella_kernel::uapi::AT_SYMLINK_NOFOLLOW;
    let keep = dir.iter().rposition(|&b| b != b'/').map_or(1, |i| i.saturating_add(1)).min(dir.len());
    let mut name = TryVec::new();
    name.extend_from_slice(dir.get(..keep).unwrap_or(dir))?;
    name.push(b'/')?;
    name.extend_from_slice(prefix)?;
    let start = name.len();
    name.extend_from_slice(b"XXXXXX\0")?;
    for attempt in 0..TMP_MAX {
        let mut bits = random_bits(u64::from(attempt));
        for slot in name.as_mut_slice().iter_mut().skip(start).take(6) {
            let i = usize::try_from(bits % 62).unwrap_or(0);
            bits /= 62;
            *slot = NAME_CHARS.get(i).copied().unwrap_or(b'x');
        }
        match CSys.fstatat(Fd::CWD, KStr::from_nul_terminated(name.as_slice())?, AT_SYMLINK_NOFOLLOW) {
            Err(Errno::ENOENT) => return Ok(name),
            Err(e) => return Err(e),
            Ok(_) => {}
        }
    }
    Err(Errno::EEXIST)
}

/// `tmpnam(buf)` and `tmpnam_r(buf)` (`reentrant`): a new name in `P_tmpdir` (`TMPDIR` is not
/// consulted), stored in `buf` (`L_tmpnam` bytes) or, for `tmpnam(NULL)`, in the library's
/// buffer; its address. `tmpnam_r` needs a buffer (`EINVAL`, S6). `P_tmpdir` not being a
/// directory is `ENOENT`.
pub fn tmpnam(buf: usize, reentrant: bool) -> KResult<usize> {
    if buf == 0 && reentrant {
        return Err(Errno::EINVAL);
    }
    if !is_dir(P_TMPDIR) {
        return Err(Errno::ENOENT);
    }
    let name = fresh_name(P_TMPDIR, DEFAULT_PREFIX)?;
    let name = name.as_slice();
    if buf == 0 {
        // The calling thread's buffer, the same at each of its calls (D-C0.2).
        let own = crate::users::result_buf::<L_TMPNAM>(crate::users::TMPNAM)?;
        own.store(name);
        return Ok(own.addr());
    }
    conv::out_bytes(buf, name.len()).copy_from_slice(name);
    Ok(buf)
}

/// `tempnam(dir, pfx)`: a new name, in a `malloc`ed block. The directory is the first of
/// `TMPDIR` (outside secure mode), `dir` and `P_tmpdir` that is a directory (none is `ENOENT`);
/// the prefix is the first five bytes of `pfx`, or `file` when it is NULL or empty.
pub fn tempnam(dir: KResult<Option<&[u8]>>, pfx: KResult<Option<&[u8]>>) -> KResult<usize> {
    let (dir, pfx) = (dir?, pfx?);
    let env = crate::process::secure_getenv(b"TMPDIR").map(crate::strz::env_value);
    let chosen = [env, dir, Some(P_TMPDIR)].into_iter().flatten().find(|d| is_dir(d)).ok_or(Errno::ENOENT)?;
    let prefix = match pfx {
        Some(p) if !p.is_empty() => p.get(..PREFIX_MAX).unwrap_or(p),
        _ => DEFAULT_PREFIX,
    };
    let name = fresh_name(chosen, prefix)?;
    let text = name.as_slice();
    caller_alloc::dup_z(text.get(..text.len().saturating_sub(1)).unwrap_or(text))
}

/// `ctermid(buf)`: `/dev/tty`, into `buf` or as a static text.
pub fn ctermid(buf: usize) -> usize {
    const TTY: &[u8; 9] = b"/dev/tty\0";
    if buf == 0 {
        return TTY.as_ptr().expose_provenance();
    }
    conv::out_bytes(buf, TTY.len()).copy_from_slice(TTY);
    buf
}
