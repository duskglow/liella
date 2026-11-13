//! `__libc_start_main` (IP-9; spec §1.1): the products' process start, in the order
//! `liella_rt::start` documents. Only in the products (feature `runtime`): a process the host C
//! library started must never run it.
//!
//! The kernel's initial stack (argc, argv[], NULL, envp[], NULL, auxv) and the program headers
//! are read here as slices (category *slice*); every decision is in `liella_rt::start`,
//! `liella_rt::tls`, `liella::config` and `crate::process`.

use core::ffi::{c_char, c_int, c_void};
use core::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use core::sync::atomic::{AtomicBool, AtomicUsize};

use liella::config::Tunables;
use liella_kernel::auxv::Auxv;
use liella_kernel::machine::Machine;
// ARCH(x86_64): static-or-dynamic test through ARCH_GET_FS; aarch64 reads TPIDR_EL0 (A-4)
use liella_kernel::ops::ArchGet;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{AT_NULL, AT_PHDR, AT_PHNUM, AT_RANDOM, AT_SECURE, AT_SYSINFO_EHDR, MAP_ANONYMOUS, MAP_PRIVATE, PROT_READ, PROT_WRITE};
use liella_legacy::exit_handlers::ATEXIT;
use liella_legacy::search_c::CWords;
// ARCH(x86_64): x87 control word reset at start-up; FPCR on aarch64 (A-9)
use liella_rt::fpu::X87_DEFAULT_CONTROL;
use liella_rt::{fatal, start, tls};

use crate::bootstrap;
use crate::conv;
use crate::cpu;
use crate::ffi;
use crate::process;
use crate::raw;
use crate::strz;
use crate::sys::CSys;
use crate::tb;

/// `main`'s C type.
pub type Main = extern "C" fn(c_int, *mut *mut c_char, *mut *mut c_char) -> c_int;
/// A `void (*)(void)` function (`init`, `fini`, `rtld_fini`).
pub type Fn0 = extern "C" fn();

fn die(msg: &[u8]) -> ! {
    fatal::fatal(&CSys, &[b"liella: ", msg, b"\n"], crate::ret::trap)
}

/// Words at `addr` before the first zero word (the argument and environment arrays).
fn count_words(addr: usize) -> usize {
    let mut n = 0usize;
    loop {
        // UNSAFE(slice): one word of the kernel's initial stack.
        #[allow(unsafe_code)]
        // SAFETY: the array is NULL-terminated (⚑ KERN, `execve`), and words up to its NULL
        // are readable and live for the process.
        let w = unsafe { conv::words_at(addr.wrapping_add(n.wrapping_mul(8)), 1) };
        if w.first().is_none_or(|&v| v == 0) {
            return n;
        }
        n = n.wrapping_add(1);
    }
}

/// Words of the auxiliary vector at `addr`, through its `AT_NULL` pair.
fn count_auxv(addr: usize) -> usize {
    let mut n = 0usize;
    loop {
        // UNSAFE(slice): one key of the auxiliary vector on the initial stack.
        #[allow(unsafe_code)]
        // SAFETY: the vector is a sequence of word pairs ending with `AT_NULL` (⚑ KERN).
        let w = unsafe { conv::words_at(addr.wrapping_add(n.wrapping_mul(8)), 1) };
        n = n.wrapping_add(2);
        if w.first().is_none_or(|&v| v == AT_NULL) {
            return n;
        }
    }
}

/// The first 64 bytes of the page at `at`, which holds the executable's program headers: the
/// candidate ELF header `liella_rt::start::bias` checks when the program has no `PT_PHDR`
/// (`process::exe_bias`, D-27.21). Not called at start-up.
pub(crate) fn exe_header(at: usize) -> [u8; 64] {
    let mut out = [0u8; 64];
    // UNSAFE(slice): the start of the mapped page holding the executable's program headers.
    #[allow(unsafe_code)]
    // SAFETY: `at` is `AT_PHDR` rounded down to its 4096-byte page (⚑ KERN); the kernel maps the
    // headers readable in that page, and the 64 bytes from its start lie inside it.
    let page = unsafe { conv::bytes_at(at, out.len()) };
    out.copy_from_slice(page);
    out
}

/// A static program's initial thread: map its static TLS and thread block, copy the TLS image,
/// set the guards from `AT_RANDOM`, and make it the thread pointer. Returns what the headers
/// said (the TLS image and the RELRO pages), read once. `PIE`: started by `liella-rcrt1.o`, the
/// static-PIE start object (D-27.10), which relocates the executable first; the choice is made
/// at link time, so `liella-crt1.o`'s start pays for no test of the executable's type.
fn initial_thread<const PIE: bool>(aux: &Auxv<'_>) -> (start::Program, Option<bootstrap::Relocated>) {
    let as_usize = |v: Option<u64>| v.and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    let (phdr, phnum) = (as_usize(aux.get(AT_PHDR)), as_usize(aux.get(AT_PHNUM)));
    // UNSAFE(slice): the program headers the kernel mapped with the executable.
    #[allow(unsafe_code)]
    // SAFETY: `AT_PHDR`/`AT_PHNUM` describe the loaded headers (⚑ KERN), which stay mapped.
    let words = unsafe { conv::words_at(phdr, phnum.saturating_mul(start::PHDR_WORDS)) };
    let mut prog = start::program(words, phnum, phdr);
    let mut pie = None;
    if !PIE {
        if prog.bias != 0 {
            // A static PIE must relocate itself before any address in static data is used.
            die(b"a static PIE starts from liella-rcrt1.o, not liella-crt1.o");
        }
    } else if let Some(ehdr) = bootstrap::static_pie_header() {
        // A static PIE relocates itself before any address in its data is used (D-27.10). Until
        // then no diagnostic path that reads relocated data may run: one write, then exit 127.
        let Ok(r) = bootstrap::relocate_self(phdr, phnum, ehdr) else {
            let _ = CSys.write(liella_kernel::fd::Fd::STDERR, b"liella: static PIE: cannot relocate the executable\n");
            CSys.exit_group(127)
        };
        // Without PT_PHDR the headers gave no bias: move the TLS image and RELRO by the real one.
        let delta = r.bias.wrapping_sub(prog.bias);
        if let Some(t) = prog.tls.as_mut() {
            t.addr = t.addr.wrapping_add(delta);
        }
        prog.relro = prog.relro.map(|(a, l)| (a.wrapping_add(delta), l));
        prog.bias = r.bias;
        // The one source of the executable's bias for later lookups (D-27.12): `%n`,
        // `dl_iterate_phdr`, `_dl_find_object` and the backtrace. Set only here, so the non-PIE
        // start does no added work.
        process::EXE_BIAS.store(r.bias, Release);
        pie = Some(r);
    }
    let Some(area) = tls::InitialArea::new(prog.tls.as_ref()) else { die(b"invalid PT_TLS segment") };
    let Ok(region) = CSys.mmap(0, area.len, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0) else {
        die(b"cannot map the initial thread block")
    };
    // The region is kept for the process: dropping it leaks the mapping on purpose.
    let Some((tp, tls_start)) = area.place(region.addr()) else { die(b"cannot place the initial thread block") };
    if let Some(img) = prog.tls {
        // UNSAFE(slice): the fresh TLS block, inside the mapping just made.
        #[allow(unsafe_code)]
        // SAFETY: `place` keeps `filesz <= memsz` bytes from `tls_start` inside the mapping.
        let dst = unsafe { conv::bytes_at_mut(tls_start, img.filesz) };
        // UNSAFE(slice): the executable's TLS image, mapped with it.
        #[allow(unsafe_code)]
        // SAFETY: the image is `filesz` readable bytes of the loaded executable.
        let src = unsafe { conv::bytes_at(img.addr, img.filesz) };
        liella::bytes::copy(dst, src);
    }
    // UNSAFE(slice): the thread block in the fresh mapping.
    #[allow(unsafe_code)]
    // SAFETY: `place` leaves `BLOCK_SIZE` bytes at the 64-aligned `tp` inside the mapping.
    let b = unsafe { tb::at(tp) };
    b.tcb.store(tp, Relaxed);
    b.self_ptr.store(tp, Relaxed);
    b.flags.store(tls::FLAG_OWN, Relaxed);
    if let Some(r) = aux.get(AT_RANDOM).and_then(|v| usize::try_from(v).ok()) {
        // UNSAFE(slice): the 16 random bytes the kernel put on the initial stack.
        #[allow(unsafe_code)]
        // SAFETY: `AT_RANDOM` names 16 readable bytes (⚑ KERN).
        let bytes = unsafe { conv::bytes_at(r, 16) };
        if let Ok(random) = <&[u8; 16]>::try_from(bytes) {
            let (canary, pointer) = tls::guards(random);
            b.stack_guard.store(canary, Relaxed);
            b.pointer_guard.store(pointer, Relaxed);
        }
    }
    if raw::set_fs(tp).is_err() {
        die(b"cannot set the thread pointer");
    }
    (prog, pie)
}

/// A static program's RELRO segment becomes read-only (STATIC-RELRO, D-1.13): the start-up
/// writes to it are done (a static PIE relocated itself in `initial_thread`, D-27.10; the
/// program's own `IRELATIVE` entries were applied by `apply_irelative` or the relocator's pass 2,
/// and their target words lie in `.got` inside RELRO), and nothing after it writes there.
/// A dynamically linked program's segments are its loader's (phase 14). The range comes from
/// the one header pass `initial_thread` made (STATIC-RELRO-2: no second walk of the headers or
/// the auxiliary vector on the start-up path). A `PT_GNU_RELRO` that spans two writable
/// segments with an unmapped page between them makes that one call fail (D-27.25): only then
/// does `relro_by_segment` walk the headers again, so the usual start carries nothing more
/// than the test of the call's result (the fallback takes no argument: nothing is kept live
/// across the call for it).
#[inline(always)]
fn protect_relro(range: Option<(usize, usize)>) {
    if let Some((addr, len)) = range {
        if raw::mprotect(addr, len, i32::try_from(PROT_READ).unwrap_or(1)).is_err() {
            relro_by_segment();
        }
    }
}

/// `PT_GNU_RELRO` made read-only segment by segment (D-27.25), after the single `mprotect` of
/// the whole range failed: the executable's headers from `AT_PHDR`/`AT_PHNUM` of the saved
/// auxiliary vector (`process::auxval`), at its bias (`EXE_BIAS`: set by a static PIE's start, 0
/// for a fixed-address program).
#[cold]
#[inline(never)]
fn relro_by_segment() {
    let get = |k: u64| process::auxval(k).and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    let (phdr, phnum) = (get(AT_PHDR), get(AT_PHNUM));
    let words = conv::program_headers(phdr, phnum.saturating_mul(start::PHDR_WORDS));
    let prot = i32::try_from(PROT_READ).unwrap_or(1);
    // A failure of a piece (not seen in practice) leaves it writable: the program still runs.
    let _ = start::relro_pieces(words, phnum, process::EXE_BIAS.load(Relaxed), start::PAGE, |a, l| raw::mprotect(a, l, prot));
}

/// Secure mode: drop the unsafe variables from the initial environment array, in place.
fn sanitize(envp: usize) {
    // UNSAFE(slice): the initial environment array and its strings (writable, on the stack).
    #[allow(unsafe_code)]
    // SAFETY: `envp` is the kernel's NULL-terminated array of NUL-terminated strings.
    let mut mem = unsafe { conv::ctext() };
    let mut j = 0usize;
    for i in 0.. {
        let p = mem.word(envp, i);
        if p == 0 {
            break;
        }
        if !start::is_unsecure(mem.text(p)) {
            mem.set_word(envp, j, p);
            j = j.saturating_add(1);
        }
    }
    mem.set_word(envp, j, 0);
}

/// `R_X86_64_IRELATIVE` (spec/abi/constants.tsv).
// ARCH(x86_64): the IRELATIVE relocation number; aarch64 has its own (AAELF64, A-6)
const R_X86_64_IRELATIVE: usize = 37;

/// A static program's own indirect functions (GNU `ifunc`, its code or a library linked into it):
/// the linker leaves one `R_X86_64_IRELATIVE` entry per target word in `.rela.iplt`, between
/// `__rela_iplt_start` and `__rela_iplt_end`; each word gets its resolver's result. The resolver
/// is called with no defined arguments: probe-settled, glibc 2.39 on x86_64 passes none either
/// (`%rdi` held a fixed 0x1d and `%rsi` varied across three CPUs; neither was `AT_HWCAP`). The
/// executable is not position-independent (a static PIE's entries are in its dynamic tables and
/// go through `bootstrap::irelative` instead), so the offsets and resolver addresses are absolute. Runs after the CPU paths are chosen (a resolver
/// may call the library), before any constructor, and before the program's RELRO is protected
/// (the target words lie in `.got`, which RELRO covers).
// LAYOUT-HOTKEY-INLINE-STABLE: release already inlines it; always inlined, its code stays in the keyed caller in both profiles.
#[inline(always)]
fn apply_irelative() {
    let (start, end) = (ffi::liella_link_rela_iplt_start(), ffi::liella_link_rela_iplt_end());
    // `Elf64_Rela`: r_offset, r_info, r_addend.
    let n = end.saturating_sub(start) / 24;
    // UNSAFE(slice): the executable's `.rela.iplt` entries and the words they name.
    #[allow(unsafe_code)]
    // SAFETY: the linker's `__rela_iplt_start`/`_end` delimit `Elf64_Rela` entries of the loaded
    // executable, and each `r_offset` is a writable word of its data (not yet protected).
    let mut mem = unsafe { conv::ctext() };
    for i in 0..n {
        let w = i.wrapping_mul(3);
        let (offset, info, addend) = (mem.word(start, w), mem.word(start, w.wrapping_add(1)), mem.word(start, w.wrapping_add(2)));
        // ARCH(x86_64): the IRELATIVE relocation number; aarch64 has its own (AAELF64, A-6)
        if info & 0xffff_ffff != R_X86_64_IRELATIVE {
            die(b"unexpected relocation type in a static executable's .rela.iplt");
        }
        let target = usize::from_ne_bytes(ffi::liella_machine_call0(addend).to_ne_bytes());
        mem.set_word(offset, 0, target);
    }
}

/// Call each `void (*)(int, char **, char **)` in the array `[start, end)`.
fn run_array(start: usize, end: usize, argc: c_int, argv: usize, envp: usize) {
    let n = end.saturating_sub(start) / 8;
    // UNSAFE(slice): a constructor array of the executable.
    #[allow(unsafe_code)]
    // SAFETY: the linker's `__*_array_start`/`_end` symbols delimit an array of function
    // addresses in the loaded executable.
    let fns = unsafe { conv::words_at(start, n) };
    for &f in fns {
        let f = usize::try_from(f).unwrap_or(0);
        if f != 0 && f != usize::MAX {
            let a = usize::from_ne_bytes(i64::from(argc).to_ne_bytes());
            let _ = ffi::liella_machine_call3(f, i64::from_ne_bytes(a.to_ne_bytes()), i64::from_ne_bytes(argv.to_ne_bytes()), i64::from_ne_bytes(envp.to_ne_bytes()));
        }
    }
}

/// A static program's destructors, registered at startup as one exit entry: `.fini_array`
/// in reverse, then `_fini`.
// LAYOUT-HOTKEY-INLINE-STABLE: a fixed boundary for its start-up/fork hot-text key (off the hot paths: the call is free).
#[inline(never)]
extern "C" fn run_fini() {
    let (start, end) = (ffi::liella_link_fini_start(), ffi::liella_link_fini_end());
    let n = end.saturating_sub(start) / 8;
    // UNSAFE(slice): the destructor array of the executable.
    #[allow(unsafe_code)]
    // SAFETY: as in `run_array`.
    let fns = unsafe { conv::words_at(start, n) };
    for &f in fns.iter().rev() {
        let f = usize::try_from(f).unwrap_or(0);
        if f != 0 && f != usize::MAX {
            let _ = ffi::liella_machine_call0(f);
        }
    }
    let fini = ffi::liella_link_fini();
    if fini != 0 {
        let _ = ffi::liella_machine_call0(fini);
    }
}

/// The address of a function (its value as a C function pointer).
#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn addr0(f: Fn0) -> usize {
    f as usize
}

/// Start step 3 for [`early_init`]: `environ`, the arguments (the native interface's), the program
/// names and `__libc_stack_end`, as [`start`] publishes them (kept apart so the static start's code
/// and its hot-text keys stay as they are; under Liella's `ld.so`, `start` skips its copy, D-27.24).
/// [`start_under_liella_ld`] calls it again to undo `.preinit_array` changes (inlined at both
/// sites: no hot-text key of its own).
#[inline(always)]
fn publish(argv_addr: usize, n_args: usize, envp: usize, stack_end: usize) {
    ffi::environ.store(envp, Release);
    process::ARGV.store(argv_addr, Release);
    process::ARGC.store(n_args, Release);
    if n_args > 0 {
        // UNSAFE(slice): argv[0] and its text.
        #[allow(unsafe_code)]
        // SAFETY: argv has argc entries before its NULL (⚑ KERN).
        let argv0 = unsafe { conv::words_at(argv_addr, 1) }.first().and_then(|&p| usize::try_from(p).ok()).unwrap_or(0);
        // UNSAFE(slice): the text of argv[0].
        #[allow(unsafe_code)]
        // SAFETY: argv[0] is NULL or a NUL-terminated string on the initial stack.
        let name = unsafe { strz::strz(core::ptr::with_exposed_provenance(argv0)) }.unwrap_or(&[]);
        let short = start::short_name(name);
        ffi::program_invocation_name.store(argv0, Release);
        ffi::program_invocation_short_name.store(argv0.wrapping_add(name.len().saturating_sub(short.len())), Release);
    }
    ffi::__libc_stack_end.store(stack_end, Release);
}

/// `__liella_libc_early_init` (`LIELLA_PRIVATE`, plan/ph14/design.md §14, D-27.22): Liella's
/// `ld.so` calls it after relocation and before any constructor, `.preinit_array` included, so
/// constructors see an initialised C library: the auxiliary vector, steps 3 and 4 (`environ`, the
/// program names, `__libc_stack_end`, the tunables and the CPU paths). The NULL-folding guard is
/// decided at the first small copy or fill (`dispatch::resolve_fold`). `__libc_start_main` then
/// skips steps 1-4 but publishes `environ` and the program names again ([`start_under_liella_ld`],
/// D-27.24): a `.preinit_array` function's `setenv` or change to the program names is undone, a
/// constructor's stays.
pub fn early_init(info: usize) {
    // UNSAFE(slice): the start description `ld.so` passes.
    #[allow(unsafe_code)]
    // SAFETY: `ld.so` passes the address of a live, aligned `StartInfo` on its stack for the call
    // (design §14); NULL is ignored.
    let Some(info) = (unsafe { core::ptr::with_exposed_provenance::<liella_rt::start::private::StartInfo>(info).as_ref() }) else { return };
    let auxv_words = count_auxv(info.auxv);
    process::AUXV_WORDS.store(auxv_words, Release);
    process::AUXV_ADDR.store(info.auxv, Release);
    // UNSAFE(slice): the auxiliary vector, measured above.
    #[allow(unsafe_code)]
    // SAFETY: `count_auxv` measured these words on the initial stack `ld.so` describes.
    let aux = Auxv::from_words(unsafe { conv::words_at(info.auxv, auxv_words) });
    crate::sys::set_sysinfo_ehdr(aux.get(AT_SYSINFO_EHDR).and_then(|v| usize::try_from(v).ok()).unwrap_or(0));
    publish(info.argv, info.argc, info.envp, info.stack_end);
    // The loader registered the initial thread's rseq area (it honoured `glibc.pthread.rseq`):
    // threads this library creates register theirs at the same offset (D-27.26).
    crate::thread::set_loader_rseq(info.rseq_size != 0);
    // The loader's per-thread TLS interface: new threads get every initially loaded module's
    // block from it (DL-15, design §7).
    crate::thread::set_loader_tls(info.tls_size, info.tls_align, info.tls_init, info.tls_release);
    // The loader's `dlopen` entry (DL-16a): `dlopen` calls it; without one it fails with a text.
    crate::posix::s27_dynlink::LOADER_DLOPEN.store(info.dlopen, Release);
    // Its `dlsym`/`dlvsym` entry (DL-16b), likewise.
    crate::posix::s27_dynlink::LOADER_DLSYM.store(info.dlsym, Release);
    // The tunables without diagnostics (`ld.so` read the string already).
    let secure = aux.get(AT_SECURE).is_some_and(|v| v != 0);
    // Secure mode's standard descriptors, before any constructor can open a file in their place
    // (`ld.so` cleared the unsafe variables already; D-27.24).
    if secure && start::open_std_fds(&CSys).is_err() {
        die(b"cannot open /dev/null on a closed standard descriptor");
    }
    let tunables = process::getenv(crate::startup_names::GLIBC_TUNABLES).map_or(Tunables::default(), |v| Tunables::parse(strz::env_value(v), secure));
    let _ = cpu::init_with(&tunables);
    for (slot, v) in PROGRAM_INIT.iter().zip([info.init, info.init_array, info.init_array_len]) {
        slot.store(v, Relaxed);
    }
    // Last: `__libc_start_main` takes the dynamic path once this is set (D-27.24).
    EARLY_DONE.store(true, Release);
}

/// Set by [`early_init`]: Liella's `ld.so` started the program and ran start steps 1-4, so
/// `__libc_start_main` must not run them again (D-27.24). Never set in a static program or under
/// another loader, which never calls `__liella_libc_early_init`.
static EARLY_DONE: AtomicBool = AtomicBool::new(false);

/// The program's `DT_INIT`, `DT_INIT_ARRAY` and its entry count, as `ld.so` passed them to
/// [`early_init`] (0: none).
static PROGRAM_INIT: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];

/// `__libc_start_main` in a program Liella's `ld.so` started (design §14, D-27.24): `ld.so` made
/// the initial thread (step 1), cleared the environment in secure mode and called [`early_init`]
/// (steps 2-4) before any constructor; it ran the program's `.preinit_array` and every shared
/// object's initialisation. Left here: the thread's kernel registrations and the library's own
/// state, `rtld_fini` (each object's termination, the program's `.fini_array` included), the
/// initial `environ` and program names published again (a `.preinit_array` function's changes do
/// not reach the constructors or `main`), then the program's own `DT_INIT` and `DT_INIT_ARRAY`
/// (new crt1, `init` NULL) or `init` (old crt1), each with `(argc, argv, envp)`, then `main`. An
/// old crt1's `fini` is not registered: `rtld_fini` already runs the program's termination, and
/// running `fini` as well would run its `.fini_array` twice.
#[inline(never)]
fn start_under_liella_ld(main: Option<Main>, argc: c_int, argv_addr: usize, init: Option<Fn0>, rtld_fini: Option<Fn0>) -> ! {
    let n_args = usize::try_from(argc).unwrap_or(0);
    let envp = argv_addr.wrapping_add(n_args.wrapping_add(1).wrapping_mul(8));
    let t = tb::current();
    if let Ok(tid) = CSys.set_tid_address(&t.tid) {
        t.tid.store(u32::try_from(tid).unwrap_or(0), Relaxed);
    }
    // Threads need the loader's TLS interface (DL-15): without it, as under another loader.
    if !crate::thread::loader_tls_ready() {
        crate::thread::forbid();
    }
    // The program's start-up x87 control word (D-10.5), before any of its code runs from here.
    let cw = ffi::liella_link_fpu_control();
    // ARCH(x86_64): x87 control word reset at start-up; FPCR on aarch64 (A-9)
    if cw != X87_DEFAULT_CONTROL {
        CSys.set_x87_control(cw);
    }
    let secure = process::secure();
    crate::heap::perturb_from_start(process::getenv(crate::startup_names::GLIBC_TUNABLES).and_then(|v| Tunables::malloc_perturb(strz::env_value(v), secure)));
    crate::nscache::start(secure);
    // rseq stays with the loader (DL-13): libc registers no area of its own here.
    crate::thread::RSEQ_OWNED.store(false, Release);
    crate::thread::init_initial(false);
    crate::cancel::install();
    // 5. Termination, then the program's initialisation.
    if let Some(f) = rtld_fini {
        process::register(process::entry(ATEXIT, addr0(f), 0, 0));
    }
    // The initial environment and names again, before the program's own initialisation.
    publish(argv_addr, n_args, envp, ffi::__libc_stack_end.load(Acquire));
    let a = |v: usize| i64::from_ne_bytes(v.to_ne_bytes());
    match init {
        // Before 2.34, crt1 passed `__libc_csu_init(argc, argv, envp)`, which runs the arrays.
        Some(f) => {
            let _ = ffi::liella_machine_call3(addr0(f), i64::from(argc), a(argv_addr), a(envp));
        }
        None => {
            let [dt_init, array, len] = PROGRAM_INIT.each_ref().map(|w| w.load(Relaxed));
            if dt_init != 0 {
                let _ = ffi::liella_machine_call3(dt_init, i64::from(argc), a(argv_addr), a(envp));
            }
            if array != 0 {
                run_array(array, array.wrapping_add(len.wrapping_mul(8)), argc, argv_addr, envp);
            }
        }
    }
    // 6. main, then exit.
    let argv_ptr: *mut *mut c_char = core::ptr::with_exposed_provenance_mut(argv_addr);
    let envp_ptr: *mut *mut c_char = core::ptr::with_exposed_provenance_mut(envp);
    let status = main.map_or(0, |m| m(argc, argv_ptr, envp_ptr));
    process::exit(status)
}

/// `__libc_start_main(main, argc, argv, init, fini, rtld_fini, stack_end)`; with `PIE`, the
/// static-PIE start `liella-rcrt1.o` calls (`liella_start_main_static_pie`, D-27.10).
// LAYOUT-HOTKEY-INLINE-STABLE: a fixed boundary for its start-up/fork hot-text key (off the hot paths: the call is free).
#[inline(never)]
pub fn start<const PIE: bool>(main: Option<Main>, argc: c_int, argv: *mut *mut c_char, init: Option<Fn0>, fini: Option<Fn0>, rtld_fini: Option<Fn0>, stack_end: *mut c_void) -> ! {
    // 1. The initial thread: a static program has no thread pointer yet.
    // ARCH(x86_64): static-or-dynamic test through ARCH_GET_FS; aarch64 reads TPIDR_EL0 (A-4)
    let static_program = CSys.arch_prctl_get(ArchGet::Fs).map_or(true, |fs| fs == 0);
    // Liella's `ld.so` ran steps 1-4 already (D-27.24); a static program never tests the flag.
    if !static_program && EARLY_DONE.load(Acquire) {
        start_under_liella_ld(main, argc, argv.expose_provenance(), init, rtld_fini)
    }
    let n_args = usize::try_from(argc).unwrap_or(0);
    let argv_addr = argv.expose_provenance();
    let envp = argv_addr.wrapping_add(n_args.wrapping_add(1).wrapping_mul(8));
    let auxv = envp.wrapping_add(count_words(envp).wrapping_add(1).wrapping_mul(8));
    let auxv_words = count_auxv(auxv);
    process::AUXV_WORDS.store(auxv_words, Release);
    process::AUXV_ADDR.store(auxv, Release);
    // UNSAFE(slice): the auxiliary vector, measured above.
    #[allow(unsafe_code)]
    // SAFETY: `count_auxv` measured these words on the initial stack.
    let aux = Auxv::from_words(unsafe { conv::words_at(auxv, auxv_words) });
    // The vDSO, from the kernel's vector in hand (otherwise its first use reads
    // `/proc/self/auxv`).
    crate::sys::set_sysinfo_ehdr(aux.get(AT_SYSINFO_EHDR).and_then(|v| usize::try_from(v).ok()).unwrap_or(0));

    let mut relro = None;
    let mut pie = None;
    if static_program {
        let (prog, relocated) = initial_thread::<PIE>(&aux);
        pie = relocated;
        crate::thread::set_tls_image(prog.tls);
        relro = prog.relro;
    } else {
        // Another `ld.so` owns the modules' TLS: no threads until phase 14 (D-22.10).
        crate::thread::forbid();
    }
    let t = tb::current();
    if let Ok(tid) = CSys.set_tid_address(&t.tid) {
        t.tid.store(u32::try_from(tid).unwrap_or(0), Relaxed);
    }

    // 2. Secure mode.
    let secure = aux.get(AT_SECURE).is_some_and(|v| v != 0);
    if secure {
        if start::open_std_fds(&CSys).is_err() {
            die(b"cannot open /dev/null on a closed standard descriptor");
        }
        sanitize(envp);
    }

    // 3. Publish the environment, the arguments (the native interface's), the program names and
    //    the stack end.
    ffi::environ.store(envp, Release);
    process::ARGV.store(argv_addr, Release);
    process::ARGC.store(n_args, Release);
    if n_args > 0 {
        // UNSAFE(slice): argv[0] and its text.
        #[allow(unsafe_code)]
        // SAFETY: argv has argc entries before its NULL (⚑ KERN).
        let argv0 = unsafe { conv::words_at(argv_addr, 1) }.first().and_then(|&p| usize::try_from(p).ok()).unwrap_or(0);
        // UNSAFE(slice): the text of argv[0].
        #[allow(unsafe_code)]
        // SAFETY: argv[0] is NULL or a NUL-terminated string on the initial stack.
        let name = unsafe { strz::strz(core::ptr::with_exposed_provenance(argv0)) }.unwrap_or(&[]);
        let short = start::short_name(name);
        ffi::program_invocation_name.store(argv0, Release);
        ffi::program_invocation_short_name.store(argv0.wrapping_add(name.len().saturating_sub(short.len())), Release);
    }
    ffi::__libc_stack_end.store(stack_end.expose_provenance(), Release);

    // 4. Tunables and the CPU paths.
    let tunables = process::getenv(crate::startup_names::GLIBC_TUNABLES).map_or(Tunables::default(), |v| {
        // UNSAFE(slice): the variable's value, inside the environment.
        #[allow(unsafe_code)]
        // SAFETY: `getenv` returned the address of a NUL-terminated value.
        let text = unsafe { strz::strz(core::ptr::with_exposed_provenance(v)) }.unwrap_or(&[]);
        // glibc's diagnostics for a malformed string or value, before any program code runs.
        // Not in secure mode (glibc prints none there, probe-settled) and not when another
        // `ld.so` started the program: that loader printed them already.
        if static_program && !secure {
            liella::config::warnings(text, |parts| {
                // One line, one write (no allocation): at most 6 pieces.
                let mut iov = [liella_kernel::mem::IoVec::new(&[]); 6];
                for (slot, piece) in iov.iter_mut().zip(parts) {
                    *slot = liella_kernel::mem::IoVec::new(piece);
                }
                let _ = CSys.writev(liella_kernel::fd::Fd::STDERR, iov.get(..parts.len()).unwrap_or(&iov));
            });
        }
        Tunables::parse(text, secure)
    });
    let cpu_state = cpu::init_with(&tunables);
    // The classed entries (D-4.53): on the AVX2 class, the AVX2 page moves over the SSE2 page,
    // here, with one thread and before the program's IFUNC resolvers and constructors. A program
    // started by another `ld.so` keeps the SSE2 page (other objects' code may have run). The
    // class is the decision `init_with` just made (no second feature read on the start-up path).
    if static_program {
        crate::classed::start(cpu::avx2_in(cpu_state), &tunables);
    }
    // The program's own indirect functions, before any of its code runs (glibc's resolvers run
    // before `.preinit_array`, probe-settled) and before its RELRO is protected.
    // A static PIE's indirect functions are in its dynamic relocation tables (pass 2 of the
    // self-relocator, D-27.10).
    // (`PIE` first: `liella-crt1.o`'s start compiles no test here.)
    if let Some(r) = if PIE { pie.as_ref() } else { None } {
        if bootstrap::irelative(r).is_err() {
            die(b"static PIE: an IRELATIVE relocation is outside the writable segment");
        }
    } else if static_program {
        apply_irelative();
    }
    // The program's start-up x87 control word (D-10.5): a dynamic program that defines its own
    // `__fpu_control` starts with it, before any of its code runs. A static program keeps the
    // default even when it defines the name, as under glibc (probe-settled, #812).
    if !static_program {
        let cw = ffi::liella_link_fpu_control();
        // ARCH(x86_64): x87 control word reset at start-up; FPCR on aarch64 (A-9)
        if cw != X87_DEFAULT_CONTROL {
            CSys.set_x87_control(cw);
        }
    }
    // NULL folding in the copy and fill entries (D-4.22): only while page zero cannot be mapped.
    // Decided lazily at the first small copy or fill (`dispatch::resolve_fold`, STARTUP-1).
    // The C `malloc` family's `M_PERTURB` from `glibc.malloc.perturb` or `MALLOC_PERTURB_`
    // (§3.2; both removed in a secure process).
    crate::heap::perturb_from_start(process::getenv(crate::startup_names::GLIBC_TUNABLES).and_then(|v| Tunables::malloc_perturb(strz::env_value(v), secure)));
    // The name-service cache (D-16.10): `LIELLA_NSCACHE`, and where the shared regions are.
    crate::nscache::start(secure);
    // The initial thread's descriptor, robust list and rseq area (D-22.7, D-22.9). Under
    // another `ld.so`, that loader registered rseq already.
    crate::thread::RSEQ_OWNED.store(static_program && tunables.rseq != Some(false), Release);
    crate::thread::init_initial(static_program);
    // The internal signal (D-20.1): cancellation requests and the setxid broadcast.
    crate::cancel::install();
    // The standard streams are set up at first use; with the heap exhausted then, their slots
    // come from the library's reserve (T22, D-12.17), so an empty program maps no heap.

    // The RELRO segment of a static program becomes read-only before any constructor runs
    // (where the reference library's strace shows its `mprotect`).
    // `relro` is `None` for a dynamically linked program (its loader's, phase 14).
    protect_relro(relro);

    // 5. Destructors, then constructors.
    if let Some(f) = rtld_fini {
        process::register(process::entry(ATEXIT, addr0(f), 0, 0));
    }
    match fini {
        Some(f) => {
            process::register(process::entry(ATEXIT, addr0(f), 0, 0));
        }
        None if static_program => {
            process::register(process::entry(ATEXIT, addr0(run_fini), 0, 0));
        }
        None => {}
    }
    let argv_ptr: *mut *mut c_char = core::ptr::with_exposed_provenance_mut(argv_addr);
    let envp_ptr: *mut *mut c_char = core::ptr::with_exposed_provenance_mut(envp);
    match init {
        // Before 2.34, crt1 passed `__libc_csu_init`, which runs the arrays itself.
        Some(f) => f(),
        None if static_program => {
            run_array(ffi::liella_link_preinit_start(), ffi::liella_link_preinit_end(), argc, argv_addr, envp);
            let init = ffi::liella_link_init();
            if init != 0 {
                let _ = ffi::liella_machine_call0(init);
            }
            run_array(ffi::liella_link_init_start(), ffi::liella_link_init_end(), argc, argv_addr, envp);
        }
        None => {}
    }

    // 6. main, then exit.
    let status = main.map_or(0, |m| m(argc, argv_ptr, envp_ptr));
    process::exit(status)
}
