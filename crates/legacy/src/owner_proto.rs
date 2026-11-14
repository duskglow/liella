//! Prototype for DL-NONSHARED-OPTOUT (handoff/DL-NONSHARED-OPTOUT.md): working out which loaded
//! module a registration (`atexit`, `at_quick_exit`) belongs to WITHOUT the `libc_nonshared.a`
//! stub passing the module's `__dso_handle`. Test-only: compiled under `selftest` alone, used
//! by no product, exports nothing.
//!
//! The model:
//!
//! * [`ModuleMap`] is the loader's map of loaded modules: sorted, non-overlapping address ranges
//!   (each module's `PT_LOAD` segments), each tagged with the module's handle. A handle is
//!   unique per load (a module unloaded and loaded again at the same address gets a new one),
//!   and is never [`PROCESS`]. [`ModuleMap::owner_of`] is a binary search.
//! * [`attribute`] is mechanism (a) of the handoff, evaluated eagerly at registration: the
//!   owner is the module holding the caller's return address; with [`Policy::Handler`]
//!   mechanism (b), the module holding the handler; an explicit non-zero `dso` (a stub, a C++
//!   static destructor, a foreign module) always wins. No module (JIT or anonymous memory):
//!   [`PROCESS`], whose handlers run only at exit.
//! * [`finalize_module`] is what the loader would do when a module is unloaded: run (exit list)
//!   or forget (quick-exit list) every entry the module owns, by handle or by a `dso` address
//!   inside its ranges (the stub's `__dso_handle` lies in the module's data); and, with the
//!   safety net on, every entry whose HANDLER lies in the module, whoever owns it, so no handler
//!   is ever called after its code is unmapped (a tail call `return atexit(f)` returns to the
//!   caller's caller, so caller attribution alone can name the wrong module).

use liella_kernel::sys::Sys;
use liella_rt::heap::Pages;

use crate::exit_handlers::{invoke, Entry, Registry, CXA, QUICK};
use crate::obstack::CCall;

/// The owner of a registration no loaded module holds: the process (handlers run at exit only).
pub const PROCESS: u64 = 0;

/// Which address names the owner of a registration without an explicit `dso`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// The module holding the caller's return address (mechanism (a)).
    Caller,
    /// The module holding the handler's code (mechanism (b)).
    Handler,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Range {
    lo: usize,
    hi: usize,
    owner: u64,
}

/// The loaded modules' address ranges, sorted by start, at most `N` ranges.
pub struct ModuleMap<const N: usize> {
    ranges: [Range; N],
    len: usize,
    next: u64,
}

impl<const N: usize> Default for ModuleMap<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ModuleMap<N> {
    /// An empty map; handles start at `0x4c00_0000_0000_0001` so they never equal a user address.
    pub const fn new() -> Self {
        ModuleMap { ranges: [Range { lo: 0, hi: 0, owner: 0 }; N], len: 0, next: 0x4c00_0000_0000_0001 }
    }

    fn live(&self) -> &[Range] {
        self.ranges.get(..self.len).unwrap_or(&[])
    }

    /// Map a module with the half-open ranges `segs`; its new handle, or `None` when a range is
    /// empty, overlaps a loaded one, or the map is full (nothing is changed then).
    pub fn load(&mut self, segs: &[(usize, usize)]) -> Option<u64> {
        if self.len.checked_add(segs.len())? > N {
            return None;
        }
        for (k, &(lo, hi)) in segs.iter().enumerate() {
            let clash = |r: &Range| lo < r.hi && r.lo < hi;
            if lo >= hi || self.live().iter().any(clash) || segs.iter().skip(k.saturating_add(1)).any(|&(l, h)| lo < h && l < hi) {
                return None;
            }
        }
        let owner = self.next;
        self.next = self.next.checked_add(1)?;
        for &(lo, hi) in segs {
            let at = self.live().partition_point(|r| r.lo < lo);
            let end = self.len;
            self.ranges.copy_within(at..end, at.checked_add(1)?);
            *self.ranges.get_mut(at)? = Range { lo, hi, owner };
            self.len = end.checked_add(1)?;
        }
        Some(owner)
    }

    /// Remove the ranges of `owner` (after [`finalize_module`]).
    pub fn unload(&mut self, owner: u64) {
        let mut keep = 0usize;
        for i in 0..self.len {
            if let Some(r) = self.ranges.get(i).copied() {
                if r.owner != owner {
                    if let Some(slot) = self.ranges.get_mut(keep) {
                        *slot = r;
                    }
                    keep = keep.saturating_add(1);
                }
            }
        }
        self.len = keep;
    }

    /// The module holding `addr`, and the number of range comparisons the search made.
    pub fn owner_of(&self, addr: usize) -> (Option<u64>, u32) {
        let (mut lo, mut hi, mut steps) = (0usize, self.len, 0u32);
        while lo < hi {
            let mid = lo.saturating_add((hi.saturating_sub(lo)) / 2);
            steps = steps.saturating_add(1);
            match self.ranges.get(mid) {
                Some(r) if addr < r.lo => hi = mid,
                Some(r) if addr >= r.hi => lo = mid.saturating_add(1),
                Some(r) => return (Some(r.owner), steps),
                None => break,
            }
        }
        (None, steps)
    }

    /// The ranges of `owner` (what the loader passes to [`finalize_module`]).
    pub fn ranges_of(&self, owner: u64) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.live().iter().filter(move |r| r.owner == owner).map(|r| (r.lo, r.hi))
    }
}

/// The owner to record for a registration of handler `f` called from return address `ret`
/// with explicit `dso` (0 for the opted-in entry points, which have none).
pub fn attribute<const N: usize>(map: &ModuleMap<N>, policy: Policy, dso: usize, ret: usize, f: usize) -> u64 {
    if dso != 0 {
        return u64::try_from(dso).unwrap_or(u64::MAX);
    }
    let at = match policy {
        Policy::Caller => ret,
        Policy::Handler => f,
    };
    map.owner_of(at).0.unwrap_or(PROCESS)
}

/// `atexit(f)` through the opted-in entry point: an exit entry `f()` owned per [`attribute`].
pub fn opted_atexit<const N: usize>(map: &ModuleMap<N>, policy: Policy, ret: usize, f: usize) -> Entry {
    [CXA, w(f), 0, attribute(map, policy, 0, ret, f)]
}

/// `at_quick_exit(f)` through the opted-in entry point.
pub fn opted_quick<const N: usize>(map: &ModuleMap<N>, policy: Policy, ret: usize, f: usize) -> Entry {
    [QUICK, w(f), 0, attribute(map, policy, 0, ret, f)]
}

fn w(x: usize) -> u64 {
    u64::try_from(x).unwrap_or(u64::MAX)
}

/// Unloading module `owner` (ranges `segs`): run its exit entries, most recent first, each
/// once, and forget its quick-exit entries. An entry is the module's when its owner word is
/// `owner` or an address inside `segs`; with `net`, also when its handler lies inside `segs`.
#[allow(clippy::too_many_arguments)]
pub fn finalize_module<S: Sys, P: Pages>(exit: &Registry, quick: &Registry, sys: &S, pages: &P, owner: u64, segs: &[(usize, usize)], net: bool, call: &mut impl CCall) {
    let inside = |v: u64| usize::try_from(v).is_ok_and(|a| segs.iter().any(|&(lo, hi)| lo <= a && a < hi));
    let mine = |e: &Entry| e[3] == owner || inside(e[3]) || (net && inside(e[1]));
    while let Some(e) = exit.take(sys, pages, |e| e[0] == CXA && mine(e)) {
        invoke(&e, 0, call);
    }
    while quick.take(sys, pages, |e| mine(e)).is_some() {}
}
