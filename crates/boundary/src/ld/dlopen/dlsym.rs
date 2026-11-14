//! `ld.so`'s `dlsym` and `dlvsym` (phase 14 DL-16b `DL-DLSYM`; plan/ph14/dl-16-dlopen.md §2;
//! spec/behaviour/dynlink.md "Symbol binding" and probes P-3..P-6). The C library's entries reach
//! [`entry`] through `StartInfo::dlsym` with the handle, the name, the version (0 for `dlsym`) and
//! the caller's return address.
//!
//! * **Scopes.** `RTLD_DEFAULT` (the NULL handle): the global scope, then, for a caller loaded by
//!   `dlopen`, the local scope it was relocated with (owner ruling #5127, probe P-5). `RTLD_NEXT`:
//!   the objects after the caller in the scope it was loaded with (the global scope for a start-time
//!   caller, its root's local scope for a `dlopen`ed one; P-5). A handle: its object's local scope
//!   (itself, then its dependencies breadth-first); the program's handle (`dlopen(NULL)`): the
//!   global scope (s27_1_c_1_3).
//! * **Answers.** `dlsym` accepts a base or default definition, `dlvsym` exactly the named version
//!   (`symbols::Want`); an `STB_GNU_UNIQUE` definition goes through the namespace's unique table and
//!   pins its definer; an IFUNC answers its resolver's result (the resolver runs after the loader's
//!   borrow ends); an `STT_TLS` symbol of a start-time object answers the calling thread's address.
//! * **Handles** are checked by one range test against the object records (D-27.32): anything else
//!   is refused with a text, never dereferenced.
//! * **Texts:** `<object>: undefined symbol: <name>[, version <v>]`, where the object is the
//!   handle's, or the caller's for the special handles (P-5); the program is named by `argv[0]`. A
//!   success clears the calling thread's pending text (#5127).
//! * **Lock.** The call takes the loader's re-entrant writer lock `CALL` and, inside it, its state
//!   lock `STATE` (a constructor may call `dlsym`): the first call builds the loader's state and a
//!   unique binding records an entry. `STATE` is released before an IFUNC resolver runs, as
//!   `dlopen` releases it before its constructors (lock order at `CALL`). Lock-free readers are
//!   DL-16c's (plan §4, `read_section`).

use super::{c_string, dlerr, file_id, loader, objs_of, scope_of, start_arg, syms_of, tb, Env, Loader, Node, Uniq, CALL, CAP, SHN_ABS, STATE};
use liella_ld::arch::Native;
use liella_ld::elf::{STB_GNU_UNIQUE, STT_GNU_IFUNC, STT_TLS};
use liella_ld::search::{self, PathBuf};
use liella_ld::symbols::{self, Request, Scope, Want};

/// `RTLD_DEFAULT` (`<dlfcn.h>`): the NULL handle.
const RTLD_DEFAULT: usize = 0;
/// `RTLD_NEXT` (`<dlfcn.h>`): `(void *) -1`.
const RTLD_NEXT: usize = usize::MAX;

/// The address of [`entry`], for `StartInfo::dlsym`.
pub(in crate::ld) fn entry_address() -> usize {
    #[allow(clippy::as_conversions)] // a function's address has no conversion other than `as`
    (entry as *const ()).expose_provenance()
}

/// What a successful lookup answers.
enum Answer {
    /// This address.
    At(usize),
    /// The result of the IFUNC resolver at this address.
    Resolve(usize),
}

/// `dlsym(handle, name)` (`version` 0) or `dlvsym(handle, name, version)` called from `caller`: the
/// address, or 0 with the calling thread's dlerror text set.
extern "C" fn entry(handle: usize, name: usize, version: usize, caller: usize) -> usize {
    CALL.lock();
    STATE.lock();
    let answer = find_locked(handle, name, version, caller);
    // The loader borrow has ended: a resolver may call back into the loader, and `fork` (which
    // takes `STATE` only) never waits for one.
    STATE.unlock();
    let at = match answer {
        Some(Answer::At(a)) => {
            dlerr::clear();
            a
        }
        Some(Answer::Resolve(r)) => {
            dlerr::clear();
            usize::from_ne_bytes(crate::ffi::liella_machine_call0(r).to_ne_bytes())
        }
        None => 0,
    };
    CALL.unlock();
    at
}

/// The object record `handle` names, by one range test (D-27.32), or `None`.
fn handle_index(l: &Loader, handle: usize) -> Option<usize> {
    let first = l.node_addr(0);
    let off = handle.checked_sub(first)?;
    let size = size_of::<Node>();
    (off.checked_rem(size)? == 0).then_some(off.checked_div(size)?).filter(|&i| i < l.n)
}

/// The object containing `caller` (the program when none does).
fn caller_index(l: &Loader, caller: usize) -> usize {
    l.nodes.get(..l.n).and_then(|ns| ns.iter().position(|nd| (nd.lo..nd.hi).contains(&caller))).unwrap_or(0)
}

/// One lookup under the lock (module documentation); `None` after setting the text.
// Every index is below `l.n` <= CAP and every count at most CAP.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn find_locked(handle: usize, name: usize, version: usize, caller: usize) -> Option<Answer> {
    let fname: &'static [u8] = if version == 0 { b"dlsym" } else { b"dlvsym" };
    let Some(l) = loader() else {
        dlerr::set(&[fname, b": cannot set up the dynamic loader's state"]);
        return None;
    };
    let sym: &'static [u8] = if name == 0 { b"" } else { c_string(name) };
    let ver: Option<&'static [u8]> = (version != 0).then(|| c_string(version));
    // The scope, and the object a failure names.
    let mut buf = [0usize; CAP];
    let mut local = [0u16; CAP];
    let (len, named) = match handle {
        RTLD_DEFAULT | RTLD_NEXT => {
            let who = caller_index(l, caller);
            let start_time = who < l.n_start;
            let len = if start_time {
                // A start-time caller: the global scope (from the caller on, for RTLD_NEXT).
                let g = &l.global[..l.n_global];
                let from = if handle == RTLD_NEXT { g.iter().position(|&o| o == who).map_or(g.len(), |p| p + 1) } else { 0 };
                let part = &g[from..];
                buf[..part.len()].copy_from_slice(part);
                part.len()
            } else {
                let root = usize::from(l.nodes[who].root);
                let llen = super::local_scope(l, root, &mut local);
                let loc = local[..llen].iter().map(|&o| usize::from(o));
                if handle == RTLD_NEXT {
                    let mut n = 0usize;
                    for o in loc.skip_while(|&o| o != who).skip(1) {
                        buf[n] = o;
                        n += 1;
                    }
                    n
                } else {
                    let mut n = l.n_global;
                    buf[..n].copy_from_slice(&l.global[..n]);
                    for o in loc {
                        if !l.nodes[o].global && n < CAP {
                            buf[n] = o;
                            n += 1;
                        }
                    }
                    n
                }
            };
            (len, who)
        }
        h => {
            let Some(i) = handle_index(l, h) else {
                dlerr::set(&[fname, b": invalid handle"]);
                return None;
            };
            if i == 0 {
                buf[..l.n_global].copy_from_slice(&l.global[..l.n_global]);
                (l.n_global, 0)
            } else {
                let llen = super::local_scope(l, i, &mut local);
                for (b, &o) in buf.iter_mut().zip(&local[..llen]) {
                    *b = usize::from(o);
                }
                (llen, i)
            }
        }
    };
    let want = ver.map_or(Want::Default, Want::Version);
    let req = Request::new(sym, want);
    let mark = l.unique.len();
    let syms = syms_of(&l.syms, l.n);
    let start = &l.global[..l.n_start_global];
    let found = symbols::lookup(syms, Scope::global(&buf[..len]), &req, &mut Uniq { table: &mut l.unique, syms, start });
    let Ok(Some(f)) = found else {
        let mut path = PathBuf::default();
        let obj = object_name(l, named, &mut path);
        match ver {
            Some(v) => dlerr::set(&[obj, b": undefined symbol: ", sym, b", version ", v]),
            None => dlerr::set(&[obj, b": undefined symbol: ", sym]),
        }
        return None;
    };
    if f.def.st.bind() == STB_GNU_UNIQUE {
        super::pin_unique(l, mark);
    }
    let o = objs_of(&l.objs, l.n)[f.obj];
    let st = f.def.st;
    let at = |v: u64| usize::try_from(v).unwrap_or(0);
    match st.kind() {
        STT_TLS => {
            let Some(p) = o.tls else {
                let mut path = PathBuf::default();
                dlerr::set(&[object_name(l, f.obj, &mut path), b": cannot find the thread-local block of symbol ", sym]);
                return None;
            };
            let off = isize::try_from(p.tp_offset).unwrap_or(0);
            Some(Answer::At(tb::tp().wrapping_add_signed(off).wrapping_add(at(st.value))))
        }
        STT_GNU_IFUNC => Some(Answer::Resolve(at(o.bias.wrapping_add(st.value)))),
        _ if st.shndx == SHN_ABS => Some(Answer::At(at(st.value))),
        _ => Some(Answer::At(at(o.bias.wrapping_add(st.value)))),
    }
}

/// The name a failure text gives object `i`: the program's `argv[0]`; a `dlopen`ed object's path
/// as opened; a start-time object's path as the start's search found it (searched again here, only
/// for a text), else its `DT_SONAME`.
fn object_name<'p>(l: &Loader, i: usize, out: &'p mut PathBuf) -> &'p [u8] {
    if i == 0 {
        let argv = start_arg(1);
        let a0 = if argv == 0 { 0 } else { usize::try_from(super::super::stack_word(argv)).unwrap_or(0) };
        let s = if a0 == 0 { &b""[..] } else { c_string(a0) };
        out.clear();
        let _ = out.push(s);
        return out.as_bytes();
    }
    let p = l.path(i);
    if !p.is_empty() {
        out.clear();
        let _ = out.push(p);
        return out.as_bytes();
    }
    if i < l.n_start && start_path(l, i, out) {
        return out.as_bytes();
    }
    let name = l.objs_upto(l.n).get(i).map_or(&b""[..], |o| o.name);
    out.clear();
    let _ = out.push(name);
    out.as_bytes()
}

/// The path at which the start found start-time object `i`: the first `DT_NEEDED` entry of a
/// start-time object that names it, as a path or searched again with that object as the requester,
/// matched by (`st_dev`, `st_ino`). False when none matches.
pub(super) fn start_path(l: &Loader, i: usize, out: &mut PathBuf) -> bool {
    let Some(node) = l.nodes.get(i) else { return false };
    let id = (node.dev, node.ino);
    let soname = l.objs_upto(l.n).get(i).and_then(super::soname);
    let env = Env::read();
    let ctx = env.ctx();
    for q in 0..l.n_start {
        let Some(o) = l.objs_upto(l.n).get(q).copied() else { continue };
        for needed in o.dynm.needed().map_while(Result::ok) {
            if needed.contains(&b'/') {
                if file_id(needed) == Some(id) {
                    out.clear();
                    return out.push(needed);
                }
                continue;
            }
            if soname != Some(needed) {
                continue;
            }
            let chain = [scope_of(l, q), scope_of(l, 0)];
            let mut hit = false;
            search::search::<Native>(needed, &chain, &ctx, &mut |p: &mut PathBuf| {
                let found = p.as_bytes().strip_suffix(b"\0").unwrap_or(p.as_bytes());
                if file_id(found) == Some(id) {
                    out.clear();
                    hit = out.push(found);
                    return true;
                }
                false
            });
            if hit {
                return true;
            }
        }
    }
    false
}
