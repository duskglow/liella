//! `ld.so`'s start (phase 14 DL-5, plan/ph14/design.md §1 and §19, R14-2, D-27.13). Feature `ld`,
//! used only by the `crates/ld-so` product.
//!
//! `liella_machine_ld_entry` (`machine/entry.S`, `-DLIELLA_LD`) calls [`liella_ld_start`] with the
//! initial stack. The steps, in order: (1) relocate the interpreter's own image with the DL-4
//! relocator ([`crate::bootstrap::relocate_image`]); (2) install the provisional thread block
//! (L14-2); (3) read argc/argv and the auxiliary vector; (4) as a program, answer `--version` and
//! `--help` ([`liella_ld::program`]); everything else, and a start by the kernel for a program's
//! `PT_INTERP`, ends with status 127 until DL-6 loads programs.
//!
//! Before step 1 returns nothing here may read a pointer stored in the image's data (design §1):
//! every reference is PC-relative (the product's version script makes every symbol local).

use core::alloc::{GlobalAlloc, Layout};
#[cfg(not(feature = "export"))]
use core::ffi::c_int;
use core::ffi::c_void;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering::Relaxed;

use liella::config::Tunables;
use liella_kernel::auxv::Auxv;
use liella_kernel::uapi::{AT_ENTRY, O_CLOEXEC, O_RDONLY, SEEK_END};
use liella_ld::arch::{Arch, Native};
use liella_ld::program::{self, Action};
use liella_ld::{boot, map, start};
use liella_legacy::search_c::CWords;
use liella_rt::tls::ThreadBlock;

use crate::{conv, raw};
use crate::ffi_sys::{liella_sys_exit_group, liella_sys_write};

/// Words in the provisional thread block: Liella's whole thread block, so every `%fs`-relative
/// field (errno, canary, pointer guard, ...) lands inside it.
const TB_WORDS: usize = core::mem::size_of::<ThreadBlock>().div_ceil(8);

/// The provisional thread block (L14-2): zero in `.bss`, its first word the thread pointer itself
/// once installed. The final block replaces it after the TLS layout (DL-6 onwards).
#[repr(C, align(64))]
struct Provisional([AtomicUsize; TB_WORDS]);

static PROVISIONAL: Provisional = Provisional([const { AtomicUsize::new(0) }; TB_WORDS]);

/// The longest argument read (an option is short; a longer argument is only echoed, cut here).
const ARG_MAX_READ: usize = 4096;

/// Write all of `bytes` to `fd` (short writes continued; errors give up: there is nowhere to report them).
fn write_all(fd: i64, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        let n = liella_sys_write(fd, bytes.as_ptr().cast::<c_void>(), i64::try_from(bytes.len()).unwrap_or(i64::MAX));
        match usize::try_from(n) {
            Ok(n) if n > 0 => bytes = bytes.get(n..).unwrap_or(&[]),
            _ => return,
        }
    }
}

/// End the process with `status`.
fn exit(status: i32) -> ! {
    loop {
        liella_sys_exit_group(i64::from(status));
    }
}

/// The word at `addr` on the initial stack.
fn stack_word(addr: usize) -> u64 {
    // UNSAFE(slice): one word of the initial stack (argc, argv[], envp[], auxv).
    #[allow(unsafe_code)]
    // SAFETY: the kernel laid out argc, argv[] and NULL, envp[] and NULL and the auxiliary vector
    // ending in AT_NULL contiguously from the entry stack pointer (⚑ KERN, execve); callers walk
    // that layout in order and never past its terminators.
    let w = unsafe { conv::words_at(addr, 1) };
    w.first().copied().unwrap_or(0)
}

/// The NUL-terminated string at `addr` (an argv element), without its NUL, at most [`ARG_MAX_READ`] bytes.
fn c_string(addr: usize) -> &'static [u8] {
    let mut len = 0usize;
    while len < ARG_MAX_READ {
        // UNSAFE(slice): one byte of an argument string.
        #[allow(unsafe_code)]
        // SAFETY: argv strings are NUL-terminated in the kernel's copy on the initial stack
        // (⚑ KERN); bytes are read one at a time up to and excluding the NUL.
        let b = unsafe { conv::bytes_at(addr.wrapping_add(len), 1) };
        if b.first().is_none_or(|&b| b == 0) {
            break;
        }
        len = len.wrapping_add(1);
    }
    // UNSAFE(slice): the argument string measured above.
    #[allow(unsafe_code)]
    // SAFETY: the `len` bytes just read one by one are mapped and stay unchanged for the process.
    unsafe { conv::bytes_at(addr, len) }
}

/// End the process with `msg` on standard error and status 127 (design §1 "Failure").
fn refuse(msg: &[u8]) -> ! {
    write_all(2, msg);
    exit(program::FAILURE)
}

/// Copy the auxiliary vector at `auxv` (key/value pairs up to `AT_NULL`) into `copy`; the number of
/// words before the `AT_NULL` pair, or `None` when it does not fit with its `AT_NULL` pair (a
/// malformed vector: the kernel passes about 30 pairs).
fn read_auxv(auxv: usize, copy: &mut [u64; AUXV_WORDS_MAX]) -> Option<usize> {
    let mut words = 0usize;
    loop {
        let key = stack_word(auxv.wrapping_add(words.wrapping_mul(8)));
        if key == 0 {
            return Some(words);
        }
        let value = stack_word(auxv.wrapping_add(words.wrapping_add(1).wrapping_mul(8)));
        let pair = copy.get_mut(words..words.wrapping_add(2)).filter(|_| words.wrapping_add(4) <= AUXV_WORDS_MAX)?;
        pair.copy_from_slice(&[key, value]);
        words = words.wrapping_add(2);
    }
}

/// Write `words` to the initial stack at `at`, one word each.
fn store_words(at: usize, words: &[u64]) {
    // UNSAFE(slice): the initial stack's words (writable).
    #[allow(unsafe_code)]
    // SAFETY: callers write inside the kernel's argv/envp/auxv area: back over the slots they read
    // (or, after secure-mode clearing, a shorter span starting at the same environment array).
    let mut mem = unsafe { conv::ctext() };
    for (i, &w) in words.iter().enumerate() {
        mem.set_word(at, i, usize::try_from(w).unwrap_or(0));
    }
}

/// Secure mode: remove every `rt::start::UNSECURE` entry (all occurrences) from the environment
/// array at `envp`, in place, keeping the order of the rest; the number of entries kept.
fn clear_unsecure(envp: usize) -> usize {
    // UNSAFE(slice): the initial environment array and its strings (writable, on the stack).
    #[allow(unsafe_code)]
    // SAFETY: `envp` is the kernel's NULL-terminated array of NUL-terminated strings.
    let mut mem = unsafe { conv::ctext() };
    let mut kept = 0usize;
    for i in 0.. {
        let p = mem.word(envp, i);
        if p == 0 {
            break;
        }
        if !liella_rt::start::is_unsecure(mem.text(p)) {
            mem.set_word(envp, kept, p);
            kept = kept.wrapping_add(1);
        }
    }
    mem.set_word(envp, kept, 0);
    kept
}

/// The environment entries at `envp` (each without its NUL, at most [`ARG_MAX_READ`] bytes).
fn env_entries(envp: usize) -> impl Iterator<Item = &'static [u8]> {
    (0usize..).map(move |i| stack_word(envp.wrapping_add(i.wrapping_mul(8)))).take_while(|&p| p != 0).map(|p| c_string(usize::try_from(p).unwrap_or(0)))
}

/// The entry of `ld.so` (called by `liella_machine_ld_entry` with the initial stack pointer
/// `sp` and the run-time address `ehdr` of the image's own ELF header, which the entry takes
/// PC-relatively: no call and no GOT slot is needed to find the image before it is relocated).
// UNSAFE(export): the Rust side of the ld entry primitive; local to `ld.so` (version script).
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub extern "C" fn liella_ld_start(sp: usize, ehdr: usize) -> ! {
    // 1. The interpreter's own relocations; nothing before this reads a stored address.
    if crate::bootstrap::relocate_image(ehdr).is_err() {
        refuse(b"ld.so: cannot relocate the interpreter\n");
    }
    // 2. The provisional thread block (L14-2).
    let tb = core::ptr::from_ref(&PROVISIONAL).expose_provenance();
    if let Some(first) = PROVISIONAL.0.first() {
        first.store(tb, Relaxed);
    }
    if crate::arch::set_thread_pointer(tb).is_err() {
        refuse(b"ld.so: cannot set the thread pointer\n");
    }
    // 3. argc, argv, envp, auxv (a copy: the vector moves when the environment shrinks).
    let argc = usize::try_from(stack_word(sp)).unwrap_or(0);
    let argv = sp.wrapping_add(8);
    let envp = argv.wrapping_add(argc.wrapping_add(1).wrapping_mul(8));
    let mut envc = 0usize;
    while stack_word(envp.wrapping_add(envc.wrapping_mul(8))) != 0 {
        envc = envc.wrapping_add(1);
    }
    let mut auxv = envp.wrapping_add(envc.wrapping_add(1).wrapping_mul(8));
    let mut copy = [0u64; AUXV_WORDS_MAX];
    let Some(words) = read_auxv(auxv, &mut copy) else { refuse(b"ld.so: the auxiliary vector is too long\n") };
    // 4. Secure mode, decided once (AT_SECURE only); the UNSECURE names leave the environment before
    // anything below reads an LD_* name (threat model rule 1). The vector then moves down to follow
    // the shorter environment, so the program finds it where the kernel's layout puts it.
    let secure = start::secure(&Auxv::from_words(copy.get(..words).unwrap_or(&[])));
    if secure {
        let kept = clear_unsecure(envp);
        auxv = envp.wrapping_add(kept.wrapping_add(1).wrapping_mul(8));
        store_words(auxv, copy.get(..words.wrapping_add(2)).unwrap_or(&[]));
    }
    // 5. The LD_* settings, the tunables (D-1.8, the secure filter) and the CPU features: the
    // features come from the arch seam only (design §0a); the hwcaps search (DL-8) reads them.
    let ld = start::ld_env(env_entries(envp));
    let _tunables = ld.tunables.map(|t| Tunables::parse(t, secure));
    let _features = Native::features(&Auxv::from_words(copy.get(..words).unwrap_or(&[])), &crate::sys::CSys);
    for name in ld.preload.map(start::preload_names).into_iter().flatten() {
        for part in start::preload_ignored(name) {
            write_all(2, part);
        }
    }
    // 6. The program: the kernel's (started for its PT_INTERP) or, run as a program, the one named.
    #[allow(clippy::as_conversions)] // a function's address has no conversion other than `as`
    let own_entry = (crate::arch::liella_machine_ld_entry as *const ()).expose_provenance();
    let aux = Auxv::from_words(copy.get(..words).unwrap_or(&[]));
    // The page size, from AT_PAGESZ through the arch seam only (DL-7, D-27.15).
    let page = Native::page_size(&aux);
    if aux.get(AT_ENTRY).and_then(|e| usize::try_from(e).ok()) != Some(own_entry) {
        let prog = start::program(&aux).unwrap_or_else(|e| refuse(e.message()));
        let bias = liella_rt::start::program(program_headers(&prog), prog.phnum, prog.phdr).bias;
        run(sp, &prog, bias, page)
    }
    let arg = |i: usize| usize::try_from(stack_word(argv.wrapping_add(i.wrapping_mul(8)))).unwrap_or(0);
    match program::action((1..argc).map(|i| c_string(arg(i)))) {
        Action::Version => {
            for part in program::version::<Native>(env!("CARGO_PKG_VERSION")) {
                write_all(1, part.as_bytes());
            }
            exit(0)
        }
        Action::Help => {
            for part in program::help::<Native>() {
                write_all(1, part.as_bytes());
            }
            exit(0)
        }
        Action::Program(_) => {
            // argv[1] names the program; it becomes argv[0] and the stack starts one word later.
            let (prog, bias) = load(arg(1), page);
            let mut copy = copy;
            start::rewrite(copy.get_mut(..words).unwrap_or(&mut []), &prog, ehdr);
            store_words(auxv, copy.get(..words.wrapping_add(2)).unwrap_or(&[]));
            store_words(argv, &[u64::try_from(argc.wrapping_sub(1)).unwrap_or(0)]);
            run(argv, &prog, bias, page)
        }
        other => {
            for part in program::refusal(other) {
                write_all(2, part);
            }
            exit(program::FAILURE)
        }
    }
}

/// The program headers of `prog` (`AT_PHDR`, `AT_PHNUM` checked by [`start::program`] or set by [`load`]).
fn program_headers(prog: &start::Program) -> &'static [u64] {
    // UNSAFE(slice): the program's headers, mapped with the program.
    #[allow(unsafe_code)]
    // SAFETY: the kernel (`AT_PHDR`/`AT_PHNUM`, ⚑ KERN) or `load` mapped `phnum` 56-byte headers
    // there, readable for the process's lifetime.
    unsafe { conv::words_at(prog.phdr, prog.phnum.saturating_mul(PHDR_WORDS)) }
}

/// Relocate the mapped program `prog` (load bias `bias`), protect its RELRO pages (pages of `page`
/// bytes) and enter it with the initial stack at `sp`. A program with dependencies or with
/// relocations other than RELATIVE/RELR is refused (DL-8 and DL-10 add them); a program without a
/// dynamic section has neither, so nothing is relocated.
fn run(sp: usize, prog: &start::Program, bias: usize, page: u64) -> ! {
    let headers = program_headers(prog);
    let has_dynamic = headers.chunks_exact(PHDR_WORDS).take(prog.phnum).any(|h| h.first().is_some_and(|w| w & 0xffff_ffff == u64::from(liella_ld::elf::pt::DYNAMIC)));
    if has_dynamic {
        let img = boot::image(headers, prog.phnum).unwrap_or_else(|_| refuse(b"ld.so: the program's segments are not relocatable\n"));
        let dynamic = bias.wrapping_add(usize::try_from(img.dynamic.0).unwrap_or(0));
        // UNSAFE(slice): the program's dynamic array (`PT_DYNAMIC`), read only.
        #[allow(unsafe_code)]
        // SAFETY: `dynamic` is the run-time address (bias + `p_vaddr`) of `PT_DYNAMIC`'s `p_memsz` bytes
        // in the mapped program.
        let dyn_words = unsafe { conv::words_at(dynamic, usize::try_from(img.dynamic.1).unwrap_or(0) / 8) };
        if start::needs_objects(dyn_words) {
            refuse(start::NEEDS_OBJECTS);
        }
        let ehdr = bias.wrapping_add(usize::try_from(img.header).unwrap_or(0));
        if crate::bootstrap::relocate_self(prog.phdr, prog.phnum, ehdr).is_err() {
            refuse(b"ld.so: cannot relocate the program (only RELATIVE relocations in this build)\n");
        }
    }
    let page = usize::try_from(page).unwrap_or(usize::MAX);
    if let Some((addr, len)) = liella_rt::start::relro_at(headers, prog.phnum, bias, page) {
        if crate::raw::mprotect(addr, len, map::PROT_READ).is_err() {
            refuse(b"ld.so: cannot protect the program's RELRO segment\n");
        }
    }
    crate::arch::enter(sp, prog.entry)
}

/// `AT_FDCWD` (kernel UAPI `linux/fcntl.h`): a relative path is taken from the current directory.
const AT_FDCWD: i64 = -100;

/// Open, read and map the program file named by the NUL-terminated path at `path` (`ld.so PROGRAM`)
/// with pages of `page` bytes: [`map::load`] (one reservation, each `PT_LOAD` inside it, nothing left
/// mapped on failure). The program and its load bias.
fn load(path: usize, page: u64) -> (start::Program, usize) {
    let fail = |msg: &[u8]| -> ! {
        write_all(2, b"ld.so: ");
        write_all(2, c_string(path));
        write_all(2, b": ");
        refuse(msg)
    };
    let fd = crate::ffi_sys::liella_sys_openat(AT_FDCWD, core::ptr::with_exposed_provenance(path), i64::from(O_RDONLY | O_CLOEXEC), 0);
    if fd < 0 {
        fail(b"cannot open the program\n");
    }
    let size = crate::ffi_sys::liella_sys_lseek(fd, 0, i64::from(SEEK_END));
    let len = usize::try_from(size).ok().filter(|&n| n >= liella_ld::elf::EHDR_SIZE).unwrap_or_else(|| fail(b"cannot read the program\n"));
    let fd32 = i32::try_from(fd).unwrap_or(-1);
    let file_at = raw::mmap(0, len, map::PROT_READ, MAP_PRIVATE_I32, fd32, 0).unwrap_or_else(|_| fail(b"cannot read the program\n"));
    // UNSAFE(slice): the program file, mapped read-only above.
    #[allow(unsafe_code)]
    // SAFETY: `len` bytes from `file_at` are a private read-only mapping of the file, unmapped only
    // after the last use of this slice below.
    let file = unsafe { conv::bytes_at(file_at, len) };
    let loaded = map::load::<Native, _>(&mut SysMapper { fd: fd32 }, file, page);
    let _ = raw::munmap(file_at, len);
    let _ = crate::ffi_sys::liella_sys_close(fd);
    let loaded = loaded.unwrap_or_else(|e| fail(e.message()));
    let at = |v: u64| usize::try_from(v).unwrap_or(0);
    (start::Program { phdr: at(loaded.phdr), phnum: usize::from(loaded.phnum), entry: at(loaded.entry) }, at(loaded.bias))
}

/// The mapper's system calls ([`map::Mapper`]) on one object's file descriptor.
struct SysMapper {
    fd: i32,
}

impl map::Mapper for SysMapper {
    fn reserve(&mut self, at: Option<u64>, len: u64) -> Result<u64, ()> {
        let (hint, flags) = match at {
            Some(a) => (a, MAP_PRIVATE_I32 | MAP_ANONYMOUS_I32 | MAP_FIXED_NOREPLACE_I32),
            None => (0, MAP_PRIVATE_I32 | MAP_ANONYMOUS_I32),
        };
        let r = raw::mmap(word(hint)?, word(len)?, 0, flags, -1, 0).map_err(|_| ())?;
        u64::try_from(r).map_err(|_| ())
    }

    fn map_file(&mut self, at: u64, len: u64, prot: i32, offset: u64) -> Result<(), ()> {
        let off = i64::try_from(offset).map_err(|_| ())?;
        raw::mmap(word(at)?, word(len)?, prot, MAP_PRIVATE_I32 | MAP_FIXED_I32, self.fd, off).map(|_| ()).map_err(|_| ())
    }

    fn map_zero(&mut self, at: u64, len: u64, prot: i32) -> Result<(), ()> {
        raw::mmap(word(at)?, word(len)?, prot, MAP_PRIVATE_I32 | MAP_FIXED_I32 | MAP_ANONYMOUS_I32, -1, 0).map(|_| ()).map_err(|_| ())
    }

    fn store(&mut self, at: u64, len: u64, bytes: Option<&[u8]>) {
        let (Ok(at), Ok(len)) = (word(at), word(len)) else { return };
        // UNSAFE(slice): bytes of a segment page the mapper mapped writable just before this call.
        #[allow(unsafe_code)]
        // SAFETY: `map::load` calls `store` only on [at, at + len) inside a page it mapped private and
        // writable through this mapper, before anything else can see the object.
        let dst = unsafe { conv::bytes_at_mut(at, len) };
        match bytes {
            Some(b) => dst.iter_mut().zip(b).for_each(|(d, s)| *d = *s),
            None => dst.fill(0),
        }
    }

    fn protect(&mut self, at: u64, len: u64, prot: i32) -> Result<(), ()> {
        raw::mprotect(word(at)?, word(len)?, prot).map_err(|_| ())
    }

    fn unmap(&mut self, at: u64, len: u64) {
        if let (Ok(at), Ok(len)) = (word(at), word(len)) {
            let _ = raw::munmap(at, len);
        }
    }
}

/// An address or length as a machine word.
fn word(v: u64) -> Result<usize, ()> {
    usize::try_from(v).map_err(|_| ())
}

/// `mmap` flags as `i32` (kernel UAPI `asm-generic/mman*.h`, `linux/mman.h`).
const MAP_PRIVATE_I32: i32 = 0x02;
const MAP_FIXED_I32: i32 = 0x10;
const MAP_ANONYMOUS_I32: i32 = 0x20;
const MAP_FIXED_NOREPLACE_I32: i32 = 0x10_0000;
/// Words the loader copies of the auxiliary vector (128 pairs, `AT_NULL` included).
const AUXV_WORDS_MAX: usize = 256;
/// Words per program header.
const PHDR_WORDS: usize = liella_rt::start::PHDR_WORDS;

/// The panic handler of `ld.so` (a panic is a loader bug): a fixed message, status 127. It formats
/// nothing, so it works whatever state the image is in.
pub fn panicked(_info: &core::panic::PanicInfo<'_>) -> ! {
    write_all(2, b"ld.so: internal error\n");
    exit(program::FAILURE)
}

/// The global allocator of `ld.so` until the loader has a heap: every request fails (DL-5 code
/// does not allocate; a request reaches `handle_alloc_error`, a panic, status 127).
pub struct NoHeap;

// UNSAFE(allocator): an allocator that never hands out memory.
#[allow(unsafe_code)]
// SAFETY: `alloc` always returns null (allowed: allocation failure) and `dealloc` is only ever
// called with pointers `alloc` returned, of which there are none.
unsafe impl GlobalAlloc for NoHeap {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    // UNSAFE(allocator): nothing to free (see the impl's SAFETY).
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

// The C runtime's memory symbols that compiled Rust code calls, local to `ld.so` (its version
// script). They are the library's own twins (`posix::s04_strings`), over the same machine kernels
// libc uses. Not in a build with `export`, where libc's exports define these names.
macro_rules! ld_mem {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {$(
        #[cfg(not(feature = "export"))]
        // UNSAFE(export): a runtime symbol of the ld.so product (local, never exported).
        #[unsafe(no_mangle)]
        #[allow(unsafe_code)]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        #[doc = concat!("`", stringify!($name), "` inside `ld.so` (local).")]
        pub extern "C" fn $name($($arg: $ty),*) -> $ret {
            crate::posix::s04_strings::$name($($arg),*)
        }
    )*};
}

ld_mem! {
    memcpy(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void;
    memmove(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void;
    memset(d: *mut c_void, c: c_int, n: usize) -> *mut c_void;
    memcmp(a: *const c_void, b: *const c_void, n: usize) -> c_int;
    bcmp(a: *const c_void, b: *const c_void, n: usize) -> c_int;
}
