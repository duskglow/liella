//! The C shapes of the `malloc` family over `liella_rt::heap` (spec §3.1–§3.3; D-3.2–D-3.5):
//! `ENOMEM` and `EINVAL` conventions, the five alignment entry points with their three validity
//! rules, `realloc`'s NULL and zero cases, `posix_memalign`'s error-number return, and the
//! implementation-neutral introspection (`mallopt`, `mallinfo`, `mallinfo2`, `malloc_stats`,
//! `malloc_info`). The debugging semantics are in `crate::malloc_debug`.
//!
//! Zero-filling (`calloc`) and moving (`realloc`) touch caller memory, so they are callbacks the
//! boundary supplies.

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering::Relaxed;

use liella::error::{Errno, Result};
use liella::num::format::{format_u64, Bounded, Case, Sink};
use liella_abi::layout::{struct_mallinfo, struct_mallinfo2};
use liella_kernel::sys::Sys;
use liella_rt::heap::{Heap, Misuse, Pages, Stats, ALIGN, PAGE};

use crate::malloc_debug::Debug;
use crate::string_compat::Cursor;

/// The heap with its kernel and memory source.
pub struct Ctx<'a, S: Sys, P: Pages> {
    /// The heap.
    pub heap: &'a Heap,
    /// The kernel (for the heap lock).
    pub sys: &'a S,
    /// The memory source.
    pub pages: &'a P,
}

impl<S: Sys, P: Pages> Ctx<'_, S, P> {
    #[inline(always)]
    fn alloc(&self, n: usize, align: usize) -> Result<liella_rt::heap::Block> {
        self.heap.alloc(self.sys, self.pages, n, align).ok_or(Errno::ENOMEM)
    }
}

/// `malloc`: `malloc(0)` returns a unique, freeable block (D-3.2). Always inlined (MALLOC-FAST):
/// out of line, the export called it with the context in memory and read its result back
/// through the stack (about 20 of a warm pair's instructions).
#[inline(always)]
pub fn malloc<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, n: usize) -> Result<usize> {
    c.alloc(n, ALIGN).map(|b| b.addr)
}

/// `calloc`: `k * n` zeroed bytes; an overflowing product is `ENOMEM`.
#[inline(always)]
pub fn calloc<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, k: usize, n: usize, zero: impl FnOnce(usize, usize)) -> Result<usize> {
    let total = k.checked_mul(n).ok_or(Errno::ENOMEM)?;
    let b = c.alloc(total, ALIGN)?;
    if !b.zeroed {
        zero(b.addr, total);
    }
    Ok(b.addr)
}

/// `free`: NULL is a no-op; `errno` is never changed.
#[inline(always)]
pub fn free<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, p: usize) -> core::result::Result<(), Misuse> {
    if p == 0 {
        return Ok(());
    }
    c.heap.free(c.sys, c.pages, p)
}

/// `realloc`: NULL is `malloc`; size 0 frees and returns NULL without an error (D-3.2); a
/// failure leaves the block intact (`ENOMEM`). Always inlined (TARC-ALLOC): out of line, the
/// export passed the context through memory and the call reloaded its fields.
#[inline(always)]
pub fn realloc<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, p: usize, n: usize, copy: impl FnOnce(usize, usize, usize)) -> core::result::Result<Result<usize>, Misuse> {
    if p == 0 {
        return Ok(malloc(c, n));
    }
    if n == 0 {
        return free(c, p).map(|()| Ok(0));
    }
    Ok(c.heap.resize(c.sys, c.pages, p, n, copy)?.map(|b| b.addr).ok_or(Errno::ENOMEM))
}

/// `reallocarray`: `realloc` with an overflow-checked product.
pub fn reallocarray<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, p: usize, k: usize, n: usize, copy: impl FnOnce(usize, usize, usize)) -> core::result::Result<Result<usize>, Misuse> {
    match k.checked_mul(n) {
        Some(total) => realloc(c, p, total, copy),
        None => Ok(Err(Errno::ENOMEM)),
    }
}

/// `aligned_alloc`: the alignment must be a power of two (`EINVAL`); any size is accepted.
pub fn aligned_alloc<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, align: usize, n: usize) -> Result<usize> {
    c.alloc(n, aligned_request(align)?).map(|b| b.addr)
}

/// `aligned_alloc`'s validity rule: the alignment the heap is asked for (`EINVAL` unless a
/// power of two).
pub fn aligned_request(align: usize) -> Result<usize> {
    if align.is_power_of_two() { Ok(align) } else { Err(Errno::EINVAL) }
}

/// `memalign`'s rule: a non-power-of-two alignment is rounded up.
pub fn memalign_request(align: usize) -> Result<usize> {
    align.max(1).checked_next_power_of_two().ok_or(Errno::EINVAL)
}

/// `posix_memalign`'s rule: a power of two and a multiple of `sizeof(void *)` (`EINVAL`).
pub fn posix_memalign_request(align: usize) -> Result<usize> {
    if !align.is_power_of_two() || align.trailing_zeros() < core::mem::size_of::<usize>().trailing_zeros() {
        return Err(Errno::EINVAL);
    }
    Ok(align)
}

/// `pvalloc`'s size: whole pages, at least one (`ENOMEM` on overflow).
pub fn pvalloc_size(n: usize) -> Result<usize> {
    n.max(1).checked_next_multiple_of(PAGE).ok_or(Errno::ENOMEM)
}

/// `aligned_alloc` with the block zeroed (`calloc` with an alignment; the native interface's
/// zeroed allocation, D-X.10): the alignment must be a power of two (`EINVAL`).
pub fn aligned_calloc<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, align: usize, n: usize, zero: impl FnOnce(usize, usize)) -> Result<usize> {
    if !align.is_power_of_two() {
        return Err(Errno::EINVAL);
    }
    let b = c.alloc(n, align)?;
    if !b.zeroed {
        zero(b.addr, n);
    }
    Ok(b.addr)
}

/// A secure block (the native interface's `liella_heap_alloc_secure`, D-X.11): `n` bytes at
/// `align` (a power of two, `EINVAL` otherwise) in a mapping of its own that reads as zero,
/// zeroed by the heap whenever it is released (`free`, `realloc` moving it).
pub fn aligned_alloc_secure<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, align: usize, n: usize) -> Result<usize> {
    if !align.is_power_of_two() {
        return Err(Errno::EINVAL);
    }
    c.heap.alloc_secure(c.sys, c.pages, n, align).map(|b| b.addr).ok_or(Errno::ENOMEM)
}

/// `memalign`: a non-power-of-two alignment is rounded up.
pub fn memalign<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, align: usize, n: usize) -> Result<usize> {
    c.alloc(n, memalign_request(align)?).map(|b| b.addr)
}

/// `posix_memalign`: the alignment must be a power of two and a multiple of `sizeof(void *)`;
/// returns the error number and leaves `*out` untouched on failure.
pub fn posix_memalign<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, mut out: impl Cursor, align: usize, n: usize) -> Result<()> {
    out.set(c.alloc(n, posix_memalign_request(align)?)?.addr);
    Ok(())
}

/// `valloc`: page-aligned.
pub fn valloc<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, n: usize) -> Result<usize> {
    c.alloc(n, PAGE).map(|b| b.addr)
}

/// `pvalloc`: page-aligned, the size rounded up to whole pages (at least one).
pub fn pvalloc<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, n: usize) -> Result<usize> {
    c.alloc(pvalloc_size(n)?, PAGE).map(|b| b.addr)
}

/// `malloc_usable_size`: 0 for NULL.
pub fn usable_size<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, p: usize) -> core::result::Result<usize, Misuse> {
    if p == 0 {
        return Ok(0);
    }
    c.heap.usable(c.sys, c.pages, p)
}

/// `malloc_trim`: 1 if any memory went back to the kernel (`pad` has no neutral meaning).
pub fn trim<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, _pad: usize) -> bool {
    c.heap.trim(c.sys, c.pages)
}

/// `M_TRIM_THRESHOLD`.
pub const M_TRIM_THRESHOLD: i32 = -1;
/// `M_TOP_PAD`.
pub const M_TOP_PAD: i32 = -2;
/// `M_MMAP_THRESHOLD`.
pub const M_MMAP_THRESHOLD: i32 = -3;
/// `M_PERTURB`.
pub const M_PERTURB: i32 = -6;
/// `M_ARENA_MAX`.
pub const M_ARENA_MAX: i32 = -8;
/// `M_MXFAST`.
pub const M_MXFAST: i32 = 1;
/// The largest `M_MXFAST` value the reference accepts (as probed on x86_64).
const MXFAST_MAX: i32 = 160;

/// `mallopt`: accepted (1) for every parameter, known or not, as the reference accepts them
/// (owner ruling 2026-10-22, #412; D-3.4), except `M_MXFAST` outside `0..=160` (0, as the
/// reference). `M_PERTURB` takes effect (the low byte of `value`, 0 for off;
/// `crate::malloc_debug`); the heap has no use for the other parameters and ignores them.
pub fn mallopt(debug: &Debug, param: i32, value: i32) -> bool {
    if param == M_PERTURB {
        debug.set_perturb(value);
    }
    param != M_MXFAST || (0..=MXFAST_MAX).contains(&value)
}

fn sat_u64(v: usize) -> u64 {
    u64::try_from(v).unwrap_or(u64::MAX)
}

/// `mallinfo2`: bytes held from the kernel (`arena` for small blocks, `hblkhd` for large ones),
/// large blocks (`hblks`), bytes in use (`uordblks`), bytes held but free (`fordblks`), and
/// releasable bytes (`keepcost`); the arena-specific fields are 0 (D-3.4).
pub fn mallinfo2<S: Sys, P: Pages>(c: &Ctx<'_, S, P>) -> struct_mallinfo2 {
    let s = c.heap.stats(c.sys, c.pages);
    let held = s.small_mapped.saturating_add(s.large_mapped);
    struct_mallinfo2 {
        arena: sat_u64(s.small_mapped),
        ordblks: 0,
        smblks: 0,
        hblks: sat_u64(s.large_blocks),
        hblkhd: sat_u64(s.large_mapped),
        usmblks: 0,
        fsmblks: 0,
        uordblks: sat_u64(s.in_use),
        fordblks: sat_u64(held.saturating_sub(s.in_use)),
        keepcost: sat_u64(s.releasable),
    }
}

/// `mallinfo`: [`mallinfo2`] with every field saturated at `INT_MAX` (D-3.4).
pub fn mallinfo<S: Sys, P: Pages>(c: &Ctx<'_, S, P>) -> struct_mallinfo {
    let m = mallinfo2(c);
    let i = |v: u64| i32::try_from(v).unwrap_or(i32::MAX);
    struct_mallinfo {
        arena: i(m.arena),
        ordblks: i(m.ordblks),
        smblks: i(m.smblks),
        hblks: i(m.hblks),
        hblkhd: i(m.hblkhd),
        usmblks: i(m.usmblks),
        fsmblks: i(m.fsmblks),
        uordblks: i(m.uordblks),
        fordblks: i(m.fordblks),
        keepcost: i(m.keepcost),
    }
}

/// `label` and `v` right-aligned in ten columns, then a newline (`malloc_stats`' layout).
fn line(out: &mut dyn Sink, label: &[u8], v: usize) {
    let mut digits = [0u8; 20];
    let mut b = Bounded::new(&mut digits);
    format_u64(&mut b, sat_u64(v), 10, Case::Lower);
    let n = b.stored();
    out.put(label);
    out.repeat(b' ', 10usize.saturating_sub(n));
    out.put(digits.get(..n).unwrap_or(&[]));
    out.put(b"\n");
}

/// The heap's statistics.
pub fn stats<S: Sys, P: Pages>(c: &Ctx<'_, S, P>) -> Stats {
    c.heap.stats(c.sys, c.pages)
}

/// `malloc_stats`: the reference's lines with neutral meanings (D-3.4). The heap has no
/// arenas: one `Arena 0` stands for the small-block segments of every thread (its system bytes
/// and the bytes of live small blocks); the totals add the large blocks, each a mapping of its
/// own, with the most of them, and the most bytes in them, mapped at once.
pub fn stats_text(s: &Stats, out: &mut dyn Sink) {
    out.put(b"Arena 0:\n");
    line(out, b"system bytes     = ", s.small_mapped);
    line(out, b"in use bytes     = ", s.small_in_use);
    out.put(b"Total (incl. mmap):\n");
    line(out, b"system bytes     = ", s.small_mapped.saturating_add(s.large_mapped));
    line(out, b"in use bytes     = ", s.in_use);
    line(out, b"max mmap regions = ", s.peak_large_blocks);
    line(out, b"max mmap bytes   = ", s.peak_large_mapped);
}

fn element(out: &mut dyn Sink, name: &[u8], kind: &[u8], count: Option<usize>, size: usize) {
    out.put(b"<");
    out.put(name);
    out.put(b" type=\"");
    out.put(kind);
    if let Some(n) = count {
        out.put(b"\" count=\"");
        format_u64(out, sat_u64(n), 10, Case::Lower);
    }
    out.put(b"\" size=\"");
    format_u64(out, sat_u64(size), 10, Case::Lower);
    out.put(b"\"/>\n");
}

/// `malloc_info(0, fp)`'s text: the reference's element structure (`spec/behaviour/mallocdbg.md`)
/// with neutral content (D-3.4). One `heap` (number 0) stands for the small-block segments;
/// its `sizes` list is empty (the heap keeps no free-chunk lists by size); `fast` is 0 (no
/// fast bins); `rest` counts the empty slabs the heap holds and their bytes (what
/// `malloc_trim` can release); `system` and `aspace` are the bytes mapped (current and most;
/// nothing is mapped without access, so `mprotect` equals `total`); the process totals add
/// the large blocks as `mmap`.
pub fn info_text(s: &Stats, out: &mut dyn Sink) {
    let all = s.small_mapped.saturating_add(s.large_mapped);
    out.put(b"<malloc version=\"1\">\n<heap nr=\"0\">\n<sizes>\n</sizes>\n");
    element(out, b"total", b"fast", Some(0), 0);
    element(out, b"total", b"rest", Some(s.empty_slabs), s.releasable);
    element(out, b"system", b"current", None, s.small_mapped);
    element(out, b"system", b"max", None, s.small_mapped);
    element(out, b"aspace", b"total", None, s.small_mapped);
    element(out, b"aspace", b"mprotect", None, s.small_mapped);
    out.put(b"</heap>\n");
    element(out, b"total", b"fast", Some(0), 0);
    element(out, b"total", b"rest", Some(s.empty_slabs), s.releasable);
    element(out, b"total", b"mmap", Some(s.large_blocks), s.large_mapped);
    element(out, b"system", b"current", None, all);
    element(out, b"system", b"max", None, s.peak_mapped.max(all));
    element(out, b"aspace", b"total", None, all);
    element(out, b"aspace", b"mprotect", None, all);
    out.put(b"</malloc>\n");
}

/// `malloc_info(options, fp)`'s check: options other than 0 are `EINVAL` (malloc_info(3)).
pub fn info_options(options: i32) -> Result<()> {
    if options == 0 { Ok(()) } else { Err(Errno::EINVAL) }
}

/// `__libc_freeres` and `tr_break`: nothing to do; the heap holds nothing that must be freed
/// at exit, and `mallwatch` is inert compat data (D-3.5).
pub fn inert() {}

/// `mprobe` while checking is off.
pub const MCHECK_DISABLED: i32 = crate::malloc_debug::MCHECK_DISABLED;

/// `malloc_get_state`/`malloc_set_state`: compat stubs that fail (D-3.5).
pub fn state_unsupported() -> Result<usize> {
    Err(Errno::ENOSYS)
}

/// `brk(want)`: `sys_brk` is the system call, which returns the new break (the old one on
/// failure, ⚑ KERN). The break is recorded in `curbrk`.
pub fn brk(want: usize, sys_brk: impl FnOnce(usize) -> usize, curbrk: &AtomicUsize) -> Result<()> {
    let got = sys_brk(want);
    curbrk.store(got, Relaxed);
    if got < want { Err(Errno::ENOMEM) } else { Ok(()) }
}

/// `sbrk(delta)`: move the break by `delta`; returns the old break.
pub fn sbrk(delta: isize, sys_brk: impl Fn(usize) -> usize, curbrk: &AtomicUsize) -> Result<usize> {
    if curbrk.load(Relaxed) == 0 {
        curbrk.store(sys_brk(0), Relaxed);
    }
    let old = curbrk.load(Relaxed);
    if delta == 0 {
        return Ok(old);
    }
    let want = old.checked_add_signed(delta).ok_or(Errno::ENOMEM)?;
    brk(want, &sys_brk, curbrk)?;
    if delta < 0 || curbrk.load(Relaxed) >= want { Ok(old) } else { Err(Errno::ENOMEM) }
}
