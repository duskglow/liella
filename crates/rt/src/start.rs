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
pub fn program(words: &[u64], phnum: usize, phdr_addr: usize) -> Program {
    let field = |h: &[u64], i: usize| h.get(i).copied().map_or(0, |v| usize::try_from(v).unwrap_or(usize::MAX));
    let (mut phdr, mut tls, mut relro) = (None, None, None);
    for h in words.chunks_exact(PHDR_WORDS).take(phnum) {
        match h.first().map_or(0, |w| w & 0xffff_ffff) {
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
    let field = |h: &[u64], i: usize| h.get(i).copied().map_or(0, |v| usize::try_from(v).unwrap_or(usize::MAX));
    let bias = program(words, phnum, phdr_addr).bias;
    let h = words.chunks_exact(PHDR_WORDS).take(phnum).find(|h| h.first().is_some_and(|w| w & 0xffff_ffff == PT_GNU_RELRO))?;
    relro_pages(field(h, 2).wrapping_add(bias), field(h, 5), page)
}

/// `[page_down(vaddr), page_down(vaddr + memsz))` as `(start, len)`; `None` when empty.
fn relro_pages(vaddr: usize, memsz: usize, page: usize) -> Option<(usize, usize)> {
    let mask = page.wrapping_sub(1);
    let start = vaddr & !mask;
    let end = vaddr.checked_add(memsz)? & !mask;
    (end > start).then(|| (start, end.wrapping_sub(start)))
}

/// Whether `addr` lies in a loadable segment of the executable that is not writable (`%n` in a
/// fortified `printf` format, D-12.4): the program headers `words` (`phnum` of them, at
/// `phdr_addr`).
pub fn read_only(words: &[u64], phnum: usize, phdr_addr: usize, addr: usize) -> bool {
    const PT_LOAD: u64 = 1;
    const PF_W: u64 = 2;
    let bias = program(words, phnum, phdr_addr).bias;
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
