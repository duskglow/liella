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
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use core::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};

use liella::config::Tunables;
use liella_kernel::auxv::Auxv;
use liella_kernel::uapi::{AT_ENTRY, AT_EXECFN, AT_RANDOM, AT_SYSINFO_EHDR, O_CLOEXEC, O_RDONLY, SEEK_END};
use liella_ld::arch::{Arch, Native};
use liella_ld::program::{self, Action};
use liella_ld::cache::Cache;
use liella_ld::search::{self, Ctx, PathBuf, Scope};
use liella_ld::{boot, elf, init, map, start};
use liella_ld::tls::{self as ltls, Area, Module, Placed};
use liella_legacy::search_c::CWords;
use liella_rt::tls::{ThreadBlock, FLAG_OWN};

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

/// The run-time address of the interpreter's own ELF header (its load bias: `ld.so` is linked at 0),
/// stored once its image is relocated.
static OWN_BASE: AtomicUsize = AtomicUsize::new(0);

/// The TLS generation (design §7): the dtv of every thread was built for it; loading or unloading a
/// module with TLS after start (DL-15/DL-16) moves it on. 0 for the initially loaded objects.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// `__rseq_offset` (§22.9, `GLIBC_2.35` of `ld.so`): the rseq area's offset from the thread pointer
/// (`liella_rt::tls::RSEQ_OFFSET`, the same in every thread), published with [`__rseq_size`].
// UNSAFE(export): an exported data object of the ld.so product.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
#[allow(non_upper_case_globals)]
pub static __rseq_offset: core::sync::atomic::AtomicIsize = core::sync::atomic::AtomicIsize::new(0);

/// `__rseq_size` (§22.9): the size of the area the loader registered for the initial thread, 0 when
/// registration is off (`glibc.pthread.rseq=0`) or the kernel refused it.
// UNSAFE(export): an exported data object of the ld.so product.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
#[allow(non_upper_case_globals)]
pub static __rseq_size: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// `__rseq_flags` (§22.9): always 0.
// UNSAFE(export): an exported data object of the ld.so product.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
#[allow(non_upper_case_globals)]
pub static __rseq_flags: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Whether the tunables leave rseq registration on (`glibc.pthread.rseq`, D-22.7), read at start.
static RSEQ_WANTED: AtomicBool = AtomicBool::new(true);

/// The bytes above the initial thread's pointer (D-27.26): the thread block, then room up to the
/// end of the rseq area at `RSEQ_OFFSET`, where the C library's descriptor puts it in every other
/// thread, so `__rseq_offset` is one value for all threads.
#[allow(clippy::as_conversions)] // const context: `try_from` is not const
const INITIAL_TAIL: u64 = {
    let end = (liella_rt::tls::RSEQ_OFFSET + liella_rt::thread::RSEQ_SIZE as usize).next_multiple_of(64) as u64;
    if end > Native::TCB_SIZE { end } else { Native::TCB_SIZE }
};
const _: () = assert!(liella_rt::tls::RSEQ_OFFSET % 32 == 0, "the rseq area is 32-byte aligned");

/// Register the initial thread's rseq area at `tp + RSEQ_OFFSET` (inside the area [`map_area`]
/// mapped, zero-filled) and publish `__rseq_offset`/`__rseq_size`: before relocation, so a
/// program's copy of them reads the final values. The registered size (0: none).
fn register_rseq(tp: usize) -> usize {
    if !RSEQ_WANTED.load(Relaxed) {
        return 0;
    }
    let at = tp.wrapping_add(liella_rt::tls::RSEQ_OFFSET);
    let size = liella_rt::thread::RSEQ_SIZE;
    let ok = crate::ffi_sys::liella_sys_rseq(core::ptr::with_exposed_provenance_mut(at), i64::from(size), 0, i64::from(liella_kernel::uapi::RSEQ_SIG)) == 0;
    if !ok {
        return 0;
    }
    __rseq_offset.store(isize::try_from(liella_rt::tls::RSEQ_OFFSET).unwrap_or(0), Release);
    __rseq_size.store(size, Release);
    usize::try_from(size).unwrap_or(0)
}

// The thread-block slots the arch table names are the fields of Liella's block (D-4.15).
#[allow(clippy::as_conversions)] // const context: no `try_from`; the values are small offsets
const _: () = {
    assert!(ltls::DTV_HEADER == 2);
    assert!(Native::TCB_SIZE as usize == core::mem::size_of::<ThreadBlock>());
    assert!(Native::DTV_SLOT as usize == core::mem::offset_of!(ThreadBlock, dtv));
    assert!(matches!(Native::SELF_SLOT, Some(0)));
    assert!(matches!(Native::STACK_GUARD_SLOT, Some(s) if s as usize == core::mem::offset_of!(ThreadBlock, stack_guard)));
    assert!(matches!(Native::POINTER_GUARD_SLOT, Some(s) if s as usize == core::mem::offset_of!(ThreadBlock, pointer_guard)));
};

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
    OWN_BASE.store(ehdr, Relaxed);
    // The canary and the pointer guard from AT_RANDOM (spec §1.1), kept in the provisional block's
    // slots; the final thread block takes them from there (DL-12).
    if let Some(r) = Auxv::from_words(copy.get(..words).unwrap_or(&[])).get(AT_RANDOM).and_then(|a| usize::try_from(a).ok()).filter(|&a| a != 0) {
        // UNSAFE(slice): the 16 AT_RANDOM bytes.
        #[allow(unsafe_code)]
        // SAFETY: the kernel put 16 random bytes at AT_RANDOM on the initial stack (⚑ KERN, execve),
        // live for the process's lifetime.
        let bytes = unsafe { conv::bytes_at(r, 16) };
        let mut random = [0u8; 16];
        random.copy_from_slice(bytes);
        let (canary, pointer) = liella_rt::tls::guards(&random);
        for (slot, v) in [(Native::STACK_GUARD_SLOT, canary), (Native::POINTER_GUARD_SLOT, pointer)] {
            if let Some(w) = slot.and_then(|s| PROVISIONAL.0.get(usize::try_from(s / 8).unwrap_or(usize::MAX))) {
                w.store(v, Relaxed);
            }
        }
    }
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
    let tunables = ld.tunables.map(|t| Tunables::parse(t, secure));
    RSEQ_WANTED.store(tunables.is_none_or(|t| t.rseq != Some(false)), Relaxed);
    let features = Native::features(&Auxv::from_words(copy.get(..words).unwrap_or(&[])), &crate::sys::CSys);
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
    // LD_TRACE_LOADED_OBJECTS lists the dependencies instead of running (ldd); never in secure mode
    // (threat model rule 11).
    let list_env = List { ld: &ld, secure, features: &features, page, vdso: aux.get(AT_SYSINFO_EHDR), base: ehdr };
    if aux.get(AT_ENTRY).and_then(|e| usize::try_from(e).ok()) != Some(own_entry) {
        if ld.trace && !secure {
            let execfn = aux.get(AT_EXECFN).and_then(|a| usize::try_from(a).ok()).unwrap_or(0);
            list(&list_env, if execfn == 0 { b"" } else { c_string(execfn) }, false)
        }
        let prog = start::program(&aux).unwrap_or_else(|e| refuse(e.message()));
        // The bias from PT_PHDR or, without one, the ELF header in the page holding the program
        // headers (ruling #4371, D-27.18).
        let page_bytes = usize::try_from(page).unwrap_or(usize::MAX);
        let header = |at: usize| {
            let mut out = [0u8; 64];
            read_image(at, &mut out);
            out
        };
        let bias = start::bias(program_headers(&prog), prog.phnum, prog.phdr, page_bytes, header)
            .unwrap_or_else(|| refuse(b"ld.so: cannot find the program's load bias (no PT_PHDR, and no ELF header before its program headers)\n"));
        run(sp, &prog, bias, page, 0)
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
        Action::Program(name) => {
            if ld.trace && !secure {
                list(&list_env, name, false)
            }
            // argv[1] names the program; it becomes argv[0] and the stack starts one word later.
            let (prog, bias) = load(arg(1), page);
            let mut copy = copy;
            start::rewrite(copy.get_mut(..words).unwrap_or(&mut []), &prog, ehdr);
            store_words(auxv, copy.get(..words.wrapping_add(2)).unwrap_or(&[]));
            store_words(argv, &[u64::try_from(argc.wrapping_sub(1)).unwrap_or(0)]);
            run(argv, &prog, bias, page, arg(1))
        }
        Action::List(name) => list(&list_env, name, true),
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
fn run(sp: usize, prog: &start::Program, bias: usize, page: u64, path: usize) -> ! {
    let headers = program_headers(prog);
    // R14-13: a program whose PT_GNU_STACK asks for an executable stack is refused (word 0 of a header
    // holds p_type in its low and p_flags in its high half).
    let types = headers.chunks_exact(PHDR_WORDS).take(prog.phnum).filter_map(|h| h.first()).map(|&w| (lo32(w), lo32(w >> 32)));
    if map::executable_stack(types) {
        refuse(b"ld.so: the program asks for an executable stack (PT_GNU_STACK with PF_X); refused\n");
    }
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
            // The program's name in messages: its path (`ld.so PROGRAM`), else argv[0].
            let at = if path != 0 { path } else { usize::try_from(stack_word(sp.wrapping_add(8))).unwrap_or(0) };
            let fini = link(prog, bias, page, if at != 0 { c_string(at) } else { b"the program" }, sp);
            crate::arch::enter(sp, prog.entry, fini)
        }
        let ehdr = bias.wrapping_add(usize::try_from(img.header).unwrap_or(0));
        if crate::bootstrap::relocate_self(prog.phdr, prog.phnum, ehdr).is_err() {
            refuse(b"ld.so: cannot relocate the program (only RELATIVE relocations in this build)\n");
        }
    }
    let page = usize::try_from(page).unwrap_or(usize::MAX);
    // Segment by segment (D-27.25): a reservation page between two writable segments stays as mapped.
    if liella_rt::start::relro_pieces(headers, prog.phnum, bias, page, |addr, len| crate::raw::mprotect(addr, len, map::PROT_READ)).is_err() {
        refuse(b"ld.so: cannot protect the program's RELRO segment\n");
    }
    // The program alone: its own TLS block (if any) and the final thread block (DL-12).
    let one = [program_tls(headers, prog.phnum)];
    let mut placed = [None];
    let area = ltls::layout::<Native>(&one, &mut placed, Native::TCB_SIZE, ltls::SURPLUS).unwrap_or_else(|_| refuse(b"ld.so: the program's TLS segment is malformed\n"));
    install_tls(&one, &[u64::try_from(bias).unwrap_or(0)], &placed, &area, map_area(&area));
    crate::arch::enter(sp, prog.entry, 0)
}

/// The initial thread's area for `area`, mapped read-write and zero-filled: its `area.align`-aligned
/// base (mapped before relocation, so the rseq area in it is registered and published first).
fn map_area(area: &Area) -> usize {
    let fail = || -> ! { refuse(b"ld.so: cannot set up the initial thread's TLS\n") };
    let (Ok(len), Ok(align)) = (usize::try_from(area.len), usize::try_from(area.align)) else { fail() };
    let prot = i32::try_from(liella_kernel::uapi::PROT_READ | liella_kernel::uapi::PROT_WRITE).unwrap_or(0);
    let flags = i32::try_from(liella_kernel::uapi::MAP_PRIVATE | liella_kernel::uapi::MAP_ANONYMOUS).unwrap_or(0);
    let Some(map_len) = len.checked_add(align) else { fail() };
    let Ok(raw_base) = raw::mmap(0, map_len, prot, flags, -1, 0) else { fail() };
    raw_base.checked_next_multiple_of(align).unwrap_or_else(|| fail())
}

/// The program's `PT_TLS` from its header words, refused unless its image lies in the file part of a
/// `PT_LOAD` (word 0 of a header holds `p_type`; words 2, 4, 5, 6 `p_vaddr`, `p_filesz`, `p_memsz`,
/// `p_align`).
fn program_tls(headers: &[u64], phnum: usize) -> Option<Module> {
    let hs = || headers.chunks_exact(PHDR_WORDS).take(phnum);
    let w = |h: &[u64], i: usize| h.get(i).copied().unwrap_or(0);
    let t = hs().find(|h| lo32(w(h, 0)) == liella_ld::elf::pt::TLS)?;
    let m = Module { vaddr: w(t, 2), filesz: w(t, 4), memsz: w(t, 5), align: w(t, 6) };
    let end = m.vaddr.checked_add(m.filesz);
    let inside = hs().any(|h| lo32(w(h, 0)) == liella_ld::elf::pt::LOAD && w(h, 2) <= m.vaddr && end.is_some_and(|e| e <= w(h, 2).saturating_add(w(h, 4))));
    if !inside {
        refuse(b"ld.so: the program's TLS image lies outside its segments\n");
    }
    Some(m)
}

/// Build the initial thread's area (design §7, D-27.20): map `area`, copy each placed module's
/// initialisation image from its loaded (and relocated) object (`biases[i]` + `p_vaddr`, `p_filesz`
/// bytes; the rest of the block stays zero), fill the thread block (self, dtv, the canary and pointer
/// guard from the provisional block, `FLAG_OWN`) and the dtv, and make it the thread pointer.
/// In the area [`map_area`] mapped at `base`.
fn install_tls(mods: &[Option<Module>], biases: &[u64], placed: &[Option<Placed>], area: &Area, base: usize) {
    let fail = || -> ! { refuse(b"ld.so: cannot set up the initial thread's TLS\n") };
    let tp = base.wrapping_add(usize::try_from(area.tp).unwrap_or(0));
    let tp64 = u64::try_from(tp).unwrap_or(0);
    for ((m, p), b) in mods.iter().zip(placed).zip(biases) {
        let (Some(m), Some(p)) = (m, p) else { continue };
        let (Ok(src), Ok(n), Ok(dst)) = (usize::try_from(b.wrapping_add(m.vaddr)), usize::try_from(m.filesz), usize::try_from(tp64.wrapping_add_signed(p.tp_offset))) else { fail() };
        if n == 0 {
            continue;
        }
        // UNSAFE(slice): a module's TLS initialisation image in its loaded object.
        #[allow(unsafe_code)]
        // SAFETY: `ltls::image_in_load` / `program_tls` checked that [p_vaddr, p_vaddr + p_filesz) lies
        // in the file part of one of the object's PT_LOADs, mapped readable at bias + p_vaddr.
        let image = unsafe { conv::bytes_at(src, n) };
        // UNSAFE(slice): the module's block in the area mapped above.
        #[allow(unsafe_code)]
        // SAFETY: `ltls::layout` placed the block's `p_memsz` >= `p_filesz` bytes inside the area's
        // `len` bytes from `base`, mapped read-write above and not otherwise referenced yet.
        unsafe { conv::bytes_at_mut(dst, n) }.copy_from_slice(image);
    }
    let dtv_at = base.wrapping_add(usize::try_from(area.dtv).unwrap_or(0));
    // DTV_HEADER (2, asserted above) words, then a slot per object.
    let mut dtv = [0u64; 2 + OBJECTS_MAX];
    let Some(dtv) = dtv.get_mut(..usize::try_from(area.dtv_words).unwrap_or(usize::MAX)) else { fail() };
    ltls::fill_dtv(dtv, GENERATION.load(Relaxed), tp64, placed);
    // UNSAFE(slice): the dtv in the area mapped above.
    #[allow(unsafe_code)]
    // SAFETY: `ltls::layout` reserved `dtv_words` words at `area.dtv` (8-aligned) inside the area.
    let out = unsafe { conv::bytes_at_mut(dtv_at, dtv.len().wrapping_mul(8)) };
    for (o, w) in out.chunks_exact_mut(8).zip(dtv.iter()) {
        o.copy_from_slice(&w.to_ne_bytes());
    }
    // UNSAFE(slice): the thread block at the new thread pointer.
    #[allow(unsafe_code)]
    // SAFETY: `ltls::layout` keeps `TCB_SIZE` bytes from `area.tp` for the block, inside the area
    // mapped above (zero-filled, 64-aligned as `area.align` >= `TCB_ALIGN`).
    let tb = unsafe { conv::bytes_at_mut(tp, core::mem::size_of::<ThreadBlock>()) };
    let mut put = |off: Option<u64>, v: u64| {
        if let Some(w) = off.and_then(|o| usize::try_from(o).ok()).and_then(|o| tb.get_mut(o..o.wrapping_add(8))) {
            w.copy_from_slice(&v.to_ne_bytes());
        }
    };
    let provisional = |slot: Option<u64>| slot.and_then(|s| PROVISIONAL.0.get(usize::try_from(s / 8).unwrap_or(usize::MAX))).map_or(0, |w| w.load(Relaxed));
    put(Native::SELF_SLOT, tp64);
    put(u64::try_from(core::mem::offset_of!(ThreadBlock, self_ptr)).ok(), tp64);
    put(Some(Native::DTV_SLOT), u64::try_from(dtv_at).unwrap_or(0));
    put(Native::STACK_GUARD_SLOT, u64::try_from(provisional(Native::STACK_GUARD_SLOT)).unwrap_or(0));
    put(Native::POINTER_GUARD_SLOT, u64::try_from(provisional(Native::POINTER_GUARD_SLOT)).unwrap_or(0));
    let flags_at = core::mem::offset_of!(ThreadBlock, flags);
    if let Some(w) = tb.get_mut(flags_at..flags_at.wrapping_add(4)) {
        w.copy_from_slice(&FLAG_OWN.to_ne_bytes());
    }
    if crate::arch::set_thread_pointer(tp).is_err() {
        fail()
    }
}

/// `__libc_stack_end` (spec §1.1, §27.4; exported by `ld.so` at `GLIBC_2.2.5`, R14-12, D-27.20):
/// the initial stack pointer, stored by [`link`] with the rseq objects before any object is
/// relocated, so a program's COPY relocation of it carries the value (D-27.26). `libc.so.6` keeps its
/// own local copy, which its start publishes from the same stack pointer (`StartInfo::stack_end`);
/// `libc.a` keeps its own definition for static programs. Unmangled only in the `ld.so` product: a
/// build with `runtime` takes the C library's definition (`adapter/runtime.c`).
// UNSAFE(export): a C data symbol needs an unmangled name; the loader defines it once.
#[cfg_attr(not(feature = "runtime"), unsafe(no_mangle))]
#[allow(unsafe_code)]
#[allow(non_upper_case_globals)]
pub static __libc_stack_end: AtomicUsize = AtomicUsize::new(0);

/// `__tls_get_addr` (spec §22.6, s22_6_c_4; exported by `ld.so` at `GLIBC_2.3`): the address of
/// `ti->offset` in module `ti->module`'s block for the calling thread. `ti` points at a `tls_index`
/// (two words: module ID, offset; x86-64 psABI §3.4.6). DL-12: static modules only; any other answer
/// is a failure (the dlopen family, DL-15/DL-16, adds the slow path).
// UNSAFE(export): the exported `__tls_get_addr` of the ld.so product.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub extern "C" fn __tls_get_addr(ti: usize) -> usize {
    // Only a block this loader made has its dtv word (D-5.11 safety, design §7): never read the
    // word of a block another loader made as a pointer.
    if crate::tb::read32::<crate::tb::Flags>() & FLAG_OWN == 0 {
        refuse(b"ld.so: __tls_get_addr called on a thread block this loader did not make\n");
    }
    let dtv = crate::tb::read::<crate::tb::Dtv>();
    // UNSAFE(slice): the caller's `tls_index`.
    #[allow(unsafe_code)]
    // SAFETY: the psABI caller passes the address of a two-word `tls_index` in its GOT (⚑ ABI).
    let index = unsafe { conv::words_at(ti, 2) };
    let (module, offset) = (index.first().copied().unwrap_or(0), index.get(1).copied().unwrap_or(0));
    let word = |i: u64| -> u64 {
        let at = dtv.wrapping_add(usize::try_from(i).unwrap_or(0).wrapping_mul(8));
        // UNSAFE(slice): one word of the calling thread's dtv.
        #[allow(unsafe_code)]
        // SAFETY: the thread block is this loader's (FLAG_OWN, checked above), so its dtv word points at
        // the vector `install_tls` built: `DTV_HEADER` + slot-count words; `ltls::lookup` reads a slot
        // only after checking it against the count in word 1.
        unsafe { conv::words_at(at, 1) }.first().copied().unwrap_or(0)
    };
    match ltls::lookup::<Native>(word, GENERATION.load(Relaxed), module, offset).and_then(|a| usize::try_from(a).ok()) {
        Some(a) => a,
        None => refuse(b"ld.so: __tls_get_addr: no TLS block for this module in this thread\n"),
    }
}

/// The low 32 bits of `w`.
fn lo32(w: u64) -> u32 {
    u32::try_from(w & 0xffff_ffff).unwrap_or(0)
}

/// Objects one start can load (the program included).
const OBJECTS_MAX: usize = 64;

/// Print `parts` after `ld.so: ` on standard error and end with `status`.
fn fail_with(parts: &[&[u8]], status: i32) -> ! {
    write_all(2, b"ld.so: ");
    parts.iter().for_each(|p| write_all(2, p));
    exit(status)
}

/// The identity of an open file, (`st_dev`, `st_ino`): one object reached by two paths (a symlink, a
/// hard link) is the same object (L14-4).
type FileId = (u64, u64);

/// Open and map read-only the file at the NUL-terminated path `path`: (descriptor, mapping, bytes,
/// identity).
fn open_file(path: usize) -> Option<(i32, usize, &'static [u8], FileId)> {
    let fd = crate::ffi_sys::liella_sys_openat(AT_FDCWD, core::ptr::with_exposed_provenance(path), i64::from(O_RDONLY | O_CLOEXEC), 0);
    let fd32 = i32::try_from(fd).ok().filter(|&f| f >= 0)?;
    let mut st = liella_kernel::uapi::Stat::default();
    if crate::ffi_sys::liella_sys_fstat(fd, core::ptr::from_mut(&mut st).cast::<c_void>()) != 0 {
        let _ = crate::ffi_sys::liella_sys_close(fd);
        return None;
    }
    let size = crate::ffi_sys::liella_sys_lseek(fd, 0, i64::from(SEEK_END));
    let Some(len) = usize::try_from(size).ok().filter(|&n| n >= liella_ld::elf::EHDR_SIZE) else {
        let _ = crate::ffi_sys::liella_sys_close(fd);
        return None;
    };
    let Ok(at) = raw::mmap(0, len, map::PROT_READ, MAP_PRIVATE_I32, fd32, 0) else {
        let _ = crate::ffi_sys::liella_sys_close(fd);
        return None;
    };
    // UNSAFE(slice): an object file, mapped read-only above.
    #[allow(unsafe_code)]
    // SAFETY: `len` bytes from `at` are a private read-only mapping of the file, unmapped only after
    // the last use of the slice (the end of `link`, or `load`).
    Some((fd32, at, unsafe { conv::bytes_at(at, len) }, (st.dev, st.ino)))
}

/// A new private anonymous read-write mapping of `len` (> 0) bytes: (address, bytes).
fn scratch(len: usize, name: &[u8]) -> (usize, &'static mut [u8]) {
    let Ok(at) = raw::mmap(0, len, map::PROT_READ | map::PROT_WRITE, MAP_PRIVATE_I32 | MAP_ANONYMOUS_I32, -1, 0) else {
        fail_with(&[name, b": out of memory for the program's tables\n"], program::FAILURE)
    };
    // UNSAFE(slice): a fresh anonymous mapping, owned by the caller.
    #[allow(unsafe_code)]
    // SAFETY: `len` bytes from `at` were just mapped private, readable and writable; nothing else
    // refers to them, and the caller unmaps them only after the last use of the slice.
    (at, unsafe { conv::bytes_at_mut(at, len) })
}

/// Copy `out.len()` bytes of the mapped program at run-time address `at` into `out` (D-27.18).
fn read_image(at: usize, out: &mut [u8]) {
    // UNSAFE(slice): bytes of the mapped program, copied before any relocation store.
    #[allow(unsafe_code)]
    // SAFETY: [at, at + len) is the program header table (`AT_PHDR`, ⚑ KERN), the start of its (whole, readable)
    // page, or the file-backed part of one readable `PT_LOAD` mapped by the kernel, `load`, or (ld.so's own ELF
    // header, D-27.23) its first PT_LOAD; the slice lives only for this copy, and nothing stores meanwhile.
    out.copy_from_slice(unsafe { conv::bytes_at(at, out.len()) });
}

/// The `len` file-backed bytes at run-time address `at` of a program `PT_LOAD` that
/// [`liella_ld::elf::Elf::image_span_kind`] calls [`liella_ld::elf::SpanKind::Live`], read in place.
fn live_segment(at: usize, len: usize) -> &'static [u8] {
    // UNSAFE(slice): a PT_LOAD of the program read in place (SpanKind::Live).
    #[allow(unsafe_code)]
    // SAFETY: the caller checked the range is the file-backed part of a readable PT_LOAD, mapped for the
    // process's lifetime. Live means: not writable, the program is not text-relocated, and no segment the
    // relocator may store into overlaps it; the relocator stores only inside writable segments of such an
    // object (reloc `inside`; ld.so's own image not at all), so the bytes do not change until `link` ends.
    unsafe { conv::bytes_at(at, len) }
}

/// `n` empty spans laid over `bytes` (the unique borrow of a fresh scratch mapping's tail, consumed
/// here): the image reader's span list, sized from the program's header count, since the number of
/// `PT_LOAD`s has no bound (D-27.15).
fn span_slots(bytes: &'static mut [u8], n: usize, name: &[u8]) -> &'static mut [liella_ld::elf::Span<'static>] {
    use liella_ld::elf::Span;
    let p = bytes.as_mut_ptr().cast::<Span<'static>>();
    if !p.is_aligned() || n.checked_mul(size_of::<Span<'static>>()).is_none_or(|need| need > bytes.len()) {
        fail_with(&[name, b": malformed program headers\n"], program::FAILURE)
    }
    for i in 0..n {
        let slot = p.wrapping_add(i);
        // UNSAFE(slice): one slot of the span list, initialised before the list is formed (D-27.18).
        #[allow(unsafe_code)]
        // SAFETY: `bytes` is the only reference to its memory and is consumed here; `slot` is aligned for `Span`
        // and inside it (`i < n`, and `n` spans fit: checked above).
        unsafe {
            slot.write(Span { vaddr: 0, bytes: &[] });
        }
    }
    // UNSAFE(slice): the span list, in a fresh anonymous mapping (D-27.18).
    #[allow(unsafe_code)]
    // SAFETY: `p` is aligned, `n` spans fit and every one holds a valid empty span (written above); nothing
    // else refers to the memory, and the mapping is unmapped only at the end of `link`, after the last use.
    unsafe {
        core::slice::from_raw_parts_mut(p, n)
    }
}

/// The program as the reader sees it, read from its memory image (ruling #4229, D-27.18): its program
/// header table and `PT_DYNAMIC` copied into one anonymous mapping, each `PT_LOAD` whose bytes cannot
/// change during loading sliced in place, and each that can change and holds a table copied into a
/// second one, all before the first relocation store. The span list lives in the first mapping, after
/// the table. Returns the table and the spans; each mapping made is stored in `maps`.
// The span index stays below `spans.len()` (checked before each store).
#[allow(clippy::indexing_slicing)]
fn image_view(
    prog: &start::Program,
    bias: usize,
    name: &[u8],
    maps: &mut [(usize, usize)],
) -> (&'static [u8], &'static [liella_ld::elf::Span<'static>]) {
    use liella_ld::elf::{pt, Elf, Span, SpanKind, PHDR_SIZE};
    let bad = |what: &[u8]| -> ! { fail_with(&[name, what], program::FAILURE) };
    let table = prog.phnum.saturating_mul(PHDR_SIZE);
    // Pass 1: the header table alone, to find PT_DYNAMIC and size the first mapping. One span per
    // PT_LOAD and one for PT_DYNAMIC's copy: at most phnum (PT_DYNAMIC, required below, is a header
    // that is not a PT_LOAD), in the same mapping after the table (a multiple of 8 bytes from a page
    // boundary, so aligned for them). Any number of PT_LOADs (D-27.15).
    let whole = prog.phnum.saturating_mul(size_of::<Span<'static>>()).saturating_add(table);
    let (at, buf) = scratch(whole, name);
    let (buf, tail) = buf.split_at_mut(table);
    read_image(prog.phdr, buf);
    let head: &'static [u8] = buf;
    let spans = span_slots(tail, prog.phnum, name);
    let probe = Elf::from_image::<Native>(head, &[]).unwrap_or_else(|_| bad(b": malformed program headers\n"));
    let loads = || probe.program_headers().filter(|p| p.p_type == pt::LOAD);
    let run_at = |vaddr: u64| bias.wrapping_add(usize::try_from(vaddr).unwrap_or(usize::MAX));
    // A range is read from memory only when it lies in the file-backed part of one readable PT_LOAD.
    let mapped = |vaddr: u64, len: u64| {
        loads().any(|p| p.flags & 4 != 0 && p.vaddr <= vaddr && vaddr.checked_add(len).is_some_and(|e| p.vaddr.checked_add(p.filesz).is_some_and(|f| e <= f)))
    };
    let dynph = probe.find(pt::DYNAMIC).unwrap_or_else(|| bad(b": no valid dynamic section\n"));
    let dlen = usize::try_from(dynph.filesz).unwrap_or(usize::MAX);
    if !mapped(dynph.vaddr, dynph.filesz) || dlen == 0 {
        bad(b": no valid dynamic section\n");
    }
    maps[0] = (at, whole);
    let (at, buf) = scratch(dlen, name);
    read_image(run_at(dynph.vaddr), buf);
    maps[1] = (at, dlen);
    let dynamic: &'static [u8] = buf;
    // Pass 2: the PT_LOADs that change and hold a table, copied into one mapping.
    let copy_len = loads().filter(|p| probe.image_span_kind(p, dynamic) == SpanKind::Copy).fold(0usize, |a, p| a.saturating_add(usize::try_from(p.filesz).unwrap_or(usize::MAX)));
    let mut copies: &'static mut [u8] = &mut [];
    if copy_len != 0 {
        let (at, buf) = scratch(copy_len, name);
        maps[2] = (at, copy_len);
        copies = buf;
    }
    let mut k = 0usize;
    for kind in [SpanKind::Copy, SpanKind::Live] {
        for p in loads().filter(|p| probe.image_span_kind(p, dynamic) == kind) {
            let len = usize::try_from(p.filesz).unwrap_or(usize::MAX);
            if !mapped(p.vaddr, p.filesz) || k == spans.len() {
                bad(b": malformed program headers\n");
            }
            let bytes: &'static [u8] = if kind == SpanKind::Copy {
                let (now, rest) = core::mem::take(&mut copies).split_at_mut(len);
                read_image(run_at(p.vaddr), now);
                copies = rest;
                now
            } else {
                live_segment(run_at(p.vaddr), len)
            };
            spans[k] = Span { vaddr: p.vaddr, bytes };
            k = k.saturating_add(1);
        }
        if kind == SpanKind::Copy {
            // After the whole-segment copies, before the live slices: PT_DYNAMIC's own copy.
            if k == spans.len() {
                bad(b": malformed program headers\n");
            }
            spans[k] = Span { vaddr: dynph.vaddr, bytes: dynamic };
            k = k.saturating_add(1);
        }
    }
    let spans: &'static [Span<'static>] = spans;
    (head, &spans[..k])
}

/// A dependency's mapped `file` as the relocator sees it (load bias `bias`). Its `name` may come from
/// the program's string table (an image span), so any lifetime.
fn parse<'a>(file: &'static [u8], name: &'a [u8], bias: u64) -> liella_ld::reloc::Obj<'a> {
    let elf = liella_ld::elf::Elf::parse::<Native>(file).unwrap_or_else(|_| fail_with(&[name, b": not a valid ELF file for this machine\n"], program::FAILURE));
    let dynm = elf.dynamic().ok().flatten().unwrap_or_else(|| fail_with(&[name, b": no valid dynamic section\n"], program::FAILURE));
    liella_ld::reloc::Obj { elf, dynm, bias, name, tls: None }
}

/// The interpreter as an object of the scope (D-27.20), read from its own memory image like the
/// program ([`image_view`], D-27.23): its ELF header at [`OWN_BASE`] (the first bytes of its first
/// `PT_LOAD`) gives its program header table; no file is opened. It relocated itself at its start,
/// so its writable segments already hold their final values and the reader copies those holding a
/// table before the relocator runs, as for the program. The scratch mappings go to `maps`.
fn own_object(name: &'static [u8], maps: &mut [(usize, usize)]) -> liella_ld::reloc::Obj<'static> {
    let base = OWN_BASE.load(Relaxed);
    let mut header = [0u8; liella_ld::elf::EHDR_SIZE];
    read_image(base, &mut header);
    let Some((phoff, phnum)) = boot::own_headers(&header) else { fail_with(&[name, b": malformed program headers\n"], program::FAILURE) };
    let me = start::Program { phdr: base.wrapping_add(usize::try_from(phoff).unwrap_or(usize::MAX)), phnum, entry: 0 };
    let (head, view) = image_view(&me, base, name, maps);
    let elf = liella_ld::elf::Elf::from_image::<Native>(head, view).unwrap_or_else(|_| fail_with(&[name, b": malformed program headers\n"], program::FAILURE));
    let dynm = elf.dynamic().ok().flatten().unwrap_or_else(|| fail_with(&[name, b": no valid dynamic section\n"], program::FAILURE));
    liella_ld::reloc::Obj { elf, dynm, bias: u64::try_from(base).unwrap_or(0), name, tls: None }
}

/// Load the program's dependencies (by path; DL-8 adds the search), check their versions, relocate
/// everything and protect the RELRO pages (DL-10, design §6, D-27.17). The program is read from its
/// memory image ([`image_view`], ruling #4229, D-27.18), never from its file; `name` names it in
/// messages (its path for `ld.so PROGRAM`, else `argv[0]`).
// Indices stay below `n`, which never passes OBJECTS_MAX (checked before each store).
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn link(prog: &start::Program, bias: usize, page: u64, name: &'static [u8], sp: usize) -> usize {
    use liella_ld::reloc::{self, Obj, Objects};
    let mut maps = [(0usize, 0usize); OBJECTS_MAX];
    // The scratch mappings of the two image views: the program's, then the interpreter's own.
    let mut views = [(0usize, 0usize); 6];
    let (prog_views, own_views) = views.split_at_mut(3);
    let (head, view) = image_view(prog, bias, name, prog_views);
    // The program's own file is not known (no file is read): (dev, ino) that no file has.
    let mut ids: [FileId; OBJECTS_MAX] = [(u64::MAX, u64::MAX); OBJECTS_MAX];
    let image = liella_ld::elf::Elf::from_image::<Native>(head, view).unwrap_or_else(|_| fail_with(&[name, b": malformed program headers\n"], program::FAILURE));
    let dynm = image.dynamic().ok().flatten().unwrap_or_else(|| fail_with(&[name, b": no valid dynamic section\n"], program::FAILURE));
    let first = Obj { elf: image, dynm, bias: u64::try_from(bias).unwrap_or(0), name, tls: None };
    let mut objs = [first; OBJECTS_MAX];
    let mut n = 1usize;
    let mut q = 0usize;
    // The interpreter itself, when an object names it (`__tls_get_addr`): already relocated, in the
    // scope with its exports, never relocated again (D-27.20); read from its memory image.
    let mut interp: Option<usize> = None;
    // The DT_NEEDED links (dependent, dependency) in the order each object names them (DL-11).
    let mut edges = [(0u8, 0u8); EDGES_MAX];
    let mut ne = 0usize;
    let mut edge = |from: usize, to: usize, name: &[u8]| {
        let (Ok(a), Ok(b)) = (u8::try_from(from), u8::try_from(to)) else { fail_with(&[name, b": too many objects\n"], program::FAILURE) };
        let Some(slot) = edges.get_mut(ne) else { fail_with(&[name, b": too many DT_NEEDED entries\n"], program::FAILURE) };
        *slot = (a, b);
        ne += 1;
    };
    while q < n {
        let needed = objs[q].dynm;
        for name in needed.needed() {
            let name = name.unwrap_or_else(|_| fail_with(&[objs[q].name, b": malformed DT_NEEDED\n"], program::FAILURE));
            if let Some(j) = objs[..n].iter().position(|o| o.name == name) {
                edge(q, j, name);
                continue;
            }
            if name == Native::INTERP.as_bytes() {
                if n == OBJECTS_MAX {
                    fail_with(&[name, b": too many objects\n"], program::FAILURE);
                }
                objs[n] = own_object(name, own_views);
                edge(q, n, name);
                interp = Some(n);
                n += 1;
                continue;
            }
            if !name.contains(&b'/') {
                fail_with(&[name, b": cannot open shared object file: the library search is not available in this build (name it by path)\n"], program::FAILURE);
            }
            if n == OBJECTS_MAX {
                fail_with(&[name, b": too many objects\n"], program::FAILURE);
            }
            // The string table entry is NUL-terminated in the mapped file.
            let cpath = name.as_ptr().expose_provenance();
            let Some((fd, at, file, id)) = open_file(cpath) else { fail_with(&[name, b": cannot open shared object file\n"], program::FAILURE) };
            // Another path to an object already loaded (L14-4): the same object, loaded once.
            if let Some(j) = ids[..n].iter().position(|x| *x == id) {
                let _ = crate::ffi_sys::liella_sys_close(i64::from(fd));
                let _ = raw::munmap(at, file.len());
                edge(q, j, name);
                continue;
            }
            let loaded = map::load::<Native, _>(&mut SysMapper { fd }, file, page);
            let _ = crate::ffi_sys::liella_sys_close(i64::from(fd));
            let loaded = loaded.unwrap_or_else(|e| fail_with(&[name, b": ", e.message()], program::FAILURE));
            maps[n] = (at, file.len());
            ids[n] = id;
            objs[n] = parse(file, name, loaded.bias);
            edge(q, n, name);
            n += 1;
        }
        q += 1;
    }
    // Static TLS of the initially loaded objects (design §7, R14-4, D-27.20).
    let mut mods: [Option<Module>; OBJECTS_MAX] = [None; OBJECTS_MAX];
    for (k, (m, o)) in mods.iter_mut().zip(&objs[..n]).enumerate() {
        if Some(k) == interp {
            continue;
        }
        *m = o.elf.find(liella_ld::elf::pt::TLS).map(|ph| Module::of(&ph));
        if m.is_some_and(|m| !ltls::image_in_load(&o.elf, &m)) {
            fail_with(&[o.name, b": the TLS image lies outside the object's segments\n"], program::FAILURE);
        }
    }
    let mut placed: [Option<Placed>; OBJECTS_MAX] = [None; OBJECTS_MAX];
    let area = ltls::layout::<Native>(&mods[..n], &mut placed[..n], INITIAL_TAIL, ltls::SURPLUS).unwrap_or_else(|e| {
        let at = match e {
            ltls::TlsError::Align(k) | ltls::TlsError::Size(k) => k,
            _ => 0,
        };
        fail_with(&[objs.get(at).map_or(&b"?"[..], |o| o.name), b": malformed or too large TLS segment\n"], program::FAILURE)
    });
    // The objects ld.so exports (R14-12, D-27.26): published before any object is relocated, so
    // a program's copy relocation of one reads its final value.
    let area_base = map_area(&area);
    __libc_stack_end.store(sp, Release);
    let rseq_size = register_rseq(area_base.wrapping_add(usize::try_from(area.tp).unwrap_or(0)));
    for (o, p) in objs[..n].iter_mut().zip(&placed[..n]) {
        o.tls = *p;
    }
    let objs = &objs[..n];
    if let Err(e) = reloc::check_versions(objs) {
        let by = |i: usize| objs.get(i).map_or(&b"?"[..], |o| o.name);
        match e {
            reloc::VersionError::NoVersions { by: b, file, version } => fail_with(&[file, b": version `", version, b"' not found (the object has no version information; required by ", by(b), b")\n"], program::FAILURE),
            reloc::VersionError::Missing { by: b, file, version } => fail_with(&[file, b": version `", version, b"' not found (required by ", by(b), b")\n"], 1),
            reloc::VersionError::Elf(b, _) => fail_with(&[by(b), b": malformed version tables\n"], program::FAILURE),
        }
    }
    let first_sym = liella_ld::symbols::Object::new(objs[0].dynm).unwrap_or_else(|_| fail_with(&[objs[0].name, b": malformed symbol tables\n"], program::FAILURE));
    let mut syms = [first_sym; OBJECTS_MAX];
    for (s, o) in syms.iter_mut().zip(objs).skip(1) {
        *s = liella_ld::symbols::Object::new(o.dynm).unwrap_or_else(|_| fail_with(&[o.name, b": malformed symbol tables\n"], program::FAILURE));
    }
    // The C library's private interface (LIELLA_PRIVATE, L14-10, D-27.22): a library from another
    // build is refused before any of its code runs or any relocation is applied.
    let early = start::private::find(objs, &syms[..n], interp, |at, out| read_image(usize::try_from(at).unwrap_or(0), out)).unwrap_or_else(|e| {
        let (start::private::PrivateError::Mismatch(k) | start::private::PrivateError::Malformed(k)) = e;
        fail_with(&[objs.get(k).map_or(&b"?"[..], |o| o.name), e.message()], program::FAILURE)
    });
    let global: [usize; OBJECTS_MAX] = core::array::from_fn(|i| i);
    let set = Objects { objs, syms: &syms[..n], global: &global[..n] };
    let mut unique = reloc::UniqueTable::<64, 4096>::default();
    // Reverse load order (L14-8), the interpreter left out: it relocated itself at its start.
    let relocated = (0..n).rev().filter(|&k| Some(k) != interp).try_for_each(|k| reloc::relocate::<Native, _>(&set, k, &mut ProcImage, page, &mut unique).map_err(|error| reloc::Failure { obj: k, error }));
    if let Err(f) = relocated {
        let name = objs.get(f.obj).map_or(&b"?"[..], |o| o.name);
        match f.error {
            reloc::RelocError::Undefined(sym) => fail_with(&[name, b": ", f.error.message(), sym, b"\n"], program::FAILURE),
            e => fail_with(&[name, b": ", e.message()], program::FAILURE),
        }
    }
    if let Err(i) = reloc::protect_relro(objs, &mut ProcImage, page) {
        fail_with(&[objs.get(i).map_or(&b"?"[..], |o| o.name), b": cannot protect the RELRO segment\n"], program::FAILURE);
    }
    // The images are copied after relocation: a `.tdata` word may hold a relocated address.
    let biases: [u64; OBJECTS_MAX] = core::array::from_fn(|k| objs.get(k).map_or(0, |o| o.bias));
    install_tls(&mods[..n], &biases[..n], &placed[..n], &area, area_base);
    // The interpreter, when in the list, ran its own start already: no hooks of its own here.
    let hooks: [init::Hooks; OBJECTS_MAX] = core::array::from_fn(|i| objs.get(i).filter(|_| Some(i) != interp).map_or_else(init::Hooks::default, |o| init::hooks(&o.dynm, o.bias, i == 0)));
    for &(at, len) in maps[1..n].iter().chain(&views).filter(|m| m.1 != 0) {
        let _ = raw::munmap(at, len);
    }
    run_init(&hooks[..n], &edges[..ne], sp, early.map(|f| f.early_init), rseq_size)
}

/// Most `DT_NEEDED` links over all objects of one start.
const EDGES_MAX: usize = 1024;

/// The words of a function array in a mapped, relocated object.
fn array_words(a: init::Array) -> &'static [u64] {
    let (Ok(addr), Ok(len)) = (usize::try_from(a.addr), usize::try_from(a.len)) else { return &[] };
    // UNSAFE(slice): an object's DT_PREINIT_ARRAY, DT_INIT_ARRAY or DT_FINI_ARRAY, read only.
    #[allow(unsafe_code)]
    // SAFETY: `addr` is bias + the array's link-time address in an object mapped for the process's
    // lifetime (the loader never unmaps an object at start), `len` its size tag / 8, both from the
    // object's dynamic array as the static linker wrote them; relocation already ran.
    unsafe {
        conv::words_at(addr, len)
    }
}

/// The objects' termination hooks in finalisation order (DL-11): `[DT_FINI_ARRAY address, entries,
/// DT_FINI]` per object, written once before the program is entered, read by [`fini_all`].
static FINI_HOOKS: [[AtomicU64; 3]; OBJECTS_MAX] = [const { [const { AtomicU64::new(0) }; 3] }; OBJECTS_MAX];
static FINI_LEN: AtomicUsize = AtomicUsize::new(0);
static FINI_DONE: AtomicBool = AtomicBool::new(false);

/// Run the initialisation functions after relocation (design §9, D-27.19): the program's
/// `DT_PREINIT_ARRAY`, then each other object's `DT_INIT` and `DT_INIT_ARRAY` in
/// [`init::init_order`], each called with `(argc, argv, envp)` from the initial stack at `sp`. The
/// program's own `DT_INIT`/`DT_INIT_ARRAY` are its C library's start (probe: the reference loader
/// runs neither). Records the finalisation order for [`fini_all`] and returns its address, the
/// termination function the program gets in `%rdx`. First of all, `early` (the C library's
/// `__liella_libc_early_init`, design §14, D-27.22) gets the initial stack, so every constructor,
/// `.preinit_array` included, sees an initialised C library.
fn run_init(hooks: &[init::Hooks], edges: &[(u8, u8)], sp: usize, early: Option<u64>, rseq_size: usize) -> usize {
    let mut order = [0u8; OBJECTS_MAX];
    let mut fini_order = [0u8; OBJECTS_MAX];
    let (Ok(len), Ok(fini_len)) = (init::init_order(hooks.len(), edges, &mut order), init::fini_order(hooks.len(), edges, &mut fini_order)) else {
        refuse(b"ld.so: cannot order the objects' initialisation\n")
    };
    let argc = stack_word(sp);
    let argv = sp.wrapping_add(8);
    let envp = argv.wrapping_add(usize::try_from(argc).unwrap_or(0).wrapping_add(1).wrapping_mul(8));
    let args = |v: usize| i64::from_ne_bytes(u64::try_from(v).unwrap_or(0).to_ne_bytes());
    let (a, b, c) = (i64::from_ne_bytes(argc.to_ne_bytes()), args(argv), args(envp));
    let call = |f: u64| {
        let _ = crate::ffi::liella_machine_call3(usize::try_from(f).unwrap_or(0), a, b, c);
    };
    if let Some(f) = early {
        let mut auxv = envp;
        while stack_word(auxv) != 0 {
            auxv = auxv.wrapping_add(8);
        }
        // The program's own DT_INIT and DT_INIT_ARRAY go to its C library's start (design §14,
        // D-27.24): `__libc_start_main` runs them after every shared object's initialisation.
        let prog = hooks.first().copied().unwrap_or_default();
        let w = |v: u64| usize::try_from(v).unwrap_or(0);
        let info = start::private::StartInfo {
            argc: usize::try_from(argc).unwrap_or(0),
            argv,
            envp,
            auxv: auxv.wrapping_add(8),
            stack_end: sp,
            init: w(prog.init),
            init_array: w(prog.init_array.addr),
            init_array_len: w(prog.init_array.len),
            rseq_size,
        };
        let at = core::ptr::from_ref(&info).expose_provenance();
        let _ = crate::ffi::liella_machine_call3(usize::try_from(f).unwrap_or(0), args(at), 0, 0);
    }
    // Only the program's preinit array is non-empty (`init::hooks` drops a shared object's).
    for h in hooks {
        init::init_calls(0, array_words(h.preinit)).for_each(call);
    }
    for &i in order.get(..len).unwrap_or(&[]) {
        let Some(h) = hooks.get(usize::from(i)).filter(|_| i != 0) else { continue };
        init::init_calls(h.init, array_words(h.init_array)).for_each(call);
    }
    for (slot, &i) in FINI_HOOKS.iter().zip(fini_order.get(..fini_len).unwrap_or(&[])) {
        let h = hooks.get(usize::from(i)).copied().unwrap_or_default();
        for (w, v) in slot.iter().zip([h.fini_array.addr, h.fini_array.len, h.fini]) {
            w.store(v, Relaxed);
        }
    }
    FINI_LEN.store(fini_len, Release);
    #[allow(clippy::as_conversions)] // a function's address has no conversion other than `as`
    (fini_all as *const ()).expose_provenance()
}

/// The termination function passed to the program (`rtld_fini`): each object's `DT_FINI_ARRAY` in
/// reverse, then its `DT_FINI`, the objects in [`init::fini_order`], the program first. Runs once.
extern "C" fn fini_all() {
    if FINI_DONE.swap(true, AcqRel) {
        return;
    }
    for slot in FINI_HOOKS.iter().take(FINI_LEN.load(Acquire)) {
        let [addr, len, fini] = [0, 1, 2].map(|k| slot.get(k).map_or(0, |w| w.load(Relaxed)));
        for f in init::fini_calls(fini, array_words(init::Array { addr, len })) {
            let _ = crate::ffi::liella_machine_call0(usize::try_from(f).unwrap_or(0));
        }
    }
}

/// The process's memory as the relocator writes it ([`liella_ld::reloc::Image`]).
struct ProcImage;

impl liella_ld::reloc::Image for ProcImage {
    fn store(&mut self, at: u64, bytes: &[u8]) {
        let Ok(at) = word(at) else { return };
        // UNSAFE(slice): a relocated field of a loaded object.
        #[allow(unsafe_code)]
        // SAFETY: the relocator checked [at, at + len) lies in a segment of the object that is mapped
        // writable at this moment, and nothing else of the process runs yet.
        unsafe { conv::bytes_at_mut(at, bytes.len()) }.copy_from_slice(bytes);
    }

    fn load(&mut self, at: u64, out: &mut [u8]) {
        let Ok(at) = word(at) else { return };
        // UNSAFE(slice): a word of a loaded object's segment.
        #[allow(unsafe_code)]
        // SAFETY: the relocator checked the range lies in a mapped, readable segment of the object.
        out.copy_from_slice(unsafe { conv::bytes_at(at, out.len()) });
    }

    fn copy(&mut self, to: u64, from: u64, len: u64) {
        let (Ok(to), Ok(from), Ok(len)) = (word(to), word(from), word(len)) else { return };
        // UNSAFE(slice): a provider's data, the source of a COPY relocation.
        #[allow(unsafe_code)]
        // SAFETY: the relocator checked [from, from + len) lies in a mapped segment of the provider.
        let src = unsafe { conv::bytes_at(from, len) };
        // UNSAFE(slice): the program's copy of that data.
        #[allow(unsafe_code)]
        // SAFETY: the relocator checked [to, to + len) lies in a writable segment of the program, a
        // different object from the provider (so the ranges do not overlap).
        unsafe { conv::bytes_at_mut(to, len) }.copy_from_slice(src);
    }

    fn call(&mut self, resolver: u64) -> u64 {
        let v = crate::ffi::liella_machine_call0(word(resolver).unwrap_or(0));
        u64::from_ne_bytes(v.to_ne_bytes())
    }

    fn protect(&mut self, at: u64, len: u64, prot: i32) -> Result<(), ()> {
        raw::mprotect(word(at)?, word(len)?, prot).map_err(|_| ())
    }
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
/// Objects a dependency list (`--list`) holds (the program included); more is refused (D-27.16).
const LIST_OBJECTS_MAX: usize = 256;
/// Bytes of resolved paths a dependency list holds; more is refused (D-27.16).
const PATHS_MAX: usize = 64 * 1024;

/// What the dependency list needs from the start (DL-8).
struct List<'a> {
    ld: &'a start::LdEnv<'static>,
    secure: bool,
    features: &'a liella_rt::cpu::Features,
    page: u64,
    vdso: Option<u64>,
    base: usize,
}

/// One object of the dependency list: the name it was needed as, its `DT_SONAME`, its file (empty
/// when not found), its resolved path (a span of the path store) and the object that needed it.
#[derive(Clone, Copy, Default)]
struct Obj {
    name: &'static [u8],
    soname: &'static [u8],
    file: &'static [u8],
    path: (usize, usize),
    parent: usize,
    bias: u64,
}

/// Open the NUL-terminated `path` and map the whole file read-only: the file and its descriptor
/// (list mode; [`open_file`] is the run path's opener, which also gives the file's identity).
fn open_whole(path: &[u8]) -> Option<(&'static [u8], i64)> {
    let fd = crate::ffi_sys::liella_sys_openat(AT_FDCWD, path.as_ptr().cast(), i64::from(O_RDONLY | O_CLOEXEC), 0);
    if fd < 0 {
        return None;
    }
    let len = usize::try_from(crate::ffi_sys::liella_sys_lseek(fd, 0, i64::from(SEEK_END))).ok().filter(|&n| n > 0);
    let at = len.and_then(|len| raw::mmap(0, len, map::PROT_READ, MAP_PRIVATE_I32, i32::try_from(fd).unwrap_or(-1), 0).ok());
    let (Some(len), Some(at)) = (len, at) else {
        let _ = crate::ffi_sys::liella_sys_close(fd);
        return None;
    };
    // UNSAFE(slice): a file mapped read-only above, never unmapped while the slice is in use.
    #[allow(unsafe_code)]
    // SAFETY: `len` bytes from `at` are a private read-only mapping of the file; the list mode keeps
    // every such mapping until the process exits (the caller unmaps only a file it rejects, after its
    // last use).
    let file = unsafe { conv::bytes_at(at, len) };
    Some((file, fd))
}

/// The dynamic section of the object file `file`, when it is one of this architecture's.
fn dynamic_of(file: &'static [u8]) -> Option<elf::Dynamic<'static>> {
    elf::Elf::parse::<Native>(file).ok()?.dynamic().ok().flatten()
}

/// The string of dynamic tag `tag` in `d`.
fn dyn_string(d: &elf::Dynamic<'static>, tag: u64) -> Option<&'static [u8]> {
    d.get(tag).and_then(|o| d.string(o).ok())
}

/// The directory part of `path` (`$ORIGIN`): `.` for a bare name, `/` for a name in the root.
fn dir_of(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/') {
        None => b".",
        Some(0) => b"/",
        Some(i) => path.get(..i).unwrap_or(b"."),
    }
}

/// `v` as 16 lower-case hexadecimal digits.
fn hex16(v: u64) -> [u8; 16] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digit = |n: u8| HEX.get(usize::from(n & 0xf)).copied().unwrap_or(b'0');
    let mut out = [b'0'; 16];
    for (pair, byte) in out.chunks_exact_mut(2).zip(v.to_be_bytes()) {
        if let [hi, lo] = pair {
            *hi = digit(byte.wrapping_shr(4));
            *lo = digit(byte);
        }
    }
    out
}

/// `ld.so --list PROGRAM` and `LD_TRACE_LOADED_OBJECTS` (ld.so(8), design §19, DL-8): find and map
/// (through [`map::load`]) every object the program needs, breadth first in `DT_NEEDED` order, each
/// once, and write one line per object as `ldd` shows them; nothing is relocated or run. The lines
/// are written once the walk is complete. A dependency that is not found is a `NAME => not found`
/// line (status 0) when tracing; with `strict` (`--list`) it is the loader's error on stderr, nothing
/// on stdout, status 127, as the reference's `ld.so --list` does (DL-8 probe).
fn list(env: &List<'_>, prog: &'static [u8], strict: bool) -> ! {
    let mut path = PathBuf::default();
    let fail = |what: &[u8], msg: &[u8]| -> ! {
        for part in [&b"ld.so: "[..], what, b": ", msg] {
            write_all(2, part);
        }
        exit(program::FAILURE)
    };
    let Some((file, fd)) = (if path.push(prog) { path.c_str().and_then(open_whole) } else { None }) else {
        fail(prog, b"cannot open shared object file\n")
    };
    let _ = crate::ffi_sys::liella_sys_close(fd);
    let interp = elf::Elf::parse::<Native>(file).ok().and_then(|e| e.interp().ok().flatten());
    if dynamic_of(file).is_none() {
        write_all(1, b"\tnot a dynamic executable\n");
        exit(1)
    }
    // The cache, once (R14-3 (a): `<prefix>/etc/ld.so.cache`); unreadable or inconsistent = none.
    let cache_file = if search::prefixed(b"etc/ld.so.cache", &mut path) { path.c_str().and_then(open_whole) } else { None };
    if let Some((_, fd)) = cache_file {
        let _ = crate::ffi_sys::liella_sys_close(fd);
    }
    let cache = cache_file.and_then(|(f, _)| Cache::parse(f));
    let ctx = Ctx {
        secure: env.secure,
        library_path: env.ld.library_path,
        origin_path: if env.secure { None } else { env.ld.origin_path },
        platform: Native::platform(env.features),
        hwcaps: Native::hwcaps(env.features),
        cache: cache.as_ref(),
    };
    let mut objs = [Obj::default(); LIST_OBJECTS_MAX];
    let mut store = [0u8; PATHS_MAX];
    let first = store.get_mut(..prog.len()).unwrap_or_else(|| fail(prog, b"path too long\n"));
    first.copy_from_slice(prog);
    let mut used = prog.len();
    if let Some(o) = objs.first_mut() {
        *o = Obj { name: prog, file, path: (0, prog.len()), ..Obj::default() };
    }
    let mut count = 1usize;
    let mut i = 0usize;
    while i < count {
        let me = i;
        i = i.wrapping_add(1);
        let Some(d) = objs.get(me).and_then(|o| dynamic_of(o.file)) else { continue };
        let (old, free) = store.split_at_mut(used);
        let old: &[u8] = old;
        let mut wrote = 0usize;
        // The requester chain: this object, then each object that loaded it, up to the program.
        let mut chain = [Scope::default(); LIST_OBJECTS_MAX];
        let mut depth = 0usize;
        let mut k = me;
        while let (Some(slot), Some(o)) = (chain.get_mut(depth), objs.get(k)) {
            let od = dynamic_of(o.file);
            let origin = old.get(o.path.0..o.path.0.saturating_add(o.path.1)).map(dir_of);
            *slot = Scope { rpath: od.as_ref().and_then(|x| dyn_string(x, elf::dt::RPATH)), runpath: od.as_ref().and_then(|x| dyn_string(x, elf::dt::RUNPATH)), origin };
            depth = depth.wrapping_add(1);
            if k == 0 {
                break;
            }
            k = o.parent;
        }
        let chain = chain.get(..depth).unwrap_or(&[]);
        for name in d.needed().flatten() {
            let known = objs.get(..count).unwrap_or(&[]).iter().any(|o| o.name == name || (!o.soname.is_empty() && o.soname == name));
            if name == Native::INTERP.as_bytes() || known {
                continue;
            }
            if count >= LIST_OBJECTS_MAX {
                fail(name, b"too many shared objects\n");
            }
            let mut hit: Option<(&'static [u8], i64)> = None;
            let mut hit_path = PathBuf::default();
            search::search::<Native>(name, chain, &ctx, &mut |p: &mut PathBuf| {
                let Some((f, fd)) = p.c_str().and_then(open_whole) else { return false };
                if elf::Elf::parse::<Native>(f).is_err() {
                    // Another architecture's or not an object: skipped, the search goes on.
                    let _ = raw::munmap(f.as_ptr().expose_provenance(), f.len());
                    let _ = crate::ffi_sys::liella_sys_close(fd);
                    return false;
                }
                hit_path.clear();
                hit_path.push(p.as_bytes());
                hit = Some((f, fd));
                true
            });
            let Some((f, fd)) = hit else {
                if strict {
                    for part in [prog, &b": error while loading shared libraries: "[..], name, b": cannot open shared object file: No such file or directory\n"] {
                        write_all(2, part);
                    }
                    exit(program::FAILURE)
                }
                if let Some(o) = objs.get_mut(count) {
                    *o = Obj { name, parent: me, ..Obj::default() };
                }
                count = count.wrapping_add(1);
                continue;
            };
            let found = hit_path.as_bytes();
            let loaded = map::load::<Native, _>(&mut SysMapper { fd: i32::try_from(fd).unwrap_or(-1) }, f, env.page);
            let _ = crate::ffi_sys::liella_sys_close(fd);
            let loaded = loaded.unwrap_or_else(|e| fail(found, e.message()));
            let at = wrote;
            let dst = free.get_mut(at..at.saturating_add(found.len())).unwrap_or_else(|| fail(found, b"too many shared objects\n"));
            dst.copy_from_slice(found);
            wrote = wrote.saturating_add(found.len());
            let soname = dynamic_of(f).and_then(|x| dyn_string(&x, elf::dt::SONAME)).unwrap_or(b"");
            if let Some(o) = objs.get_mut(count) {
                *o = Obj { name, soname, file: f, path: (used.saturating_add(at), found.len()), parent: me, bias: loaded.bias };
            }
            count = count.wrapping_add(1);
        }
        used = used.saturating_add(wrote);
    }
    if let Some(v) = env.vdso.filter(|&v| v != 0) {
        for part in [&b"\tlinux-vdso.so.1 (0x"[..], &hex16(v), b")\n"] {
            write_all(1, part);
        }
    }
    for o in objs.get(1..count).unwrap_or(&[]) {
        if o.file.is_empty() {
            for part in [&b"\t"[..], o.name, b" => not found\n"] {
                write_all(1, part);
            }
        } else {
            let found = store.get(o.path.0..o.path.0.saturating_add(o.path.1)).unwrap_or(&[]);
            for part in [&b"\t"[..], o.name, b" => ", found, b" (0x", &hex16(o.bias), b")\n"] {
                write_all(1, part);
            }
        }
    }
    let interp = interp.unwrap_or(Native::INTERP.as_bytes());
    let base = u64::try_from(env.base).unwrap_or(0);
    for part in [&b"\t"[..], interp, b" (0x", &hex16(base), b")\n"] {
        write_all(1, part);
    }
    exit(0)
}

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
