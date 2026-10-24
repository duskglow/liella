//! Directory-tree glue (spec §13.1–13.3, §13.8–13.9; D-13.1 to D-13.6, D-13.13): the C
//! objects of the working-directory, directory-stream, walk, canonicalization and
//! temporary-name functions, over `liella::fs` and `liella_legacy::{fs_tree, fts}`.
//!
//! * `DIR` and `FTS` objects are slots this module leaks and recycles, never frees (as the
//!   stdio streams are): the handle is the slot's address, so a handle used after `closedir` or
//!   `fts_close` still names readable memory and is refused with `EBADF`. Each slot has a lock,
//!   and its state is used only under it; different streams are independent (spec §13.2).
//! * `readdir` returns the address of the kernel's record in the stream's buffer (the record is
//!   `struct dirent` on x86-64), valid until the stream reads again.
//! * Caller-owned results (`getcwd(NULL, ...)`, `realpath(..., NULL)`, `scandir`'s list, the
//!   `FTSENT` blocks) come from the caller's allocator (`crate::caller_alloc`).
//! * The `__*_chk` forms take the object's real size (D-C0.5, D-13.5): a result that the plain
//!   form would write beyond the object terminates through `__chk_fail`.

use core::sync::atomic::{AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella::error::{Errno, KResult};
use liella::fs::canon;
use liella::fs::cwd;
use liella::fs::dir::{self, DirStream};
use liella::fs::temp::{self, Create, Made};
use liella::mem::{TryBox, TryVec};
use liella_abi::callback::{DirFilterFn, SlotCompareFn};
use liella_abi::layout::{struct_dirent, struct_dirent64};
use liella_kernel::fd::Fd;
use liella_kernel::path::{KStr, PATH_MAX};
use liella_legacy::fs_tree;
use liella_legacy::fts::{Fts, FtsMem};
use liella_rt::sync::RawMutex;

use crate::sys::CSys;
use crate::{caller_alloc, conv, process, ret, strz, tb};

fn addr_of<T>(r: &T) -> usize {
    core::ptr::from_ref(r).expose_provenance()
}

/// Copy `text` and a terminator into the caller's object `buf`; a text that does not fit is a
/// write beyond the object (`__chk_fail`).
fn put_text(buf: &mut [u8], text: &[u8]) {
    let Some(dst) = buf.get_mut(..text.len().saturating_add(1)) else { ret::chk_fail() };
    let (t, nul) = dst.split_at_mut(text.len());
    t.copy_from_slice(text);
    nul.fill(0);
}

/// A caller-owned block of `cap` zeroed bytes holding `text` (and its terminator).
fn alloc_text(text: &[u8], cap: usize) -> KResult<usize> {
    let cap = cap.max(text.len().saturating_add(1));
    let p = caller_alloc::calloc(cap, 1);
    let dst = conv::fresh::<u8>(p, cap).ok_or(Errno::ENOMEM)?;
    if let Some(d) = dst.get_mut(..text.len()) {
        d.copy_from_slice(text);
    }
    Ok(p.expose_provenance())
}

// ------------------------------------------------------------------------------ slots

const MAGIC_DIR: usize = 0x4c69_656c_6c61_4449;
const MAGIC_FTS: usize = 0x4c69_656c_6c61_4654;

/// A `DIR`.
#[repr(C)]
struct DirSlot {
    magic: AtomicUsize,
    lock: RawMutex,
    state: AtomicUsize,
    next: AtomicUsize,
}

#[derive(Default)]
struct DirState {
    stream: Option<DirStream>,
    /// A closed stream's buffer, kept with the recycled slot for the next stream (WL-HOT-DU:
    /// a tree walker opens and closes a stream per directory; no allocation or clearing then).
    spare: Option<TryVec<u8>>,
}

/// At most this many closed slots keep a buffer (`STREAM_BUFFER` bytes each): 256 KiB per
/// process at most, whatever number of streams was once open at the same time.
const MAX_SPARES: usize = 8;
static SPARES: AtomicUsize = AtomicUsize::new(0);

/// An `FTS`: the C structure's nine words first (spec Appendix C), then the slot.
#[repr(C)]
struct FtsSlot {
    fts: [AtomicUsize; 9],
    magic: AtomicUsize,
    lock: RawMutex,
    state: AtomicUsize,
    next: AtomicUsize,
}

#[derive(Default)]
struct FtsState {
    fts: Option<Fts>,
}

/// Closed slots of each kind, linked through `next`.
static DIR_FREE: AtomicUsize = AtomicUsize::new(0);
static FTS_FREE: AtomicUsize = AtomicUsize::new(0);
/// Serializes the free lists.
static POOL: RawMutex = RawMutex::new();

fn dir_slot(a: usize) -> KResult<&'static DirSlot> {
    tb::dir_slot_at::<DirSlot>(a).filter(|s| s.magic.load(Acquire) == MAGIC_DIR).ok_or(Errno::EINVAL)
}

fn fts_slot(a: usize) -> KResult<&'static FtsSlot> {
    tb::dir_slot_at::<FtsSlot>(a).filter(|s| s.magic.load(Acquire) == MAGIC_FTS).ok_or(Errno::EINVAL)
}

fn new_dir_slot() -> KResult<usize> {
    let reused = {
        let _g = lock_unless_alone(&POOL);
        let a = DIR_FREE.load(Acquire);
        if a != 0 {
            DIR_FREE.store(dir_slot(a).map_or(0, |s| s.next.load(Relaxed)), Release);
        }
        a
    };
    if reused != 0 {
        return Ok(reused);
    }
    let state: &'static mut DirState = TryBox::new(DirState::default())?.leak();
    let slot: &'static mut DirSlot = TryBox::new(DirSlot { magic: AtomicUsize::new(MAGIC_DIR), lock: RawMutex::new(), state: AtomicUsize::new(addr_of(state)), next: AtomicUsize::new(0) })?.leak();
    Ok(addr_of(slot))
}

fn recycle_dir(a: usize) {
    let Ok(s) = dir_slot(a) else { return };
    let _g = lock_unless_alone(&POOL);
    s.next.store(DIR_FREE.load(Acquire), Relaxed);
    DIR_FREE.store(a, Release);
}

fn new_fts_slot() -> KResult<usize> {
    let reused = {
        let _g = lock_unless_alone(&POOL);
        let a = FTS_FREE.load(Acquire);
        if a != 0 {
            FTS_FREE.store(fts_slot(a).map_or(0, |s| s.next.load(Relaxed)), Release);
        }
        a
    };
    if reused != 0 {
        return Ok(reused);
    }
    let state: &'static mut FtsState = TryBox::new(FtsState::default())?.leak();
    let slot: &'static mut FtsSlot = TryBox::new(FtsSlot {
        fts: [const { AtomicUsize::new(0) }; 9],
        magic: AtomicUsize::new(MAGIC_FTS),
        lock: RawMutex::new(),
        state: AtomicUsize::new(addr_of(state)),
        next: AtomicUsize::new(0),
    })?
    .leak();
    Ok(addr_of(slot))
}

fn recycle_fts(a: usize) {
    let Ok(s) = fts_slot(a) else { return };
    let _g = lock_unless_alone(&POOL);
    s.next.store(FTS_FREE.load(Acquire), Relaxed);
    FTS_FREE.store(a, Release);
}

/// `m` taken, unless the calling thread is the process's only one (design rule P5,
/// `thread::alone`; as the stdio stream locks): nobody else can then hold or wait for it, and the
/// two atomic read-modify-writes are most of a buffered `readdir` (DU-3).
#[inline]
fn lock_unless_alone(m: &RawMutex) -> Option<liella_rt::sync::MutexGuard<'_, CSys>> {
    if crate::thread::alone() { None } else { Some(m.lock(&CSys)) }
}

/// Run `f` on the state of the `DIR` at `d`, under its lock.
fn with_dir<T>(d: usize, f: impl FnOnce(&mut DirState) -> KResult<T>) -> KResult<T> {
    let slot = dir_slot(d)?;
    let _g = lock_unless_alone(&slot.lock);
    f(tb::dir_state_at::<DirState>(slot.state.load(Relaxed)))
}

/// Run `f` on the open stream of the `DIR` at `d` (`EBADF` once closed).
fn with_stream<T>(d: usize, f: impl FnOnce(&mut DirStream) -> KResult<T>) -> KResult<T> {
    with_dir(d, |st| f(st.stream.as_mut().ok_or(Errno::EBADF)?))
}

// ------------------------------------------------------------------------------ §13.2 streams

/// A new `DIR` whose stream `make` opens (given the slot's spare buffer, if any): its address.
/// On failure the slot goes back to the free list.
fn install(make: impl FnOnce(Option<TryVec<u8>>) -> KResult<DirStream>) -> KResult<usize> {
    let a = new_dir_slot()?;
    let slot = dir_slot(a)?;
    let r = {
        let _g = lock_unless_alone(&slot.lock);
        let st = tb::dir_state_at::<DirState>(slot.state.load(Relaxed));
        let spare = st.spare.take();
        if spare.is_some() {
            SPARES.fetch_sub(1, Relaxed);
        }
        make(spare).map(|s| st.stream = Some(s))
    };
    match r {
        Ok(()) => Ok(a),
        Err(e) => {
            recycle_dir(a);
            Err(e)
        }
    }
}

/// `opendir(path)` (and the stream `scandir` reads, relative to `dir`).
pub fn opendir(dir: Fd, path: KResult<KStr<'_>>) -> KResult<usize> {
    let path = path?;
    install(|spare| DirStream::open_reusing(&CSys, dir, path, dir::STREAM_BUFFER, spare))
}

/// `fdopendir(fd)`: the descriptor belongs to the stream on success only.
pub fn fdopendir(fd: i32) -> KResult<usize> {
    install(|spare| DirStream::adopt(&CSys, Fd::new(fd), dir::STREAM_BUFFER, spare))
}

/// `closedir(d)`: the descriptor is closed (and released even when `close` reports an error).
pub fn closedir(d: usize) -> KResult<()> {
    let r = with_dir(d, |st| {
        let s = st.stream.take().ok_or(Errno::EBADF)?;
        let (r, buf) = s.close_keep(&CSys);
        if SPARES.fetch_add(1, Relaxed) < MAX_SPARES {
            st.spare = Some(buf);
        } else {
            SPARES.fetch_sub(1, Relaxed);
        }
        Ok(r)
    })?;
    recycle_dir(d);
    r
}

/// `readdir(d)`: the next record's address, `None` at the end.
/// Forced inline into `readdir` and `readdir64`, with only the common case here: the only
/// thread reading a buffered record needs no lock, no system call and no saved registers; the
/// rest (locking, refills, errors) is [`readdir_slow`] (DU-3).
#[inline(always)]
pub fn readdir(d: usize) -> KResult<Option<usize>> {
    let slot = dir_slot(d)?;
    if crate::thread::alone() {
        let st = tb::dir_state_at::<DirState>(slot.state.load(Relaxed));
        if let Some(Ok(Some(a))) = st.stream.as_mut().map(DirStream::buffered_addr) {
            return Ok(Some(a));
        }
    }
    readdir_slow(slot)
}

/// [`readdir`] in full: under the lock (unless alone), refilling the buffer when needed.
#[inline(never)]
fn readdir_slow(slot: &DirSlot) -> KResult<Option<usize>> {
    let _g = lock_unless_alone(&slot.lock);
    let st = tb::dir_state_at::<DirState>(slot.state.load(Relaxed));
    st.stream.as_mut().ok_or(Errno::EBADF)?.next_addr(&CSys)
}

/// An entry's fields as the `_r` forms store them: `d_ino`, `d_off`, `d_reclen`, `d_type`,
/// `d_name`.
type Fields = (u64, i64, u16, u8, [i8; 256]);

/// The fields of the next entry for the `_r` forms: `None` at the end.
fn next_fields(d: usize) -> KResult<Option<Fields>> {
    with_stream(d, |s| {
        let Some(e) = s.next(&CSys)? else { return Ok(None) };
        let name = s.name(&e);
        if name.len() > 255 {
            return Err(Errno::ENAMETOOLONG);
        }
        let mut n = [0i8; 256];
        for (d, &c) in n.iter_mut().zip(name) {
            *d = i8::from_ne_bytes([c]);
        }
        Ok(Some((e.ino, e.off, e.reclen, e.kind, n)))
    })
}

/// `readdir_r(d, entry, result)`: 0 with `*result` the entry (or NULL at the end), or the
/// error number (`*result` NULL).
pub fn readdir_r(d: usize, entry: Option<&mut struct_dirent>, result: Option<&mut *mut struct_dirent>) -> KResult<()> {
    let (Some(entry), Some(result)) = (entry, result) else { return Err(Errno::EFAULT) };
    *result = core::ptr::null_mut();
    if let Some((ino, off, reclen, kind, name)) = next_fields(d)? {
        (entry.d_ino, entry.d_off, entry.d_reclen, entry.d_type, entry.d_name) = (ino, off, reclen, kind, name);
        *result = core::ptr::from_mut(entry);
    }
    Ok(())
}

/// `readdir64_r`: as [`readdir_r`].
pub fn readdir64_r(d: usize, entry: Option<&mut struct_dirent64>, result: Option<&mut *mut struct_dirent64>) -> KResult<()> {
    let (Some(entry), Some(result)) = (entry, result) else { return Err(Errno::EFAULT) };
    *result = core::ptr::null_mut();
    if let Some((ino, off, reclen, kind, name)) = next_fields(d)? {
        (entry.d_ino, entry.d_off, entry.d_reclen, entry.d_type, entry.d_name) = (ino, off, reclen, kind, name);
        *result = core::ptr::from_mut(entry);
    }
    Ok(())
}

/// `telldir(d)`.
pub fn telldir(d: usize) -> KResult<i64> {
    with_stream(d, |s| Ok(s.tell(&CSys)))
}

/// `seekdir(d, pos)`.
pub fn seekdir(d: usize, pos: i64) -> KResult<()> {
    with_stream(d, |s| s.seek(&CSys, pos))
}

/// `rewinddir(d)`.
pub fn rewinddir(d: usize) -> KResult<()> {
    with_stream(d, |s| s.rewind(&CSys))
}

/// `dirfd(d)`.
pub fn dirfd(d: usize) -> KResult<i64> {
    with_stream(d, |s| Ok(i64::from(s.fd().raw())))
}

/// `getdirentries(fd, buf, n, basep)`: `*basep` is the position before the read.
pub fn getdirentries(fd: i32, buf: KResult<&mut [u8]>, basep: Option<&mut i64>) -> KResult<usize> {
    use liella_kernel::sys::Sys;
    let (buf, basep) = (buf?, basep.ok_or(Errno::EFAULT)?);
    let base = CSys.lseek(Fd::new(fd), 0, liella_kernel::uapi::SEEK_CUR)?;
    let n = CSys.getdents64(Fd::new(fd), buf)?;
    *basep = i64::try_from(base).unwrap_or(i64::MAX);
    Ok(n)
}

// ------------------------------------------------------------------------------ §13.3 scandir

/// What a `scandir` holds while its callbacks run (OD-6 revised): the open directory, the
/// entries kept so far (each from the caller's allocator) and the sort's second buffer.
#[derive(Default)]
struct Scan {
    stream: Option<DirStream>,
    kept: TryVec<usize>,
    spare: Option<TryVec<usize>>,
    /// Whether `kept` (else `spare`) holds every entry (`fs_tree::sort_slots_in`).
    kept_holds: core::cell::Cell<bool>,
}

/// A `scandir` callback threw: the directory is closed and the entries freed.
fn scan_unwound(st: &mut Scan) {
    if let Some(s) = st.stream.take() {
        let _ = s.close(&CSys);
    }
    let all = if st.kept_holds.get() { Some(&st.kept) } else { st.spare.as_ref() };
    for &a in all.into_iter().flat_map(|v| v.iter()) {
        caller_alloc::release(a);
    }
    st.kept = TryVec::default();
    st.spare = None;
}

/// `scandirat(dir, path, &list, filter, compar)`: the number of entries kept; `*list` is
/// written only on success. The callbacks run in a guard frame: if one throws, the directory is
/// closed and the entries kept so far are freed (OD-6 revised).
pub fn scandirat<T>(dir: Fd, path: KResult<KStr<'_>>, list: Option<&mut *mut *mut T>, filter: Option<DirFilterFn>, compar: Option<SlotCompareFn>) -> KResult<i64> {
    let list = list.ok_or(Errno::EFAULT)?;
    let mut st = Scan { kept: TryVec::with_capacity(32)?, kept_holds: core::cell::Cell::new(true), ..Scan::default() };
    st.stream = Some(DirStream::open(&CSys, dir, path?, dir::STREAM_BUFFER)?);
    let body = |st: &mut Scan| -> KResult<usize> {
        let Scan { stream, kept, spare, kept_holds } = st;
        let s = stream.as_mut().ok_or(Errno::EBADF)?;
        while let Some(e) = s.next(&CSys)? {
            let keep = filter.is_none_or(|f| f(core::ptr::with_exposed_provenance(s.record_addr(&e))) != 0);
            if keep {
                kept.reserve(1)?;
                let a = caller_alloc::dup_bytes(s.record(&e))?;
                kept.push(a)?;
            }
        }
        if let Some(c) = compar {
            fs_tree::sort_slots_in(kept, |a| *a, c, spare, kept_holds)?;
        }
        let mut bytes = TryVec::with_capacity(kept.len().saturating_mul(8))?;
        for a in kept.iter() {
            bytes.extend_from_slice(&a.to_ne_bytes())?;
        }
        caller_alloc::dup_bytes(&bytes)
    };
    let r = crate::posix::s22_threads::guarded(&mut st, body, scan_unwound).unwrap_or(Err(Errno::EFAULT));
    if let Some(s) = st.stream.take() {
        let _ = s.close(&CSys);
    }
    match r {
        Ok(array) => {
            *list = core::ptr::with_exposed_provenance_mut(array);
            Ok(i64::try_from(st.kept.len()).unwrap_or(i64::MAX))
        }
        Err(e) => {
            for &a in st.kept.iter() {
                caller_alloc::release(a);
            }
            Err(e)
        }
    }
}

// ------------------------------------------------------------------------------ §13.1 cwd

/// `getcwd(buf, size)`: `buf`, or (for a NULL `buf`) a new block of `size` bytes (the exact
/// size for 0).
pub fn getcwd(buf: KResult<Option<&mut [u8]>>, size: usize) -> KResult<usize> {
    match buf? {
        Some(b) => {
            cwd::into_buffer(&CSys, b)?;
            Ok(b.as_ptr().expose_provenance())
        }
        None => {
            let p = cwd::current(&CSys)?;
            if size != 0 && p.len() >= size {
                return Err(Errno::ERANGE);
            }
            alloc_text(&p, size)
        }
    }
}

/// `__getcwd_chk(buf, size, buflen)`: as `getcwd`, terminating if the path would be written
/// beyond the `buflen`-byte object.
pub fn getcwd_chk(buf: KResult<Option<&mut [u8]>>, size: usize, buflen: usize) -> KResult<usize> {
    match buf? {
        Some(b) if size > buflen => match cwd::into_buffer(&CSys, b) {
            Ok(_) => Ok(b.as_ptr().expose_provenance()),
            Err(Errno::ERANGE) => match cwd::current(&CSys) {
                Ok(p) if p.len() < size => ret::chk_fail(),
                _ => Err(Errno::ERANGE),
            },
            Err(e) => Err(e),
        },
        other => getcwd(Ok(other), size),
    }
}

/// `getwd(buf)` (`buf` holds `PATH_MAX` bytes, or `buflen` for `__getwd_chk`): a path longer
/// than `PATH_MAX` is `ENAMETOOLONG` (getwd(3)); a NULL buffer is `EINVAL`.
pub fn getwd(buf: KResult<Option<&mut [u8]>>) -> KResult<usize> {
    let b = buf?.ok_or(Errno::EINVAL)?;
    let p = cwd::current(&CSys)?;
    if p.len() >= PATH_MAX {
        return Err(Errno::ENAMETOOLONG);
    }
    put_text(b, &p);
    Ok(b.as_ptr().expose_provenance())
}

/// `get_current_dir_name()`: a new block holding `$PWD` if it is a plain absolute path naming the
/// working directory, else the path `getcwd` gives. `$PWD` is ignored in secure-execution mode
/// (owner ruling 2026-10-03: a setuid program must not report a path its caller chose).
pub fn get_current_dir_name() -> KResult<usize> {
    let pwd = process::secure_getenv(b"PWD").map(strz::env_value);
    let p = cwd::named(&CSys, pwd)?;
    alloc_text(&p, 0)
}

// ------------------------------------------------------------------------------ §13.8 realpath

/// `realpath(path, resolved)` with `resolved` the caller's object (`PATH_MAX` bytes, or
/// `resolvedlen` for `__realpath_chk`) or NULL (a new block). With an object, a result longer
/// than `PATH_MAX` is `ENAMETOOLONG`, and a failure leaves there the prefix
/// `liella::fs::canon` reports (when it fits `PATH_MAX`); otherwise the object is untouched.
pub fn realpath(path: KResult<Option<&[u8]>>, resolved: KResult<Option<&mut [u8]>>) -> KResult<usize> {
    let path = path?.ok_or(Errno::EINVAL)?;
    let out = resolved?;
    let r = canon::canonicalize(&CSys, path);
    match out {
        None => match r {
            Ok(p) => alloc_text(&p, 0),
            Err(c) => Err(c.errno),
        },
        Some(b) => match r {
            Ok(p) if p.len() < PATH_MAX => {
                put_text(b, &p);
                Ok(b.as_ptr().expose_provenance())
            }
            Ok(_) => Err(Errno::ENAMETOOLONG),
            Err(c) => {
                if let Some(pre) = c.prefix.as_ref().filter(|p| p.len() < PATH_MAX) {
                    put_text(b, pre);
                }
                Err(c.errno)
            }
        },
    }
}

// ------------------------------------------------------------------------------ §13.9 names

/// `mkostemps(template, suffixlen, flags)` and its relatives: the new file's descriptor.
pub fn mkostemps(template: KResult<&mut [u8]>, suffix: i32, flags: i32) -> KResult<Fd> {
    let suffix = usize::try_from(suffix).map_err(|_| Errno::EINVAL)?;
    match temp::make(&CSys, Fd::CWD, template?, suffix, Create::File(conv::bits(flags)))? {
        Made::File(fd) => Ok(fd),
        Made::Done => Err(Errno::EIO),
    }
}

/// `mkdtemp(template)`.
pub fn mkdtemp(template: KResult<&mut [u8]>) -> KResult<()> {
    temp::make(&CSys, Fd::CWD, template?, 0, Create::Dir).map(|_| ())
}

/// `mktemp(template)`: the error to report in `errno`, if any (the template is then empty).
pub fn mktemp(template: KResult<&mut [u8]>) -> Option<Errno> {
    match template {
        Ok(t) => fs_tree::mktemp(&CSys, t),
        Err(e) => Some(e),
    }
}

// ------------------------------------------------------------------------------ §13.3 fts

/// The caller's allocator for `FTSENT` blocks.
struct Blocks;

impl FtsMem for Blocks {
    fn alloc(&mut self, head: usize, tail: &[&[u8]]) -> Option<usize> {
        let len = tail.iter().try_fold(head, |a, t| a.checked_add(t.len()))?.checked_add(1)?;
        isize::try_from(len).ok()?;
        let p = caller_alloc::malloc(len);
        if p.is_null() {
            return None;
        }
        conv::fresh_fill_z(p.cast::<u8>(), head, tail);
        Some(p.expose_provenance())
    }

    fn free(&mut self, addr: usize) {
        caller_alloc::release(addr);
    }

    fn bytes(&mut self, addr: usize, len: usize) -> &mut [u8] {
        tb::walk_block(addr, len)
    }

    fn copy(&mut self, from: (usize, usize), to: (usize, usize), len: usize) {
        // Two different live blocks (the trait's contract), each slice inside its own.
        let src: &[u8] = tb::walk_block(from.0, from.1.saturating_add(len));
        let dst = tb::walk_block(to.0, to.1.saturating_add(len));
        if let (Some(s), Some(d)) = (src.get(from.1..), dst.get_mut(to.1..)) {
            d.copy_from_slice(s);
        }
    }
}

/// Publish the stream's state in the C `FTS` words.
fn publish(slot: &FtsSlot, f: &Fts) {
    let rfd = f.saved_cwd().map_or(-1, Fd::raw);
    let words: [usize; 9] = [
        f.current(),
        0,
        0,
        0,
        0,
        usize::try_from(u32::from_ne_bytes(rfd.to_ne_bytes())).unwrap_or(0),
        0,
        0,
        usize::try_from(u32::from_ne_bytes(f.options().to_ne_bytes())).unwrap_or(0),
    ];
    for (w, v) in slot.fts.iter().zip(words) {
        w.store(v, Relaxed);
    }
}

/// Run `f` on the open stream at `a`, under its lock, and publish its state.
fn with_fts<T>(a: usize, f: impl FnOnce(&mut Fts) -> KResult<T>) -> KResult<T> {
    let slot = fts_slot(a)?;
    let _g = slot.lock.lock(&CSys);
    let st = tb::dir_state_at::<FtsState>(slot.state.load(Relaxed));
    let fts = st.fts.as_mut().ok_or(Errno::EBADF)?;
    let r = f(fts);
    // After `fts_open` published every word, only `fts_cur` changes (`fts_rfd` and
    // `fts_options` are fixed for the stream's life): storing all nine on every call cost ~26
    // instructions per `fts_read` (FTS-FLAT-FIXES).
    if let Some(w) = slot.fts.first() {
        w.store(fts.current(), Relaxed);
    }
    r
}

/// `fts_open(argv, options, compar)`.
pub fn fts_open(argv: KResult<alloc::vec::Vec<&[u8]>>, options: i32, compar: Option<SlotCompareFn>) -> KResult<usize> {
    let argv = argv?;
    let a = new_fts_slot()?;
    let r = (|| -> KResult<()> {
        let slot = fts_slot(a)?;
        let _g = slot.lock.lock(&CSys);
        let f = Fts::open(&CSys, &mut Blocks, &argv, options, compar)?;
        publish(slot, &f);
        tb::dir_state_at::<FtsState>(slot.state.load(Relaxed)).fts = Some(f);
        Ok(())
    })();
    match r {
        Ok(()) => Ok(a),
        Err(e) => {
            recycle_fts(a);
            Err(e)
        }
    }
}

/// `fts_read(ftsp)`.
pub fn fts_read(a: usize) -> KResult<Option<usize>> {
    with_fts(a, |f| f.read(&CSys, &mut Blocks))
}

/// `fts_children(ftsp, options)`.
pub fn fts_children(a: usize, options: i32) -> KResult<Option<usize>> {
    with_fts(a, |f| f.children(&CSys, &mut Blocks, options))
}

/// `fts_set(ftsp, f, instr)`.
pub fn fts_set(a: usize, entry: usize, instr: i32) -> KResult<()> {
    with_fts(a, |f| f.set(&mut Blocks, entry, instr))
}

/// `fts_close(ftsp)`.
pub fn fts_close(a: usize) -> KResult<()> {
    let slot = fts_slot(a)?;
    let f = {
        let _g = slot.lock.lock(&CSys);
        let st = tb::dir_state_at::<FtsState>(slot.state.load(Relaxed));
        st.fts.take().ok_or(Errno::EBADF)?
    };
    let r = f.close(&CSys, &mut Blocks);
    for w in slot.fts.iter() {
        w.store(0, Relaxed);
    }
    recycle_fts(a);
    r
}
