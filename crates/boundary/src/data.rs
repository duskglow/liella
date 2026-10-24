//! Category *export*: exported data objects (environ, optind, stdin, tzname, ...) as `#[repr(C)]`
//! structs of atomics with the sizes of spec Appendix C.5; no `static mut`. Finding F6.
//!
//! Implementation phase: 4 (implementation-plan.md §6). Phase 2 adds the ctype table pointers
//! (spec §5.1) and the `"."` of `basename`/`dirname`.

/// The static `"."` that `basename`/`dirname` return for an empty path or one without a
/// directory part (D-4.9). Callers must not modify it (spec §4.15).
pub static DOT: [u8; 2] = *b".\0";

// ------------------------------------------------------------------------------ ctype (§5.1)
//
// The table pointers the classification macros index (from -128). glibc keeps them per thread
// and repoints them on `uselocale`; until the thread blocks of phase 4 they are process-wide
// and fixed at the "C" tables (the only locale before phase 7). `__ctype_b_loc` and its
// siblings return the address of these variables; the `__ctype_*` names are also exported
// as the compat data objects of the same names.

use core::sync::atomic::AtomicPtr;

use liella_legacy::ctype_tables::{C_CLASS, C_CLASS32, C_TOLOWER, C_TOUPPER, ZERO};

macro_rules! table_ptr {
    ($(#[$m:meta])* $name:ident: $t:ty = $table:ident) => {
        $(#[$m])*
        // UNSAFE(export): a C data symbol needs an unmangled name; the attribute is unsafe
        // because two symbols of one name would conflict at link time. Names are unique per
        // version node (spec Appendix B).
        #[cfg_attr(feature = "export", unsafe(no_mangle))]
        #[allow(unsafe_code)]
        #[allow(non_upper_case_globals)]
        pub static $name: AtomicPtr<$t> = AtomicPtr::new($table.as_ptr().wrapping_add(ZERO).cast_mut());
    };
}

table_ptr!(
    /// `__ctype_b`: the class table (`_ISbit` masks) at character 0.
    __ctype_b: u16 = C_CLASS
);
table_ptr!(
    /// `__ctype_tolower`: the `tolower` table at character 0.
    __ctype_tolower: i32 = C_TOLOWER
);
table_ptr!(
    /// `__ctype_toupper`: the `toupper` table at character 0.
    __ctype_toupper: i32 = C_TOUPPER
);
table_ptr!(
    /// `__ctype32_b` (compat): the 32-bit class table (`_ISwbit` masks) at character 0.
    __ctype32_b: u32 = C_CLASS32
);
table_ptr!(
    /// `__ctype32_tolower` (compat).
    __ctype32_tolower: i32 = C_TOLOWER
);
table_ptr!(
    /// `__ctype32_toupper` (compat).
    __ctype32_toupper: i32 = C_TOUPPER
);

// ------------------------------------------------------------------------------ §3 objects
//
// The allocator hooks are inert compat data (2.34+; D-3.5): old binaries can read and assign
// them, and nothing calls through them. `__curbrk` is the program break as `brk`/`sbrk` last
// left it. The obstack failure handler is a function pointer C programs may replace.

use core::sync::atomic::{AtomicI32, AtomicUsize};

macro_rules! data_word {
    ($(#[$m:meta])* $name:ident: $t:ty = $init:expr) => {
        $(#[$m])*
        // UNSAFE(export): a C data symbol needs an unmangled name; names are unique per
        // version node (spec Appendix B).
        #[cfg_attr(feature = "export", unsafe(no_mangle))]
        #[allow(unsafe_code)]
        #[allow(non_upper_case_globals)]
        pub static $name: $t = $init;
    };
}

// The compat-only objects below are not exported from here: `libc.a` must not offer them (R15-2),
// and `libc.so.6` gets them at their versions and manifest sizes from the generated
// `csys/src/machine/compat_aliases.S` (LG-6, `xtask/src/compat_objects.rs`). These statics keep
// the in-process tests of their contents.
macro_rules! compat_word {
    ($(#[$m:meta])* $name:ident: $t:ty = $init:expr) => {
        $(#[$m])*
        #[allow(non_upper_case_globals)]
        pub static $name: $t = $init;
    };
}

compat_word!(
    /// `__malloc_hook` (compat, inert).
    __malloc_hook: AtomicUsize = AtomicUsize::new(0)
);
compat_word!(
    /// `__free_hook` (compat, inert).
    __free_hook: AtomicUsize = AtomicUsize::new(0)
);
compat_word!(
    /// `__realloc_hook` (compat, inert).
    __realloc_hook: AtomicUsize = AtomicUsize::new(0)
);
compat_word!(
    /// `__memalign_hook` (compat, inert).
    __memalign_hook: AtomicUsize = AtomicUsize::new(0)
);
compat_word!(
    /// `__after_morecore_hook` (compat, inert).
    __after_morecore_hook: AtomicUsize = AtomicUsize::new(0)
);
compat_word!(
    /// `__malloc_initialize_hook` (compat, inert).
    __malloc_initialize_hook: AtomicUsize = AtomicUsize::new(0)
);
compat_word!(
    /// `__morecore` (compat, inert: the heap never uses `brk`, D-3.7).
    __morecore: AtomicUsize = AtomicUsize::new(0)
);
compat_word!(
    /// `mallwatch` (compat, inert).
    mallwatch: AtomicUsize = AtomicUsize::new(0)
);
data_word!(
    /// `_obstack` (compat pointer, inert).
    _obstack: AtomicUsize = AtomicUsize::new(0)
);
data_word!(
    /// `__curbrk`: the program break `brk`/`sbrk` last set (0 until first used).
    __curbrk: AtomicUsize = AtomicUsize::new(0)
);
data_word!(
    /// `obstack_exit_failure`: the status the default failure handler exits with.
    obstack_exit_failure: AtomicI32 = AtomicI32::new(1)
);
data_word!(
    /// `obstack_alloc_failed_handler`: called when a chunk cannot be allocated. Initially the
    /// default handler, which reports and exits with `obstack_exit_failure`.
    #[allow(clippy::as_conversions)] // a function's address as a C data pointer, in a const initialiser
    obstack_alloc_failed_handler: core::sync::atomic::AtomicPtr<()> = core::sync::atomic::AtomicPtr::new(crate::heap::default_obstack_failure as *mut ())
);

// ------------------------------------------------------------------------------ §14.9 objects
//
// The GNU regex syntax (`re_syntax_options`, `reg_syntax_t`: 0 is `RE_SYNTAX_EMACS`) that
// `re_compile_pattern` and `re_comp` compile with, and `re_max_failures`, the budget of a search
// with back-references (D-14.4; 2000 as observed, `crate::regex`).

data_word!(
    /// `re_syntax_options`.
    re_syntax_options: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0)
);
data_word!(
    /// `re_max_failures`.
    re_max_failures: AtomicI32 = AtomicI32::new(2000)
);

// ------------------------------------------------------------------------------ §20.8 objects
//
// `sys_siglist`, `_sys_siglist` and `sys_sigabbrev` (compat since 2.32): 65 pointers to the
// signal descriptions and abbreviations (`liella_legacy::signal_compat`), NULL for 0 and for
// the numbers without one, as glibc's `GLIBC_2.3.3` objects.

// A `const fn` loop over fixed sizes: `i` stays below 32 and both arrays are larger.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn sig_table(t: &'static [&'static [u8]; 32]) -> [AtomicPtr<u8>; 65] {
    let mut out = [const { AtomicPtr::new(core::ptr::null_mut()) }; 65];
    let mut i = 1;
    while i < 32 {
        out[i] = AtomicPtr::new(t[i].as_ptr().cast_mut());
        i += 1;
    }
    out
}

// Not exported from here, as the malloc hooks above (LG-6: `compat_aliases.S`).
compat_word!(
    /// `sys_siglist` (compat).
    sys_siglist: [AtomicPtr<u8>; 65] = sig_table(&liella_legacy::signal_compat::DESCRIPTIONS)
);
compat_word!(
    /// `_sys_siglist` (compat).
    _sys_siglist: [AtomicPtr<u8>; 65] = sig_table(&liella_legacy::signal_compat::DESCRIPTIONS)
);
compat_word!(
    /// `sys_sigabbrev` (compat).
    sys_sigabbrev: [AtomicPtr<u8>; 65] = sig_table(&liella_legacy::signal_compat::ABBREVIATIONS)
);

// ------------------------------------------------------------------------------ threads (§22)

/// `__libc_single_threaded` (§22.1): non-zero until the process creates a thread.
// UNSAFE(export): a C data symbol needs an unmangled name; unique per version node.
#[cfg_attr(feature = "export", unsafe(no_mangle))]
#[allow(unsafe_code)]
#[allow(non_upper_case_globals)]
pub static __libc_single_threaded: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(1);

/// `__rseq_offset` (§22.9): the rseq area's offset from the thread pointer, in a process
/// Liella started (glibc's `ld.so` defines its own; `libc.so.6` keeps this one local).
// UNSAFE(export): as above.
#[cfg_attr(feature = "runtime", unsafe(no_mangle))]
#[allow(unsafe_code)]
#[allow(non_upper_case_globals)]
pub static __rseq_offset: core::sync::atomic::AtomicIsize = core::sync::atomic::AtomicIsize::new(0);

/// `__rseq_size` (§22.9): the registered area's size, 0 when registration is off or failed.
// UNSAFE(export): as above.
#[cfg_attr(feature = "runtime", unsafe(no_mangle))]
#[allow(unsafe_code)]
#[allow(non_upper_case_globals)]
pub static __rseq_size: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// `__rseq_flags` (§22.9): always 0.
// UNSAFE(export): as above.
#[cfg_attr(feature = "runtime", unsafe(no_mangle))]
#[allow(unsafe_code)]
#[allow(non_upper_case_globals)]
pub static __rseq_flags: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

// ------------------------------------------------------------------------------ §12 objects
//
// The standard streams (D-12.11): `_IO_2_1_stdin_`, `_IO_2_1_stdout_` and `_IO_2_1_stderr_` are
// `FILE` objects with the vtable word (224 bytes, Appendix C.5), set up at first use by
// `crate::stdio`; only `_fileno` is preset. `stdin`, `stdout` and `stderr` are assignable
// pointers to them, read on every implicit use. `_IO_list_all` heads the open-streams list.

use liella_legacy::stdio_compat::{word, FILE_WORDS};

// These objects are exported only by the products (feature `runtime`), not in the test pass
// with exports: the host C library's own code reads the objects of these names (its exit-time
// flush walks `_IO_list_all`), and must keep reading its own.
macro_rules! product_data {
    ($(#[$m:meta])* $name:ident: $t:ty = $init:expr) => {
        $(#[$m])*
        // UNSAFE(export): a C data symbol needs an unmangled name; names are unique per
        // version node (spec Appendix B).
        #[cfg_attr(feature = "runtime", unsafe(no_mangle))]
        #[allow(unsafe_code)]
        #[allow(non_upper_case_globals)]
        pub static $name: $t = $init;
    };
}

/// A `FILE` object as atomic words (C code reads and moves its buffer window).
#[repr(C, align(8))]
pub struct FileObject(pub [AtomicUsize; FILE_WORDS]);

impl FileObject {
    /// A standard stream on descriptor `fd`, not yet set up.
    #[allow(clippy::indexing_slicing)] // a constant index below FILE_WORDS
    const fn standard(fd: usize) -> FileObject {
        let mut w = [const { AtomicUsize::new(0) }; FILE_WORDS];
        w[word::FILENO] = AtomicUsize::new(fd);
        FileObject(w)
    }
}

product_data!(
    /// `_IO_2_1_stdin_`.
    _IO_2_1_stdin_: FileObject = FileObject::standard(0)
);
product_data!(
    /// `_IO_2_1_stdout_`.
    _IO_2_1_stdout_: FileObject = FileObject::standard(1)
);
product_data!(
    /// `_IO_2_1_stderr_`.
    _IO_2_1_stderr_: FileObject = FileObject::standard(2)
);
product_data!(
    /// `stdin`.
    stdin: AtomicPtr<FileObject> = AtomicPtr::new(core::ptr::addr_of!(_IO_2_1_stdin_).cast_mut())
);
product_data!(
    /// `stdout`.
    stdout: AtomicPtr<FileObject> = AtomicPtr::new(core::ptr::addr_of!(_IO_2_1_stdout_).cast_mut())
);
product_data!(
    /// `stderr`.
    stderr: AtomicPtr<FileObject> = AtomicPtr::new(core::ptr::addr_of!(_IO_2_1_stderr_).cast_mut())
);
product_data!(
    /// `_IO_list_all`: the most recently opened stream (0 when none); each `_chain` the next.
    _IO_list_all: AtomicUsize = AtomicUsize::new(0)
);

// The libio jump tables (§12.13, F32, R15-3, IO-VTABLES): read-only objects of the manifest's
// size (`spec/abi/versions.tsv`, 168 bytes), all zero. Liella's stdio never dispatches through
// them and its `FILE` holds no pointer to them, so overwriting a stream's words redirects no
// call (differences row "libio jump tables"). Like the reference's, they lie in read-only
// memory (probe: spec/behaviour/compat-libio.md).
product_data!(
    /// `_IO_file_jumps`: the byte-stream jump table, read-only and never called through.
    _IO_file_jumps: [u8; 168] = [0; 168]
);
product_data!(
    /// `_IO_wfile_jumps`: the wide-stream jump table, read-only and never called through.
    _IO_wfile_jumps: [u8; 168] = [0; 168]
);

product_data!(
    /// `error_message_count` (§2.3): the messages `error` and `error_at_line` wrote.
    error_message_count: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0)
);
product_data!(
    /// `error_one_per_line` (§2.3): non-zero makes `error_at_line` skip a repeated location.
    error_one_per_line: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0)
);
product_data!(
    /// `error_print_progname` (§2.3): a function printing the program-name prefix, or 0.
    error_print_progname: AtomicUsize = AtomicUsize::new(0)
);

product_data!(
    /// `optarg` (§1.6): the last option's argument.
    optarg: AtomicUsize = AtomicUsize::new(0)
);
product_data!(
    /// `optind` (§1.6): the next element to process; 0 restarts the parse.
    optind: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(1)
);
product_data!(
    /// `opterr` (§1.6): zero silences the diagnostics.
    opterr: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(1)
);
product_data!(
    /// `optopt` (§1.6): the option character of the last error.
    optopt: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(63)
);

// ------------------------------------------------------------------------------ §23.7 objects

product_data!(
    /// `getdate_err` (§23.7): the error number of the last failed `getdate` (1–8). Exported
    /// by the products only, so that test builds never write the host C library's object.
    getdate_err: AtomicI32 = AtomicI32::new(0)
);

// ------------------------------------------------------------------------------ §23.6 objects
//
// `tzname`, `timezone` and `daylight` (and their `__` names) are C objects in the products
// (`crate::ffi`, defined by `adapter/runtime.c`, where each pair shares one object). Test builds
// keep private objects here, never exported, so that the host C library's objects of those
// names are neither interposed on nor written.

#[cfg(not(feature = "runtime"))]
mod tz_objects {
    use core::sync::atomic::{AtomicI32, AtomicI64, AtomicUsize};

    /// `__tzname` in test builds (NULL until the first selection).
    pub(super) static TZNAME: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
    /// `__timezone` in test builds.
    pub(super) static TIMEZONE: AtomicI64 = AtomicI64::new(0);
    /// `__daylight` in test builds.
    pub(super) static DAYLIGHT: AtomicI32 = AtomicI32::new(0);
}

/// The time-zone globals `tzset` writes (spec §23.6).
#[cfg(not(feature = "runtime"))]
pub fn tz_globals() -> liella_legacy::time_compat::Globals<'static> {
    liella_legacy::time_compat::Globals { tzname: &tz_objects::TZNAME, timezone: &tz_objects::TIMEZONE, daylight: &tz_objects::DAYLIGHT }
}

/// The time-zone globals `tzset` writes (spec §23.6).
#[cfg(feature = "runtime")]
pub fn tz_globals() -> liella_legacy::time_compat::Globals<'static> {
    liella_legacy::time_compat::Globals { tzname: &crate::ffi::__tzname, timezone: &crate::ffi::__timezone, daylight: &crate::ffi::__daylight }
}

// ------------------------------------------------------------------------------ §15.3 objects

/// A `struct in6_addr` (16 bytes, aligned as its 32-bit view).
#[repr(C, align(4))]
pub struct In6Addr(pub [u8; 16]);

product_data!(
    /// `in6addr_any` (§15.3): `::`.
    in6addr_any: In6Addr = In6Addr([0; 16])
);
product_data!(
    /// `in6addr_loopback` (§15.3): `::1`.
    in6addr_loopback: In6Addr = In6Addr([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])
);

// ------------------------------------------------------------------------------ §15.8 objects

product_data!(
    /// `__check_rhosts_file` (§15.8): nonzero (the default) makes `ruserok` consult `~/.rhosts`.
    __check_rhosts_file: AtomicI32 = AtomicI32::new(1)
);
product_data!(
    /// `__rcmd_errstr` (§15.8): the message of the last failed trust-file check (NULL until one
    /// fails).
    __rcmd_errstr: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut())
);
product_data!(
    /// `rexecoptions` (§15.8): kept for the ABI; nothing reads it.
    rexecoptions: AtomicI32 = AtomicI32::new(0)
);

// ------------------------------------------------------------------------------ §16.3 objects
//
// `h_errlist` and `h_nerr` (no installed header declares them): the texts of `h_errno` 0 to 4,
// as `hstrerror` gives them, and their count. Products only, as the other objects the host C
// library's own code may read.

// A `const fn` loop over a fixed size: `i` stays below 5.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn h_table() -> [AtomicPtr<u8>; 5] {
    let mut out = [const { AtomicPtr::new(core::ptr::null_mut()) }; 5];
    let mut i = 0;
    while i < 5 {
        out[i] = AtomicPtr::new(liella::names::msg::H_ERRLIST[i].as_ptr().cast_mut());
        i += 1;
    }
    out
}

product_data!(
    /// `h_errlist` (§16.3): five pointers to the `h_errno` texts.
    h_errlist: [AtomicPtr<u8>; 5] = h_table()
);
product_data!(
    /// `h_nerr` (§16.3): the entries of `h_errlist`.
    h_nerr: AtomicI32 = AtomicI32::new(5)
);

/// Whether `ruserok` consults `~/.rhosts` (`__check_rhosts_file`).
pub fn check_rhosts() -> bool {
    __check_rhosts_file.load(core::sync::atomic::Ordering::Relaxed) != 0
}

/// Point `__rcmd_errstr` at `m` (a static, terminated message).
pub fn set_rcmd_errstr(m: &'static [u8]) {
    __rcmd_errstr.store(m.as_ptr().cast_mut(), core::sync::atomic::Ordering::Relaxed);
}

// ------------------------------------------------------------------------------ §16.4 objects
//
// `_res`: the initial thread's resolver state (`struct __res_state`, 568 bytes; the compat
// symbol old programs name; `__res_state()` returns its address in that thread, D-16.3).
// `__p_type_syms`, `__p_class_syms` and `_res_opcodes`: the symbol tables of the debugging
// printers. `_res_hconf`: the `host.conf` settings as the reference lays them out (72 bytes:
// the initialized flag at 0, the trim-domain count at 24 and its pointers at 32, the flags at
// 64, `multi` 0x10 and `reorder` 0x08; observed), filled at the first host lookup. Products
// only, as the other objects the host C library's own code may read.

use core::sync::atomic::AtomicU64;

use liella::names::dns::text::{Sym, CLASS_SYMS, OPCODES, TYPE_SYMS};

/// A `struct __res_state` as atomic words.
#[repr(C, align(8))]
pub struct ResObject(pub [AtomicU64; 71]);

product_data!(
    /// `_res` (§16.4): the initial thread's resolver state.
    _res: ResObject = ResObject([const { AtomicU64::new(0) }; 71])
);

/// A `struct res_sym` (number, name, human name).
#[repr(C)]
pub struct ResSym {
    /// `number`.
    pub number: AtomicI32,
    /// `name`.
    pub name: AtomicPtr<u8>,
    /// `humanname`.
    pub human: AtomicPtr<u8>,
}

// A `const fn` loop over a fixed size: `i` stays below N.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn sym_table<const N: usize>(t: &[Sym; N]) -> [ResSym; N] {
    let mut out = [const { ResSym { number: AtomicI32::new(0), name: AtomicPtr::new(core::ptr::null_mut()), human: AtomicPtr::new(core::ptr::null_mut()) } }; N];
    let mut i = 0;
    while i < N {
        let s = &t[i];
        let name = if s.name.len() > 1 { s.name.as_ptr().cast_mut() } else { core::ptr::null_mut() };
        let human = match s.human {
            Some(h) => h.as_ptr().cast_mut(),
            None => core::ptr::null_mut(),
        };
        out[i] = ResSym { number: AtomicI32::new(s.number), name: AtomicPtr::new(name), human: AtomicPtr::new(human) };
        i += 1;
    }
    out
}

// A `const fn` loop over a fixed size: `i` stays below 16.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn opcode_table() -> [AtomicPtr<u8>; 16] {
    let mut out = [const { AtomicPtr::new(core::ptr::null_mut()) }; 16];
    let mut i = 0;
    while i < 16 {
        out[i] = AtomicPtr::new(OPCODES[i].as_ptr().cast_mut());
        i += 1;
    }
    out
}

product_data!(
    /// `__p_type_syms` (§16.4).
    __p_type_syms: [ResSym; 45] = sym_table(&TYPE_SYMS)
);
product_data!(
    /// `__p_class_syms` (§16.4).
    __p_class_syms: [ResSym; 7] = sym_table(&CLASS_SYMS)
);
product_data!(
    /// `_res_opcodes` (§16.4): the opcode names.
    _res_opcodes: [AtomicPtr<u8>; 16] = opcode_table()
);
product_data!(
    /// `_res_hconf` (§16.4): the `host.conf` settings (see above).
    _res_hconf: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9]
);

/// Whether `_res_hconf` was filled.
pub fn hconf_filled() -> bool {
    _res_hconf[0].load(core::sync::atomic::Ordering::Relaxed) != 0
}

/// Record the `host.conf` settings in `_res_hconf` (`multi`).
pub fn set_hconf(multi: bool) {
    use core::sync::atomic::Ordering::Relaxed;
    _res_hconf[0].store(1, Relaxed);
    _res_hconf[8].store(if multi { 0x10 } else { 0 }, Relaxed);
}

// ------------------------------------------------------------------------------ §7.7 objects
//
// The gettext data no installed header declares (sizes from the symbol table, Appendix C.3):
// `_nl_msg_cat_cntr` moves when a setting that selects catalogs changes (`crate::catalog`),
// `_nl_domain_bindings` is NULL until the first binding and then the newest binding record
// (opaque), and the two strings. Products only, as the other objects the host C library's own code
// may read.

product_data!(
    /// `_nl_msg_cat_cntr` (§7.7): the change counter `gettext.h` caching code reads.
    _nl_msg_cat_cntr: AtomicI32 = AtomicI32::new(0)
);
product_data!(
    /// `_nl_domain_bindings` (§7.7): NULL until the first binding.
    _nl_domain_bindings: AtomicUsize = AtomicUsize::new(0)
);
product_data!(
    /// `_nl_default_dirname` (§7.7): the directory of unbound domains.
    _nl_default_dirname: [u8; 18] = liella_legacy::intl::DEFAULT_DIR_TEXT
);
product_data!(
    /// `_libc_intl_domainname` (§7.7): the C library's own domain.
    _libc_intl_domainname: [u8; 5] = liella_legacy::intl::LIBC_DOMAIN_TEXT
);

/// The address of the table `__p_type_syms` (`true`) or `__p_class_syms`.
pub fn sym_table_addr(types: bool) -> usize {
    if types { core::ptr::addr_of!(__p_type_syms).expose_provenance() } else { core::ptr::addr_of!(__p_class_syms).expose_provenance() }
}

// ------------------------------------------------------------------------------ §10 objects
//
// `signgam` (and its 2.23 name `__signgam`): the sign of Γ(x) that `lgamma`, `lgammaf`,
// `lgammal` and `gamma` store (D-10.4; MT-Unsafe by contract). Data of the libm product.

macro_rules! libm_data {
    ($(#[$m:meta])* $name:ident: $t:ty = $init:expr) => {
        $(#[$m])*
        // UNSAFE(export): a C data symbol needs an unmangled name; names are unique per
        // version node (spec Appendix B).
        #[cfg_attr(feature = "libm", unsafe(no_mangle))]
        #[allow(unsafe_code)]
        #[allow(non_upper_case_globals)]
        pub static $name: $t = $init;
    };
}

libm_data!(
    /// `signgam` (§10.3): the sign of Γ(x) of the last `lgamma` call.
    signgam: AtomicI32 = AtomicI32::new(0)
);
libm_data!(
    /// `__signgam` (§10.3, 2.23): the same sign under the reserved name.
    __signgam: AtomicI32 = AtomicI32::new(0)
);
