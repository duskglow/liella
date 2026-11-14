//! `ld.so`'s `dladdr`, `dladdr1` and `dlinfo` (phase 14 DL-18 `DL-DLADDR`; plan/ph14/design.md §8
//! and §10, plan/ph14/dl-16-dlopen.md §4; spec/behaviour/dynlink.md "dladdr and dlinfo"). The C
//! library's entries reach [`dladdr_entry`] through `StartInfo::dladdr` and [`dlinfo_entry`]
//! through `StartInfo::dlinfo`.
//!
//! * **Readers.** Both read the published objects through [`read_section`] and take no lock
//!   (D-27.1): a crash handler or profiler may call `dladdr` while another thread is in `dlopen`.
//!   Only the process's first loader call builds the loader's state under the lock.
//! * **The object** holding an address: the first object in chain order whose mapped range (lowest
//!   to highest `PT_LOAD` address) holds it. `dli_fname` is its path (a start-time object's as the
//!   start found it, a `dlopen`ed one's as opened), the program's `argv[0]`; `dli_fbase` the page
//!   holding its lowest `PT_LOAD` address.
//! * **The symbol** (probe-settled): the defined dynamic symbol, not `STT_TLS` nor `SHN_ABS`, that
//!   holds the address (`st_value <= addr < st_value + st_size`, or `addr == st_value` for size 0)
//!   with the highest start; at equal starts the lowest symbol index. An IFUNC is reported by its
//!   resolver's address (its `st_value`). None holds it: `dli_sname` and `dli_saddr` NULL.
//! * **`dlinfo`** answers `RTLD_DI_LMID`, `LINKMAP`, `ORIGIN`, `SERINFO`, `SERINFOSIZE`,
//!   `TLS_MODID`, `TLS_DATA` and `PHDR`; any other request is refused with the reference's text.
//!   A handle must be a published link map (one comparison per object, never dereferenced).

use super::dlsym::start_path;
use super::{dlerr, dyn_string, read_section, start_arg, tb, Env, Loader, View, CAP, DT_RPATH, DT_RUNPATH, SHN_ABS, START_PAGE};
use core::sync::atomic::Ordering::Relaxed;
use liella_ld::arch::Native;
use liella_ld::elf::{dt, pt, Sym, SHN_UNDEF, STT_TLS};
use liella_ld::search::{self, PathBuf, Scope};
use liella_ld::symbols::Object;

/// `dladdr1` flags (`<dlfcn.h>`).
const RTLD_DL_SYMENT: i32 = 1;
const RTLD_DL_LINKMAP: i32 = 2;
/// `dlinfo` requests (`<dlfcn.h>`).
const RTLD_DI_LMID: i32 = 1;
const RTLD_DI_LINKMAP: i32 = 2;
const RTLD_DI_SERINFO: i32 = 4;
const RTLD_DI_SERINFOSIZE: i32 = 5;
const RTLD_DI_ORIGIN: i32 = 6;
const RTLD_DI_TLS_MODID: i32 = 9;
const RTLD_DI_TLS_DATA: i32 = 10;
const RTLD_DI_PHDR: i32 = 11;
/// `Elf64_Sym`'s size.
const SYM_SIZE: usize = 24;
/// `Dl_serinfo`: `dls_size`, `dls_cnt`, then the `Dl_serpath` array (`dls_name`, `dls_flags`).
const SERINFO_HEAD: usize = 16;
const SERPATH_SIZE: usize = 16;

/// The address of [`dladdr_entry`], for `StartInfo::dladdr`.
pub(in crate::ld) fn dladdr_address() -> usize {
    #[allow(clippy::as_conversions)] // a function's address has no conversion other than `as`
    (dladdr_entry as *const ()).expose_provenance()
}

/// The address of [`dlinfo_entry`], for `StartInfo::dlinfo`.
pub(in crate::ld) fn dlinfo_address() -> usize {
    #[allow(clippy::as_conversions)] // a function's address has no conversion other than `as`
    (dlinfo_entry as *const ()).expose_provenance()
}

/// Write `v` at the caller's address `at`.
fn put<T: Copy>(at: usize, v: T) {
    // UNSAFE(slice): a store into the caller's out-parameter (dladdr(3), dlinfo(3)).
    #[allow(unsafe_code)]
    // SAFETY: the C contract: `at` is the caller's object of type `T` (a `Dl_info`, the `dladdr1`
    // extra pointer, or the `dlinfo` argument for this request); NULL was refused before. Unaligned
    // stores, as a caller's buffer may be any `char` array.
    unsafe { core::ptr::with_exposed_provenance_mut::<T>(at).write_unaligned(v) }
}

/// Read a `T` at the caller's address `at`.
fn get<T: Copy>(at: usize) -> T {
    // UNSAFE(slice): a load from the caller's `Dl_serinfo` (dlinfo(3)).
    #[allow(unsafe_code)]
    // SAFETY: the C contract: `at` is inside the caller's `Dl_serinfo`, filled by the sizing call.
    unsafe { core::ptr::with_exposed_provenance::<T>(at).read_unaligned() }
}

/// Copy `s` and a NUL to the caller's buffer at `at`.
fn put_str(at: usize, s: &[u8]) {
    for (k, &b) in s.iter().chain(&[0]).enumerate() {
        put(at.wrapping_add(k), b);
    }
}

/// The symbol of `syms` (load bias `bias`) that holds `addr`, with its index (module documentation).
fn containing(syms: &Object<'_>, bias: u64, addr: u64) -> Option<(u32, Sym)> {
    let tab = syms.symtab();
    let mut best: Option<(u32, Sym)> = None;
    for k in 0..tab.count()? {
        let Ok(s) = tab.get(k) else { break };
        if s.shndx == SHN_UNDEF || s.shndx == SHN_ABS || s.kind() == STT_TLS {
            continue;
        }
        let start = bias.wrapping_add(s.value);
        let Some(off) = addr.checked_sub(start) else { continue };
        if off >= s.size && off != 0 {
            continue;
        }
        if best.is_none_or(|(_, b)| start > bias.wrapping_add(b.value)) {
            best = Some((k, s));
        }
    }
    best
}

/// The C string `dli_fname` gives object `i` (module documentation).
fn file_name(v: &View, i: usize) -> usize {
    let (path, len) = v.path(i);
    if len != 0 {
        return path;
    }
    if i == 0 {
        let argv = start_arg(1);
        let a0 = if argv == 0 { 0 } else { usize::try_from(super::super::stack_word(argv)).unwrap_or(0) };
        if a0 != 0 {
            return a0;
        }
    }
    v.l_name(i)
}

/// `dladdr(addr, info)` (`flags` 0) and `dladdr1(addr, info, extra, flags)`: 1 with `*info` filled
/// (and `*extra` for `RTLD_DL_SYMENT`/`RTLD_DL_LINKMAP`), or 0 with both untouched when no object
/// holds `addr`. The dlerror state is not touched.
extern "C" fn dladdr_entry(addr: usize, info: usize, extra: usize, flags: i32) -> i32 {
    if info == 0 {
        return 0;
    }
    let found = read_section(|v| {
        let i = v.find(|i| {
            let (lo, hi) = v.range(i);
            lo <= addr && addr < hi
        })?;
        let o = v.obj(i);
        let at = |x: u64| usize::try_from(x).unwrap_or(0);
        let page = usize::try_from(START_PAGE.load(Relaxed)).unwrap_or(4096).max(1);
        let base = v.range(i).0 & !page.wrapping_sub(1);
        let sym = containing(v.syms(i), o.bias, u64::try_from(addr).unwrap_or(0));
        let (sname, saddr, entry) = match (sym, o.dynm.get(dt::STRTAB), o.dynm.get(dt::SYMTAB)) {
            (Some((k, s)), Some(strtab), Some(symtab)) => {
                let table = at(o.bias.wrapping_add(symtab));
                (at(o.bias.wrapping_add(strtab).wrapping_add(u64::from(s.name))), at(o.bias.wrapping_add(s.value)), table.wrapping_add(usize::try_from(k).unwrap_or(0).wrapping_mul(SYM_SIZE)))
            }
            _ => (0, 0, 0),
        };
        put(info, [file_name(v, i), base, sname, saddr]);
        if extra != 0 {
            match flags {
                RTLD_DL_SYMENT => put(extra, entry),
                RTLD_DL_LINKMAP => put(extra, v.node_addr(i)),
                _ => {}
            }
        }
        Some(())
    });
    i32::from(found.flatten().is_some())
}

/// Object `i`'s search scope as a reader sees it: its `DT_RPATH`, `DT_RUNPATH` and `$ORIGIN`.
fn scope_in(v: &View, i: usize) -> Scope<'static> {
    let o = v.obj(i);
    Scope { rpath: dyn_string(&o.dynm, DT_RPATH), runpath: dyn_string(&o.dynm, DT_RUNPATH), origin: origin(v, i) }
}

/// The directory of object `i`'s path (`$ORIGIN`), `None` when its path is unknown.
fn origin(v: &View, i: usize) -> Option<&'static [u8]> {
    let (at, len) = v.path(i);
    if len == 0 {
        return None;
    }
    // UNSAFE(slice): a published span of the loader's path arena.
    #[allow(unsafe_code)]
    // SAFETY: the span was written before its node was published and is never rewritten; the
    // loader's mapping is never unmapped.
    let path = unsafe { crate::conv::bytes_at(at, len) };
    let cut = path.iter().rposition(|&b| b == b'/')?;
    path.get(..cut.max(1))
}

/// `RTLD_DI_SERINFOSIZE` (`fill` false) or `RTLD_DI_SERINFO` (`fill` true) of object `i` into the
/// caller's `Dl_serinfo` at `arg`. The sizing call stores `dls_size` and `dls_cnt`; the filling call
/// writes at most the caller's `dls_cnt` entries and never past its `dls_size` bytes, the strings
/// after the entries, and stores the entries written in `dls_cnt` (D-NEW, DL-DLADDR).
fn serinfo(v: &View, i: usize, arg: usize, fill: bool) {
    let env = Env::read_paths();
    let ctx = env.ctx();
    let me = scope_in(v, i);
    let chain_two = [me, scope_in(v, 0)];
    let chain = if i == 0 { &chain_two[..1] } else { &chain_two[..] };
    let (mut cnt, mut bytes) = (0usize, 0usize);
    search::dirs::<Native>(chain, &ctx, &mut |d| {
        cnt = cnt.saturating_add(1);
        bytes = bytes.saturating_add(d.len()).saturating_add(1);
        true
    });
    if !fill {
        put(arg, SERINFO_HEAD.saturating_add(cnt.saturating_mul(SERPATH_SIZE)).saturating_add(bytes));
        put(arg.wrapping_add(8), u32::try_from(cnt).unwrap_or(u32::MAX));
        return;
    }
    let size: usize = get(arg);
    let room: u32 = get(arg.wrapping_add(8));
    let slots = cnt.min(usize::try_from(room).unwrap_or(0));
    let mut text = SERINFO_HEAD.saturating_add(slots.saturating_mul(SERPATH_SIZE));
    let mut k = 0usize;
    search::dirs::<Native>(chain, &ctx, &mut |d| {
        let end = text.saturating_add(d.len()).saturating_add(1);
        if k >= slots || end > size {
            return false;
        }
        put_str(arg.wrapping_add(text), d);
        let entry = arg.wrapping_add(SERINFO_HEAD).wrapping_add(k.wrapping_mul(SERPATH_SIZE));
        put(entry, arg.wrapping_add(text));
        put(entry.wrapping_add(8), 0u32);
        (text, k) = (end, k.wrapping_add(1));
        true
    });
    put(arg.wrapping_add(8), u32::try_from(k).unwrap_or(0));
}

/// The run-time address of object `i`'s program header table and its entry count.
fn phdr(v: &View, i: usize) -> (usize, usize) {
    let o = v.obj(i);
    let n = o.elf.program_headers().count();
    let at = |x: u64| usize::try_from(o.bias.wrapping_add(x)).unwrap_or(0);
    if let Some(p) = o.elf.find(pt::PHDR) {
        return (at(p.vaddr), n);
    }
    let off = o.elf.header().phoff;
    let load = o.elf.program_headers().find(|p| p.p_type == pt::LOAD && p.offset <= off && off < p.offset.saturating_add(p.filesz));
    (load.map_or(0, |p| at(p.vaddr.wrapping_add(off.wrapping_sub(p.offset)))), n)
}

/// One `dlinfo` request on object `i`: its result, or the failure text.
fn request(v: &View, i: usize, req: i32, arg: usize) -> Result<i32, &'static [u8]> {
    match req {
        RTLD_DI_LMID => put(arg, 0i64),
        RTLD_DI_LINKMAP => put(arg, v.node_addr(i)),
        RTLD_DI_SERINFO | RTLD_DI_SERINFOSIZE => serinfo(v, i, arg, req == RTLD_DI_SERINFO),
        RTLD_DI_ORIGIN => put_str(arg, origin(v, i).ok_or(&b"dlinfo: the origin of the object is unknown"[..])?),
        RTLD_DI_TLS_MODID => put(arg, v.obj(i).tls.map_or(0, |p| usize::try_from(p.module).unwrap_or(0))),
        RTLD_DI_TLS_DATA => {
            let data = v.obj(i).tls.map_or(0, |p| tb::tp().wrapping_add_signed(isize::try_from(p.tp_offset).unwrap_or(0)));
            put(arg, data);
        }
        RTLD_DI_PHDR => {
            let (at, n) = phdr(v, i);
            put(arg, at);
            return Ok(i32::try_from(n).unwrap_or(i32::MAX));
        }
        _ => return Err(b"unsupported dlinfo request"),
    }
    Ok(0)
}

/// `dlinfo(handle, request, arg)`: the request's result (0, or the header count for
/// `RTLD_DI_PHDR`), or -1 with the calling thread's dlerror text set. A success clears a pending
/// text (#5127).
extern "C" fn dlinfo_entry(handle: usize, req: i32, arg: usize) -> i32 {
    let r = read_section(|v| {
        let i = v.find(|i| v.node_addr(i) == handle).ok_or(&b"dlinfo: invalid handle"[..])?;
        if arg == 0 {
            return Err(&b"dlinfo: invalid argument"[..]);
        }
        request(v, i, req, arg)
    });
    match r {
        Some(Ok(n)) => {
            dlerr::clear();
            n
        }
        Some(Err(text)) => {
            dlerr::set(&[text]);
            -1
        }
        None => {
            dlerr::set(&[b"dlinfo: invalid handle"]);
            -1
        }
    }
}

/// Record each start-time object's path as the start found it (its `$ORIGIN`, `dli_fname`), once,
/// while the loader is built under the lock; the program and `ld.so` keep none.
// Indices stay below n_start < CAP.
#[allow(clippy::indexing_slicing)]
pub(super) fn keep_start_paths(l: &mut Loader) {
    for i in 1..l.n_start.min(CAP) {
        if l.nodes[i].dev == u64::MAX || l.nodes[i].path.1 != 0 {
            continue;
        }
        let mut out = PathBuf::default();
        if start_path(l, i, &mut out) {
            if let Some(span) = l.keep(out.as_bytes()) {
                l.nodes[i].path = span;
            }
        }
    }
}
