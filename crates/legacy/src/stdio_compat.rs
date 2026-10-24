//! The C `FILE` (spec §12; D-12.2, D-12.11): the object C programs hold, its buffer window, the
//! state behind it, the open-streams list, and the stdio operations over them.
//!
//! **The object.** A `FILE *` addresses [`FILE_WORDS`] words: `struct _IO_FILE` (Appendix C.7)
//! and the vtable word that follows it in the exported `_IO_2_1_*_` objects. Header-inline
//! `getc`/`putc` and `feof`/`ferror` read the buffer window and `_flags` directly (Appendix
//! C.9), so those words always describe the stream: before an operation the engine's window is
//! read back from them (C code may have advanced it), and after the operation written to them.
//! Liella's own state is an [`Ext`] record that `_codecvt` points to, and the stream's recursive
//! lock a [`StreamLock`] that `_lock` points to; both are private to the library.
//!
//! **Locking order.** The open-streams list lock is taken before a stream's lock, never while
//! holding one (except by try-lock, in the line-buffered flush that input triggers).

#![allow(clippy::arithmetic_side_effects)] // offsets within buffers the stream allocated, checked against their lengths

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella::error::{Errno, KResult};
use liella::stream::{Backing, Policy, Role, State, Transfer, Whence, RESERVE};
use liella::iconv;
use liella::locale::Locale;
use liella::text::{self, Decoded, Encoding, MbState};
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_rt::futex::Scope;
use liella_rt::sync::RawMutex;

/// The words of a `FILE` object: `struct _IO_FILE` (216 bytes) and the vtable word (224 bytes,
/// the size of the exported `_IO_2_1_*_` objects).
pub const FILE_WORDS: usize = 28;

/// Word indices in a `FILE` (Appendix C.7 offsets / 8).
pub mod word {
    /// `_flags` (low 32 bits).
    pub const FLAGS: usize = 0;
    /// `_IO_read_ptr`.
    pub const READ_PTR: usize = 1;
    /// `_IO_read_end`.
    pub const READ_END: usize = 2;
    /// `_IO_read_base`.
    pub const READ_BASE: usize = 3;
    /// `_IO_write_base`.
    pub const WRITE_BASE: usize = 4;
    /// `_IO_write_ptr`.
    pub const WRITE_PTR: usize = 5;
    /// `_IO_write_end`.
    pub const WRITE_END: usize = 6;
    /// `_IO_buf_base`.
    pub const BUF_BASE: usize = 7;
    /// `_IO_buf_end`.
    pub const BUF_END: usize = 8;
    /// `_chain`: the next stream in the open-streams list.
    pub const CHAIN: usize = 13;
    /// `_fileno` (low 32 bits) and `_flags2` (high 32 bits).
    pub const FILENO: usize = 14;
    /// `_lock`: the address of the stream's [`super::StreamLock`].
    pub const LOCK: usize = 17;
    /// `_codecvt`: the address of the stream's [`super::Ext`] (0 before first use).
    pub const EXT: usize = 19;
    /// `_mode` (low 32 bits): the orientation, negative byte, positive wide, 0 undecided.
    pub const MODE: usize = 24;
}

/// `_IO_EOF_SEEN` (public, Appendix C.9).
pub const EOF_SEEN: usize = 0x10;
/// `_IO_ERR_SEEN` (public, Appendix C.9).
pub const ERR_SEEN: usize = 0x20;
/// `_IO_USER_LOCK` (public, Appendix C.9): the caller locks (`__fsetlocking`).
pub const USER_LOCK: usize = 0x8000;

/// `BUFSIZ`: the buffer of a stream whose backing gives no block size.
pub const BUFSIZ: usize = 8192;

fn get(file: &[AtomicUsize], i: usize) -> usize {
    file.get(i).map_or(0, |w| w.load(Relaxed))
}

fn set(file: &[AtomicUsize], i: usize, v: usize) {
    if let Some(w) = file.get(i) {
        w.store(v, Relaxed);
    }
}

/// The low 32 bits of a word as a C `int`.
fn low_int(v: usize) -> i32 {
    i32::from_ne_bytes(u32::try_from(v & 0xFFFF_FFFF).unwrap_or(0).to_ne_bytes())
}

fn with_low_int(v: usize, x: i32) -> usize {
    (v & !0xFFFF_FFFF) | usize::try_from(u32::from_ne_bytes(x.to_ne_bytes())).unwrap_or(0)
}

/// `_fileno`.
pub fn fileno(file: &[AtomicUsize]) -> i32 {
    low_int(get(file, word::FILENO))
}

// ------------------------------------------------------------------------------ the lock

/// A stream's recursive lock (`flockfile`): a futex-based mutex, its owner's TID and depth.
#[derive(Debug, Default)]
pub struct StreamLock {
    raw: RawMutex,
    owner: AtomicU32,
    depth: AtomicU32,
}

impl StreamLock {
    /// Unlocked.
    pub const fn new() -> StreamLock {
        StreamLock { raw: RawMutex::new(), owner: AtomicU32::new(0), depth: AtomicU32::new(0) }
    }

    /// Take the lock for thread `me` (recursively).
    ///
    /// `depth` is only ever written by the thread holding `raw` (the owner), so it is updated with
    /// a plain load and store, never a read-modify-write: on x86 a `fetch_add` is a LOCK-prefixed
    /// instruction, a full barrier costing tens of cycles per stdio call (MUTEX-FAST). Another
    /// thread reads `owner` only to compare it with its own TID, which a stale value never equals.
    pub fn lock<S: Sys>(&self, sys: &S, me: u32) {
        if !self.lock_fast(me) {
            self.lock_slow(sys, me);
        }
    }

    /// [`StreamLock::lock`] without waiting (FILE-LOCK-2): taken (again) by `me`, or false when
    /// another thread holds it and nothing was taken. No call, so a caller's fast path needs no
    /// stack frame; on false it goes the full way ([`StreamLock::lock`]).
    #[inline(always)]
    pub fn lock_fast(&self, me: u32) -> bool {
        if self.owner.load(Relaxed) == me {
            self.depth.store(self.depth.load(Relaxed).wrapping_add(1), Relaxed);
            return true;
        }
        if !self.raw.try_lock() {
            return false;
        }
        self.owner.store(me, Relaxed);
        self.depth.store(1, Relaxed);
        true
    }

    /// [`StreamLock::lock`] when the lock is held by another thread: wait for it.
    #[cold]
    #[inline(never)]
    fn lock_slow<S: Sys>(&self, sys: &S, me: u32) {
        self.raw.lock_raw(sys, Scope::Private);
        self.owner.store(me, Relaxed);
        self.depth.store(1, Relaxed);
    }

    /// Take the lock if free or already held by `me`.
    #[inline]
    pub fn try_lock(&self, me: u32) -> bool {
        if self.owner.load(Relaxed) == me {
            self.depth.store(self.depth.load(Relaxed).wrapping_add(1), Relaxed);
            return true;
        }
        if !self.raw.try_lock() {
            return false;
        }
        self.owner.store(me, Relaxed);
        self.depth.store(1, Relaxed);
        true
    }

    /// Release one level held by `me`.
    pub fn unlock<S: Sys>(&self, sys: &S, me: u32) {
        if self.owner.load(Relaxed) != me {
            return;
        }
        if self.unlock_held() {
            self.wake(sys);
        }
    }

    /// Release one level of a lock the calling thread holds (it took it with
    /// [`StreamLock::lock_fast`] or [`StreamLock::lock`] and has not released it since, so the owner
    /// check of [`StreamLock::unlock`] is known to pass). True when the lock became free with a
    /// waiter to wake: the caller then calls [`StreamLock::wake`] (FILE-LOCK-2: out of its fast path).
    #[inline(always)]
    pub fn unlock_held(&self) -> bool {
        // Owner-only (see `lock`): a plain load and store.
        let d = self.depth.load(Relaxed);
        if d <= 1 {
            self.depth.store(0, Relaxed);
            self.owner.store(0, Relaxed);
            self.raw.release()
        } else {
            self.depth.store(d - 1, Relaxed);
            false
        }
    }

    /// Wake one waiter after [`StreamLock::unlock_held`] returned true.
    #[cold]
    #[inline(never)]
    pub fn wake<S: Sys>(&self, sys: &S) {
        self.raw.wake_one(sys, Scope::Private);
    }

    /// In a `fork` child (only the forking thread `me` survives): a lock another thread held
    /// becomes free.
    pub fn reset_in_child(&self, me: u32) {
        if self.owner.load(Relaxed) != me {
            self.owner.store(0, Relaxed);
            self.depth.store(0, Relaxed);
            self.raw.reset();
        }
    }
}

// ------------------------------------------------------------------------------ the state

/// A fixed memory buffer (`fmemopen`, §12.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MemStream {
    /// The buffer.
    pub addr: usize,
    /// Its size.
    pub size: usize,
    /// The end of the data.
    pub len: usize,
    /// The position.
    pub pos: usize,
    /// The library allocated the buffer (freed at close).
    pub owned: bool,
    /// The `GLIBC_2.2.5` contract (`fmemopen@GLIBC_2.2.5`, LG-2): reads run to `size`, not to
    /// `len`, and `append` only places the first position.
    pub old: bool,
    /// Writes append.
    pub append: bool,
    /// Writes keep a NUL after the data (`w` and `a` modes, not `r+`): a write that would run
    /// to the end leaves room for it, unless the data itself ends with a NUL.
    pub nul: bool,
}

/// A growing buffer (`open_memstream`, `open_wmemstream`, §12.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct GrowStream {
    /// The caller's `char **` (or `wchar_t **`).
    pub ptr_var: usize,
    /// The caller's `size_t *`.
    pub size_var: usize,
    /// The buffer (`malloc`ed; the caller frees it).
    pub buf: usize,
    /// Its capacity in bytes, room for the terminator included.
    pub cap: usize,
    /// The end of the data, in bytes (a seek back moves it back).
    pub len: usize,
    /// The bytes ever written (the terminator follows them); what lies between `len` and
    /// this stays in the buffer, as in the reference library.
    pub filled: usize,
    /// The position, in bytes.
    pub pos: usize,
    /// Units of 4 bytes (`open_wmemstream`).
    pub wide: bool,
}

/// Caller callbacks (`fopencookie`, §12.3): function addresses, 0 where absent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CookieStream {
    /// The cookie.
    pub cookie: usize,
    /// `ssize_t read(void *, char *, size_t)`.
    pub read: usize,
    /// `ssize_t write(void *, const char *, size_t)`.
    pub write: usize,
    /// `int seek(void *, off64_t *, int)`.
    pub seek: usize,
    /// `int close(void *)`.
    pub close: usize,
}

/// What a stream reads and writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    /// Closed, or not yet set up.
    #[default]
    Closed,
    /// A file descriptor.
    Fd(i32),
    /// A fixed memory buffer.
    Mem(MemStream),
    /// A growing buffer.
    Grow(GrowStream),
    /// Caller callbacks.
    Cookie(CookieStream),
}

/// How a wide-oriented stream's characters become bytes (D-12.7, D-12.14).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Codec {
    /// Not wide-oriented.
    #[default]
    Unset,
    /// A multibyte encoding: the locale's when the stream became wide-oriented.
    Text(Encoding),
    /// `wchar_t` units as they are (`open_wmemstream`).
    Units,
    /// A charset named by `fopen`'s `,ccs=`: conversions to and from UCS-4; output
    /// transliterates what the charset lacks (as the reference library does).
    Charset {
        /// Charset to UCS-4 (little-endian).
        dec: iconv::Conv,
        /// UCS-4 (little-endian) to the charset.
        enc: iconv::Conv,
    },
}

impl Codec {
    /// The codec of a charset (`,ccs=`).
    pub fn charset(c: iconv::Codec) -> Codec {
        let ucs4 = iconv::Codec::Ucs4(iconv::Order::Le);
        let conv = |from, to, translit| iconv::Conv { from, to, translit, ignore: false, state: iconv::State::default() };
        // Output in UTF-16 or UTF-32 without a byte order is little-endian, without a BOM.
        let out = match c {
            iconv::Codec::Utf16(None) => iconv::Codec::Utf16(Some(iconv::Order::Le)),
            iconv::Codec::Utf32(None) => iconv::Codec::Utf32(Some(iconv::Order::Le)),
            other => other,
        };
        Codec::Charset { dec: conv(c, ucs4, false), enc: conv(ucs4, out, true) }
    }
}

/// Wide characters a stream can give back (`ungetwc`); the reference library takes three.
pub const WIDE_BACK: usize = 4;

/// The wide layer of a stream (§12.10; D-12.14): the codec, the input conversion state, and the
/// wide characters given back, which are read before any byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Wide {
    /// The codec (fixed when the stream becomes wide-oriented).
    pub codec: Codec,
    /// The input conversion state: a character's leading bytes read so far.
    pub decode: MbState,
    /// Characters given back, the last one first out.
    back: [u32; WIDE_BACK],
    /// How many.
    back_len: usize,
    /// The last character read (`_IO_sungetwc`).
    last: Option<u32>,
    /// A charset character's leading bytes read so far (`Codec::Charset`).
    pending: [u8; 8],
    /// How many.
    pending_len: usize,
    /// The bytes of a partial character (`Codec::Text`) read so far.
    partial: usize,
}

/// Liella's state of one stream (D-12.11).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Ext {
    /// The engine's state.
    pub state: State,
    /// The backing.
    pub kind: Kind,
    /// The buffer (`RESERVE` bytes of pushback room, then the data), 0 until first use.
    pub buf: usize,
    /// Its length, the reserve included.
    pub buf_len: usize,
    /// The library allocated it (freed at close).
    pub buf_owned: bool,
    /// The data size `setvbuf` asked for (0: the default).
    pub want: usize,
    /// The caller's buffer from `setvbuf` (0: none).
    pub user_buf: usize,
    /// The wide layer (§12.10).
    pub wide: Wide,
    /// The stream was set up (as opposed to a standard stream not used yet).
    pub open: bool,
    /// Its `fprintf` plan cache (D-12.21; used under the lock), made once the stream is busy.
    pub printf_plans: PlanSlot,
    /// A descriptor backing's offset, learnt from its last successful seek and advanced by the
    /// reads since (D-12.22); meaningful only with [`Ext::pos_known`].
    pub pos: u64,
    /// [`Ext::pos`] is known: cleared by a write or a failed transfer, and at setup.
    pub pos_known: bool,
}

/// Where a stream's `fprintf` plan cache lives (D-12.21): a [`liella::printf::PlanCache`] made
/// on the [`PlanSlot::AFTER`]th formatted call and kept for the slot's life, across `fclose`
/// and `freopen` (a cache keyed on the format's bytes is not state). Outside the record so a
/// one-shot printer pays neither its ~220 bytes in every stream's slot nor its fill: inline,
/// the slot crossed a heap size class and cost every process ~2.7k instructions and two page
/// faults (printf-loop, PRINTF-STAR-2).
#[derive(Clone, Copy, Debug, Default)]
pub struct PlanSlot {
    /// The cache's address; 0 until it is made.
    pub addr: usize,
    /// Formatted calls counted before it is made (at most [`PlanSlot::AFTER`]).
    pub calls: u8,
}

impl PlanSlot {
    /// The formatted call that makes the cache: earlier ones parse as before.
    pub const AFTER: u8 = 8;
}

impl PartialEq for PlanSlot {
    /// A cache is not state: it never makes two streams' records differ.
    fn eq(&self, _: &PlanSlot) -> bool {
        true
    }
}

impl Eq for PlanSlot {}

/// The size of a stream's spare buffer ([`Env::spare`]), the pushback reserve included.
pub const SPARE: usize = RESERVE + 120;

/// What the library provides to the stream operations (the POSIX layer).
pub trait Env {
    /// The kernel.
    type S: Sys;
    /// The kernel.
    fn sys(&self) -> &Self::S;
    /// The calling thread's TID.
    fn tid(&self) -> u32;
    /// The `len` bytes at `addr`: a buffer this module allocated, or memory a caller gave a
    /// stream. Used only while the stream's lock is held.
    #[allow(clippy::mut_from_ref)]
    fn bytes(&self, addr: usize, len: usize) -> &'static mut [u8];
    /// The word at `addr` (a caller's variable).
    fn load_word(&self, addr: usize) -> usize;
    /// Store the word at `addr` (a caller's variable).
    fn store_word(&self, addr: usize, v: usize);
    /// `malloc` (0 when out of memory).
    fn malloc(&self, n: usize) -> usize;
    /// The spare buffer of the stream whose record is at `ext` (its address and size, at least
    /// [`SPARE`] bytes; `(0, 0)` for none): used under the stream's lock when a buffer cannot be
    /// allocated (T22).
    fn spare(&self, ext: usize) -> (usize, usize) {
        let _ = ext;
        (0, 0)
    }
    /// The index of the first `c` in `s` (`memchr`).
    fn find(&self, s: &[u8], c: u8) -> Option<usize>;
    /// The `getdelim` front path's scan and copy (GETDELIM-FUSE, D-12.19): for the first `c` in
    /// `s` at an index `i < d.len() - 1`, `s[..=i]` and a NUL go to `d[..=i + 1]` and the result
    /// is `i + 1`; nothing past that NUL is written. `None` when there is no such `c`; a prefix
    /// of the record may then be in `d` (the general path writes the same bytes there again).
    /// A library may fuse the two passes ([`find_copy`] is the two-pass form).
    #[inline(always)]
    fn find_copy(&self, s: &[u8], d: &mut [u8], c: u8) -> Option<usize> {
        find_copy(self, s, d, c)
    }
    /// `realloc` (0 when out of memory; the block is kept).
    fn realloc(&self, addr: usize, n: usize) -> usize;
    /// `free`.
    fn free(&self, addr: usize);
    /// Call the C function at `f` with three arguments.
    fn call3(&self, f: usize, a: usize, b: usize, c: usize) -> isize;
    /// The head of the open-streams list (`_IO_list_all`).
    fn list_head(&self) -> &AtomicUsize;
    /// The words of the stream at `addr` (a `FILE *` from the list).
    fn file_at(&self, addr: usize) -> &'static [AtomicUsize];
    /// The lock of the stream whose `FILE` is at `fp` and whose `_codecvt` word is `ext`, when
    /// `ext` is the address of a record the library made for that very `FILE` (D-12.19: the
    /// check that keeps a crafted or corrupted `FILE` from steering the library's writes);
    /// `false` for anything else (0: a standard stream not set up yet).
    fn record(&self, fp: usize, ext: usize) -> bool;
    /// The lock of the stream whose record [`Env::record`] accepted at `ext`.
    fn lock_of(&self, ext: usize) -> &'static StreamLock;
    /// The record at `addr` (an `_codecvt` word). Used only while the stream's lock is held.
    #[allow(clippy::mut_from_ref)]
    fn ext_at(&self, addr: usize) -> &'static mut Ext;
}

// ------------------------------------------------------------------------------ backings

/// A stream's backing for one operation.
struct Live<'a, E: Env> {
    env: &'a E,
    kind: &'a mut Kind,
    /// [`Ext::pos`] and [`Ext::pos_known`], kept up to date by every descriptor transfer.
    pos: &'a mut u64,
    pos_known: &'a mut bool,
}

/// Track a descriptor read of `r` in the known offset (D-12.22).
#[inline(always)]
fn pos_after_read(pos: &mut u64, known: &mut bool, r: &KResult<usize>) {
    match r {
        // FTELL-CACHE: end of input forgets the offset, so `ftell` there asks the kernel, as the
        // reference library does after `fgetc` reaches the end (probe f21612fa).
        Ok(0) => *known = false,
        Ok(n) if *known => match u64::try_from(*n).ok().and_then(|n| pos.checked_add(n)) {
            Some(p) => *pos = p,
            None => *known = false,
        },
        Ok(_) => {}
        Err(_) => *known = false,
    }
}

fn fd_of(fd: i32) -> Fd {
    Fd::new(fd)
}

fn whence_code(w: Whence) -> u32 {
    match w {
        Whence::Set => liella_kernel::uapi::SEEK_SET,
        Whence::Cur => liella_kernel::uapi::SEEK_CUR,
        Whence::End => liella_kernel::uapi::SEEK_END,
    }
}

fn isize_result(r: isize) -> KResult<usize> {
    usize::try_from(r).map_err(|_| Errno::EIO)
}

impl<E: Env> Backing for Live<'_, E> {
    fn read(&mut self, buf: &mut [u8]) -> KResult<usize> {
        match self.kind {
            Kind::Closed => Err(Errno::EBADF),
            Kind::Fd(fd) => {
                let r = self.env.sys().read(fd_of(*fd), buf);
                pos_after_read(self.pos, self.pos_known, &r);
                r
            }
            Kind::Mem(m) => {
                let end = if m.old { m.size } else { m.len };
                let avail = end.saturating_sub(m.pos);
                let n = avail.min(buf.len());
                let src = self.env.bytes(m.addr, m.size);
                if let (Some(d), Some(s)) = (buf.get_mut(..n), src.get(m.pos..m.pos + n)) {
                    liella::mem::copy(d, s);
                }
                m.pos += n;
                Ok(n)
            }
            Kind::Grow(_) => Err(Errno::EBADF),
            Kind::Cookie(c) => {
                // A missing callback is an error, as the reference library behaves.
                if c.read == 0 {
                    return Err(Errno::EBADF);
                }
                let r = self.env.call3(c.read, c.cookie, core::ptr::from_mut(buf).cast::<u8>().expose_provenance(), buf.len());
                isize_result(r)
            }
        }
    }

    fn write(&mut self, buf: &[u8]) -> KResult<usize> {
        match self.kind {
            Kind::Closed => Err(Errno::EBADF),
            Kind::Fd(fd) => {
                // Not tracked (an `O_APPEND` descriptor writes at the end): unknown until the
                // next seek.
                *self.pos_known = false;
                self.env.sys().write(fd_of(*fd), buf)
            }
            Kind::Mem(m) => mem_write(self.env, m, buf),
            Kind::Grow(g) => grow_write(self.env, g, buf),
            Kind::Cookie(c) => {
                if c.write == 0 {
                    return Err(Errno::EBADF);
                }
                let r = self.env.call3(c.write, c.cookie, core::ptr::from_ref(buf).cast::<u8>().expose_provenance(), buf.len());
                match isize_result(r) {
                    Ok(0) | Err(_) => Err(Errno::EIO),
                    ok => ok,
                }
            }
        }
    }

    fn seek(&mut self, offset: i64, whence: Whence) -> KResult<u64> {
        match self.kind {
            Kind::Closed => Err(Errno::EBADF),
            Kind::Fd(fd) => {
                let r = self.env.sys().lseek(fd_of(*fd), offset, whence_code(whence));
                *self.pos_known = r.is_ok();
                if let Ok(at) = r {
                    *self.pos = at;
                }
                r
            }
            Kind::Mem(m) => {
                let base = match whence {
                    Whence::Set => 0,
                    Whence::Cur => m.pos,
                    Whence::End => m.len,
                };
                let to = i64::try_from(base).ok().and_then(|b| b.checked_add(offset)).ok_or(Errno::EINVAL)?;
                let to = usize::try_from(to).map_err(|_| Errno::EINVAL)?;
                if to > m.size {
                    return Err(Errno::EINVAL);
                }
                m.pos = to;
                u64::try_from(to).map_err(|_| Errno::EOVERFLOW)
            }
            Kind::Grow(g) => {
                let unit = if g.wide { 4 } else { 1 };
                let base = match whence {
                    Whence::Set => 0,
                    Whence::Cur => g.pos / unit,
                    Whence::End => g.len / unit,
                };
                let to = i64::try_from(base).ok().and_then(|b| b.checked_add(offset)).ok_or(Errno::EINVAL)?;
                let to = usize::try_from(to).map_err(|_| Errno::EINVAL)?;
                g.pos = to.checked_mul(unit).ok_or(Errno::EINVAL)?;
                // A seek back ends the data there, as the reference library's memory streams
                // do: what follows is not published, and `SEEK_END` counts from there (the
                // bytes stay in the buffer).
                if g.pos < g.len {
                    g.len = g.pos;
                }
                u64::try_from(to).map_err(|_| Errno::EOVERFLOW)
            }
            Kind::Cookie(c) => {
                if c.seek == 0 {
                    return Err(Errno::ESPIPE);
                }
                // The callback takes the offset by address and stores the result there.
                let mut off = offset;
                let code = usize::try_from(whence_code(whence)).unwrap_or(0);
                let r = self.env.call3(c.seek, c.cookie, core::ptr::from_mut(&mut off).expose_provenance(), code);
                if r != 0 {
                    return Err(Errno::EINVAL);
                }
                u64::try_from(off).map_err(|_| Errno::EINVAL)
            }
        }
    }

    fn position(&self) -> Option<u64> {
        match self.kind {
            Kind::Fd(_) if *self.pos_known => Some(*self.pos),
            _ => None,
        }
    }
}

/// A write to a fixed buffer (the reference library's observed rules, §12.3): as much as fits;
/// in the modes that keep a NUL after the data, a write that would run past the end stops one
/// byte short, and a NUL follows the data whenever it ends before the buffer does.
fn mem_write<E: Env>(env: &E, m: &mut MemStream, data: &[u8]) -> KResult<usize> {
    if m.append && !m.old {
        m.pos = m.len;
    }
    let add_nul = usize::from(m.nul && data.last() != Some(&0));
    let mut n = data.len();
    if m.pos + n > m.size {
        if m.pos + add_nul >= m.size {
            return Err(Errno::ENOSPC);
        }
        n = m.size - m.pos - add_nul;
    }
    let buf = env.bytes(m.addr, m.size);
    if let (Some(d), Some(s)) = (buf.get_mut(m.pos..m.pos + n), data.get(..n)) {
        liella::mem::copy(d, s);
    }
    m.pos += n;
    if m.pos > m.len {
        m.len = m.pos;
        if m.len < m.size && add_nul == 1 {
            if let Some(b) = buf.get_mut(m.len) {
                *b = 0;
            }
        }
    }
    Ok(n)
}

/// An `fmemopen` stream's backing for `mode` over the `size` bytes at `addr` (`owned`: the
/// library allocated them, zeroed): `r` and `r+` hold `size` bytes of data, `w` none (`w+`
/// also stores a NUL first), `a` up to the first NUL.
///
/// `old` is the `fmemopen@GLIBC_2.2.5` contract (probe record `spec/behaviour/compat.glibc.txt`):
/// `w` stores a NUL first without `+` too, `r+` holds the data up to the first NUL (its
/// `SEEK_END`), and every writable mode keeps a NUL after the data.
pub fn mem_backing<E: Env>(env: &E, addr: usize, size: usize, m: &Mode, owned: bool, old: bool) -> MemStream {
    let buf = env.bytes(addr, size);
    let strlen = |buf: &[u8]| buf.iter().position(|&b| b == 0).unwrap_or(size);
    let (len, pos) = match m.first {
        b'r' if old && m.write => (strlen(buf), 0),
        b'r' => (size, 0),
        b'w' => {
            if old || (m.read && m.write) {
                if let Some(b) = buf.first_mut() {
                    *b = 0;
                }
            }
            (0, 0)
        }
        _ => {
            let l = strlen(buf);
            (l, l)
        }
    };
    MemStream { addr, size, len, pos, owned, old, append: m.append, nul: old || m.first != b'r' }
}

/// A write to a growing buffer: grows it (zero-filling a gap after a seek past the end), and
/// keeps a terminator after the data.
fn grow_write<E: Env>(env: &E, g: &mut GrowStream, data: &[u8]) -> KResult<usize> {
    let unit = if g.wide { 4 } else { 1 };
    let end = g.pos.checked_add(data.len()).ok_or(Errno::ENOMEM)?;
    let need = end.max(g.filled).checked_add(unit).ok_or(Errno::ENOMEM)?;
    if need > g.cap {
        let cap = need.max(g.cap.saturating_mul(2)).max(64);
        let nb = env.realloc(g.buf, cap);
        if nb == 0 {
            return Err(Errno::ENOMEM);
        }
        g.buf = nb;
        g.cap = cap;
    }
    let buf = env.bytes(g.buf, g.cap);
    if g.pos > g.filled {
        if let Some(gap) = buf.get_mut(g.filled..g.pos) {
            liella::mem::fill(gap, 0);
        }
    }
    if let Some(d) = buf.get_mut(g.pos..end) {
        liella::mem::copy(d, data);
    }
    g.pos = end;
    g.len = g.len.max(end);
    if end > g.filled {
        g.filled = end;
        if let Some(t) = buf.get_mut(end..end + unit) {
            liella::mem::fill(t, 0);
        }
    }
    Ok(data.len())
}

/// Publish a growing buffer through the caller's variables: the buffer, and the size (the
/// position, in units, if it lies before the end of the data, else the data's length).
fn grow_publish<E: Env>(env: &E, g: &GrowStream) {
    let unit = if g.wide { 4 } else { 1 };
    env.store_word(g.ptr_var, g.buf);
    env.store_word(g.size_var, g.pos.min(g.len) / unit);
}

// ------------------------------------------------------------------------------ the window

/// Read the window and the indicators back from the `FILE` words (C code may have moved them).
fn load(file: &[AtomicUsize], ext: &mut Ext) {
    // STDIO-3: every `FILE` slice holds FILE_WORDS words; with that length known the word
    // accesses below need no bounds checks (a shorter slice, never made, keeps the checked way).
    match file.get(..FILE_WORDS) {
        Some(words) => load_words(words, ext),
        None => load_words(file, ext),
    }
}

#[inline(always)]
fn load_words(file: &[AtomicUsize], ext: &mut Ext) {
    let flags = get(file, word::FLAGS);
    ext.state.eof = flags & EOF_SEEN != 0;
    ext.state.err = flags & ERR_SEEN != 0;
    if ext.buf == 0 {
        return;
    }
    let off = |p: usize| p.wrapping_sub(ext.buf);
    match ext.state.role {
        Role::Reading => ext.state.rpos = off(get(file, word::READ_PTR)).min(ext.state.rend),
        Role::Writing => ext.state.wpos = off(get(file, word::WRITE_PTR)).clamp(RESERVE, ext.buf_len),
        Role::Idle => {}
    }
}

/// Write the window and the indicators to the `FILE` words. The published write window is
/// open only on a byte-oriented stream that is writing (D-12.19, the window-closed invariant:
/// on any other stream `_IO_write_ptr == _IO_write_end`), so the front path's one test
/// `ptr < end` also says "set up, byte-oriented, writing, fully buffered".
fn store(file: &[AtomicUsize], ext: &Ext) {
    // STDIO-3: as in [`load`], the known length removes the per-word bounds checks.
    match file.get(..FILE_WORDS) {
        Some(words) => store_words(words, ext),
        None => store_words(file, ext),
    }
}

#[inline(always)]
fn store_words(file: &[AtomicUsize], ext: &Ext) {
    let s = &ext.state;
    let b = ext.buf;
    // STDIO-4: no buffer (before first use, after `setvbuf`, at close): every window word is 0,
    // written without the per-word "is there a buffer" test of the general case below.
    if b == 0 {
        for i in word::READ_PTR..=word::BUF_END {
            set(file, i, 0);
        }
        store_flags(file, s);
        return;
    }
    let at = |o: usize| b + o;
    let (rp, re) = if s.role == Role::Reading { (at(s.rpos), at(s.rend)) } else { (at(RESERVE), at(RESERVE)) };
    let (wp, we) = match s.role {
        Role::Writing if low_int(get(file, word::MODE)) < 0 => (at(s.wpos), at(s.wend)),
        Role::Writing => (at(s.wpos), at(s.wpos)),
        _ => (at(RESERVE), at(RESERVE)),
    };
    set(file, word::READ_PTR, rp);
    set(file, word::READ_END, re);
    set(file, word::READ_BASE, at(RESERVE));
    set(file, word::WRITE_BASE, at(RESERVE));
    set(file, word::WRITE_PTR, wp);
    set(file, word::WRITE_END, we);
    set(file, word::BUF_BASE, at(RESERVE));
    set(file, word::BUF_END, b + ext.buf_len);
    store_flags(file, s);
}

/// The end-of-file and error indicators into `_flags`.
#[inline(always)]
fn store_flags(file: &[AtomicUsize], s: &State) {
    let flags = get(file, word::FLAGS) & !(EOF_SEEN | ERR_SEEN);
    let eof = if s.eof { EOF_SEEN } else { 0 };
    let err = if s.err { ERR_SEEN } else { 0 };
    set(file, word::FLAGS, flags | eof | err);
}

/// [`store`] for an operation that kept the buffer and neither started nor ended in the write
/// role (STDIO-4: a seek, a refill, a read): only the read window and the indicators can differ
/// from what the last [`store`] published. The base, write and buffer words then still hold
/// what that store wrote for this buffer and a non-writing role (every `FILE` word in this
/// module is written by [`store`], except `_IO_read_ptr` by the front paths and the write
/// window by the write paths, which run only in the write role), so they are left alone.
fn store_read(file: &[AtomicUsize], ext: &Ext) {
    match file.get(..FILE_WORDS) {
        Some(words) => store_read_words(words, ext),
        None => store_read_words(file, ext),
    }
}

#[inline(always)]
fn store_read_words(file: &[AtomicUsize], ext: &Ext) {
    let s = &ext.state;
    let b = ext.buf;
    let (rp, re) = if s.role == Role::Reading { (b + s.rpos, b + s.rend) } else { (b + RESERVE, b + RESERVE) };
    set(file, word::READ_PTR, rp);
    set(file, word::READ_END, re);
    store_flags(file, s);
}

/// The data size of the default buffer: the backing's block size for a descriptor.
fn default_size<E: Env>(env: &E, kind: &Kind) -> usize {
    match kind {
        Kind::Fd(fd) => env.sys().fstat(fd_of(*fd)).ok().and_then(|st| usize::try_from(st.blksize).ok()).filter(|&b| b > 0).unwrap_or(BUFSIZ),
        _ => BUFSIZ,
    }
}

/// Allocate the buffer on first use.
fn ensure_buffer<E: Env>(env: &E, ext: &mut Ext) -> KResult<()> {
    if ext.buf != 0 {
        return Ok(());
    }
    if ext.user_buf != 0 && ext.want > RESERVE {
        ext.buf = ext.user_buf;
        ext.buf_len = ext.want;
        ext.buf_owned = false;
        return Ok(());
    }
    let data = if ext.state.policy == Policy::Unbuffered {
        1
    } else if ext.want > 0 {
        ext.want
    } else {
        default_size(env, &ext.kind)
    };
    let len = data + RESERVE;
    let addr = env.malloc(len);
    if addr == 0 {
        // No memory (T22): the stream's own spare buffer, so that diagnostics still go out.
        let (spare, room) = env.spare(core::ptr::from_ref(&*ext).addr());
        if spare == 0 || room <= RESERVE {
            return Err(Errno::ENOMEM);
        }
        ext.buf = spare;
        ext.buf_len = len.min(room);
        ext.buf_owned = false;
        return Ok(());
    }
    ext.buf = addr;
    ext.buf_len = len;
    ext.buf_owned = true;
    Ok(())
}

/// Run `f` on the stream's engine state, buffer and backing, with the window synchronised
/// with the `FILE` words on both sides.
fn run<E: Env, T>(env: &E, file: &[AtomicUsize], ext: &mut Ext, f: impl FnOnce(&mut State, &mut [u8], &mut dyn Backing) -> T) -> KResult<T> {
    if ext.kind == Kind::Closed {
        return Err(Errno::EBADF);
    }
    // STDIO-4: a buffer already in place and no write role on either side: only the read
    // window and the indicators can have changed ([`store_read`]).
    let kept = ext.buf != 0 && ext.state.role != Role::Writing;
    ensure_buffer(env, ext)?;
    load(file, ext);
    let buf = env.bytes(ext.buf, ext.buf_len);
    let mut live = Live { env, kind: &mut ext.kind, pos: &mut ext.pos, pos_known: &mut ext.pos_known };
    let r = f(&mut ext.state, buf, &mut live);
    if kept && ext.state.role != Role::Writing {
        store_read(file, ext);
    } else {
        store(file, ext);
    }
    Ok(r)
}

// ------------------------------------------------------------------------------ open streams

static LIST: RawMutex = RawMutex::new();

/// Set while a thread holds `LIST` and waits for stream locks under it (`flush_all`,
/// `_flushlbf`). Input on a stream holds that stream's lock, so it must not wait for `LIST` then.
static FLUSHING: AtomicBool = AtomicBool::new(false);

/// The calling thread holds the open-streams list lock while `f` runs.
fn with_list<E: Env, T>(env: &E, f: impl FnOnce() -> T) -> T {
    LIST.lock_raw(env.sys(), Scope::Private);
    let r = f();
    LIST.unlock(env.sys(), Scope::Private);
    r
}

/// Add the stream at `fp` to the open-streams list (the head, as the reference library does).
pub fn link<E: Env>(env: &E, fp: usize, file: &[AtomicUsize]) {
    with_list(env, || {
        let head = env.list_head();
        set(file, word::CHAIN, head.load(Acquire));
        head.store(fp, Release);
    });
}

/// Remove the stream at `fp` from the open-streams list.
pub fn unlink<E: Env>(env: &E, fp: usize) {
    with_list(env, || {
        let head = env.list_head();
        let next_of = |a: usize| get(env.file_at(a), word::CHAIN);
        if head.load(Acquire) == fp {
            head.store(next_of(fp), Release);
            return;
        }
        let mut at = head.load(Acquire);
        while at != 0 {
            let next = next_of(at);
            if next == fp {
                set(env.file_at(at), word::CHAIN, next_of(fp));
                return;
            }
            at = next;
        }
    });
}

/// Every open stream's address, with the list lock held.
fn each_stream<E: Env>(env: &E, mut f: impl FnMut(usize)) {
    let mut at = env.list_head().load(Acquire);
    while at != 0 {
        f(at);
        at = get(env.file_at(at), word::CHAIN);
    }
}

/// `fcloseall`: `close_one` is called on the head of the open-streams list until the list is
/// empty; it must remove the stream it is given (as `fclose` does). The list lock is not held
/// during the call.
pub fn close_every<E: Env>(env: &E, close_one: &mut dyn FnMut(usize)) {
    loop {
        let head = with_list(env, || env.list_head().load(Acquire));
        if head == 0 {
            return;
        }
        close_one(head);
    }
}

/// The lock and record of the stream whose words are `file` (`None`: not set up, or not a
/// record the library made for this `FILE`, [`Env::record`]).
#[inline(always)]
pub fn parts<E: Env>(env: &E, file: &[AtomicUsize]) -> Option<(&'static StreamLock, usize)> {
    let ext = get(file, word::EXT);
    env.record(file.as_ptr().addr(), ext).then(|| (env.lock_of(ext), ext))
}

/// The `_codecvt` word (the record's address, unchecked: see [`Env::record`]).
#[inline(always)]
pub fn ext_word(file: &[AtomicUsize]) -> usize {
    get(file, word::EXT)
}

/// Flush every stream with pending output (`fflush(NULL)`, `exit`): 0, or the last error.
pub fn flush_all<E: Env>(env: &E) -> KResult<()> {
    let mut result = Ok(());
    with_list(env, || {
        FLUSHING.store(true, Release);
        each_stream(env, |fp| {
            let file = env.file_at(fp);
            if let Some((lock, ext)) = parts(env, file) {
                lock.lock(env.sys(), env.tid());
                let ext = env.ext_at(ext);
                if ext.state.role == Role::Writing || matches!(ext.kind, Kind::Grow(_)) {
                    if let Err(e) = flush(env, file, ext) {
                        result = Err(e);
                    }
                }
                lock.unlock(env.sys(), env.tid());
            }
        });
        FLUSHING.store(false, Release);
    });
    result
}

/// Flush the line-buffered streams with pending output (`_flushlbf`; ISO C: before input
/// from a line-buffered or unbuffered stream). With `try_only` (input, which holds its own
/// stream's lock), a stream's lock is never waited for while another is held: streams busy in
/// another thread are skipped, and so is the whole flush while another thread is flushing every
/// stream (it may be waiting for the caller's stream). A list held only to open or close a stream
/// is waited for.
pub fn flush_line_buffered<E: Env>(env: &E, try_only: bool) {
    let me = env.tid();
    if try_only {
        while !LIST.try_lock() {
            if FLUSHING.load(Acquire) {
                return;
            }
            env.sys().sched_yield();
        }
    } else {
        LIST.lock_raw(env.sys(), Scope::Private);
        FLUSHING.store(true, Release);
    }
    each_stream(env, |fp| {
        let file = env.file_at(fp);
        if let Some((lock, ext)) = parts(env, file) {
            let got = if try_only {
                lock.try_lock(me)
            } else {
                lock.lock(env.sys(), me);
                true
            };
            if got {
                let ext = env.ext_at(ext);
                if ext.state.role == Role::Writing && ext.state.policy == Policy::Line {
                    let _ = run(env, file, ext, |s, b, back| s.flush(b, back));
                }
                lock.unlock(env.sys(), me);
            }
        }
    });
    if !try_only {
        FLUSHING.store(false, Release);
    }
    LIST.unlock(env.sys(), Scope::Private);
}

/// Take (`true`) or release the open-streams list lock around `fork`, so that the child sees a
/// consistent list (D-18.6).
pub fn hold_list<S: Sys>(sys: &S, take: bool) {
    if take {
        LIST.lock_raw(sys, Scope::Private);
    } else {
        LIST.unlock(sys, Scope::Private);
    }
}

/// `_IO_list_resetlock`: the open-streams list lock becomes free, whoever held it.
pub fn reset_list_lock() {
    LIST.reset();
}

/// After `fork`, in the child: every stream lock and the list lock are free (D-18.6).
pub fn reset_locks_in_child<E: Env>(env: &E) {
    LIST.reset();
    each_stream(env, |fp| {
        if let Some((lock, _)) = parts(env, env.file_at(fp)) {
            lock.reset_in_child(env.tid());
        }
    });
}

// ------------------------------------------------------------------------------ opening

/// A parsed `fopen` mode (D-12.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mode {
    /// The stream reads.
    pub read: bool,
    /// The stream writes.
    pub write: bool,
    /// Writes append (`a`).
    pub append: bool,
    /// Create the file (`w`, `a`).
    pub create: bool,
    /// Truncate it (`w`).
    pub truncate: bool,
    /// `x`: fail if it exists.
    pub exclusive: bool,
    /// `e`: close on exec.
    pub cloexec: bool,
    /// The first letter (`r`, `w` or `a`).
    pub first: u8,
}

/// Parse a mode: `r`, `w` or `a`, then any of `+ b c e m x` (`c`: no cancellation points,
/// which stdio has none of; `m`: read through a mapping, served by reading), up to a comma
/// (`,ccs=` is handled by the caller). `EINVAL` for anything else first.
pub fn parse_mode(mode: &[u8]) -> KResult<Mode> {
    let mut m = Mode { first: *mode.first().ok_or(Errno::EINVAL)?, ..Mode::default() };
    match m.first {
        b'r' => m.read = true,
        b'w' => {
            m.write = true;
            m.create = true;
            m.truncate = true;
        }
        b'a' => {
            m.write = true;
            m.create = true;
            m.append = true;
        }
        _ => return Err(Errno::EINVAL),
    }
    for &c in mode.iter().skip(1).take_while(|&&c| c != b',') {
        match c {
            b'+' => {
                m.read = true;
                m.write = true;
            }
            b'x' => m.exclusive = true,
            b'e' => m.cloexec = true,
            _ => {}
        }
    }
    Ok(m)
}

/// The `open` flags of a mode.
pub fn open_flags(m: &Mode) -> u32 {
    use liella_kernel::uapi::{O_APPEND, O_CLOEXEC, O_CREAT, O_EXCL, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY};
    let mut f = match (m.read, m.write) {
        (true, true) => O_RDWR,
        (false, true) => O_WRONLY,
        _ => O_RDONLY,
    };
    if m.create {
        f |= O_CREAT;
    }
    if m.truncate {
        f |= O_TRUNC;
    }
    if m.append {
        f |= O_APPEND;
    }
    if m.exclusive {
        f |= O_EXCL;
    }
    if m.cloexec {
        f |= O_CLOEXEC;
    }
    f
}

/// Set up a stream on `kind` in a fresh record and `FILE` (fully buffered; a descriptor that
/// is a terminal is line-buffered).
pub fn setup<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, kind: Kind, m: &Mode, fd: i32) {
    let policy = match kind {
        Kind::Fd(f) if env.sys().tcgets(fd_of(f)).is_ok() => Policy::Line,
        _ => Policy::Full,
    };
    *ext = Ext { state: State::new(m.read, m.write, m.append, policy), kind, open: true, printf_plans: ext.printf_plans, ..Ext::default() };
    set(file, word::FLAGS, 0);
    set(file, word::FILENO, usize::try_from(u32::from_ne_bytes(fd.to_ne_bytes())).unwrap_or(0));
    set(file, word::MODE, 0);
    store(file, ext);
}

/// Set up a standard stream (`fd` 0, 1, 2) at first use: `stdin` reads, the others write;
/// `stderr` is unbuffered.
pub fn setup_standard<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, fd: i32) {
    let m = if fd == 0 { Mode { read: true, first: b'r', ..Mode::default() } } else { Mode { write: true, first: b'w', ..Mode::default() } };
    let flags = get(file, word::FLAGS);
    setup(env, file, ext, Kind::Fd(fd), &m, fd);
    // A program may have set the indicators or the locking mode before first use.
    set(file, word::FLAGS, flags);
    load(file, ext);
    if fd == 2 {
        ext.state.policy = Policy::Unbuffered;
    }
}

// ------------------------------------------------------------------------------ closing

/// `fclose` after the stream left the open-streams list: flush, release the backing and the
/// buffer. The stream is gone either way; the first error is reported.
pub fn close<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext) -> KResult<()> {
    let flushed = if ext.kind == Kind::Closed { Ok(()) } else { flush(env, file, ext) };
    let closed = match ext.kind {
        Kind::Closed => Ok(()),
        Kind::Fd(fd) => env.sys().close(fd_of(fd)),
        Kind::Mem(m) => {
            if m.owned {
                env.free(m.addr);
            }
            Ok(())
        }
        Kind::Grow(g) => {
            // Closing terminates the data at the published size (a flush does not), as the
            // reference library does.
            let unit = if g.wide { 4 } else { 1 };
            let at = g.pos.min(g.len);
            if let Some(t) = env.bytes(g.buf, g.cap).get_mut(at..at + unit) {
                liella::mem::fill(t, 0);
            }
            Ok(())
        }
        Kind::Cookie(c) => {
            if c.close != 0 && env.call3(c.close, c.cookie, 0, 0) != 0 {
                Err(Errno::EIO)
            } else {
                Ok(())
            }
        }
    };
    if ext.buf_owned && ext.buf != 0 {
        env.free(ext.buf);
    }
    *ext = Ext { printf_plans: ext.printf_plans, ..Ext::default() };
    store(file, ext);
    set(file, word::FILENO, usize::try_from(u32::from_ne_bytes((-1i32).to_ne_bytes())).unwrap_or(0));
    flushed.and(closed)
}

// ------------------------------------------------------------------------------ operations

/// A byte operation makes an undecided stream byte-oriented (§12.1). On a wide-oriented stream
/// it fails (D-12.14): `false`.
fn byte_oriented(file: &[AtomicUsize]) -> bool {
    let Some(w) = file.get(word::MODE) else { return true };
    let v = w.load(Relaxed);
    match low_int(v) {
        0 => {
            w.store(with_low_int(v, -1), Relaxed);
            true
        }
        m => m < 0,
    }
}

/// A wide operation makes an undecided stream wide-oriented, converting with `enc` (the
/// calling thread's locale encoding) from then on. On a byte-oriented stream it fails
/// (D-12.14): `false`.
pub fn wide_oriented(file: &[AtomicUsize], ext: &mut Ext, enc: Encoding) -> bool {
    let Some(w) = file.get(word::MODE) else { return false };
    let v = w.load(Relaxed);
    match low_int(v) {
        0 => w.store(with_low_int(v, 1), Relaxed),
        m if m < 0 => return false,
        _ => {}
    }
    if ext.wide.codec == Codec::Unset {
        ext.wide.codec = Codec::Text(enc);
    }
    true
}

/// Make a fresh stream wide-oriented on `codec` (`open_wmemstream`, `fopen`'s `,ccs=`).
pub fn set_wide(file: &[AtomicUsize], ext: &mut Ext, codec: Codec) {
    set(file, word::MODE, with_low_int(get(file, word::MODE), 1));
    ext.wide.codec = codec;
}

/// The charset of a mode's `,ccs=` (as written; `None` without one).
pub fn mode_charset(mode: &[u8]) -> Option<&[u8]> {
    let at = mode.iter().position(|&c| c == b',')?;
    let rest = mode.get(at + 1..)?;
    let rest = rest.strip_prefix(b" ").unwrap_or(rest);
    rest.strip_prefix(b"ccs=")
}

/// Whether reading from this stream first flushes the line-buffered streams (ISO C).
fn input_flushes(ext: &Ext) -> bool {
    ext.state.policy != Policy::Full && (ext.state.role != Role::Reading || ext.state.rpos >= ext.state.rend)
}

/// `fgetc` and `__uflow`: the next byte, `None` at end of input (and on a wide-oriented
/// stream, D-12.14).
/// STDIO-6: the engine only; the refill path ([`refill`]) is the caller's to try first, before
/// its locking plumbing, so a first read (no buffer) pays nothing for it here.
pub fn getc<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext) -> KResult<Option<u8>> {
    if !byte_oriented(file) {
        return Ok(None);
    }
    getc_raw(env, file, ext)
}

/// The refill path of [`getc`] (STDIO-5): a byte-oriented, fully buffered, readable descriptor
/// stream with its buffer in place, not writing, and an empty read window gets one `read(2)`
/// straight into the buffer, the read window and indicators published as [`run`] would (its
/// [`store_read`] case: the buffer kept, no write role before or after), and the first byte.
/// Exactly what the engine does for that stream ([`State::getc`]: enter reading, fill, take one
/// byte), minus the engine's plumbing (buffer check, load, dyn backing, closure). `None`: not
/// such a stream; nothing was touched and the caller takes the engine's way ([`getc`]).
/// `ext` is a record [`Env::record`] accepted for this `FILE`; the caller holds the lock or
/// needs none (as for [`get_front`]).
#[inline(always)]
pub fn refill<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext) -> Option<KResult<Option<u8>>> {
    let words = file.get(..FILE_WORDS)?;
    let Kind::Fd(fd) = ext.kind else { return None };
    let (b, len) = (ext.buf, ext.buf_len);
    let s = &mut ext.state;
    if b == 0 || s.policy != Policy::Full || !s.readable || s.role == Role::Writing || low_int(get(words, word::MODE)) >= 0 {
        return None;
    }
    // As [`load`]: bytes left in the window (C code may have moved `_IO_read_ptr`) are the
    // engine's to hand out.
    if s.role == Role::Reading && get(words, word::READ_PTR).wrapping_sub(b) < s.rend {
        return None;
    }
    let flags = get(words, word::FLAGS);
    s.eof = flags & EOF_SEEN != 0;
    s.err = flags & ERR_SEEN != 0;
    s.role = Role::Reading;
    s.rpos = RESERVE;
    s.rend = RESERVE;
    let buf = env.bytes(b, len);
    let r = env.sys().read(fd_of(fd), buf.get_mut(RESERVE..).unwrap_or(&mut []));
    // As [`Live`] and [`State::fill`] would: track the offset, note where the window ends.
    pos_after_read(&mut ext.pos, &mut ext.pos_known, &r);
    s.mark_window(if ext.pos_known { Some(ext.pos) } else { None }, &r, len);
    let r = match r {
        Ok(0) => {
            s.eof = true;
            Ok(None)
        }
        Ok(n) => {
            s.rend = RESERVE + n.min(len.saturating_sub(RESERVE));
            match buf.get(RESERVE) {
                Some(&c) => {
                    s.rpos = RESERVE + 1;
                    // The indicators did not change: only the two window words go out.
                    set(words, word::READ_PTR, b + s.rpos);
                    set(words, word::READ_END, b + s.rend);
                    return Some(Ok(Some(c)));
                }
                None => Err(Errno::EINVAL),
            }
        }
        Err(e) => {
            s.err = true;
            Err(e)
        }
    };
    store_read_words(words, ext);
    Some(r)
}

/// The next byte, whatever the orientation (the wide layer's input).
fn getc_raw<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext) -> KResult<Option<u8>> {
    // STDIO-4: a fully buffered stream never flushes the others first ([`input_flushes`]), so
    // only the other policies need the window before [`run`] loads it.
    if ext.state.policy != Policy::Full {
        load(file, ext);
        if input_flushes(ext) {
            flush_line_buffered(env, true);
        }
    }
    run(env, file, ext, |s, b, back| s.getc(b, back))?
}

/// `fread`: bytes into `out`.
pub fn read<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, out: &mut [u8]) -> Transfer {
    if !byte_oriented(file) {
        return Transfer { done: 0, error: Some(Errno::EINVAL) };
    }
    load(file, ext);
    let was_reading = ext.state.role == Role::Reading;
    match read_direct(env, ext, out) {
        Direct::Done(t) => {
            // FREAD-DIRECT: a whole direct transfer on a stream already in the read role changed
            // nothing `store_read` publishes (the window stays empty, no indicator was set: an
            // end of input or an error stops short of `out.len()`), so the store is skipped.
            if !(was_reading && t.done == out.len()) {
                store_read(file, ext);
            }
            t
        }
        d => read_general(env, file, ext, out, d),
    }
}

/// [`read`] past its direct path (out of line, so that the direct path's caller needs no frame
/// for the engine's set-up): the engine reads the rest.
#[inline(never)]
fn read_general<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, out: &mut [u8], d: Direct) -> Transfer {
    let done = match d {
        Direct::Done(t) => return t,
        Direct::Part(n) => {
            store_read(file, ext);
            n
        }
        Direct::No => 0,
    };
    let Some(rest) = out.get_mut(done..) else { return Transfer { done, error: None } };
    if input_flushes(ext) {
        flush_line_buffered(env, true);
    }
    let t = run(env, file, ext, |s, b, back| s.read(b, back, rest)).unwrap_or_else(|e| Transfer { done: 0, error: Some(e) });
    Transfer { done: done + t.done, error: t.error }
}

/// What [`read_direct`] did.
enum Direct {
    /// It does not apply: nothing was read and nothing changed.
    No,
    /// These first bytes were read; what is left is shorter than the buffer, and the engine
    /// ([`State::read`]) takes over, exactly where its own loop would.
    Part(usize),
    /// The whole transfer.
    Done(Transfer),
}

/// The `fread` direct path (FREAD-BIG): a request at least as large as the buffer on a fully
/// buffered stream that is already set up, with nothing read ahead, goes from the backing
/// straight into `out` without the engine's set-up (`ensure_buffer`, a second [`load`], the
/// window store). It makes the backing calls, and leaves the state, that [`State::read`] would:
/// the same direct reads while the rest is at least as large as the buffer, end of input and
/// errors as its loop sets them (the indicators; a read error is returned with the bytes moved
/// before it), a stream that was idle left in the read role with an empty window. Only for a
/// fully buffered stream, so that no line-buffered flush ([`input_flushes`]) is due. `ext` was
/// just [`load`]ed; the caller publishes the state ([`store_read`]: no write role on either side).
#[inline(always)]
fn read_direct<E: Env>(env: &E, ext: &mut Ext, out: &mut [u8]) -> Direct {
    let cap = ext.buf_len.saturating_sub(RESERVE);
    let s = &mut ext.state;
    if ext.buf == 0 || cap == 0 || out.len() < cap || s.policy != Policy::Full || !s.readable || ext.kind == Kind::Closed {
        return Direct::No;
    }
    match s.role {
        Role::Reading if s.rpos >= s.rend => {}
        Role::Idle => {
            s.role = Role::Reading;
            s.rpos = RESERVE;
            s.rend = RESERVE;
        }
        _ => return Direct::No,
    }
    // The backing moves past the (empty) window without it: a later seek must not count from the
    // window's end (D-12.22, [`State::win_known`]).
    s.win_known = false;
    // FREAD-DIRECT: a descriptor backing is read here without the `Backing` dispatch; the first
    // read takes the whole request, the loop runs only after a short read. The same reads,
    // indicators, results and position tracking (D-12.22) as the general loop below.
    if let Kind::Fd(fd) = ext.kind {
        let mut done = 0usize;
        while let Some(rest) = out.get_mut(done..) {
            let r = env.sys().read(fd_of(fd), rest);
            pos_after_read(&mut ext.pos, &mut ext.pos_known, &r);
            match r {
                Ok(0) => {
                    s.eof = true;
                    return Direct::Done(Transfer { done, error: None });
                }
                Ok(n) => done += n,
                Err(e) => {
                    s.err = true;
                    return Direct::Done(Transfer { done, error: Some(e) });
                }
            }
            if out.len().saturating_sub(done) < cap {
                break;
            }
        }
        return if done == out.len() { Direct::Done(Transfer { done, error: None }) } else { Direct::Part(done) };
    }
    // D-12.22 (merged with stdio-seekwin): direct reads keep the descriptor position current like the engine's.
    let mut live = Live { env, kind: &mut ext.kind, pos: &mut ext.pos, pos_known: &mut ext.pos_known };
    let mut done = 0usize;
    while let Some(rest) = out.get_mut(done..).filter(|r| r.len() >= cap) {
        match live.read(rest) {
            Ok(0) => {
                s.eof = true;
                return Direct::Done(Transfer { done, error: None });
            }
            Ok(n) => done += n,
            Err(e) => {
                s.err = true;
                return Direct::Done(Transfer { done, error: Some(e) });
            }
        }
    }
    if done == out.len() { Direct::Done(Transfer { done, error: None }) } else { Direct::Part(done) }
}

/// `fwrite`, `fputs`: bytes from `data`.
pub fn write<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, data: &[u8]) -> Transfer {
    if !byte_oriented(file) {
        return Transfer { done: 0, error: Some(Errno::EINVAL) };
    }
    write_raw(env, file, ext, data)
}

/// Bytes from `data`, whatever the orientation (the wide layer's output).
fn write_raw<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, data: &[u8]) -> Transfer {
    if window_put(env, file, ext, data) {
        return Transfer { done: data.len(), error: None };
    }
    if let Some(t) = write_direct(env, file, ext, data) {
        return t;
    }
    run(env, file, ext, |s, b, back| s.write(b, back, data)).unwrap_or_else(|e| Transfer { done: 0, error: Some(e) })
}

/// The output direct path (FPUTS-NBF): an unbuffered stream, or a line-buffered one given a text
/// shorter than its buffer that ends in a newline, already writing with nothing pending, on a
/// descriptor, writes `data` with `write(2)` straight from the caller's bytes, without the
/// engine's set-up ([`load`], [`State::write`]'s drain, the window [`store`]). The same `write`
/// calls, results, state and `FILE` words as [`State::write`] (stderr pays this per line):
/// - unbuffered (and a line-buffered text at least as large as the buffer, which the engine
///   writes through the same way): the bytes written before an error are the count;
/// - line-buffered: the engine copies `data` into the empty buffer and writes it through its last
///   newline (the last byte), so the same bytes in the same calls; an error drops them and
///   reports all of them accepted, as the engine's buffered copy does.
///
/// After an error the stream is idle with its error indicator set, else writing with an empty
/// window: the window's `FILE` words are the same closed window at the reserve either way, so
/// only an error writes the state back. `None`: it does not apply, nothing changed.
#[inline(always)]
fn write_direct<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, data: &[u8]) -> Option<Transfer> {
    let Kind::Fd(fd) = ext.kind else { return None };
    let s = &ext.state;
    let line = match s.policy {
        Policy::Unbuffered => false,
        Policy::Line if data.last() == Some(&b'\n') => true,
        _ => return None,
    };
    // Writing with nothing pending, and the published write pointer at the reserve (what
    // [`load`] would read back): no byte waits in the buffer.
    if ext.buf == 0 || s.role != Role::Writing || s.wpos != RESERVE || s.wend != RESERVE || get(file, word::WRITE_PTR) != ext.buf.wrapping_add(RESERVE) {
        return None;
    }
    let line = line && data.len() < ext.buf_len.saturating_sub(RESERVE);
    ext.pos_known = false;
    let mut done = 0usize;
    while let Some(rest) = data.get(done..).filter(|r| !r.is_empty()) {
        let e = match env.sys().write(fd_of(fd), rest) {
            Ok(n) if n > 0 => {
                done += n;
                continue;
            }
            Ok(_) => Errno::EIO,
            Err(e) => e,
        };
        load(file, ext);
        let s = &mut ext.state;
        (s.role, s.rpos, s.rend, s.wpos, s.wend, s.win_known, s.err) = (Role::Idle, RESERVE, RESERVE, RESERVE, RESERVE, false, true);
        store(file, ext);
        return Some(Transfer { done: if line { data.len() } else { done }, error: Some(e) });
    }
    ext.state.win_known = false;
    Some(Transfer { done, error: None })
}

/// The fast path of byte output inside the operations, whatever the orientation (the wide
/// layer's bytes too): when `data` fits in the engine's write window, copy it there and advance
/// `_IO_write_ptr`, exactly as header-inline `putc` does (D-12.11); the next operation's `load`
/// sees the bytes. The window is the engine's (`wend`), not the published one, which a
/// wide-oriented stream keeps closed ([`store`]; there both words move together). The window is
/// open only on a fully buffered stream that is writing (a line-buffered or unbuffered one keeps
/// it empty), so the policy's flushes are unchanged. Whether it applied.
#[inline(always)]
fn window_put<E: Env>(env: &E, file: &[AtomicUsize], ext: &Ext, data: &[u8]) -> bool {
    if ext.buf == 0 || ext.state.role != Role::Writing {
        return false;
    }
    let (p, shut) = (get(file, word::WRITE_PTR), low_int(get(file, word::MODE)) >= 0);
    let e = ext.buf.saturating_add(ext.state.wend.min(ext.buf_len));
    if p < ext.buf || e.saturating_sub(p) < data.len() {
        return false;
    }
    let at = p - ext.buf;
    match env.bytes(ext.buf, ext.buf_len).get_mut(at..at + data.len()) {
        Some(dst) => {
            liella::mem::copy(dst, data);
            set(file, word::WRITE_PTR, p + data.len());
            if shut {
                set(file, word::WRITE_END, p + data.len());
            }
            true
        }
        None => false,
    }
}

/// The engine's write window held across one operation that writes many pieces (`vfprintf`;
/// STDIO-HOT, D-12.19): the conditions and the bounds are exactly [`window_put`]'s, checked once
/// when it is opened, and `_IO_write_ptr` is published once ([`Window::publish`]) instead of per
/// piece. Offsets into the stream's buffer: `at` is where the next byte goes, `end` the window's
/// end (`at == end`: closed). Valid only while the caller holds the stream's lock (or needs
/// none) and makes no other call on the stream: before any other operation on it, publish,
/// and open a new window after.
pub struct Window {
    buf: usize,
    len: usize,
    at: usize,
    end: usize,
}

impl Window {
    /// The open window of a byte-oriented, fully buffered stream that is writing, else a
    /// closed one (the conditions of [`window_put`]; an undecided stream stays closed until an
    /// operation orients it).
    #[inline]
    pub fn open(file: &[AtomicUsize], ext: &Ext) -> Window {
        let closed = Window::closed();
        if ext.buf == 0 || ext.state.role != Role::Writing || low_int(get(file, word::MODE)) >= 0 {
            return closed;
        }
        let p = get(file, word::WRITE_PTR);
        let end = ext.state.wend.min(ext.buf_len);
        match p.checked_sub(ext.buf) {
            Some(at) if at <= end => Window { buf: ext.buf, len: ext.buf_len, at, end },
            _ => closed,
        }
    }

    /// A closed window (every piece goes the general way).
    #[inline]
    pub const fn closed() -> Window {
        Window { buf: 0, len: 0, at: 0, end: 0 }
    }

    /// Copy `data` into the window when it fits; whether it did.
    #[inline(always)]
    pub fn put<E: Env>(&mut self, env: &E, data: &[u8]) -> bool {
        if self.end - self.at < data.len() {
            return false;
        }
        let to = self.at + data.len();
        match env.bytes(self.buf, self.len).get_mut(self.at..to) {
            Some(dst) => {
                liella::mem::copy(dst, data);
                self.at = to;
                true
            }
            None => false,
        }
    }

    /// Publish the bytes put so far: `_IO_write_ptr` moves to the window's position (nothing
    /// for a closed window).
    #[inline]
    pub fn publish(&self, file: &[AtomicUsize]) {
        if self.buf != 0 {
            set(file, word::WRITE_PTR, self.buf + self.at);
        }
    }
}

/// The front path of byte output (P5, P5b; D-12.19): when `data` fits in the published write
/// window, copy it there and advance `_IO_write_ptr`; whether it applied. By the window-closed
/// invariant ([`store`]) an open window means a set-up, byte-oriented, fully buffered stream
/// that is writing, so no other state is consulted; the one bound is that the bytes land inside
/// the stream's own buffer (`ptr - buf < len`), whatever the window words say. Anything else (a
/// closed or full window) is left to [`putc`] and [`write`], so the answers and side effects
/// stay theirs. `ext` is a record [`Env::record`] accepted for this `FILE`; the caller holds the
/// stream's lock or needs none, and `p` is `_IO_write_ptr` as [`window_room`] read it then.
#[inline(always)]
pub fn put_front<E: Env>(env: &E, file: &[AtomicUsize], ext: &Ext, data: &[u8], p: usize) -> bool {
    if ext.buf == 0 {
        return false;
    }
    let at = p.wrapping_sub(ext.buf);
    match env.bytes(ext.buf, ext.buf_len).get_mut(at..).and_then(|s| s.get_mut(..data.len())) {
        Some(dst) => {
            front_copy(dst, data);
            set(file, word::WRITE_PTR, p + data.len());
            true
        }
        None => false,
    }
}

/// [`put_front`] for 1 to [`FRONT_INLINE`] bytes only, with no call in it (STDIO-LOCKED-PATH):
/// false, with nothing done, for any other length. The locked leaves bound their data before
/// they take the lock, but the compiler did not carry that bound into [`front_copy`], whose
/// `memcpy` call for longer data then gave the `fputs` locked leaf a stack frame.
#[inline(always)]
pub fn put_front_short<E: Env>(env: &E, file: &[AtomicUsize], ext: &Ext, data: &[u8], p: usize) -> bool {
    let n = data.len();
    if ext.buf == 0 || n.wrapping_sub(1) >= FRONT_INLINE {
        return false;
    }
    let at = p.wrapping_sub(ext.buf);
    match env.bytes(ext.buf, ext.buf_len).get_mut(at..).and_then(|s| s.get_mut(..n)) {
        Some(dst) => {
            short_copy(dst, data);
            set(file, word::WRITE_PTR, p + n);
            true
        }
        None => false,
    }
}

/// [`put_front_short`]'s copy (`dst` and `data` of one length, 1 to [`FRONT_INLINE`] bytes):
/// as [`front_copy`], the bound taken as given, so neither branch has a call.
#[inline(always)]
fn short_copy(dst: &mut [u8], data: &[u8]) {
    const HALF: usize = FRONT_INLINE / 2;
    let n = data.len();
    if n > HALF {
        for at in [0, n.wrapping_sub(HALF)] {
            let end = at.wrapping_add(HALF);
            if let (Some(a), Some(b)) = (dst.get_mut(at..end), data.get(at..end)) {
                if let (Ok(a), Ok(b)) = (<&mut [u8; HALF]>::try_from(a), <&[u8; HALF]>::try_from(b)) {
                    *a = *b;
                }
            }
        }
    } else {
        liella::mem::copy(dst, data);
    }
}

/// The bytes a front path can copy without a call: [`front_copy`]'s two overlapping 32-byte
/// pieces (FWRITE-FRONT).
pub const FRONT_INLINE: usize = 64;

/// [`put_front`]'s copy (`dst` and `data` of one length): 33 to [`FRONT_INLINE`] bytes as two
/// overlapping 32-byte pieces, plain loads and stores; anything else as [`liella::mem::copy`]
/// (inline up to 32 bytes, a call beyond). FWRITE-FRONT: a 33-64 byte line (sort's, uniq's)
/// was a `memcpy` call that also gave the whole `fwrite` export a frame.
#[inline(always)]
fn front_copy(dst: &mut [u8], data: &[u8]) {
    const HALF: usize = FRONT_INLINE / 2;
    #[inline(always)]
    fn piece(d: &mut [u8], s: &[u8], at: usize) {
        let end = at.wrapping_add(HALF);
        if let (Some(a), Some(b)) = (d.get_mut(at..end), s.get(at..end)) {
            if let (Ok(a), Ok(b)) = (<&mut [u8; HALF]>::try_from(a), <&[u8; HALF]>::try_from(b)) {
                *a = *b;
            }
        }
    }
    let n = data.len();
    if n > HALF && n <= FRONT_INLINE && dst.len() == n {
        piece(dst, data, 0);
        piece(dst, data, n - HALF);
    } else {
        liella::mem::copy(dst, data);
    }
}

/// `_IO_write_ptr` when the published write window has room for `n` bytes (the front path's
/// first test, before the record is looked at; D-12.19).
#[inline(always)]
pub fn window_room(file: &[AtomicUsize], n: usize) -> Option<usize> {
    let (p, e) = (get(file, word::WRITE_PTR), get(file, word::WRITE_END));
    (p < e && e - p >= n).then_some(p)
}

/// `_IO_read_ptr` when the published read window holds a byte (the read front path's first
/// test, before the record is looked at; STDIO-2, D-12.19).
#[inline(always)]
pub fn read_room(file: &[AtomicUsize]) -> Option<usize> {
    let (p, e) = (get(file, word::READ_PTR), get(file, word::READ_END));
    (p < e).then_some(p)
}

/// Whether the published `_IO_buf_base` names a buffer (STDIO-6): a hint only, so that a stream
/// without one (a first read) skips the refill path's checks; [`refill`] re-checks the record.
#[inline(always)]
pub fn buf_hint(file: &[AtomicUsize]) -> bool {
    get(file, word::BUF_BASE) != 0
}

/// The read front path (STDIO-2, D-12.19): on a byte-oriented stream whose published read
/// window holds a byte, take it and advance `_IO_read_ptr`; `None` when it does not apply and
/// [`getc`] must decide. The byte is the one [`getc`] would return: the window is the engine's
/// (`store` publishes `rpos..rend`; pushed-back bytes lie inside it, `ungetc` moves `rpos`
/// back), a byte in the window never flushes the line-buffered streams (`input_flushes`) and
/// changes no indicator. Bounds: the byte lies before the engine's own end of data and inside
/// the stream's buffer, whatever the window words say. No read window is open on a wide stream
/// by this test alone, so the orientation is checked (`_mode < 0`). `ext` is a record
/// [`Env::record`] accepted for this `FILE`; the caller holds the lock or needs none; `p` is
/// `_IO_read_ptr` as [`read_room`] read it then.
#[inline(always)]
pub fn get_front<E: Env>(env: &E, file: &[AtomicUsize], ext: &Ext, p: usize) -> Option<u8> {
    if low_int(get(file, word::MODE)) >= 0 || ext.buf == 0 {
        return None;
    }
    let at = p.wrapping_sub(ext.buf);
    if at >= ext.state.rend {
        return None;
    }
    let b = *env.bytes(ext.buf, ext.buf_len).get(at)?;
    set(file, word::READ_PTR, p + 1);
    Some(b)
}

/// `fputc` and `__overflow`.
pub fn putc<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, c: u8) -> KResult<()> {
    if !byte_oriented(file) {
        return Err(Errno::EINVAL);
    }
    if window_put(env, file, ext, &[c]) {
        return Ok(());
    }
    run(env, file, ext, |s, b, back| s.putc(b, back, c))?
}

/// `ungetc`.
pub fn ungetc<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, c: u8) -> KResult<()> {
    if !byte_oriented(file) {
        return Err(Errno::EINVAL);
    }
    ungetc_raw(env, file, ext, c)
}

fn ungetc_raw<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, c: u8) -> KResult<()> {
    run(env, file, ext, |s, b, back| s.ungetc(b, back, c))?
}

/// `_IO_sungetc` (§12.13, IO-VTABLES): give back the last byte read, which the next read
/// returns again, as `ungetc` of that byte. Only a byte still in the current read window (from
/// `RESERVE` up to the read position) counts as read; anything else (nothing read yet, a
/// writing or idle stream, a wide stream, no buffer) is `EINVAL` and changes nothing. Liella
/// never steps the read position below the window.
pub fn unget_last<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext) -> KResult<u8> {
    if !byte_oriented(file) || ext.buf == 0 {
        return Err(Errno::EINVAL);
    }
    load(file, ext);
    if !matches!(ext.state.role, Role::Reading) || ext.state.rpos <= RESERVE {
        return Err(Errno::EINVAL);
    }
    let at = ext.state.rpos.checked_sub(1).ok_or(Errno::EINVAL)?;
    let b = *env.bytes(ext.buf, ext.buf_len).get(at).ok_or(Errno::EINVAL)?;
    ungetc_raw(env, file, ext, b).map(|()| b)
}

/// `fgets`: at most `out.len() - 1` bytes, stopping after a newline, then a NUL. The bytes
/// stored before the NUL; `None` if end of input or an error came before any byte.
pub fn gets_into<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, out: &mut [u8]) -> KResult<Option<usize>> {
    let room = out.len().saturating_sub(1);
    let mut n = 0;
    // STDIO-FGETS: the line is scanned and copied from the window in pieces, one engine run per
    // piece, not per byte. A stream that is not fully buffered reads one window per run, so the
    // line-buffered streams are flushed before every read of the backing, as `getc` does.
    if room > 0 && byte_oriented(file) {
        let once = ext.state.policy != Policy::Full;
        while n < room {
            load(file, ext);
            if input_flushes(ext) {
                flush_line_buffered(env, true);
            }
            let dst = out.get_mut(n..room).unwrap_or(&mut []);
            match run(env, file, ext, |s, b, back| s.read_line_into(b, back, b'\n', &|w, c| env.find(w, c), dst, once)).and_then(|r| r) {
                Ok((k, done)) => {
                    n += k;
                    if done {
                        break;
                    }
                }
                Err(e) => {
                    if n == 0 {
                        return Err(e);
                    }
                    break;
                }
            }
        }
    }
    if let Some(t) = out.get_mut(n) {
        *t = 0;
    }
    Ok((n > 0 || room == 0).then_some(n))
}

/// A record up to and including `delim`, appended to `out` (the library's own line readers:
/// the mount tables, §13.13): the bytes read, 0 at end of input. A read error sets the error
/// indicator, as `getdelim` does.
pub fn read_record<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, delim: u8, out: &mut liella::mem::TryVec<u8>) -> KResult<usize> {
    if !byte_oriented(file) {
        return Ok(0);
    }
    load(file, ext);
    if input_flushes(ext) {
        flush_line_buffered(env, true);
    }
    match run(env, file, ext, |s, b, back| s.read_until_with(b, back, delim, &|w, c| env.find(w, c), &mut |piece| out.extend_from_slice(piece)))? {
        Ok(n) => Ok(n),
        Err(e) => {
            ext.state.err = true;
            store(file, ext);
            Err(e)
        }
    }
}

/// The `getdelim` front path (GETDELIM, D-12.19): the record lies whole inside the published
/// read window and fits the caller's buffer as it is. The same window and seal invariants as
/// [`get_front`] (byte-oriented, a byte of the engine's data before `_IO_read_end`, inside the
/// stream's buffer, whatever the window words say); [`Env::find`] scans for `delim` in the
/// window, in one pass with the copy ([`Env::find_copy`]). The record's bytes and a NUL go to
/// `*lineptr` and `_IO_read_ptr` moves past the delimiter: nothing else changes, as with
/// [`getc`] (an in-window byte never flushes a line-buffered stream and sets no indicator).
/// `None`, with nothing consumed (a prefix of the record may be in `*lineptr`, which the general
/// path then writes again), when it does not apply (no delimiter in the window, `*lineptr` null or too small,
/// a wide or unset stream): the general [`getdelim`] decides. `p` is `_IO_read_ptr` as
/// [`read_room`] read it, the lock is held or needed by none.
#[inline(always)]
pub fn getdelim_front<E: Env>(env: &E, file: &[AtomicUsize], ext: &Ext, p: usize, lineptr: usize, cap_var: usize, delim: u8) -> Option<usize> {
    let window: &[u8] = read_window(env, file, ext, p)?;
    let buf = env.load_word(lineptr);
    if buf == 0 {
        return None;
    }
    // The record and its NUL must fit `*n` bytes; more than the window and a NUL is never needed.
    let room = env.load_word(cap_var).min(window.len().saturating_add(1));
    let n = env.find_copy(window, env.bytes(buf, room), delim)?;
    set(file, word::READ_PTR, p + n);
    Some(n)
}

/// [`Env::find_copy`] in two passes: [`Env::find`] over the bytes that fit, then the copy.
#[inline(always)]
pub fn find_copy<E: Env + ?Sized>(env: &E, s: &[u8], d: &mut [u8], c: u8) -> Option<usize> {
    let lim = d.len().checked_sub(1)?;
    let n = env.find(s.get(..lim).unwrap_or(s), c)?.checked_add(1)?;
    liella::mem::copy(d.get_mut(..n)?, s.get(..n)?);
    *d.get_mut(n)? = 0;
    Some(n)
}

/// The engine's unread bytes from `_IO_read_ptr` = `p` on, for the read front paths: `None`
/// on a wide or unset stream or when `p` is not before the engine's own end of data (the
/// window and seal invariants of [`get_front`]; never past the stream's buffer).
#[inline(always)]
fn read_window<E: Env>(env: &E, file: &[AtomicUsize], ext: &Ext, p: usize) -> Option<&'static mut [u8]> {
    if low_int(get(file, word::MODE)) >= 0 || ext.buf == 0 {
        return None;
    }
    let at = p.wrapping_sub(ext.buf);
    if at >= ext.state.rend {
        return None;
    }
    env.bytes(ext.buf, ext.buf_len).get_mut(at..ext.state.rend)
}

/// The `fgets` front path (STDIO-2, D-12.19), the mirror of [`getdelim_front`]: the line (up
/// to and including a newline, or `out.len() - 1` bytes, whichever comes first) lies inside
/// the published read window. Its bytes and a NUL go to `out` and `_IO_read_ptr` moves past
/// them; nothing else changes, as with [`getc`] (in-window bytes never flush a line-buffered
/// stream and set no indicator). `None`, with nothing consumed and nothing written, when it
/// does not apply (the window ends first, `out` holds fewer than 2 bytes, a wide or unset
/// stream): [`gets_into`] decides. `p` is `_IO_read_ptr` as [`read_room`] read it, the lock is
/// held or needed by none.
#[inline(always)]
pub fn gets_front<E: Env>(env: &E, file: &[AtomicUsize], ext: &Ext, p: usize, out: &mut [u8]) -> Option<usize> {
    let room = out.len().checked_sub(1)?;
    let window: &[u8] = read_window(env, file, ext, p)?;
    let n = match env.find(window.get(..room).unwrap_or(window), b'\n') {
        Some(i) => i + 1,
        None if window.len() >= room && room > 0 => room,
        None => return None,
    };
    liella::mem::copy(out.get_mut(..n)?, window.get(..n)?);
    *out.get_mut(n)? = 0;
    set(file, word::READ_PTR, p + n);
    Some(n)
}

/// The `fread` front path (STDIO-2, D-12.19): all of `out` lies inside the published read
/// window. The bytes go to `out` and `_IO_read_ptr` moves past them; nothing else changes, as
/// with [`get_front`]. `None`, with nothing consumed, when the window holds fewer bytes (or
/// the stream is wide or unset): [`read`] decides. `p` as for [`gets_front`].
#[inline(always)]
pub fn read_front<E: Env>(env: &E, file: &[AtomicUsize], ext: &Ext, p: usize, out: &mut [u8]) -> Option<usize> {
    let window: &[u8] = read_window(env, file, ext, p)?;
    let n = out.len();
    liella::mem::copy(out, window.get(..n)?);
    set(file, word::READ_PTR, p + n);
    Some(n)
}

/// `getdelim`: a record up to and including `delim` into the caller's `malloc` buffer
/// (`*lineptr`, `*n`), grown as needed and terminated. The length, or `None` at end of input.
pub fn getdelim<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, lineptr: usize, cap_var: usize, delim: u8) -> KResult<Option<usize>> {
    let mut buf = env.load_word(lineptr);
    let mut cap = if buf == 0 { 0 } else { env.load_word(cap_var) };
    let mut len = 0usize;
    if !byte_oriented(file) {
        return Ok(None);
    }
    load(file, ext);
    if input_flushes(ext) {
        flush_line_buffered(env, true);
    }
    let r = run(env, file, ext, |s, b, back| {
        s.read_until_with(b, back, delim, &|w, c| env.find(w, c), &mut |piece| {
            let need = len.checked_add(piece.len()).and_then(|x| x.checked_add(1)).ok_or(Errno::EOVERFLOW)?;
            if need > cap {
                let ncap = need.max(cap.saturating_mul(2)).max(120);
                let nb = env.realloc(buf, ncap);
                if nb == 0 {
                    return Err(Errno::ENOMEM);
                }
                buf = nb;
                cap = ncap;
                env.store_word(lineptr, buf);
                env.store_word(cap_var, cap);
            }
            if let Some(d) = env.bytes(buf, cap).get_mut(len..len + piece.len()) {
                liella::mem::copy(d, piece);
            }
            len += piece.len();
            Ok(())
        })
    })?;
    match r {
        Ok(0) => Ok(None),
        Ok(_) => {
            if let Some(t) = env.bytes(buf, cap).get_mut(len) {
                *t = 0;
            }
            Ok(Some(len))
        }
        Err(e) => {
            ext.state.err = true;
            store(file, ext);
            Err(e)
        }
    }
}

/// `fflush` of one stream. A growing buffer's variables are updated.
pub fn flush<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext) -> KResult<()> {
    if ext.kind == Kind::Closed {
        return Err(Errno::EBADF);
    }
    // STDIO-4: without a buffer nothing is pending or read ahead (the read and write roles
    // need one), so there is nothing to flush - and no buffer to allocate for it (`fclose`
    // of a stream never used made a block-size query and a malloc/free pair here).
    if ext.buf != 0 {
        let gave_back = ext.state.role == Role::Reading;
        run(env, file, ext, |s, b, back| s.flush(b, back))??;
        // FTELL-CACHE (D-12.22): after giving read-ahead back the descriptor is handed over
        // (POSIX 2.5.1), so the next `ftell` asks the kernel again, as the reference library
        // does (probe f21612fa).
        if gave_back {
            ext.pos_known = false;
        }
    }
    if let Kind::Grow(g) = ext.kind {
        grow_publish(env, &g);
    }
    Ok(())
}

/// `__fpurge`.
pub fn purge(file: &[AtomicUsize], ext: &mut Ext) {
    load(file, ext);
    ext.state.purge();
    ext.wide.forget_input();
    store(file, ext);
}

/// `fseeko`: the new position.
pub fn seek<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, offset: i64, whence: Whence) -> KResult<u64> {
    // A relative seek counts from the logical position, before the characters given back.
    let offset = if whence == Whence::Cur { offset.saturating_sub(i64::try_from(ext.wide.back_bytes()).unwrap_or(i64::MAX)) } else { offset };
    if let Some(at) = seek_window_fd(env, file, ext, offset, whence, false) {
        return Ok(at);
    }
    let r = run(env, file, ext, |s, b, back| s.seek(b, back, offset, whence))??;
    ext.wide.forget_input();
    if let Kind::Grow(g) = ext.kind {
        grow_publish(env, &g);
    }
    Ok(r)
}

/// SEEKWIN-3: the in-window seek of a descriptor stream (D-12.22) without the engine run: the
/// same steps as [`run`] with `State::seek` taking its window path (the buffer is in place and
/// the role is reading, so [`run`] would load, seek the descriptor back to the window end through
/// the backing, and [`store_read`]), minus the backing's dispatch and the closure. `None` when the
/// target is not in a known window: the caller takes the general path, nothing changed.
/// `rewind` also clears the error indicator (end of input is cleared by every seek).
#[inline(always)]
fn seek_window_fd<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, offset: i64, whence: Whence, rewind: bool) -> Option<u64> {
    let Kind::Fd(fd) = ext.kind else { return None };
    if ext.buf == 0 {
        return None;
    }
    load(file, ext);
    let (target, to) = ext.state.window_target(offset, whence)?;
    // `window_target` checked that `win_end` fits a signed offset.
    let end = i64::try_from(ext.state.win_end).unwrap_or(i64::MAX);
    let r = env.sys().lseek(fd_of(fd), end, liella_kernel::uapi::SEEK_SET);
    ext.pos_known = r.is_ok();
    if let Ok(p) = r {
        ext.pos = p;
    }
    ext.state.land_in_window(to);
    if rewind {
        ext.state.err = false;
    }
    store_read(file, ext);
    ext.wide.forget_input();
    Some(target)
}

/// `rewind`: [`seek`] to the start with both indicators cleared, in one engine run (STDIO-4:
/// the indicators go out with the window instead of a second update of `_flags`). Errors are
/// not reported; the indicators are cleared whatever happened.
pub fn rewind<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext) {
    // SEEKWIN-3: in the window, both indicators go out with the read window.
    if seek_window_fd(env, file, ext, 0, Whence::Set, true).is_some() {
        return;
    }
    let ran = run(env, file, ext, |s, b, back| {
        let r = s.seek(b, back, 0, Whence::Set);
        s.eof = false;
        s.err = false;
        r
    });
    match ran {
        Ok(Ok(_)) => {
            ext.wide.forget_input();
            if let Kind::Grow(g) = ext.kind {
                grow_publish(env, &g);
            }
        }
        Ok(Err(_)) => {}
        Err(_) => clear_indicators(file),
    }
}

/// SEEKWIN-4: `ftell` of a reading descriptor stream whose offset is known (FTELL-CACHE,
/// D-12.22) without the engine run: the same answer as [`State::tell`] through [`run`] (the
/// known offset less the bytes read ahead, then less the characters given back), with no
/// system call and nothing to store (the window and the indicators do not change). Only the
/// read pointer is read back from the `FILE` words: C code moves it, nothing else here.
/// `None`: another role, an unknown offset or no buffer; the caller takes the general path.
#[inline(always)]
fn tell_window_fd(file: &[AtomicUsize], ext: &mut Ext) -> Option<KResult<u64>> {
    if !matches!(ext.kind, Kind::Fd(_)) || !ext.pos_known || ext.buf == 0 || ext.state.role != Role::Reading {
        return None;
    }
    ext.state.rpos = get(file, word::READ_PTR).wrapping_sub(ext.buf).min(ext.state.rend);
    let unread = u64::try_from(ext.state.unread()).ok()?;
    let back = u64::try_from(ext.wide.back_bytes()).ok()?;
    Some(ext.pos.checked_sub(unread).ok_or(Errno::EINVAL).and_then(|p| p.checked_sub(back).ok_or(Errno::EIO)))
}

/// `ftello`: on a wide stream, before the characters given back (as their encoded length).
pub fn tell<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext) -> KResult<u64> {
    if let Some(r) = tell_window_fd(file, ext) {
        return r;
    }
    let p = run(env, file, ext, |s, _, back| s.tell(back))??;
    p.checked_sub(u64::try_from(ext.wide.back_bytes()).unwrap_or(u64::MAX)).ok_or(Errno::EIO)
}

/// FTELL-BOUNDARY: [`tell`]'s answer when it comes from the window ([`tell_window_fd`]) and is
/// not an error, for the library's front path (inlined into the exports, no lock plumbing).
/// `None`: anything else; the caller then goes [`tell`]'s way, which answers the same or reports
/// the error (the only change made here, the read position taken back from the `FILE` words, is
/// the one [`tell`] makes first as well).
#[inline(always)]
pub fn tell_front(file: &[AtomicUsize], ext: &mut Ext) -> Option<u64> {
    tell_window_fd(file, ext)?.ok()
}

/// [`tell_front`] with the conversion state `fgetpos` stores beside the position.
#[inline(always)]
pub fn getpos_front(file: &[AtomicUsize], ext: &mut Ext) -> Option<(u64, [u8; 8])> {
    let p = tell_front(file, ext)?;
    Some((p, ext.wide.decode.to_bytes()))
}

// ------------------------------------------------------------------------------ the wide layer

impl Wide {
    /// Forget the characters given back and a partial character (a seek, a purge).
    fn forget_input(&mut self) {
        self.back_len = 0;
        self.decode = MbState::INITIAL;
        self.last = None;
        self.pending_len = 0;
        self.partial = 0;
    }

    /// The bytes read but not yet delivered: the characters given back (by their encoded
    /// length) and a partial character.
    #[inline(always)]
    fn back_bytes(&self) -> usize {
        // SEEKWIN-3: nothing given back (every byte stream, most wide ones): no walk (27 of an
        // in-window `fseek(SEEK_CUR)`'s 370 instructions went to setting the walk up; SEEKWIN-4:
        // the early-out inline, the walk out of line - a call still cost 23 in `ftell`).
        if self.back_len == 0 {
            return self.partial + self.pending_len;
        }
        self.back_bytes_walk()
    }

    #[inline(never)]
    fn back_bytes_walk(&self) -> usize {
        let back: usize = self.back.iter().take(self.back_len).map(|&c| self.encoded_len(c)).sum();
        back + self.partial + self.pending_len
    }

    fn encoded_len(&self, c: u32) -> usize {
        match self.codec {
            Codec::Text(enc) => text::encode(c, enc, &mut [0u8; 16]).unwrap_or(1),
            Codec::Units => 4,
            Codec::Charset { mut enc, .. } => enc.convert(&c.to_le_bytes(), &mut [0u8; 16], &Locale::C).written,
            Codec::Unset => 0,
        }
    }
}

fn set_error(file: &[AtomicUsize], ext: &mut Ext) {
    load(file, ext);
    ext.state.err = true;
    store(file, ext);
}

/// `fgetwc`: the next wide character, `None` at end of input (an incomplete character there
/// included, as the reference library has it). An invalid sequence is `EILSEQ` with the
/// error indicator set, its first byte left unread. On a byte-oriented stream, `None`.
pub fn getwc<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, enc: Encoding) -> KResult<Option<u32>> {
    if !wide_oriented(file, ext, enc) {
        return Ok(None);
    }
    let c = getwc_inner(env, file, ext)?;
    ext.wide.last = c;
    Ok(c)
}

fn getwc_inner<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext) -> KResult<Option<u32>> {
    if ext.wide.back_len > 0 {
        ext.wide.back_len -= 1;
        return Ok(ext.wide.back.get(ext.wide.back_len).copied());
    }
    match ext.wide.codec {
        Codec::Units => {
            let mut u = [0u8; 4];
            for slot in &mut u {
                match getc_raw(env, file, ext)? {
                    Some(b) => *slot = b,
                    None => return Ok(None),
                }
            }
            Ok(Some(u32::from_ne_bytes(u)))
        }
        Codec::Text(enc) => loop {
            // At end of file a partial character stays pending (the position excludes it).
            let Some(b) = getc_raw(env, file, ext)? else { return Ok(None) };
            let mut st = ext.wide.decode;
            match text::decode(&mut st, &[b], enc) {
                Decoded::Char(c, _) => {
                    ext.wide.decode = MbState::INITIAL;
                    ext.wide.partial = 0;
                    return Ok(Some(c));
                }
                Decoded::Null => {
                    ext.wide.decode = MbState::INITIAL;
                    ext.wide.partial = 0;
                    return Ok(Some(0));
                }
                Decoded::Incomplete => {
                    ext.wide.decode = st;
                    ext.wide.partial += 1;
                }
                Decoded::Invalid | Decoded::Continuation(_) => {
                    ext.wide.decode = MbState::INITIAL;
                    ext.wide.partial = 0;
                    ungetc_raw(env, file, ext, b)?;
                    set_error(file, ext);
                    return Err(Errno::EILSEQ);
                }
            }
        },
        Codec::Charset { mut dec, enc } => loop {
            let Some(b) = getc_raw(env, file, ext)? else { return Ok(None) };
            let w = &mut ext.wide;
            let Some(slot) = w.pending.get_mut(w.pending_len) else {
                w.pending_len = 0;
                set_error(file, ext);
                return Err(Errno::EILSEQ);
            };
            *slot = b;
            w.pending_len += 1;
            let mut out = [0u8; 4];
            let o = dec.convert(w.pending.get(..w.pending_len).unwrap_or(&[]), &mut out, &Locale::C);
            // The descriptor keeps the byte order a BOM chose.
            w.codec = Codec::Charset { dec, enc };
            if o.written == 4 {
                w.pending_len = 0;
                return Ok(Some(u32::from_le_bytes(out)));
            }
            match o.result {
                // A BOM: consumed, nothing to deliver.
                Ok(_) if o.read == w.pending_len => w.pending_len = 0,
                Err(Errno::EINVAL) | Ok(_) => {}
                Err(_) => {
                    w.pending_len = 0;
                    set_error(file, ext);
                    return Err(Errno::EILSEQ);
                }
            }
        },
        Codec::Unset => Err(Errno::EINVAL),
    }
}

/// `ungetwc`: give `c` back (up to [`WIDE_BACK`] characters); clears the end-of-file indicator.
pub fn ungetwc(file: &[AtomicUsize], ext: &mut Ext, c: u32, enc: Encoding) -> KResult<()> {
    if !wide_oriented(file, ext, enc) {
        return Err(Errno::EINVAL);
    }
    let slot = ext.wide.back.get_mut(ext.wide.back_len).ok_or(Errno::EAGAIN)?;
    *slot = c;
    ext.wide.back_len += 1;
    load(file, ext);
    ext.state.eof = false;
    store(file, ext);
    Ok(())
}

/// `_IO_sungetwc`: give back the last character read.
pub fn unget_last_wide(file: &[AtomicUsize], ext: &mut Ext, enc: Encoding) -> KResult<u32> {
    let c = ext.wide.last.take().ok_or(Errno::EINVAL)?;
    ungetwc(file, ext, c, enc).map(|()| c)
}

/// `fputwc`: write `c` converted by the stream's codec; a character the encoding cannot
/// represent is `EILSEQ` with the error indicator set. On a byte-oriented stream, `EINVAL`.
pub fn putwc<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, c: u32, enc: Encoding) -> KResult<()> {
    if !wide_oriented(file, ext, enc) {
        return Err(Errno::EINVAL);
    }
    let mut b = [0u8; 16];
    let n = match ext.wide.codec {
        Codec::Text(enc) => match text::encode(c, enc, &mut b) {
            Some(n) => n,
            None => {
                set_error(file, ext);
                return Err(Errno::EILSEQ);
            }
        },
        Codec::Units => {
            if let Some(d) = b.get_mut(..4) {
                d.copy_from_slice(&c.to_ne_bytes());
            }
            4
        }
        Codec::Charset { dec, mut enc } => {
            let o = enc.convert(&c.to_le_bytes(), &mut b, &Locale::C);
            // The descriptor remembers that it wrote a BOM.
            ext.wide.codec = Codec::Charset { dec, enc };
            if o.result.is_err() {
                set_error(file, ext);
                return Err(Errno::EILSEQ);
            }
            o.written
        }
        Codec::Unset => return Err(Errno::EINVAL),
    };
    let t = write_raw(env, file, ext, b.get(..n).unwrap_or(&[]));
    t.error.map_or(Ok(()), Err)
}

/// Diagnostic text (`perror`, `err`, `error`): through the codec of a wide-oriented stream (the
/// bytes decoded in `enc`, invalid ones skipped); otherwise as bytes, making an undecided
/// stream byte-oriented only if `orient`.
pub fn write_text<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, data: &[u8], orient: bool, enc: Encoding) -> KResult<()> {
    let mode = low_int(get(file, word::MODE));
    if mode > 0 {
        let mut i = 0;
        while i < data.len() {
            match text::decode(&mut { MbState::INITIAL }, data.get(i..).unwrap_or(&[]), enc) {
                Decoded::Char(c, n) => {
                    putwc(env, file, ext, c, enc)?;
                    i += n.max(1);
                }
                _ => i += 1,
            }
        }
        return Ok(());
    }
    if orient {
        byte_oriented(file);
    }
    write_raw(env, file, ext, data).error.map_or(Ok(()), Err)
}

/// `fgetws`: at most `out.len() - 1` wide characters, stopping after a newline, then a null
/// one. The count stored before the null; `None` if end of input came before any; an error
/// is reported as such.
pub fn getws_into<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, out: &mut [i32], enc: Encoding) -> KResult<Option<usize>> {
    let room = out.len().saturating_sub(1);
    if room == 0 {
        return Ok(None);
    }
    let mut n = 0;
    while n < room {
        match getwc(env, file, ext, enc)? {
            Some(c) => {
                if let Some(slot) = out.get_mut(n) {
                    *slot = i32::from_ne_bytes(c.to_ne_bytes());
                }
                n += 1;
                if c == u32::from(b'\n') {
                    break;
                }
            }
            None => break,
        }
    }
    if let Some(t) = out.get_mut(n) {
        *t = 0;
    }
    Ok((n > 0 || room == 0).then_some(n))
}

/// `clearerr`.
pub fn clear_indicators(file: &[AtomicUsize]) {
    if let Some(w) = file.get(word::FLAGS) {
        w.fetch_and(!(EOF_SEEN | ERR_SEEN), Relaxed);
    }
}

/// `setvbuf(buf, mode, size)`: must come before the first transfer (`EINVAL` otherwise, and
/// for an unknown mode). `mode`: 0 `_IOFBF`, 1 `_IOLBF`, 2 `_IONBF`.
pub fn setvbuf<E: Env>(env: &E, file: &[AtomicUsize], ext: &mut Ext, user: usize, mode: i32, size: usize) -> KResult<()> {
    let policy = match mode {
        0 => Policy::Full,
        1 => Policy::Line,
        2 => Policy::Unbuffered,
        _ => return Err(Errno::EINVAL),
    };
    load(file, ext);
    // After I/O the buffer is flushed first (the reference library accepts a late call); bytes
    // read ahead that cannot be given back keep the old buffer.
    if ext.state.role != Role::Idle {
        run(env, file, ext, |s, b, back| s.flush(b, back))??;
        if ext.state.role != Role::Idle {
            return Err(Errno::EINVAL);
        }
    }
    if ext.buf_owned && ext.buf != 0 {
        env.free(ext.buf);
    }
    ext.buf = 0;
    ext.buf_len = 0;
    ext.buf_owned = false;
    ext.state.policy = policy;
    // Without a caller buffer the size is not used: the stream keeps its default size, as the
    // reference library does.
    let caller = policy != Policy::Unbuffered && user != 0;
    ext.user_buf = if caller { user } else { 0 };
    ext.want = if caller { size } else { 0 };
    store(file, ext);
    Ok(())
}

/// The `<stdio_ext.h>` queries (§12.4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Info {
    /// `__fbufsize`.
    pub bufsize: usize,
    /// `__fpending`.
    pub pending: usize,
    /// `__flbf`.
    pub line_buffered: bool,
    /// `__freading`: read-only, or the last operation read.
    pub reading: bool,
    /// `__fwriting`: write-only, or the last operation wrote.
    pub writing: bool,
    /// `__freadable`.
    pub readable: bool,
    /// `__fwritable`.
    pub writable: bool,
}

/// The `<stdio_ext.h>` facts of a stream.
pub fn info(file: &[AtomicUsize], ext: &mut Ext) -> Info {
    load(file, ext);
    let s = &ext.state;
    Info {
        bufsize: if ext.user_buf != 0 { ext.want } else { ext.buf_len.saturating_sub(RESERVE) },
        pending: s.pending(),
        line_buffered: s.policy == Policy::Line,
        reading: (s.readable && !s.writable) || s.role == Role::Reading,
        writing: (s.writable && !s.readable) || s.role == Role::Writing,
        readable: s.readable,
        writable: s.writable,
    }
}

/// `__fsetlocking`: 0 query, 1 internal, 2 by caller; the previous (1 or 2).
pub fn set_locking(file: &[AtomicUsize], how: i32) -> i32 {
    let w = file.get(word::FLAGS);
    let before = w.map_or(0, |w| w.load(Relaxed));
    let prev = if before & USER_LOCK != 0 { 2 } else { 1 };
    match (how, w) {
        (1, Some(w)) => {
            w.fetch_and(!USER_LOCK, Relaxed);
        }
        (2, Some(w)) => {
            w.fetch_or(USER_LOCK, Relaxed);
        }
        _ => {}
    }
    prev
}

/// Whether the caller does the locking (`__fsetlocking(FSETLOCKING_BYCALLER)`).
pub fn by_caller(file: &[AtomicUsize]) -> bool {
    get(file, word::FLAGS) & USER_LOCK != 0
}

/// `fwide(mode)`: the orientation after the call (negative byte, positive wide, 0 undecided).
/// Once decided it does not change; a stream becoming wide converts with `enc` from then on.
pub fn fwide(file: &[AtomicUsize], ext: &mut Ext, mode: i32, enc: Encoding) -> i32 {
    let cur = low_int(get(file, word::MODE));
    if cur != 0 || mode == 0 {
        return cur;
    }
    if mode > 0 {
        wide_oriented(file, ext, enc);
    } else {
        byte_oriented(file);
    }
    mode.signum()
}
