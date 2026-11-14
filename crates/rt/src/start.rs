//! Startup sequencing (IP-9; spec §1.1, §1.4; D-1.1): the decisions `__libc_start_main` makes,
//! over the words the boundary read from the initial stack and the program headers.
//!
//! The order (carried out by `liella_boundary::runtime`):
//! 1. the thread pointer and static TLS for the initial thread, then the canary and pointer
//!    guard from `AT_RANDOM` (when the process has no thread pointer yet: a static program);
//! 2. in secure mode (`AT_SECURE`), descriptors 0–2 and the environment are sanitised;
//! 3. `environ`, the program names and `__libc_stack_end` are published;
//! 4. tunables (`GLIBC_TUNABLES`) and the CPU paths are decided;
//! 5. destructors are registered (`rtld_fini`, or for a static program the `.fini_array` and
//!    `_init`'s partner `_fini`), constructors run (`.preinit_array`, `_init`, `.init_array`);
//! 6. `main(argc, argv, envp)`, then `exit` with its result.

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::ops::FcntlInt;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::O_RDWR;

use crate::tls::TlsImage;

const PT_LOAD: u64 = 1;
const PT_PHDR: u64 = 6;
const PT_TLS: u64 = 7;
const PT_GNU_RELRO: u64 = 0x6474_e552;
/// Words per `Elf64_Phdr` (56 bytes).
pub const PHDR_WORDS: usize = 7;

/// What startup needs from the program headers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Program {
    /// The executable's TLS segment, if any (address adjusted by the bias).
    pub tls: Option<TlsImage>,
    /// The load bias: 0 for a non-PIE executable.
    pub bias: usize,
    /// The pages of the executable's `PT_GNU_RELRO` segment, as `(start, len)` (see [`relro`]).
    pub relro: Option<(usize, usize)>,
}

/// The page size the RELRO range is rounded to: x86_64's only base page size, the value the
/// kernel passes as `AT_PAGESZ` (a constant, so start-up does not search the auxiliary vector).
pub const PAGE: usize = 4096;

/// Read `phnum` program headers from `words` (the headers at runtime address `phdr_addr`), in
/// one pass (STATIC-RELRO-2: start-up reads them once). The bias comes from `PT_PHDR`; without
/// one the executable is taken as non-PIE. The first header of each type counts.
///
/// Every start pays this loop once per header (LAYOUT-DATA-CLUSTERS-4C, D-X.64's aligned clusters
/// add a `PT_LOAD`), so it keeps one bound and leaves a `PT_LOAD`, most of any program's headers,
/// after one compare: the table is cut to `phnum` whole headers first (`chunks_exact` then has the
/// only bound; `take` was a second counter per header).
pub fn program(words: &[u64], phnum: usize, phdr_addr: usize) -> Program {
    let field = |h: &[u64], i: usize| h.get(i).copied().map_or(0, |v| usize::try_from(v).unwrap_or(usize::MAX));
    let (mut phdr, mut tls, mut relro) = (None, None, None);
    // At most `words.len()`: the product cannot wrap.
    let table = words.get(..phnum.min(words.len() / PHDR_WORDS).wrapping_mul(PHDR_WORDS)).unwrap_or_default();
    for h in table.chunks_exact(PHDR_WORDS) {
        let ty = h.first().map_or(0, |w| w & 0xffff_ffff);
        if ty == PT_LOAD {
            continue;
        }
        match ty {
            PT_PHDR if phdr.is_none() => phdr = Some(h),
            PT_TLS if tls.is_none() => tls = Some(h),
            PT_GNU_RELRO if relro.is_none() => relro = Some(h),
            _ => {}
        }
    }
    let bias = phdr.map_or(0, |h| phdr_addr.wrapping_sub(field(h, 2)));
    let tls = tls.map(|h| TlsImage {
        addr: field(h, 2).wrapping_add(bias),
        filesz: field(h, 4),
        memsz: field(h, 5),
        align: field(h, 6),
    });
    let relro = relro.and_then(|h| relro_pages(field(h, 2).wrapping_add(bias), field(h, 5), PAGE));
    Program { tls, bias, relro }
}

/// The pages a static program makes read-only before its constructors run (STATIC-RELRO): the
/// executable's `PT_GNU_RELRO` segment (address adjusted by the bias), as `(start, len)`, from
/// its first byte's page down to the last page it fills completely. A partial last page is left
/// writable: it may share data that must stay writable. The linker ends the segment on a page
/// boundary (every probe of the reference library: its `mprotect` covers exactly
/// `[page_down(vaddr), vaddr + memsz)`), so in practice this is the whole segment. `None`
/// without the segment, or when it covers no whole page. `page` is a power of two.
pub fn relro(words: &[u64], phnum: usize, phdr_addr: usize, page: usize) -> Option<(usize, usize)> {
    relro_at(words, phnum, program(words, phnum, phdr_addr).bias, page)
}

/// [`relro`] with the load bias given by the caller: `ld.so` maps the program it runs and knows
/// its bias, which `PT_PHDR` cannot supply for an object without one (a shared object run as a
/// program); for a kernel-started program without `PT_PHDR` it finds the bias from the ELF
/// header in the image (D-27.18).
pub fn relro_at(words: &[u64], phnum: usize, bias: usize, page: usize) -> Option<(usize, usize)> {
    let field = |h: &[u64], i: usize| h.get(i).copied().map_or(0, |v| usize::try_from(v).unwrap_or(usize::MAX));
    let h = words.chunks_exact(PHDR_WORDS).take(phnum).find(|h| h.first().is_some_and(|w| w & 0xffff_ffff == PT_GNU_RELRO))?;
    relro_pages(field(h, 2).wrapping_add(bias), field(h, 5), page)
}

/// The load bias of a mapped program whose `phnum` program headers `words` are at run-time address
/// `phdr` (D-27.18, ruling #4371): `phdr` minus the table's own link-time address. That address is
/// `PT_PHDR`'s `p_vaddr` when the program has one. Without it, the table's address comes from the
/// `PT_LOAD` whose file-backed part holds it (`p_vaddr - p_offset + e_phoff`), and `e_phoff` from the
/// ELF header, which then begins the `page`-byte page holding `phdr` (the bias and `p_vaddr - p_offset`
/// are page-aligned, so a header whose table starts in its first page lies there). `header(at)` reads
/// that header's first 64 bytes (it is called only without `PT_PHDR`); `None` when they are not the
/// header of this table (magic, `e_phoff`, `e_phentsize`, `e_phnum`) or no `PT_LOAD` holds the table.
/// The one implementation for `ld.so` (`liella_ld::start::bias`) and libc's own lookups of the
/// executable (DL-LIBC-START-BIAS-3).
pub fn bias(words: &[u64], phnum: usize, phdr: usize, page: usize, header: impl FnOnce(usize) -> [u8; 64]) -> Option<usize> {
    const PT_LOAD: u64 = 1;
    const PHENT: u64 = 56;
    let heads = || words.chunks_exact(PHDR_WORDS).take(phnum);
    let ty = |h: &[u64]| h.first().map_or(0, |w| w & 0xffff_ffff);
    let field = |h: &[u64], i: usize| h.get(i).copied().unwrap_or(0);
    if let Some(h) = heads().find(|h| ty(h) == PT_PHDR) {
        return Some(phdr.wrapping_sub(usize::try_from(field(h, 2)).ok()?));
    }
    if phnum == 0 || phdr == 0 {
        // No table to find (no PT_LOAD can hold it): never read a header at an unknown address.
        return None;
    }
    let at = phdr & !page.wrapping_sub(1);
    let ehdr = header(at);
    let half = |i: usize| ehdr.get(i..i.wrapping_add(2)).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes);
    let phoff = u64::try_from(phdr.wrapping_sub(at)).ok()?;
    let e_phoff = ehdr.get(32..40).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes);
    let fits = ehdr.starts_with(b"\x7fELF\x02") && e_phoff == Some(phoff) && half(54) == Some(56) && half(56).map(usize::from) == Some(phnum);
    if !fits {
        return None;
    }
    let end = phoff.checked_add(u64::try_from(phnum).ok()?.checked_mul(PHENT)?)?;
    let load = heads().find(|h| ty(h) == PT_LOAD && field(h, 1) <= phoff && field(h, 1).checked_add(field(h, 4)).is_some_and(|e| end <= e))?;
    let own = field(load, 2).checked_sub(field(load, 1))?.checked_add(phoff)?;
    Some(phdr.wrapping_sub(usize::try_from(own).ok()?))
}

/// [`relro_at`]'s pages split by segment (D-27.25): `protect(start, len)` once for each `PT_LOAD`
/// whose pages meet them, with the part inside that segment's pages (from its first byte's page to
/// the page holding its last byte), in table order; the first error stops the walk and is
/// returned. A `PT_GNU_RELRO` that spans two writable segments is protected segment by segment,
/// so a page between them (unmapped, or a loader's reservation) is never touched; with one
/// segment the single call covers exactly [`relro_at`]'s range. Nothing is called without the
/// segment, or for pages no `PT_LOAD` holds.
///
/// # Errors
///
/// The first error `protect` returns.
pub fn relro_pieces<E>(words: &[u64], phnum: usize, bias: usize, page: usize, mut protect: impl FnMut(usize, usize) -> Result<(), E>) -> Result<(), E> {
    const PT_LOAD: u64 = 1;
    let Some((from, len)) = relro_at(words, phnum, bias, page) else { return Ok(()) };
    let field = |h: &[u64], i: usize| h.get(i).copied().map_or(0, |v| usize::try_from(v).unwrap_or(usize::MAX));
    for h in words.chunks_exact(PHDR_WORDS).take(phnum).filter(|h| h.first().is_some_and(|w| w & 0xffff_ffff == PT_LOAD)) {
        if let Some((at, n)) = relro_piece(from, from.wrapping_add(len), field(h, 2).wrapping_add(bias), field(h, 5), page) {
            protect(at, n)?;
        }
    }
    Ok(())
}

/// The part of the page range `[from, to)` inside the pages of a segment at `vaddr` with `memsz`
/// bytes (`[page_down(vaddr), page_up(vaddr + memsz))`), as `(start, len)`; `None` when they do
/// not meet or the segment's end overflows. The one intersection for [`relro_pieces`] and
/// `ld.so`'s objects (`liella_ld::reloc::protect_relro`). `page` is a power of two.
#[must_use]
pub fn relro_piece(from: usize, to: usize, vaddr: usize, memsz: usize, page: usize) -> Option<(usize, usize)> {
    let mask = page.wrapping_sub(1);
    let lo = (vaddr & !mask).max(from);
    let hi = (vaddr.checked_add(memsz)?.checked_add(mask)? & !mask).min(to);
    (hi > lo).then(|| (lo, hi.wrapping_sub(lo)))
}

/// `[page_down(vaddr), page_down(vaddr + memsz))` as `(start, len)`; `None` when empty.
fn relro_pages(vaddr: usize, memsz: usize, page: usize) -> Option<(usize, usize)> {
    let mask = page.wrapping_sub(1);
    let start = vaddr & !mask;
    let end = vaddr.checked_add(memsz)? & !mask;
    (end > start).then(|| (start, end.wrapping_sub(start)))
}

/// Whether `addr` lies in a loadable segment of the executable that is not writable (`%n` in a
/// fortified `printf` format, D-12.4): the program headers `words` (`phnum` of them), loaded
/// `bias` bytes from their link addresses. The caller supplies the bias (D-27.12): a static
/// PIE has no `PT_PHDR` to take it from.
pub fn read_only(words: &[u64], phnum: usize, bias: usize, addr: usize) -> bool {
    const PT_LOAD: u64 = 1;
    const PF_W: u64 = 2;
    let field = |h: &[u64], i: usize| h.get(i).copied().map_or(0, |v| usize::try_from(v).unwrap_or(usize::MAX));
    words.chunks_exact(PHDR_WORDS).take(phnum).any(|h| {
        let w0 = h.first().copied().unwrap_or(0);
        let start = field(h, 2).wrapping_add(bias);
        w0 & 0xffff_ffff == PT_LOAD && (w0 >> 32) & PF_W == 0 && addr >= start && addr.wrapping_sub(start) < field(h, 5)
    })
}

/// Environment variables removed in secure mode (spec §1.1; owner ruling 2026-10-03), matched by
/// exact name, every occurrence. Established from documentation and black-box observation only:
/// the ld.so(8) list ("Secure-execution mode": `GCONV_PATH` … `TZDIR`), plus the names a
/// set-group-ID probe of the reference library showed removed in static and dynamic programs
/// alike (`GLIBC_TUNABLES`, `LD_BIND_NOT`, `LD_BIND_NOW`, `LD_VERBOSE`, `LD_WARN`,
/// `MALLOC_ARENA_MAX`, `MALLOC_ARENA_TEST`, `MALLOC_MMAP_MAX_`, `MALLOC_MMAP_THRESHOLD_`,
/// `MALLOC_PERTURB_`, `MALLOC_TOP_PAD_`, `MALLOC_TRIM_THRESHOLD_`; the probe and its candidate
/// list are in `spec/security/threat-model.md`). Liella's own security-sensitive variables
/// (`HOSTALIASES`, `RES_OPTIONS`, `LOCALDOMAIN`, `TMPDIR`, `TZDIR`, `LOCPATH`, `GCONV_PATH`,
/// `NLSPATH`) are all in that list already. `PWD` is not removed: it is ignored where it is read.
/// An earlier version of this list cited a glibc source file and removed every `MALLOC_` name and
/// `LD_USE_LOAD_BIAS`; it was replaced on 2026-10-03.
pub const UNSECURE: &[&[u8]] = &[
    b"GCONV_PATH", b"GETCONF_DIR", b"GLIBC_TUNABLES", b"HOSTALIASES", b"LD_AUDIT", b"LD_BIND_NOT",
    b"LD_BIND_NOW", b"LD_DEBUG", b"LD_DEBUG_OUTPUT", b"LD_DYNAMIC_WEAK", b"LD_HWCAP_MASK",
    b"LD_LIBRARY_PATH", b"LD_ORIGIN_PATH", b"LD_PRELOAD", b"LD_PROFILE", b"LD_SHOW_AUXV",
    b"LD_VERBOSE", b"LD_WARN", b"LOCALDOMAIN", b"LOCPATH", b"MALLOC_ARENA_MAX", b"MALLOC_ARENA_TEST",
    b"MALLOC_MMAP_MAX_", b"MALLOC_MMAP_THRESHOLD_", b"MALLOC_PERTURB_", b"MALLOC_TOP_PAD_",
    b"MALLOC_TRACE", b"MALLOC_TRIM_THRESHOLD_", b"NIS_PATH", b"NLSPATH", b"RESOLV_HOST_CONF",
    b"RES_OPTIONS", b"TMPDIR", b"TZDIR",
];

/// The name of an environment entry (`NAME=VALUE`, or the whole entry without `=`).
pub fn entry_name(entry: &[u8]) -> &[u8] {
    entry.split(|&b| b == b'=').next().unwrap_or(entry)
}

/// Whether secure mode removes `entry`. `GLIBC_TUNABLES` is removed whole: in secure mode no
/// tunable is honoured (D-1.8).
pub fn is_unsecure(entry: &[u8]) -> bool {
    UNSECURE.contains(&entry_name(entry))
}

/// The final path component of `path` (`program_invocation_short_name`, spec §1.4): the bytes
/// after the last `/` (all of `path` if it has none).
pub fn short_name(path: &[u8]) -> &[u8] {
    path.rsplit(|&b| b == b'/').next().unwrap_or(path)
}

/// Secure mode: open `/dev/null` on each of descriptors 0–2 that is closed (spec §1.1). The
/// lowest free descriptor is the one being filled, because 0–2 are checked in order. Fails if
/// `/dev/null` cannot be opened or lands elsewhere; the caller then aborts, as glibc does.
pub fn open_std_fds<S: Sys>(sys: &S) -> KResult<()> {
    for n in 0..3 {
        let fd = Fd::new(n);
        match sys.fcntl_int(fd, FcntlInt::GetFd, 0) {
            Err(Errno::EBADF) => {
                let path = KStr::from_nul_terminated(b"/dev/null\0")?;
                let got = sys.openat(Fd::CWD, path, O_RDWR, 0)?;
                if got != fd {
                    let _ = sys.close(got);
                    return Err(Errno::EBADF);
                }
            }
            Err(e) => return Err(e),
            Ok(_) => {}
        }
    }
    Ok(())
}

/// The data half of the private start interface between `ld.so` and `libc.so.6` (version node
/// `LIELLA_PRIVATE`, plan/ph14/design.md §14, L14-10, R14-12, D-27.22): the names, the build tag
/// both products carry and [`private::StartInfo`]. Never in a public header. It lives here, beside
/// [`bias`], because both products use it: the C library defines the tag and reads `StartInfo`,
/// the loader (`liella_ld::start::private`) checks the tag and builds `StartInfo`; keeping it out
/// of the loader crate keeps the C library's code independent of the loader's.
pub mod private {
    /// The version node of the private interface.
    pub const NODE: &[u8] = b"LIELLA_PRIVATE";
    /// `__liella_libc_early_init(const StartInfo *)`: libc's start steps 3 and 4, called by `ld.so`
    /// after relocation and before any constructor (`.preinit_array` included).
    pub const EARLY_INIT: &[u8] = b"__liella_libc_early_init";
    /// `size_t __liella_thread_walk(size_t (*cb)(size_t ctx, size_t tp), size_t ctx)`: under the
    /// C library's thread-list lock, `cb(ctx, tp)` per live thread, then `cb(ctx, 0)` (DL-16d:
    /// `dlopen` gives a new module's static TLS block to every thread). Looked up only by a
    /// `dlopen` that loads an object with `PT_TLS`.
    pub const THREAD_WALK: &[u8] = b"__liella_thread_walk";
    /// `__liella_build_tag`: [`TAG_LEN`] bytes, [`TAG_PREFIX`] then the build's identity.
    pub const BUILD_TAG: &[u8] = b"__liella_build_tag";
    /// The tag's first bytes (a test finds the tag in a file by them).
    pub const TAG_PREFIX: &[u8] = b"LIELLA-BUILD-TAG:";
    /// The tag's size in bytes.
    pub const TAG_LEN: usize = 64;
    /// A build's tag from its identity: each product passes `option_env!("LIELLA_BUILD_TAG")` (a hash
    /// of the product sources `cargo xtask so` sets for both), so a loader and a C library from
    /// different builds carry different tags. The variable is read only by the two crates holding a
    /// tag (the loader and the boundary), never here: a crate reading it is rebuilt with all its
    /// dependants whenever any product source changes.
    #[must_use]
    pub const fn tag(id: Option<&str>) -> [u8; TAG_LEN] {
        tag_bytes(match id {
            Some(t) => t.as_bytes(),
            None => b"unversioned",
        })
    }

    /// [`TAG_PREFIX`] then `id`, cut at [`TAG_LEN`] bytes, zero-filled.
    const fn tag_bytes(id: &[u8]) -> [u8; TAG_LEN] {
        let mut out = [0u8; TAG_LEN];
        let mut dst: &mut [u8] = &mut out;
        let (mut first, mut second) = (TAG_PREFIX, id);
        loop {
            let b = if let Some((&b, rest)) = first.split_first() {
                first = rest;
                b
            } else if let Some((&b, rest)) = second.split_first() {
                second = rest;
                b
            } else {
                break;
            };
            let Some((d, rest)) = dst.split_first_mut() else { break };
            *d = b;
            dst = rest;
        }
        out
    }

    /// What `ld.so` passes to [`EARLY_INIT`]: the initial stack as the program sees it.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct StartInfo {
        /// `argc`.
        pub argc: usize,
        /// `argv` (the address of `argv[0]`).
        pub argv: usize,
        /// `envp` (after secure-mode clearing).
        pub envp: usize,
        /// The auxiliary vector.
        pub auxv: usize,
        /// The stack end (`__libc_stack_end`): the initial stack pointer.
        pub stack_end: usize,
        /// The program's `DT_INIT` (relocated address, 0: none). `ld.so` leaves the program's
        /// own initialisation to `__libc_start_main` (design §14, D-27.24).
        pub init: usize,
        /// The program's `DT_INIT_ARRAY` (relocated address, 0: none).
        pub init_array: usize,
        /// Entries in the program's `DT_INIT_ARRAY`.
        pub init_array_len: usize,
        /// The size of the initial thread's rseq area the loader registered at the thread pointer
        /// plus `tls::RSEQ_OFFSET` (`__rseq_size` as the loader publishes it); 0: none registered,
        /// so the C library registers none for new threads either (D-27.26).
        pub rseq_size: usize,
        /// The loader's per-thread TLS interface (DL-15 `DL-TLS-THREADS`, design §7): the bytes a
        /// new thread's static area needs directly below its thread pointer (every initially loaded
        /// module's block, the surplus and the dtv; 0: the C library may not create threads).
        pub tls_size: usize,
        /// The alignment the thread pointer of a new thread needs (a power of two).
        pub tls_align: usize,
        /// `extern "C" fn(tp) -> usize`: fill the static area below `tp` of a thread not started yet
        /// (each module's image, then zeros to its `p_memsz`, and the dtv) and store the dtv's
        /// address in the thread block's dtv word. The block's other words are the C library's.
        pub tls_init: usize,
        /// `extern "C" fn(tp) -> usize`: the thread at `tp` is ending (its last call into the
        /// loader); the area itself goes with the thread's mapping.
        pub tls_release: usize,
        /// The loader's `dlopen` entry (DL-16a, plan/ph14/dl-16-dlopen.md §1): `(file, mode, caller)`
        /// to a handle or NULL, the failure text set in the calling thread's dlerror record; 0: none.
        pub dlopen: usize,
        /// The loader's `dlsym`/`dlvsym` entry (DL-16b, plan §2): `(handle, name, version, caller)`
        /// (`version` NULL for `dlsym`) to an address or NULL with the failure text set; 0: none.
        pub dlsym: usize,
        /// The loader's readers (DL-16c, plan/ph14/dl-16-dlopen.md §4): `dl_iterate_phdr`'s walk,
        /// `(callback, data)` to the walk's result, over every loaded object; 0: none.
        pub dl_iterate: usize,
        /// `_dl_find_object`'s lookup, `(pc, result)` to 0 or -1, over every loaded object; 0: none.
        pub dl_find_object: usize,
        /// `fork`'s step (DL-16c): `(0)` before the clone (waits for a `dlopen` that is changing
        /// the loader's state), `(1)` in the parent and `(2)` in the child after it; 0: none.
        pub dl_fork: usize,
        /// The loader's `dladdr`/`dladdr1` entry (DL-18): `(addr, info, extra, flags)` to 1 with
        /// `*info` (and `*extra`) filled, or 0 when no object holds `addr`; 0: none.
        pub dladdr: usize,
        /// The loader's `dlinfo` entry (DL-18): `(handle, request, arg)` to the request's result or
        /// -1 with the failure text set; 0: none.
        pub dlinfo: usize,
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// no spec clause (D-27.22): the tag is the prefix then the identity, zero-filled, cut at its size.
        #[test]
        fn private_tag_layout() {
            assert!(tag(None).starts_with(b"LIELLA-BUILD-TAG:unversioned\0"));
            let t = tag(Some("abc"));
            let (head, rest) = t.split_at(TAG_PREFIX.len() + 3);
            assert_eq!(head, b"LIELLA-BUILD-TAG:abc");
            assert!(rest.iter().all(|&b| b == 0));
            assert_eq!(tag_bytes(&[b'x'; 100]).last(), Some(&b'x'));
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn phdr(kind: u64, vaddr: u64, filesz: u64, memsz: u64, align: u64) -> [u64; 7] {
        [kind | (4 << 32), 0, vaddr, vaddr, filesz, memsz, align]
    }

    #[test]
    fn program_headers_give_the_tls_image_and_the_bias() {
        let mut w = Vec::new();
        w.extend(phdr(1, 0x40_0000, 0x1000, 0x1000, 0x1000));
        w.extend(phdr(PT_TLS, 0x40_2000, 8, 64, 16));
        let p = program(&w, 2, 0x40_0040);
        assert_eq!(p, Program { tls: Some(TlsImage { addr: 0x40_2000, filesz: 8, memsz: 64, align: 16 }), bias: 0, relro: None });
        let mut pie = Vec::new();
        pie.extend(phdr(PT_PHDR, 0x40, 0x1c0, 0x1c0, 8));
        pie.extend(phdr(PT_TLS, 0x2000, 8, 64, 16));
        let p = program(&pie, 2, 0x5555_0040);
        assert_eq!(p.bias, 0x5555_0000);
        assert_eq!(p.tls.unwrap().addr, 0x5555_2000);
        assert_eq!(program(&w, 1, 0).tls, None, "phnum limits the headers read");
        assert_eq!(program(&w[..10], 2, 0).tls, None, "a truncated header is ignored");
        // `phnum` above the whole headers in the table: the cut keeps those (here the PT_PHDR
        // that gives the bias) rather than dropping the table.
        assert_eq!(program(&pie[..10], 2, 0x5555_0040), Program { tls: None, bias: 0x5555_0000, relro: None }, "the whole headers before a truncated one are read");
    }

    // Spec §12.6 (D-12.4, D-27.12): the `%n` check tests the segments where they were loaded,
    // by the bias the caller gives, not by a `PT_PHDR` the headers may lack (a static PIE).
    #[test]
    fn read_only_tests_the_segments_moved_by_the_given_bias() {
        let mut w = Vec::new();
        w.extend(phdr(1, 0, 0x2000, 0x2000, 0x1000)); // PT_LOAD R (flags 4)
        w.extend([1 | (6 << 32), 0, 0x3000, 0x3000, 0x1000, 0x1000, 0x1000]); // PT_LOAD RW
        let bias = 0x7f00_0000_0000;
        assert!(read_only(&w, 2, bias, bias + 0x10));
        assert!(read_only(&w, 2, bias, bias + 0x1fff));
        assert!(!read_only(&w, 2, bias, bias + 0x2000), "past the read-only segment");
        assert!(!read_only(&w, 2, bias, bias + 0x3010), "the writable segment");
        assert!(!read_only(&w, 2, bias, 0x10), "the link address is not where it was loaded");
        assert!(read_only(&w, 2, 0, 0x10));
        assert!(!read_only(&w, 1, bias, bias + 0x3010) && !read_only(&w, 0, bias, bias + 0x10), "phnum limits the headers read");
    }

    // Spec §1.1 (D-27.25, amends D-27.10): a PT_GNU_RELRO spanning writable segments is protected
    // segment by segment; a page between two segments is never in a range; one segment gives
    // exactly the single range `relro_at` gives.
    #[test]
    fn relro_pieces_stop_at_segment_boundaries() {
        let pieces = |w: &[u64], n: usize, bias: usize| {
            let mut v = Vec::new();
            relro_pieces::<()>(w, n, bias, 0x1000, |a, l| {
                v.push((a, l));
                Ok(())
            })
            .unwrap();
            v
        };
        let rw = |vaddr: u64, memsz: u64| [1 | (6 << 32), 0, vaddr, vaddr, memsz, memsz, 0x1000];
        // RELRO 0x10_0f30..0x10_5000 over three RW segments with a one-page hole after the first
        // (0x10_2000..0x10_3000) and two adjacent ones after it; a segment past RELRO is untouched.
        let mut w = Vec::new();
        w.extend(phdr(1, 0, 0x1000, 0x1000, 0x1000));
        w.extend(rw(0x10_0f30, 0x10d0)); // pages 0x10_0000..0x10_2000
        w.extend(rw(0x10_3000, 0x1000)); // 0x10_3000..0x10_4000
        w.extend(rw(0x10_4000, 0x1800)); // 0x10_4000..0x10_6000
        w.extend(phdr(PT_GNU_RELRO, 0x10_0f30, 0x40d0, 0x40d0, 1));
        w.extend(rw(0x10_8000, 0x1000));
        let bias = 0x5555_0000_0000;
        assert_eq!(
            pieces(&w, 6, bias),
            [(bias + 0x10_0000, 0x2000), (bias + 0x10_3000, 0x1000), (bias + 0x10_4000, 0x1000)],
            "the hole page 0x10_2000 is in no piece; each piece stays inside its segment's pages"
        );
        // One segment: one call, the range relro_at gives.
        let mut one = Vec::new();
        one.extend(rw(0x10_0f30, 0x5000));
        one.extend(phdr(PT_GNU_RELRO, 0x10_0f30, 0x40d0, 0x40d0, 1));
        assert_eq!(pieces(&one, 2, 0), [relro_at(&one, 2, 0, 0x1000).unwrap()]);
        assert_eq!(pieces(&one, 1, 0), [], "no PT_GNU_RELRO, nothing protected");
        // The first error stops the walk.
        let mut calls = 0;
        assert_eq!(
            relro_pieces(&w, 6, 0, 0x1000, |_, _| {
                calls += 1;
                Err(calls)
            }),
            Err(1)
        );
        assert_eq!(relro_piece(0x1000, 0x5000, usize::MAX - 0x10, 0x100, 0x1000), None, "an overflowing segment end");
    }

    #[test]
    fn relro_is_the_segment_from_its_first_page_to_its_last_whole_page() {
        // The reference library's static program (probe 2026-10-22): vaddr 0x4a5f30, memsz
        // 0x40d0, mprotect(0x4a5000, 0x5000, PROT_READ).
        let mut w = Vec::new();
        w.extend(phdr(1, 0x40_0000, 0x1000, 0x1000, 0x1000));
        w.extend(phdr(PT_GNU_RELRO, 0x4a_5f30, 0x40d0, 0x40d0, 1));
        assert_eq!(relro(&w, 2, 0x40_0040, 0x1000), Some((0x4a_5000, 0x5000)));
        assert_eq!(program(&w, 2, 0x40_0040).relro, Some((0x4a_5000, 0x5000)), "the one-pass reader agrees");
        assert_eq!(relro(&w, 1, 0x40_0040, 0x1000), None, "phnum limits the headers read");
        // A partial last page stays writable.
        let mut p = Vec::new();
        p.extend(phdr(PT_GNU_RELRO, 0x4a_5f30, 0x40c0, 0x40c0, 1));
        assert_eq!(relro(&p, 1, 0, 0x1000), Some((0x4a_5000, 0x4000)));
        // Less than one whole page: nothing to protect.
        let mut s = Vec::new();
        s.extend(phdr(PT_GNU_RELRO, 0x4a_5f30, 0x10, 0x10, 1));
        assert_eq!(relro(&s, 1, 0, 0x1000), None);
        // A PIE's segment moves with the bias.
        let mut pie = Vec::new();
        pie.extend(phdr(PT_PHDR, 0x40, 0x1c0, 0x1c0, 8));
        pie.extend(phdr(PT_GNU_RELRO, 0x2f30, 0x10d0, 0x10d0, 1));
        assert_eq!(relro(&pie, 2, 0x5555_0040, 0x1000), Some((0x5555_2000, 0x2000)));
        assert_eq!(program(&pie, 2, 0x5555_0040).relro, Some((0x5555_2000, 0x2000)));
        assert_eq!(relro(&[], 0, 0, 0x1000), None, "no segment");
        // Without PT_PHDR the caller's bias moves the segment.
        assert_eq!(relro_at(&pie[PHDR_WORDS..], 1, 0x7f00_0000, 0x1000), Some((0x7f00_2000, 0x2000)));
    }

    #[test]
    fn secure_mode_removes_the_listed_names_exactly() {
        for e in [&b"LD_PRELOAD=/x.so"[..], b"MALLOC_ARENA_MAX=1", b"MALLOC_TRIM_THRESHOLD_", b"GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2", b"TMPDIR=/tmp", b"LD_BIND_NOW="] {
            assert!(is_unsecure(e), "{}", String::from_utf8_lossy(e));
        }
        // Exact names only: the probe of 2026-10-03 shows MALLOC_CHECK_ and LD_USE_LOAD_BIAS kept.
        for e in [&b"PATH=/bin"[..], b"LD_PRELOADX=1", b"MALLOCX=1", b"MALLOC_CHECK_=3", b"LD_USE_LOAD_BIAS=1", b"HOME=/root", b""] {
            assert!(!is_unsecure(e), "{}", String::from_utf8_lossy(e));
        }
    }

    #[test]
    fn short_names_are_the_final_component() {
        assert_eq!(short_name(b"/usr/bin/ls"), b"ls");
        assert_eq!(short_name(b"ls"), b"ls");
        assert_eq!(short_name(b"dir/"), b"");
        assert_eq!(short_name(b""), b"");
    }
}
