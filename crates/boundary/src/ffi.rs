//! Category *extern*: declarations of the C layer's non-generated functions
//! (`csys/include/liella_csys.h`): the version probe and the class-*machine* primitives. The
//! class-*sys* wrappers are declared in `ffi_sys.rs`, generated from `csys/syscalls.tsv`.
//!
//! Rules (implementation-plan.md §2.1):
//! * Every item is declared `safe`, so calls need no `unsafe` block.
//! * Items whose parameters are Rust references (`&mut [u32; 4]`, `&X87Env`) are sound for
//!   every argument by construction: a reference is a valid pointer of the right size.
//! * Items taking an address as an integer (`vcall*`) are `pub(crate)` and called only with
//!   addresses the boundary itself validated (the vDSO symbol table). Their proof obligation
//!   is written on each declaration.

use core::ffi::c_void;

use liella_kernel::machine::X87Env;

// Soundness of the declarations: they match liella_csys.h. Items with only integer or reference
// parameters are sound for every argument (they touch registers, or exactly the referenced
// object); the call-at-address items are pub(crate) and documented below.
//
/// A spawn child's entry function (see `liella_machine_clone3_run`).
pub(crate) type SpawnEntry = extern "C" fn(usize) -> !;

/// What `liella_machine_strdiff` returns (`struct liella_strdiff`, two words: rax and rdx).
/// [`liella_machine_env_slot`]'s answer (rax, rdx by the SysV rule for a 16-byte struct of two
/// INTEGER eightbytes; csys's `struct liella_env_slot`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct EnvSlot {
    /// The entry found, or 0.
    pub entry: usize,
    /// The address of its slot, or on a miss of the array's NULL slot (0 when the array is 0).
    pub slot: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct StrDiff {
    /// The first index at which the strings differ or the first one's byte is the terminator.
    pub index: usize,
    /// Nonzero iff a byte of the first string at `0..=index` has bit 7 set.
    pub high: usize,
}

// UNSAFE(extern): Rust cannot call C or assembly without declaring the foreign function.
#[allow(unsafe_code)]
unsafe extern "C" {
    safe fn liella_csys_abi_version() -> u32;

    /// `%fs:0`.
    #[allow(dead_code)] // Rust reads %fs inline since TINY-HOT (tb.rs); the C layer still calls it
    pub(crate) safe fn liella_machine_tp() -> usize;
    /// `rdtsc` (x86_64 only; D-7.12, GETTEXT-6): no arguments, no memory access. Faults (SIGSEGV) only in a process
    /// that disabled the counter with `prctl(PR_SET_TSC)` (a debugger or replay tool; the kernel default allows it).
    #[cfg(target_arch = "x86_64")]
    pub(crate) safe fn liella_machine_rdtsc() -> u64;
    /// `cpuid`; writes exactly the four words of `out`.
    pub(crate) safe fn liella_machine_cpuid(leaf: u32, subleaf: u32, out: &mut [u32; 4]);
    /// `xgetbv(0)`. Faults (`#UD`) unless CPUID.1:ECX.OSXSAVE: callers check first.
    pub(crate) safe fn liella_machine_xgetbv0() -> u64;
    pub(crate) safe fn liella_machine_mxcsr_get() -> u32;
    /// Faults (`#GP`) on bits outside `MXCSR_MASK`: callers mask first.
    pub(crate) safe fn liella_machine_mxcsr_set(value: u32);
    pub(crate) safe fn liella_machine_mxcsr_mask() -> u32;
    pub(crate) safe fn liella_machine_x87_cw_get() -> u32;
    pub(crate) safe fn liella_machine_x87_cw_set(value: u32);
    pub(crate) safe fn liella_machine_x87_sw_get() -> u32;
    pub(crate) safe fn liella_machine_x87_clex();
    /// Writes exactly the 28 bytes of `env`.
    pub(crate) safe fn liella_machine_x87_env_store(env: &mut X87Env);
    /// Reads exactly the 28 bytes of `env`.
    pub(crate) safe fn liella_machine_x87_env_load(env: &X87Env);
    /// `sqrtsd`: no memory access; rounds in MXCSR's direction and raises its flags there.
    pub(crate) safe fn liella_machine_sqrtsd(x: f64) -> f64;
    /// `sqrtss`: as `liella_machine_sqrtsd`.
    pub(crate) safe fn liella_machine_sqrtss(x: f32) -> f32;
    /// `fsqrt` under the current x87 control word: reads the 10 bytes of `x`, writes the 10 of
    /// `out`, leaves the x87 stack as it found it.
    pub(crate) safe fn liella_machine_fsqrt(x: &liella_abi::ctypes::LdBits, out: &mut liella_abi::ctypes::LdBits);
    /// Faults (`#UD`) unless CPUID.7:ECX.OSPKE: callers check first.
    pub(crate) safe fn liella_machine_rdpkru() -> u32;
    /// Same condition; callers also keep key 0 accessible.
    pub(crate) safe fn liella_machine_wrpkru(value: u32);
    /// Does nothing; the pointer is never dereferenced.
    pub(crate) safe fn liella_machine_barrier(p: *const u8);
    /// `mcount`/`__fentry__` test fixtures (machine/mcount.S): the addresses of the fentry site,
    /// the mcount site and their caller.
    pub(crate) safe fn liella_machine_probe_addrs(out: &mut [usize; 3]);
    /// Calls the mcount site (`mcount != 0`) or the fentry site `n` times from the caller above.
    pub(crate) safe fn liella_machine_probe_run(mcount: u32, n: u32);
    /// Calls `f(a, b)`. Proof obligation: `f` is a function in the kernel's vDSO image with
    /// that C signature (looked up by `crate::sys`), and `b` is valid for what it writes.
    pub(crate) safe fn liella_machine_vcall2(f: usize, a: i64, b: *mut c_void) -> i64;
    /// Calls `f(a, b, c)`. Same obligation as `liella_machine_vcall2`.
    pub(crate) safe fn liella_machine_vcall3(f: usize, a: *mut c_void, b: *mut c_void, c: *mut c_void) -> i64;
    /// Calls the C function at `f` with no arguments. Proof obligation: `f` is the address of a
    /// C function the C caller supplied for exactly this use (an `obstack` failure handler),
    /// callable as `void (*)(void)` or with ignored arguments.
    pub(crate) safe fn liella_machine_call0(f: usize) -> i64;
    /// `pthread_once(once, init)` (machine/once.S): runs `init` in its own frame with the once
    /// personality (S11). Proof obligation: `once` is the address of a live `pthread_once_t`
    /// (or 0) and `init` a C function `void (*)(void)` the C caller supplied for it.
    pub(crate) safe fn liella_machine_once(once: usize, init: usize) -> i32;
    /// `body(ctx)` in its own frame (machine/guard.S, OD-6 revised): if a C++ exception or a
    /// forced unwind leaves it, `undo(ctx)` runs during the unwinder's cleanup phase. Proof
    /// obligation: `body` is an `extern "C" fn(usize) -> i64` and `undo` an `extern "C" fn(usize)`
    /// of this library, and `ctx` the address of the live state both expect (`s22_threads::guarded`).
    pub(crate) safe fn liella_machine_guard(body: usize, ctx: usize, undo: usize) -> i64;
    /// Calls the C function at `f` with three integer-class arguments. Proof obligation: `f`
    /// is a C function the C caller supplied for this use (an obstack's `chunkfun`/`freefun`,
    /// a `printf` handler or a registered type's `va_arg` function, D-12.8; an argp parser,
    /// help filter or `argp_program_version_hook`, D-1.7), whose parameters are the leading
    /// integer-class arguments passed, and the pointers passed are valid for what its contract
    /// lets it do with them.
    pub(crate) safe fn liella_machine_call3(f: usize, a: i64, b: i64, c: i64) -> i64;
    /// Calls the C function at `f` with four integer-class arguments. Proof obligation: `f` is
    /// a C function the C caller supplied for this use (a `printf` arginfo function), whose
    /// parameters are the leading integer-class arguments passed, and the arrays passed have
    /// the room the count passed says.
    pub(crate) safe fn liella_machine_call4(f: usize, a: i64, b: i64, c: i64, d: i64) -> i64;
    /// Selects the scanners' path: 0 = SSE2, 1 = AVX2. Proof obligation: 1 only when the CPU
    /// has AVX2 and the OS saves its state (`crate::cpu`).
    pub(crate) safe fn liella_machine_set_scan_level(level: u32);
    /// The scanners' current path (0 = SSE2, 1 = AVX2), for the self-tests (SHORT-3). No obligation.
    pub(crate) safe fn liella_machine_scan_level_get() -> u32;
    /// Bytes before the first NUL at `s` (aligned vector reads, never across a page). Proof
    /// obligation: `s` is non-NULL and a NUL-terminated string (`crate::strz`).
    pub(crate) safe fn liella_machine_strlen(s: *const u8) -> usize;
    /// As `liella_machine_strlen`, at most `max`. Proof obligation: `s` is non-NULL, readable
    /// up to a NUL or `max` bytes.
    pub(crate) safe fn liella_machine_strnlen(s: *const u8, max: usize) -> usize;
    /// The direct `strcpy` entry (machine/scan.S, STRCPY-SHORT-2; the product's `strcpy`): the
    /// one-pass copy for a terminator in the first 64 bytes of `s`'s page, else (and for a NULL)
    /// a tail jump to `liella_hot_strcpy_rest`. Returns `d`.
    /// Proof obligation (the `strcpy` twin, posix/s04_strings.rs): the `strcpy` contract.
    pub(crate) safe fn liella_machine_strcpy(d: *mut u8, s: *const u8) -> *mut u8;
    /// The direct `strncpy` entry (machine/scan.S, STRNCPY-SHORT; the product's `strncpy`): one
    /// pass (copy and pad in the same stores) for 1 <= `n` <= 64 with the first 64 bytes of `s`
    /// in its page, else (and for a NULL) a tail jump to `liella_hot_strncpy_rest`. Returns `d`.
    /// Proof obligation (the `strncpy` twin, posix/s04_strings.rs): the `strncpy` contract.
    pub(crate) safe fn liella_machine_strncpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8;
    /// The direct `__strncpy_chk` entry (machine/scan.S, STRNCPY-FAMILY): `n` <= `destlen` checked
    /// before any store, then the `strncpy` body; else (and for a NULL) a tail jump to
    /// `liella_hot_strncpy_chk_rest`. Returns `d`.
    /// Proof obligation (the `__strncpy_chk` twin, posix/s04_strings.rs): the fortify contract.
    pub(crate) safe fn liella_machine_strncpy_chk(d: *mut u8, s: *const u8, n: usize, destlen: usize) -> *mut u8;
    /// The direct `stpncpy` entry (machine/scan.S, STRNCPY-FAMILY): the `strncpy` body returning
    /// `d` + min(strlen(`s`), `n`); else (and for a NULL) a tail jump to `liella_hot_stpncpy_rest`.
    /// Proof obligation (the `stpncpy` twin, posix/s04_strings.rs): the `stpncpy` contract.
    pub(crate) safe fn liella_machine_stpncpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8;
    /// The direct `__stpncpy_chk` entry (machine/scan.S, STPNCPY-ALIAS): `n` <= `destlen` checked
    /// before any store, then the `stpncpy` body; else (and for a NULL) a tail jump to
    /// `liella_hot_stpncpy_chk_rest`. Returns `d` + min(strlen(`s`), `n`).
    /// Proof obligation (the `__stpncpy_chk` twin, posix/s04_strings.rs): the fortify contract.
    pub(crate) safe fn liella_machine_stpncpy_chk(d: *mut u8, s: *const u8, n: usize, destlen: usize) -> *mut u8;
    /// The direct `stpcpy`/`__stpcpy` entry (machine/scan.S, STRCPY-FAMILY): as `liella_machine_strcpy`,
    /// returning the end pointer; the rest is `liella_hot_stpcpy_rest`. Proof obligation: the
    /// `stpcpy` contract.
    pub(crate) safe fn liella_machine_stpcpy(d: *mut u8, s: *const u8) -> *mut u8;
    /// The direct `__stpcpy_chk` entry: `len + 1 <= room` checked before any store. Proof
    /// obligation: the fortify contract (`d` has room for `room` bytes).
    pub(crate) safe fn liella_machine_stpcpy_chk(d: *mut u8, s: *const u8, room: usize) -> *mut u8;
    /// The direct `__strcpy_chk` entry: as `liella_machine_stpcpy_chk`, returning `d`; the rest is
    /// `liella_hot_strcpy_chk_rest`. Proof obligation: the fortify contract.
    pub(crate) safe fn liella_machine_strcpy_chk(d: *mut u8, s: *const u8, room: usize) -> *mut u8;
    /// A pointer to the first byte `c` at `s` (STR-K2: a pointer, not an index, so `rawmemchr`
    /// tail-calls it). Proof obligation: `s` is non-NULL and readable up to and including such a
    /// byte.
    pub(crate) safe fn liella_machine_rawmemchr(s: *const u8, c: i32) -> *const u8;
    /// The direct rawmemchr entry (machine/scan.S, SCAN-TAIL; `rawmemchr` and `__rawmemchr` in
    /// the products): a NULL `s` goes to `strz::find` (abort), any other enters
    /// `liella_machine_rawmemchr`. Proof obligation (`crate::strz::find_direct`): the rawmemchr
    /// contract (`s` readable up to and including a byte `c`).
    pub(crate) safe fn liella_machine_rawmemchr_entry(s: *const u8, c: i32) -> *const u8;
    /// A pointer to the first byte `c` in `[s, s + n)`, or NULL (STR-K3: a pointer, so `memchr`
    /// tail-calls it). Proof obligation: `s` is non-NULL, `1 <= n <= isize::MAX` (the kernel
    /// reads `s`'s page without testing `n`), and the bytes of `[s, s + n)` are readable up to the first `c`
    /// (C17 7.24.5.1p2; `crate::strz::find_n`).
    pub(crate) safe fn liella_machine_memchr(s: *const u8, c: i32, n: usize) -> *const u8;
    /// The direct memchr entry (machine/scan.S, MEMCHR-FOLD; `memchr` in the products): as
    /// `liella_machine_memchr` behind one gate. Proof obligation (`crate::strz::find_n_direct`):
    /// the `memchr` contract (`s` readable up to the first `c` or `n` bytes). NULL folds for
    /// `1 <= n <= isize::MAX` only while NULL folding is on (`crate::dispatch`: the first load of
    /// every path lies in `[s & -64, s + 64)`, page zero for a NULL `s`); every other case goes to
    /// `strz::find_n`. In assembly because a NULL access is UB in Rust.
    pub(crate) safe fn liella_machine_memchr_entry(s: *const u8, c: i32, n: usize) -> *const u8;
    /// The index of the first byte where `[a, a + n)` and `[b, b + n)` differ, or `n` (loads
    /// stay inside both). Proof obligation: both are readable for `n` bytes (from two `&[u8]`:
    /// `crate::conv::compare`).
    pub(crate) safe fn liella_machine_memcmp(a: *const u8, b: *const u8, n: usize) -> usize;
    /// The order of the strings at `a` and `b` as `strcmp` defines it, -1, 0 or 1, in one pass
    /// (loads never leave the page holding an unended byte; the bytes deciding the order are
    /// read again and a writer's change rescanned in the kernel, D-4.29). Proof obligation: `a`
    /// and `b` are non-NULL NUL-terminated strings (`crate::strz::order`).
    pub(crate) safe fn liella_machine_strcmp(a: *const u8, b: *const u8) -> i32;
    /// The direct strcmp entry: the S6 NULL tests, then the kernel (STRCMP-ENTRY, machine/scan.S).
    pub(crate) safe fn liella_machine_strcmp_entry(a: *const u8, b: *const u8) -> i32;
    /// The first index at which the strings at `a` and `b` differ or `a`'s byte is the
    /// terminator, and whether a byte of `a` up to it has bit 7 set ([`StrDiff`]; loads never
    /// leave the page holding an unended byte, D-4.37). Proof obligation: `a` and `b` are
    /// non-NULL NUL-terminated strings (`crate::strz::coll`).
    pub(crate) safe fn liella_machine_strdiff(a: *const u8, b: *const u8) -> StrDiff;
    /// The first byte equal to `c` at `s`, or NULL if the terminator comes first, in one pass
    /// (SHORT-1, D-4.35; aligned 16-byte blocks, never across a page). Proof obligation: `s` is
    /// NULL (the kernel takes the S6 abort through `liella_hot_strchr_null`, STR-CHR-CMP-SHORT) or a
    /// NUL-terminated string (`crate::strz::chr`).
    pub(crate) safe fn liella_machine_strchr(s: *const u8, c: i32) -> *const u8;
    /// As `liella_machine_strchr`, but the terminator's address when `c` does not occur.
    pub(crate) safe fn liella_machine_strchrnul(s: *const u8, c: i32) -> *const u8;
    /// A pointer to the last byte `c` in `[s, s + n)`, or NULL (SHORT-1, D-4.35). Proof
    /// obligation: `s` is non-NULL, `1 <= n <= isize::MAX`, and `[s, s + n)` is readable
    /// (`crate::strz::rfind_n`).
    pub(crate) safe fn liella_machine_memrchr(s: *const u8, c: i32, n: usize) -> *const u8;
    /// The direct memrchr entry (machine/scan.S, SCAN-RAW; `memrchr` in the products): `n == 0`,
    /// `n > isize::MAX` and a NULL `s` go to `strz::rfind_n` (NULL or abort), any other call
    /// enters `liella_machine_memrchr`. Proof obligation (`crate::strz::rfind_n_direct`): the
    /// memrchr contract (`s` readable for `n` bytes).
    pub(crate) safe fn liella_machine_memrchr_entry(s: *const u8, c: i32, n: usize) -> *const u8;
    /// The order of at most `n` bytes of the strings at `a` and `b` as `strncmp` defines it,
    /// -1, 0 or 1, in one pass (SHORT-1, D-4.35). Proof obligation: `n == 0` (reads nothing), or each
    /// of `a` and `b` is NULL (the S6 abort through `liella_hot_strncmp_null`, STR-CHR-CMP-SHORT) or
    /// readable up to its terminator or `n` bytes (`crate::strz::order_n`).
    pub(crate) safe fn liella_machine_strncmp(a: *const u8, b: *const u8, n: usize) -> i32;
    /// How often `liella_machine_strcmp` rescanned after a writer's change (selftest).
    #[cfg(feature = "selftest")]
    pub(crate) safe fn liella_machine_strcmp_rescan_count() -> u64;
    /// Selftest, AVX2 only (D-4.38): `vzeroupper`, then `f(a, b)` with no code in between, its
    /// result stored in `out`; returns the OR of the upper halves of ymm0..ymm15 afterwards (0 =
    /// `f` left no upper state behind). `f` gets `a` and `b` unchanged.
    #[cfg(feature = "selftest")]
    pub(crate) safe fn liella_machine_clean_upper_call(
        f: extern "C" fn(*const u8, *const u8) -> i32,
        a: *const u8,
        b: *const u8,
        out: &mut i32,
    ) -> u64;
    /// Selftest: sets ymm15 to all ones and returns 0 (the positive control for
    /// `liella_machine_clean_upper_call`; reads nothing).
    #[cfg(feature = "selftest")]
    pub(crate) safe fn liella_machine_ymm15_dirty_cmp(a: *const u8, b: *const u8) -> i32;
    /// The order of the first `n` bytes of `a` and `b` after folding `A`-`Z` (STRCASECMP-SHORT,
    /// STRCASECMP-SHORT-2): -1, 0 or 1, for the locale `loc` (a `&Locale` whose case map below 128
    /// is ASCII). One AVX2 kernel (the caller checks `cpu::casecmp_kernel`); when the deciding bytes
    /// hold one of 128 or more it tail-jumps to `liella_hot_casecmp_rest` (`strz::casecmp_rest`),
    /// which answers for `loc`; so does a NULL `a` or `b` (PERF-STRCASECMP-AVX2: the kernel tests
    /// them, not its caller). Proof obligation: each non-NULL one readable up to a NUL or `n`
    /// bytes, `n != 0`.
    pub(crate) safe fn liella_machine_casecmp_ord_avx2(a: *const u8, b: *const u8, n: usize, loc: *const core::ffi::c_void) -> i32;
    /// [`liella_machine_casecmp_ord_avx2`] with SSE2 only (every x86-64 CPU): the SSE2 index kernel,
    /// then the deciding bytes re-read and ordered; a changed text also goes to the rest entry.
    pub(crate) safe fn liella_machine_casecmp_ord_sse2(a: *const u8, b: *const u8, n: usize, loc: *const core::ffi::c_void) -> i32;
    /// The first index `i < count` with `h[i] == b0` and `h[i + off] == b1`, or `count`
    /// (FIND-SEQ-AVX2; the candidate scan of `conv::find_slice`). Proof obligation: `count >= 16`
    /// and `[h, h + count + off)` is readable (from a `&[u8]` of that length). Loads stay inside it.
    pub(crate) safe fn liella_machine_pairscan(h: *const u8, count: usize, off: usize, b0: i32, b1: i32) -> usize;
    /// `liella_machine_pairscan` with an ASCII fold (STRSTR-FIND, `strcasestr`): bit 0 / bit 1 of `fl` OR
    /// 0x20 into the haystack byte compared with `b0` / `b1` (the caller passes them lowered and sets a bit
    /// only for an ASCII letter). Same proof obligation as `liella_machine_pairscan`.
    pub(crate) safe fn liella_machine_pairscan_ci(h: *const u8, count: usize, off: usize, b0: i32, b1: i32, fl: u32) -> usize;
    /// The fused candidate and terminator scan of `strstr` (STRSTR-VEC): `2k` when `p + k` is the
    /// first position q at or after `p` with byte q == `c1` and byte q - `off` == `c0` and no NUL
    /// in [p, q]; `2k + 1` when `p + k` is the first NUL at or after `p` and no candidate precedes
    /// it. Proof obligation: the `off` bytes before `p` are string bytes before its NUL and `p` is
    /// at or before that NUL (`crate::strz::find_str`). Loads past the NUL stay in its page.
    pub(crate) safe fn liella_machine_strpair(p: *const u8, off: usize, c0: i32, c1: i32) -> usize;
    /// `liella_machine_strpair` over 4-byte units (`wcsstr`; `off` and the answer in units).
    /// Proof obligation: as `liella_machine_strpair` in units, and `p` 4-byte aligned
    /// (`crate::strz::find_wcs`).
    pub(crate) safe fn liella_machine_wcspair(p: *const i32, off: usize, c0: i32, c1: i32) -> usize;
    /// The address of the first entry of the NULL-terminated array `env` (0: none) that is the
    /// `len` bytes at `name` followed by `=`, or 0 (byte loop, never past an entry's NUL).
    /// Proof obligation: `env` is 0 or the address of a NULL-terminated array of pointers to
    /// NUL-terminated strings (the `environ` contract), and `name` is readable for `len` bytes
    /// (from a `&[u8]`: `crate::conv::env_find`).
    pub(crate) safe fn liella_machine_env_find(env: usize, name: *const u8, len: usize) -> usize;
    /// `liella_machine_env_find`'s scan, also answering the slot it stopped at ([`EnvSlot`]:
    /// the hit's slot, or the NULL slot on a miss; ENVIRON-SMALL-CYCLE). Proof obligation: as
    /// `liella_machine_env_find` (from a `&[u8]` without NUL: `CText::env_find_end`).
    pub(crate) safe fn liella_machine_env_slot(env: usize, name: *const u8, len: usize) -> EnvSlot;
    /// `rep movsb`; returns `dst`. Proof obligation: `dst` and `src` are valid for `n` bytes and
    /// disjoint (the arguments come from a `&mut [u8]` and a `&[u8]`: `crate::conv::copy`).
    pub(crate) safe fn liella_machine_copy_erms(dst: *mut u8, src: *const u8, n: usize) -> *mut u8;
    /// One unaligned 64-byte store of the head, then `rep movsb` from the next 64-byte-aligned
    /// destination (MEMCPY-POLICY-2); returns `dst`. Proof obligation: `n >= 64`, and `dst` and
    /// `src` are valid for `n` bytes and disjoint (`crate::hot::avx2_erms`, `conv::copy_large`).
    pub(crate) safe fn liella_machine_copy_erms_a64(dst: *mut u8, src: *const u8, n: usize) -> *mut u8;
    /// The AVX2 copy (machine/scan.S). Proof obligation: the CPU has AVX2 and the OS saves the
    /// YMM state (`crate::cpu::avx2`), `n >= 64`, and `dst` and `src` are valid for `n` bytes
    /// and disjoint (from a `&mut [u8]` and a `&[u8]`: `crate::conv::copy_units`).
    pub(crate) safe fn liella_machine_copy_avx2(dst: *mut u8, src: *const u8, n: usize);
    /// The same AVX2 copy for regions that may overlap (MEM-BULK; machine/scan.S, a second name
    /// for `liella_machine_copy_avx2`): `n` bytes from `s` to `d`; returns `d`. Proof obligation:
    /// the CPU has AVX2 and the OS saves the YMM state (`crate::dispatch::avx2` or
    /// `crate::cpu::copy_avx2`), `n >= 64`, `d` valid for writing and `s` for reading `n` bytes
    /// (the `memmove` contract after its NULL tests: `crate::hot::move_rest`,
    /// `crate::conv::move_regions`; the internal copy's contract: `crate::hot::internal_large`).
    pub(crate) safe fn liella_machine_move_avx2(d: *mut u8, s: *const u8, n: usize) -> *mut u8;
    /// The SSE2 build of the same copy (MEM-FALLBACK; machine/scan.S), for CPUs without AVX2:
    /// `n` bytes from `s` to `d`, overlap allowed; returns `d`. SSE2 is part of the x86-64
    /// baseline. Proof obligation: `n >= 64`, `d` valid for writing and `s` for reading `n` bytes
    /// (the `memmove` contract after its NULL tests: `crate::hot::move_large`,
    /// `crate::conv::move_regions`; the internal copy's contract: `crate::hot::internal_large`).
    pub(crate) safe fn liella_machine_move_sse2(d: *mut u8, s: *const u8, n: usize) -> *mut u8;
    /// The AVX2 fill (MEM-FALLBACK; machine/scan.S), for AVX2 CPUs where `rep stosb` is not
    /// chosen: `n` bytes at `d` set to `c`; returns `d`. Proof obligation: the CPU has AVX2 and
    /// the OS saves the YMM state (`crate::dispatch::avx2`), `n >= 128`, and `d` valid for writing
    /// `n` bytes (from a `&mut [u8]`: `crate::conv::fill`; or the `memset` contract after its NULL
    /// test: `crate::hot::fill_large`).
    pub(crate) safe fn liella_machine_fill_avx2(d: *mut u8, c: i32, n: usize) -> *mut u8;
    /// `rep stosb`. Proof obligation: `dst` is valid for writing `n` bytes (from a
    /// `&mut [u8]`: `crate::conv::fill`; or the `memset` contract after its NULL test, for a
    /// range inside the caller's region: `crate::hot::fill_rest`).
    pub(crate) safe fn liella_machine_fill_erms(dst: *mut u8, c: i32, n: usize);
    /// The direct copy entry (machine/fold.S, D-4.22 COPY-FILL; `memcpy` and `memmove` in the
    /// products): `n` bytes from `s` to `d`, overlap allowed; returns `d`. Proof obligation
    /// (`crate::hot::move_bytes`): the `memmove` contract (`d`, `s` valid for `n` bytes). NULL
    /// folds only for 1-64 while NULL folding is on (`crate::dispatch`: page zero cannot be
    /// mapped); every other size goes to `hot::move_rest`. In assembly because a NULL access is
    /// UB in Rust.
    /// The fold limit read by the direct entries (machine/fold.S): 64 while NULL folding is on,
    /// else 0; written only by `crate::dispatch` (its proof obligation).
    pub(crate) safe static liella_machine_fold_limit: core::sync::atomic::AtomicUsize;
    /// The memchr entry's gate (machine/fold.S, MEMCHR-FOLD): `isize::MAX` while NULL folding is
    /// on, else 0; written only by `crate::dispatch` (its proof obligation), with the fold limit.
    pub(crate) safe static liella_machine_chr_limit: core::sync::atomic::AtomicUsize;
    /// The kernels' AVX2 flag (machine/fold.S, D-4.25), read by the 65-256 copy classes there;
    /// written only by `crate::dispatch::select` (its proof obligation: `true` only while the CPU
    /// has AVX2 and the OS saves the YMM state).
    pub(crate) safe static liella_machine_avx2: core::sync::atomic::AtomicBool;
    /// The compare entries' 33-64 class word (machine/fold.S, MEMCMP-EQ64, SEQ-REGRESS): 32 only
    /// while NULL folding is on AND the AVX2 builds run, else 0; written only by `crate::dispatch`
    /// (its proof obligation: never nonzero while the fold limit is 0 or the AVX2 flag is false).
    pub(crate) safe static liella_machine_cmp_wide: core::sync::atomic::AtomicUsize;
    /// The compare entries' 8-32 (8-64 without AVX2) class word (machine/fold.S, SEQ-REGRESS): 25
    /// with folding and AVX2, 57 with folding alone, 0 with folding off; written only by
    /// `crate::dispatch` (proof obligation: never nonzero while the fold limit is 0, and
    /// `cmp_wide + cmp_low <= 57` at every moment).
    pub(crate) safe static liella_machine_cmp_low: core::sync::atomic::AtomicUsize;
    /// The compare entries' 65-128 class word (machine/fold.S, MEMCMP-MID-3): 128 while the AVX2
    /// builds run, else 0; sizes above 64 with `n <= word` take the AVX2 65-128 class. Written only
    /// by `crate::dispatch::select` (proof obligation: never above 64 while the AVX2 flag is false).
    pub(crate) safe static liella_machine_cmp_mid: core::sync::atomic::AtomicUsize;
    /// The compare entries' 129-2048 class word (machine/fold.S, MEMCMP-MID-4): `hot::CMP_LONG`
    /// while the AVX2 builds run, else 0; sizes above the 65-128 word with `n <= word` take the
    /// AVX2 long class (which re-tests `n <= 128` itself). Written only by `crate::dispatch::select`
    /// (proof obligation: never nonzero while the AVX2 flag is false).
    pub(crate) safe static liella_machine_cmp_long: core::sync::atomic::AtomicUsize;
    pub(crate) safe fn liella_machine_memmove(d: *mut u8, s: *const u8, n: usize) -> *mut u8;
    /// The direct `mempcpy` entry (machine/fold.S, WL-HOT-SEQ2; `mempcpy` and `__mempcpy` in the
    /// products): the copy of `liella_machine_memmove`, returning `d + n`. Proof obligation
    /// (`crate::hot::move_bytes_past`): as `liella_machine_memmove`.
    pub(crate) safe fn liella_machine_mempcpy(d: *mut u8, s: *const u8, n: usize) -> *mut u8;
    /// The 65-256 copy classes of the direct entry without its NULL and range tests
    /// (machine/fold.S, COPY-256), for the library's internal copy: `n` bytes from `s` to `d`,
    /// overlap allowed; returns nothing. Proof obligation (`crate::hot::internal_move`):
    /// `65 <= n <= 256`, `d` non-NULL and valid for writing and `s` for reading `n` bytes.
    pub(crate) safe fn liella_machine_move_mid(d: *mut u8, s: *const u8, n: usize);
    /// The direct fill entry (`memset` in the products): `n` bytes at `d` set to `c as u8`;
    /// returns `d`. Proof obligation as `liella_machine_memmove`, for `d` alone; other sizes go
    /// to `hot::fill_rest`.
    pub(crate) safe fn liella_machine_memset(d: *mut u8, c: i32, n: usize) -> *mut u8;
    /// The direct compare entry (machine/fold.S, MEMCMP-FOLD; `memcmp` in the products): -1, 0 or
    /// 1 for the order of `n` bytes at `a` against `n` bytes at `b`. Proof obligation
    /// (`crate::hot::compare_direct`): the `memcmp` contract (`a`, `b` readable for `n` bytes).
    /// NULL folds only for 8-64 while NULL folding is on (`crate::dispatch`); every other case goes
    /// to `hot::compare_rest`. In assembly because a NULL access is UB in Rust.
    pub(crate) safe fn liella_machine_compare(a: *const u8, b: *const u8, n: usize) -> i32;
    /// The direct equality entry (machine/fold.S, MEMCMP-FOLD; `bcmp` and `__memcmpeq` in the
    /// products): 0 when equal, else 1. Proof obligation as `liella_machine_compare`
    /// (`crate::hot::equal_direct`); other cases go to `hot::equal_rest_entry`.
    pub(crate) safe fn liella_machine_equal(a: *const u8, b: *const u8, n: usize) -> i32;
    /// The trap instruction (`hlt`): faults in user mode; returns only if that is survived.
    pub(crate) safe fn liella_machine_trap();
    /// `clone(2)` for a thread (machine/thread_start.S): the new thread pops an entry function
    /// and its argument from `stack` and calls it. Proof obligation: `stack` is the 16-aligned
    /// top of a fresh, writable stack holding those two words (`crate::thread::create` wrote
    /// them); `tls` is the new thread's block, set up for it; `ptid` and `ctid` are its TID
    /// word, which outlives the thread (the mapping is reused only once the kernel cleared it).
    pub(crate) safe fn liella_machine_clone_thread(flags: u64, stack: usize, ptid: &core::sync::atomic::AtomicU32, ctid: &core::sync::atomic::AtomicU32, tls: usize) -> i64;
    /// `clone(2)` for `clone(3)` (the same code as `liella_machine_clone_thread`). Proof
    /// obligation: `stack` holds the function and argument words (`crate::fork::clone` wrote
    /// them) in the C caller's stack; `ptid`, `ctid` and `tls` are the C caller's values, which
    /// the kernel uses only as `flags` asks (the caller's contract, spec §18.2).
    pub(crate) safe fn liella_machine_clone_raw(flags: u64, stack: usize, ptid: usize, ctid: usize, tls: usize) -> i64;
    /// `clone3(2)` for a spawn child that shares the caller's memory (machine/thread_start.S,
    /// D-18.3): the child runs `entry(arg)` on the stack `args` names and never returns from it.
    /// Proof obligation (`crate::raw::spawn_clone_vm`): `args` is a `clone3` argument block
    /// whose `stack`/`stack_size` name a private, writable, 16-aligned stack that outlives the
    /// child's exec; `arg` is the address of a context that outlives the child (the caller is
    /// suspended by `CLONE_VFORK` until then); `entry` neither returns nor touches state the
    /// suspended caller relies on.
    pub(crate) safe fn liella_machine_clone3_run(args: *const c_void, size: i64, entry: SpawnEntry, arg: usize) -> i64;
    /// A cancellation point's system call inside the cancellation window (D-22.11): returns
    /// `-ECANCELED` without an effect when the calling thread's word (`word`) says the request
    /// is due, else the kernel's result. Proof obligation: as for any system call made through
    /// `ffi_sys` (the arguments are the call's own, prepared by `crate::sys`).
    pub(crate) safe fn liella_machine_syscall_cancel(word: &core::sync::atomic::AtomicU32, nr: i64, a: i64, b: i64, c: i64, d: i64, e: i64, f: i64) -> i64;
    /// As [`liella_machine_syscall_cancel`] for system calls of at most four arguments, all in
    /// registers (CANCEL-FAST): the same window. Proof obligation: the same.
    pub(crate) safe fn liella_machine_syscall_cancel4(word: &core::sync::atomic::AtomicU32, nr: i64, a: i64, b: i64, c: i64, d: i64) -> i64;
    /// The direct cancellation points (machine/points.S, CANCEL-FAST-2; `read`, `write`,
    /// `pread`/`pread64`, `pwrite`/`pwrite64`, `close`/`__close` in the products): the C
    /// function's result, `errno` set on failure, a due request acted on. Proof obligation
    /// (`crate::posix::s11_lowio`, the exports' Rust twins): the C contract of the function
    /// (the buffer valid for `n` bytes); the entries pass every other case to the Rust path.
    pub(crate) safe fn liella_machine_read(fd: i32, buf: *mut c_void, n: usize) -> isize;
    pub(crate) safe fn liella_machine_write(fd: i32, buf: *const c_void, n: usize) -> isize;
    pub(crate) safe fn liella_machine_pread(fd: i32, buf: *mut c_void, n: usize, off: i64) -> isize;
    pub(crate) safe fn liella_machine_pwrite(fd: i32, buf: *const c_void, n: usize, off: i64) -> isize;
    pub(crate) safe fn liella_machine_close(fd: i32) -> i32;
    /// Set once any asynchronous I/O starts (machine/points.S): `close` then takes the Rust path,
    /// which cancels the descriptor's requests. Written only by `crate::aio::mark_used`.
    pub(crate) safe static liella_machine_aio_used: core::sync::atomic::AtomicBool;
    /// The cancellation window's start, end and cancelled exit (addresses), for the internal
    /// signal's handler.
    pub(crate) safe fn liella_machine_cancel_bounds(out: &mut [usize; 3]);
    /// `siglongjmp`: restores the mask the buffer saved, then the registers. Proof obligation:
    /// `env` was filled by `liella_machine_sigsetjmp` (through `setjmp`, `__sigsetjmp` or
    /// `__sigsetjmp_cancel` in C) in a frame that has not returned, and no Rust frame with a
    /// destructor lies between here and there (`crate::cancel`, `__longjmp_chk`).
    pub(crate) safe fn liella_machine_siglongjmp(env: usize, val: i32) -> !;
    /// `_longjmp`: as `liella_machine_siglongjmp`, without the mask. Same obligation.
    #[allow(dead_code)] // reached only from C (`_longjmp`, machine/setjmp.S)
    pub(crate) safe fn liella_machine_longjmp_nomask(env: usize, val: i32) -> !;
    /// Where a `makecontext` function returns to. Never called from Rust; only its address is
    /// taken.
    pub(crate) safe fn liella_machine_start_context();
    /// The next argument of a `va_list` as a register-sized value (`va_arg(*ap, long)`).
    /// Proof obligation: `ap` is the `va_list *` a C adapter passed for this call (it lives
    /// until the adapter returns), and no more arguments are read than the C caller passed
    /// (the count it gave, e.g. `makecontext`'s `argc`).
    pub(crate) safe fn liella_va_long(ap: usize) -> i64;
    /// The next argument of a `va_list` as a `double`. Obligation: as for `liella_va_long`,
    /// and the argument is a `double` (the format says so).
    pub(crate) safe fn liella_va_double(ap: usize) -> f64;
    /// The next argument of a `va_list` as a `long double`, stored at `out`. Obligation: as for
    /// `liella_va_double`, for a `long double`.
    pub(crate) safe fn liella_va_ldouble(ap: usize, out: &mut crate::va_source::LongDouble);
    /// Addresses of a static executable's constructor and destructor lists and of `_init` and
    /// `_fini`, 0 where the link provides none (weak references in `adapter/runtime.c`).
    /// Used by the products' runtime only (feature `runtime`).
    #[cfg_attr(not(feature = "runtime"), allow(dead_code))]
    pub(crate) safe fn liella_link_preinit_start() -> usize;
    #[cfg_attr(not(feature = "runtime"), allow(dead_code))]
    pub(crate) safe fn liella_link_preinit_end() -> usize;
    #[cfg_attr(not(feature = "runtime"), allow(dead_code))]
    pub(crate) safe fn liella_link_init_start() -> usize;
    #[cfg_attr(not(feature = "runtime"), allow(dead_code))]
    pub(crate) safe fn liella_link_init_end() -> usize;
    #[cfg_attr(not(feature = "runtime"), allow(dead_code))]
    pub(crate) safe fn liella_link_fini_start() -> usize;
    #[cfg_attr(not(feature = "runtime"), allow(dead_code))]
    pub(crate) safe fn liella_link_fini_end() -> usize;
    #[cfg_attr(not(feature = "runtime"), allow(dead_code))]
    pub(crate) safe fn liella_link_init() -> usize;
    #[cfg_attr(not(feature = "runtime"), allow(dead_code))]
    pub(crate) safe fn liella_link_fini() -> usize;
    /// The value of `__fpu_control` (D-10.5): the program's definition when it has one.
    #[cfg_attr(not(feature = "runtime"), allow(dead_code))]
    pub(crate) safe fn liella_link_fpu_control() -> u16;
    /// The bounds of a static executable's `.rela.iplt` (`__rela_iplt_start`/`_end`, its
    /// `R_X86_64_IRELATIVE` entries), 0 where the link defines none.
    #[cfg_attr(not(feature = "runtime"), allow(dead_code))]
    pub(crate) safe fn liella_link_rela_iplt_start() -> usize;
    #[cfg_attr(not(feature = "runtime"), allow(dead_code))]
    pub(crate) safe fn liella_link_rela_iplt_end() -> usize;
    /// The address of the ELF header of the image the library is linked into (`__ehdr_start`),
    /// 0 where the link defines none (`backtrace`, D-2.7).
    pub(crate) safe fn liella_link_ehdr() -> usize;
    /// The start of a static executable's `.eh_frame` (the start object's label), 0 in a link
    /// without Liella's start object.
    pub(crate) safe fn liella_link_eh_frame_start() -> usize;
    /// The terminator after the last `.eh_frame` input section of the link.
    pub(crate) safe fn liella_link_eh_frame_end() -> usize;
    /// `getcontext`'s capture into `uc`, a `ucontext_t` (968 bytes, spec/abi/layouts.tsv): the
    /// caller's general registers, stack pointer and return address, the x87 environment, MXCSR
    /// and the signal mask; returns 0. Soundness: the primitive writes only inside the
    /// `ucontext_t` at `uc` (machine/context.S: below offset 0x1c4), which the exclusive
    /// reference covers.
    pub(crate) safe fn liella_machine_getcontext(uc: &mut [u64; 121]) -> i32;
    /// The `SA_RESTORER` trampoline. Never called from Rust; only its address is taken.
    pub(crate) safe fn liella_machine_restorer();
}

/// ABI version of the linked C syscall layer (`LIELLA_CSYS_ABI_VERSION` in `liella_csys.h`).
pub fn csys_abi_version() -> u32 {
    liella_csys_abi_version()
}

// The C data objects of spec §1.3 and §1.4: `char **environ` and the two `char *` program names.
// In the products `adapter/runtime.c` defines them (with their alias names); in test builds the
// host C library does, and these names reach its objects (D-2.1). They are pointer-sized words
// the C side reads and writes as plain pointers; Rust touches them only through these atomics,
// and the contract makes concurrent modification a data race in C (MT-Unsafe `env`).
//
// UNSAFE(extern): Rust cannot name a C data object without declaring it.
#[allow(unsafe_code)]
unsafe extern "C" {
    /// `environ` (`__environ`, `_environ`): the address of the `NULL`-terminated entry array.
    pub safe static environ: core::sync::atomic::AtomicUsize;
    /// `program_invocation_name` (`__progname_full`).
    pub safe static program_invocation_name: core::sync::atomic::AtomicUsize;
    /// `program_invocation_short_name` (`__progname`).
    pub safe static program_invocation_short_name: core::sync::atomic::AtomicUsize;
}

// The time-zone globals of spec §23.6: `__tzname` (`tzname`), `__timezone` (`timezone`) and
// `__daylight` (`daylight`), names sharing objects, which Rust statics cannot express. In the
// products `adapter/runtime.c` defines them; test builds use the private objects of
// `crate::data` instead, so that Liella's `tzset` never writes the host C library's (whose own
// code reads them). C programs read them and may write them; Rust writes them only through
// these atomics.
#[cfg(feature = "runtime")]
// UNSAFE(extern): Rust cannot name a C data object without declaring it.
#[allow(unsafe_code)]
unsafe extern "C" {
    /// `__tzname` (`tzname`): the standard and DST abbreviations, as `char *`.
    pub safe static __tzname: [core::sync::atomic::AtomicUsize; 2];
    /// `__timezone` (`timezone`): the standard offset, seconds west of UTC.
    pub safe static __timezone: core::sync::atomic::AtomicI64;
    /// `__daylight` (`daylight`): whether the zone has DST.
    pub safe static __daylight: core::sync::atomic::AtomicI32;
}

// argp's program-wide variables (spec §1.7). In the products `adapter/runtime.c` defines them
// weakly, so that a program's own definitions take their place; in test builds the host C
// library's objects are reached. Pointer-sized words (a `const char *` or a function address)
// and an `int`, which C programs set before parsing; Rust only reads them.
//
// UNSAFE(extern): Rust cannot name a C data object without declaring it.
#[allow(unsafe_code)]
unsafe extern "C" {
    /// `argp_program_version`: the `--version` text, or NULL.
    pub safe static argp_program_version: core::sync::atomic::AtomicUsize;
    /// `argp_program_version_hook`: a `void (*)(FILE *, struct argp_state *)` printing the
    /// version, or NULL.
    pub safe static argp_program_version_hook: core::sync::atomic::AtomicUsize;
    /// `argp_program_bug_address`: the bug-report address, or NULL.
    pub safe static argp_program_bug_address: core::sync::atomic::AtomicUsize;
    /// `argp_err_exit_status`: the exit status after a usage error (`EX_USAGE`, 64).
    pub safe static argp_err_exit_status: core::sync::atomic::AtomicI32;
}

// `__libc_stack_end` (spec §1.1), defined by `adapter/runtime.c` in the products only.
//
// UNSAFE(extern): Rust cannot name a C data object without declaring it.
#[cfg(feature = "runtime")]
#[allow(unsafe_code)]
unsafe extern "C" {
    /// The initial stack's end, as `_start` passed it.
    pub safe static __libc_stack_end: core::sync::atomic::AtomicUsize;
}
