//! The thread block (IP-8; spec §1.1, §22.6, §27.3; ERRATA E7): per-thread state at the thread
//! pointer, reached through `Machine::thread_pointer` and handed to safe code by the boundary.
//!
//! **Layout.** Compiled code reads fixed offsets from `%fs` (the x86-64 psABI and GCC
//! conventions, which the spec leaves implicit, ERRATA E7): `%fs:0` holds the thread pointer
//! itself, `%fs:0x28` the stack-protector canary. The block keeps glibc's `tcbhead_t` prefix
//! up to 0x80 so that these, the pointer guard (0x30), the CET word (0x48) and the split-stack
//! word (0x70) are where code expects them. Liella's own fields sit in what glibc reserves as
//! `__glibc_unused2` (0x80–0x280): a block glibc's own `ld.so` laid out is then also a valid
//! Liella block (D-22.10).
//!
//! **Static TLS** (variant II): the executable's TLS block ends at the thread pointer,
//! `round_up(p_memsz, p_align)` bytes below it, as the linker assumes for local-exec accesses.
//! The initial area is decided in D-22.12 (7).

use core::mem::offset_of;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize};

use crate::thread::{DESC_SIZE, KEY_AREA, PAGE, SLOT_AREA};

/// The thread block.
#[repr(C, align(64))]
pub struct ThreadBlock {
    /// 0x00: the thread pointer itself (`%fs:0`).
    pub tcb: AtomicUsize,
    /// 0x08: the dynamic thread vector (phase 14).
    pub dtv: AtomicUsize,
    /// 0x10: the thread pointer again (glibc's `self`).
    pub self_ptr: AtomicUsize,
    /// 0x18: non-zero once the process has more than one thread.
    pub multiple_threads: AtomicU32,
    /// 0x1c: glibc's `gscope_flag` (unused).
    pub gscope: AtomicU32,
    /// 0x20: glibc's `sysinfo` (unused on x86-64).
    pub sysinfo: AtomicUsize,
    /// 0x28: the stack-protector canary (`-fstack-protector`).
    pub stack_guard: AtomicUsize,
    /// 0x30: the pointer guard (pointer mangling).
    pub pointer_guard: AtomicUsize,
    /// 0x38–0x80: glibc's vgetcpu cache, CET `feature_1` (0x48), transactional-memory words,
    /// split-stack limit (0x70) and SSP base: kept zero.
    reserved: [AtomicUsize; 9],
    /// 0x80: this thread's `errno` (spec §2.1, D-2.1).
    pub errno: AtomicI32,
    /// 0x84: the kernel thread ID (`set_tid_address` clears it at exit).
    pub tid: AtomicU32,
    /// 0x88: the head of the C++ `thread_local` destructor list (`__cxa_thread_atexit_impl`).
    pub tls_dtors: AtomicUsize,
    /// 0x90: the thread's allocator cache (phase 6, D-3.10).
    pub heap_cache: AtomicUsize,
    /// 0x98: the thread's NUMA node + 1, 0 if unknown (phase 6, D-3.10).
    pub node: AtomicU32,
    /// 0x9c: Liella flags ([`FLAG_OWN`]).
    pub flags: AtomicU32,
    /// 0xa0: the thread's descriptor (`liella_rt::thread::Thread`), 0 until it has one
    /// (D-22.9; zero in a block glibc made, D-22.10).
    pub thread: AtomicUsize,
    /// 0xa8: the thread's `uselocale` choice, a `locale_t`; 0 for the global locale (D-7.8).
    pub locale: AtomicUsize,
    /// 0xb0: the pointer `__ctype_b_loc` returns the address of: the class table of the thread's
    /// current locale, kept current by `uselocale`, `setlocale` and thread creation (D-5.5).
    pub ctype_b: AtomicUsize,
    /// 0xb8: the pointer `__ctype_toupper_loc` returns the address of.
    pub ctype_toupper: AtomicUsize,
    /// 0xc0: the pointer `__ctype_tolower_loc` returns the address of.
    pub ctype_tolower: AtomicUsize,
    /// 0xc8: the `struct lconv` `localeconv` returns (96 bytes, per thread, D-7.8).
    pub lconv: [AtomicUsize; 12],
    /// 0x128: this thread's `h_errno` (spec §16.3; `__h_errno_location`).
    pub h_errno: AtomicI32,
    /// 0x12c: unused.
    pad: AtomicU32,
    /// 0x130: this thread's resolver state, a `struct __res_state` (`__res_state()`, spec
    /// §16.4; D-16.3): the process's `_res` for the initial thread, a block the library
    /// allocates for any other; 0 until first used.
    pub res: AtomicUsize,
    /// 0x138: the thread's current locale view (`liella::locale::view::LocaleView`), kept with
    /// the ctype words by the same writers; 0 until stored (D-7.13).
    pub view: AtomicUsize,
    /// 0x140: the thread's cache words (TB-SLOTS): one cache line of per-thread caches the
    /// library keeps for speed, never an answer by themselves (each owner validates what it
    /// reads); zero in a fresh block. Read with one `%fs`-relative load (`tb::read`), so a
    /// per-thread cache costs no descriptor lookup. The owners: [`CACHE_TZ_PERIOD`].
    pub cache: [AtomicUsize; CACHE_WORDS],
    /// 0x180–0x300: room for later phases (0x180–0x280 is glibc's reserved area).
    tail: [AtomicUsize; 48],
}

/// The number of [`ThreadBlock::cache`] words.
pub const CACHE_WORDS: usize = 8;
/// `cache[0..3]`: the thread's `localtime` period (P7, `time::Period::to_words`).
pub const CACHE_TZ_PERIOD: usize = 0;

/// The block's size.
pub const BLOCK_SIZE: usize = size_of::<ThreadBlock>();
/// `__rseq_offset`: the rseq area's offset from the thread pointer, the same in every thread
/// (the descriptor follows the thread block, D-22.9).
pub const RSEQ_OFFSET: usize = BLOCK_SIZE + crate::thread::RSEQ_IN_DESC;
const _: () = assert!(BLOCK_SIZE % 64 == 0, "the descriptor sits right after the block");
/// `errno`'s offset from the thread pointer (`__errno_location`).
pub const ERRNO_OFFSET: usize = offset_of!(ThreadBlock, errno);
/// `h_errno`'s offset from the thread pointer (`__h_errno_location`).
pub const H_ERRNO_OFFSET: usize = offset_of!(ThreadBlock, h_errno);
/// The canary's offset (fixed by GCC and Clang).
pub const STACK_GUARD_OFFSET: usize = offset_of!(ThreadBlock, stack_guard);
/// `flags` bit: the runtime set this block up (rather than glibc's `ld.so`).
pub const FLAG_OWN: u32 = 1;

const _: () = {
    assert!(offset_of!(ThreadBlock, tcb) == 0x00);
    assert!(offset_of!(ThreadBlock, self_ptr) == 0x10);
    assert!(offset_of!(ThreadBlock, stack_guard) == 0x28);
    assert!(offset_of!(ThreadBlock, pointer_guard) == 0x30);
    assert!(offset_of!(ThreadBlock, errno) == 0x80);
    assert!(offset_of!(ThreadBlock, flags) == 0x9c);
    assert!(offset_of!(ThreadBlock, thread) == 0xa0);
    assert!(offset_of!(ThreadBlock, locale) == 0xa8);
    assert!(offset_of!(ThreadBlock, ctype_tolower) == 0xc0);
    assert!(offset_of!(ThreadBlock, lconv) == 0xc8);
    assert!(offset_of!(ThreadBlock, h_errno) == 0x128);
    assert!(offset_of!(ThreadBlock, res) == 0x130);
    assert!(offset_of!(ThreadBlock, view) == 0x138);
    assert!(offset_of!(ThreadBlock, cache) == 0x140);
    // One cache line (0x140-0x180), inside glibc's reserved area (a block glibc's `ld.so`
    // made has it zero).
    assert!(size_of::<[AtomicUsize; CACHE_WORDS]>() == 64);
    assert!(offset_of!(ThreadBlock, tail) == 0x180);
    assert!(size_of::<ThreadBlock>() == 0x300);
};

/// The executable's TLS segment (`PT_TLS`), its address already adjusted by the load bias.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlsImage {
    /// The initialisation image.
    pub addr: usize,
    /// Initialised bytes (`.tdata`).
    pub filesz: usize,
    /// Total bytes (`.tdata` + `.tbss`).
    pub memsz: usize,
    /// Alignment (a power of two; 0 or 1 means none).
    pub align: usize,
}

impl TlsImage {
    /// The alignment as a power of two (0 and 1 both mean 1); `None` if it is not one.
    fn alignment(&self) -> Option<usize> {
        let a = self.align.max(1);
        a.is_power_of_two().then_some(a)
    }

    /// The block's size below the thread pointer: `round_up(memsz, align)`.
    pub fn block_size(&self) -> Option<usize> {
        self.memsz.checked_next_multiple_of(self.alignment()?)
    }
}

/// What follows the thread pointer in the initial area: the block, the descriptor, the key
/// area and the result slots (D-22.9).
const TAIL: usize = BLOCK_SIZE + DESC_SIZE + KEY_AREA + SLOT_AREA;

/// The initial thread's memory: the TLS block, then the thread block at an address aligned for
/// both, then (as in every thread's mapping, D-22.9) the descriptor at `tp + BLOCK_SIZE` and the
/// key area after it, so that the rseq area's offset from the thread pointer is the same in
/// every thread (`__rseq_offset`). `len` bytes are mapped at some page-aligned `base`;
/// [`InitialArea::place`] gives the addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitialArea {
    /// Bytes to map.
    pub len: usize,
    /// The TLS block's size (below the thread pointer).
    tls: usize,
    /// The thread pointer's alignment: the TLS alignment, at least the block's 64.
    align: usize,
}

impl InitialArea {
    /// The area for an executable with TLS `image` (or none). `None` if the sizes overflow or
    /// the alignment is not a power of two.
    pub fn new(image: Option<&TlsImage>) -> Option<InitialArea> {
        let (tls, align) = match image {
            None => (0, 64),
            Some(img) if img.filesz > img.memsz => return None,
            Some(img) => (img.block_size()?, img.alignment()?.max(64)),
        };
        // The base is only page-aligned: a stricter alignment may need up to `align - PAGE`
        // bytes more before the thread pointer.
        let slack = align.saturating_sub(PAGE);
        let len = tls.checked_next_multiple_of(align)?.checked_add(slack)?.checked_add(TAIL)?;
        Some(InitialArea { len, tls, align })
    }

    /// (thread pointer, start of the TLS block) for the area mapped at `base`.
    pub fn place(&self, base: usize) -> Option<(usize, usize)> {
        let tp = base.checked_add(self.tls)?.checked_next_multiple_of(self.align)?;
        let start = tp.checked_sub(self.tls)?;
        let end = tp.checked_add(TAIL)?;
        (end <= base.checked_add(self.len)?).then_some((tp, start))
    }
}

/// The canary and pointer guard from the 16 `AT_RANDOM` bytes (spec §1.1). The canary's lowest
/// byte is zero, as glibc makes it, so that a string overrun into it stops there.
pub fn guards(random: &[u8; 16]) -> (usize, usize) {
    let [_, c1, c2, c3, c4, c5, c6, c7, p0, p1, p2, p3, p4, p5, p6, p7] = *random;
    let canary = u64::from_le_bytes([0, c1, c2, c3, c4, c5, c6, c7]);
    let pointer = u64::from_le_bytes([p0, p1, p2, p3, p4, p5, p6, p7]);
    // x86-64: usize is 64 bits.
    (usize::try_from(canary).unwrap_or(0), usize::try_from(pointer).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
    use super::*;

    #[test]
    fn the_area_places_tls_below_an_aligned_thread_pointer() {
        let img = TlsImage { addr: 0x1000, filesz: 10, memsz: 100, align: 16 };
        let a = InitialArea::new(Some(&img)).unwrap();
        let (tp, tls) = a.place(0x7000_0000).unwrap();
        assert_eq!(tp % 64, 0);
        assert_eq!(tp - tls, 112, "round_up(100, 16)");
        assert!(tls >= 0x7000_0000 && tp + BLOCK_SIZE <= 0x7000_0000 + a.len);
        let big = TlsImage { addr: 0, filesz: 0, memsz: 5000, align: 8192 };
        let a = InitialArea::new(Some(&big)).unwrap();
        let (tp, tls) = a.place(0x7000_1000).unwrap();
        assert_eq!(tp % 8192, 0);
        assert_eq!(tp - tls, 8192);
        assert!(tls >= 0x7000_1000 && tp + BLOCK_SIZE <= 0x7000_1000 + a.len);
        assert_eq!(InitialArea::new(Some(&TlsImage { addr: 0, filesz: 0, memsz: 1, align: 3 })), None);
        assert_eq!(InitialArea::new(Some(&TlsImage { addr: 0, filesz: 9, memsz: 1, align: 8 })), None);
        let none = InitialArea::new(None).unwrap();
        assert_eq!(none.place(0x1000).unwrap(), (0x1000, 0x1000));
    }

    #[test]
    fn guards_come_from_at_random_with_a_zero_low_canary_byte() {
        let r: [u8; 16] = core::array::from_fn(|i| u8::try_from(i + 1).unwrap_or(0));
        let (c, p) = guards(&r);
        assert_eq!(c & 0xff, 0);
        assert_eq!(c, 0x0807_0605_0403_0200);
        assert_eq!(p, 0x100f_0e0d_0c0b_0a09);
    }
}
