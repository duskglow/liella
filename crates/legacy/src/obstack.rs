//! Obstacks (spec §3.5; D-3.6): the out-of-line half of the GNU obstack protocol. The
//! `<obstack.h>` macros manipulate `struct obstack` directly, so its fields keep exactly the
//! glibc meaning: `chunk`, `object_base`, `next_free`, `chunk_limit`, `alignment_mask`, the
//! caller's `chunkfun`/`freefun` (with `extra_arg` when `use_extra_arg` is set), and the
//! flag bits `use_extra_arg` (bit 0), `maybe_empty_object` (bit 1) and `alloc_failed` (bit 2).
//!
//! A chunk is `{ char *limit; struct _obstack_chunk *prev; char contents[]; }` in caller
//! memory, reached through [`CWords`]; the C callbacks run through [`CCall`]. When a chunk
//! cannot be allocated the failure handler runs; it must not return (if it does, the process
//! aborts).

use liella_abi::layout::struct_obstack;

use crate::search_c::CWords;

/// Calling C functions the caller supplied as data.
pub trait CCall {
    /// `f(a, b, c)` with integer-class arguments; the integer-class result.
    fn call3(&mut self, f: usize, a: usize, b: usize, c: usize) -> usize;
    /// `f()`.
    fn call0(&mut self, f: usize);
    /// Terminate the process (an unrecoverable misuse, or a failure handler that returned).
    fn fatal(&mut self) -> !;
}

const USE_EXTRA_ARG: u8 = 1;
const MAYBE_EMPTY_OBJECT: u8 = 2;
const ALLOC_FAILED: u8 = 4;

/// The offset of `contents` in a chunk header.
const CONTENTS: usize = 16;

/// The default chunk size glibc uses when 0 is passed: a 4 KiB block less malloc's overhead.
pub const DEFAULT_CHUNK: usize = 4064;

fn flag(h: &struct_obstack, bit: u8) -> bool {
    h.use_extra_arg[0] & bit != 0
}

fn set_flag(h: &mut struct_obstack, bit: u8, on: bool) {
    if on {
        h.use_extra_arg[0] |= bit;
    } else {
        h.use_extra_arg[0] &= !bit;
    }
}

fn mask(h: &struct_obstack) -> usize {
    usize::try_from(h.alignment_mask).unwrap_or(0)
}

/// `__PTR_ALIGN(chunk, chunk->contents, mask)`.
fn aligned_contents(chunk: usize, m: usize) -> usize {
    chunk.saturating_add(CONTENTS).saturating_add(m) & !m
}

fn chunk_alloc(h: &struct_obstack, size: usize, c: &mut impl CCall) -> usize {
    if flag(h, USE_EXTRA_ARG) { c.call3(h.chunkfun, h.extra_arg, size, 0) } else { c.call3(h.chunkfun, size, 0, 0) }
}

fn chunk_free(h: &struct_obstack, chunk: usize, c: &mut impl CCall) {
    if flag(h, USE_EXTRA_ARG) {
        c.call3(h.freefun, h.extra_arg, chunk, 0);
    } else {
        c.call3(h.freefun, chunk, 0, 0);
    }
}

/// Run the failure handler (`obstack_alloc_failed_handler`); it must not return.
fn failed(handler: usize, c: &mut impl CCall) -> ! {
    c.call0(handler);
    c.fatal()
}

fn begin(h: &mut struct_obstack, size: i64, alignment: i64, mem: &mut impl CWords, c: &mut impl CCall, handler: usize) -> i32 {
    let align = usize::try_from(alignment).ok().filter(|&a| a != 0).unwrap_or(16);
    let size = usize::try_from(size).ok().filter(|&s| s != 0).unwrap_or(DEFAULT_CHUNK);
    h.chunk_size = i64::try_from(size).unwrap_or(i64::MAX);
    h.alignment_mask = i32::try_from(align.saturating_sub(1)).unwrap_or(15);
    let chunk = chunk_alloc(h, size, c);
    if chunk == 0 {
        failed(handler, c);
    }
    h.chunk = chunk;
    h.object_base = aligned_contents(chunk, mask(h));
    h.next_free = h.object_base;
    h.chunk_limit = chunk.saturating_add(size);
    mem.set_word(chunk, 0, h.chunk_limit);
    mem.set_word(chunk, 1, 0);
    set_flag(h, MAYBE_EMPTY_OBJECT, false);
    set_flag(h, ALLOC_FAILED, false);
    1
}

/// `_obstack_begin(h, size, alignment, chunkfun, freefun)`: 0 for size or alignment selects
/// the default. Returns 1 (a failure runs the handler instead).
#[allow(clippy::too_many_arguments)] // the C signature
pub fn obstack_begin(h: &mut struct_obstack, size: i64, alignment: i64, chunkfun: usize, freefun: usize, mut mem: impl CWords, mut c: impl CCall, handler: usize) -> i32 {
    h.chunkfun = chunkfun;
    h.freefun = freefun;
    set_flag(h, USE_EXTRA_ARG, false);
    begin(h, size, alignment, &mut mem, &mut c, handler)
}

/// `_obstack_begin_1`: as [`obstack_begin`], the callbacks taking `arg` first.
#[allow(clippy::too_many_arguments)] // the C signature
pub fn obstack_begin_1(h: &mut struct_obstack, size: i64, alignment: i64, chunkfun: usize, freefun: usize, arg: usize, mut mem: impl CWords, mut c: impl CCall, handler: usize) -> i32 {
    h.chunkfun = chunkfun;
    h.freefun = freefun;
    h.extra_arg = arg;
    set_flag(h, USE_EXTRA_ARG, true);
    begin(h, size, alignment, &mut mem, &mut c, handler)
}

/// `_obstack_newchunk(h, length)`: move the growing object to a new chunk with room for
/// `length` more bytes, freeing the old chunk if the object was all it held.
pub fn obstack_newchunk(h: &mut struct_obstack, length: usize, mut mem: impl CWords, mut c: impl CCall, copy: impl FnOnce(usize, usize, usize), handler: usize) {
    let old = h.chunk;
    let obj_size = h.next_free.saturating_sub(h.object_base);
    let m = mask(h);
    let wanted = obj_size.saturating_add(length).saturating_add(obj_size >> 3).saturating_add(m).saturating_add(100);
    let new_size = wanted.max(usize::try_from(h.chunk_size).unwrap_or(0));
    let new = chunk_alloc(h, new_size, &mut c);
    if new == 0 {
        set_flag(h, ALLOC_FAILED, true);
        failed(handler, &mut c);
    }
    h.chunk = new;
    mem.set_word(new, 1, old);
    h.chunk_limit = new.saturating_add(new_size);
    mem.set_word(new, 0, h.chunk_limit);
    let base = aligned_contents(new, m);
    copy(h.object_base, base, obj_size);
    if !flag(h, MAYBE_EMPTY_OBJECT) && h.object_base == aligned_contents(old, m) {
        mem.set_word(new, 1, mem.word(old, 1));
        chunk_free(h, old, &mut c);
    }
    h.object_base = base;
    h.next_free = base.saturating_add(obj_size);
    set_flag(h, MAYBE_EMPTY_OBJECT, false);
}

/// `_obstack_allocated_p(h, obj)`: 1 if `obj` lies in one of the obstack's chunks.
pub fn obstack_allocated_p(h: &struct_obstack, obj: usize, mem: impl CWords) -> i32 {
    let mut lp = h.chunk;
    while lp != 0 && (lp >= obj || mem.word(lp, 0) < obj) {
        lp = mem.word(lp, 1);
    }
    i32::from(lp != 0)
}

/// `obstack_free(h, obj)`: release `obj` and everything allocated after it; NULL releases
/// everything. An `obj` in no chunk aborts, as in glibc.
pub fn obstack_free(h: &mut struct_obstack, obj: usize, mut mem: impl CWords, mut c: impl CCall) {
    let mut lp = h.chunk;
    while lp != 0 && (lp >= obj || mem.word(lp, 0) < obj) {
        let prev = mem.word(lp, 1);
        chunk_free(h, lp, &mut c);
        lp = prev;
        set_flag(h, MAYBE_EMPTY_OBJECT, true);
    }
    if lp != 0 {
        h.object_base = obj;
        h.next_free = obj;
        h.chunk_limit = mem.word(lp, 0);
        h.chunk = lp;
    } else if obj != 0 {
        c.fatal();
    } else {
        h.chunk = 0;
    }
    let _ = &mut mem;
}

/// `_obstack_memory_used(h)`: bytes in all chunks.
pub fn obstack_memory_used(h: &struct_obstack, mem: impl CWords) -> i32 {
    let mut total = 0usize;
    let mut lp = h.chunk;
    while lp != 0 {
        total = total.saturating_add(mem.word(lp, 0).saturating_sub(lp));
        lp = mem.word(lp, 1);
    }
    i32::try_from(total).unwrap_or(i32::MAX)
}

/// The message of the default failure handler.
pub const EXHAUSTED: &[u8] = b"memory exhausted\n";
