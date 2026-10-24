//! Exit handlers (spec §1.2; D-1.2): the `atexit`/`on_exit`/`__cxa_atexit` list, the
//! `at_quick_exit` list, and the per-thread C++ `thread_local` destructor list.
//!
//! **Registries.** An entry is four words: kind, function, argument, owning object (DSO). The
//! first 32 entries are inside the registry itself, so the ISO C minimum of 32 registrations
//! never needs memory. Later ones go to an extension mapped on first use through `Pages` (up
//! to [`MAX_ENTRIES`]). Entries run LIFO, each exactly once: an entry is taken (its kind
//! cleared) under the lock before it is called, and the lock is not held during the call, so a
//! handler may register more handlers (which then run next) or call `exit` again (which then
//! runs the remaining ones, spec §1.2 failure clause 2).
//!
//! **Thread destructors** form a list of 4-word nodes from the heap, headed in the thread
//! block; they run LIFO at thread exit and, for the calling thread, at `exit`.

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering::{AcqRel, Acquire, Relaxed, Release}};

use liella_kernel::sys::Sys;
use liella_rt::heap::{Pages, PAGE};
use liella_rt::sync::RawMutex;

use crate::obstack::CCall;

/// `atexit(fn)`: `fn()`.
pub const ATEXIT: u64 = 1;
/// `on_exit(fn, arg)`: `fn(status, arg)`.
pub const ON_EXIT: u64 = 2;
/// `__cxa_atexit(fn, arg, dso)`: `fn(arg)`.
pub const CXA: u64 = 3;
/// `at_quick_exit(fn)` / `__cxa_at_quick_exit(fn, dso)`: `fn()`.
pub const QUICK: u64 = 4;

/// Entries held in the registry itself (the ISO C minimum).
pub const FIXED: usize = 32;
/// The most entries one registry holds.
pub const MAX_ENTRIES: usize = 1 << 20;

/// An entry: kind, function, argument, owning object.
pub type Entry = [u64; 4];

/// One registry.
pub struct Registry {
    lock: RawMutex,
    len: AtomicUsize,
    fixed: [AtomicU64; 4 * FIXED],
    ext: AtomicUsize,
}

impl Default for Registry {
    fn default() -> Registry {
        Registry::new()
    }
}

fn w(x: usize) -> u64 {
    u64::try_from(x).unwrap_or(u64::MAX)
}

impl Registry {
    /// Hold the registry's lock until [`Registry::release`] (`fork`, the AS-safety harness).
    pub fn hold<S: Sys>(&self, sys: &S) {
        self.lock.lock_raw(sys, liella_rt::futex::Scope::Private);
    }

    /// Release what [`Registry::hold`] took.
    pub fn release<S: Sys>(&self, sys: &S) {
        self.lock.unlock(sys, liella_rt::futex::Scope::Private);
    }

    /// An empty registry.
    pub const fn new() -> Registry {
        Registry { lock: RawMutex::new(), len: AtomicUsize::new(0), fixed: [const { AtomicU64::new(0) }; 4 * FIXED], ext: AtomicUsize::new(0) }
    }

    fn slot<'a, P: Pages>(&'a self, pages: &'a P, i: usize) -> Option<&'a [AtomicU64]> {
        match i.checked_sub(FIXED) {
            None => self.fixed.get(i.checked_mul(4)?..i.checked_mul(4)?.checked_add(4)?),
            Some(j) => match self.ext.load(Acquire) {
                0 => None,
                base => Some(pages.words(base.checked_add(j.checked_mul(32)?)?, 4)),
            },
        }
    }

    /// Add an entry; `false` without memory (spec §1.2 failure clause 1).
    pub fn register<S: Sys, P: Pages>(&self, sys: &S, pages: &P, e: Entry) -> bool {
        let _g = self.lock.lock(sys);
        let i = self.len.load(Relaxed);
        if i >= MAX_ENTRIES {
            return false;
        }
        if i >= FIXED && self.ext.load(Relaxed) == 0 {
            match pages.map((MAX_ENTRIES - FIXED) * 32, PAGE, None) {
                Some(base) => self.ext.store(base, Release),
                None => return false,
            }
        }
        let Some(s) = self.slot(pages, i) else { return false };
        for (k, v) in e.iter().enumerate().skip(1) {
            if let Some(word) = s.get(k) {
                word.store(*v, Relaxed);
            }
        }
        if let Some(kind) = s.first() {
            kind.store(e[0], Release);
        }
        self.len.store(i.saturating_add(1), Release);
        true
    }

    /// Take the most recently registered live entry.
    pub fn pop<S: Sys, P: Pages>(&self, sys: &S, pages: &P) -> Option<Entry> {
        let _g = self.lock.lock(sys);
        loop {
            let i = self.len.load(Relaxed).checked_sub(1)?;
            self.len.store(i, Release);
            if let Some(e) = self.slot(pages, i).and_then(Self::take_slot) {
                return Some(e);
            }
        }
    }

    /// Take the most recently registered live entry for which `pred` holds (others stay).
    pub fn take<S: Sys, P: Pages>(&self, sys: &S, pages: &P, pred: impl Fn(&Entry) -> bool) -> Option<Entry> {
        let _g = self.lock.lock(sys);
        for i in (0..self.len.load(Relaxed)).rev() {
            let Some(s) = self.slot(pages, i) else { continue };
            let e = Self::read(s);
            if e[0] != 0 && pred(&e) {
                return Self::take_slot(s);
            }
        }
        None
    }

    fn read(s: &[AtomicU64]) -> Entry {
        core::array::from_fn(|k| s.get(k).map_or(0, |v| v.load(Acquire)))
    }

    fn take_slot(s: &[AtomicU64]) -> Option<Entry> {
        let mut e = Self::read(s);
        e[0] = s.first()?.swap(0, AcqRel);
        (e[0] != 0).then_some(e)
    }

    /// Live entries (for tests and statistics).
    pub fn live<S: Sys, P: Pages>(&self, sys: &S, pages: &P) -> usize {
        let _g = self.lock.lock(sys);
        (0..self.len.load(Relaxed)).filter(|&i| self.slot(pages, i).and_then(|s| s.first()).is_some_and(|k| k.load(Acquire) != 0)).count()
    }
}

/// Call one entry: `fn()`, `fn(status, arg)` or `fn(arg)`.
pub fn invoke(e: &Entry, status: i32, call: &mut impl CCall) {
    let f = usize::try_from(e[1]).unwrap_or(0);
    let arg = usize::try_from(e[2]).unwrap_or(0);
    // `on_exit` passes the status as an `int`: its bit pattern, sign-extended into the register.
    let st = usize::from_ne_bytes(i64::from(status).to_ne_bytes());
    match e[0] {
        ON_EXIT => call.call3(f, st, arg, 0),
        CXA => call.call3(f, arg, 0, 0),
        _ => call.call3(f, 0, 0, 0),
    };
}

/// Run every entry, most recent first, including entries registered meanwhile.
pub fn run_all<S: Sys, P: Pages>(r: &Registry, sys: &S, pages: &P, status: i32, call: &mut impl CCall) {
    while let Some(e) = r.pop(sys, pages) {
        invoke(&e, status, call);
    }
}

/// `__cxa_finalize(dso)`: run the exit entries of `dso` (all of them for 0), most recent first,
/// each once; and forget the quick-exit entries of `dso` (they must not run after it is gone).
pub fn finalize<S: Sys, P: Pages>(exit: &Registry, quick: &Registry, sys: &S, pages: &P, dso: usize, call: &mut impl CCall) {
    let dso = w(dso);
    while let Some(e) = exit.take(sys, pages, |e| dso == 0 || (e[0] == CXA && e[3] == dso)) {
        invoke(&e, 0, call);
    }
    if dso != 0 {
        while quick.take(sys, pages, |e| e[3] == dso).is_some() {}
    }
}

/// `__cxa_thread_atexit_impl`: push `(f, obj, dso)` on the list headed at `head`, in the 4-word
/// node at `node` (fresh memory the caller allocated).
pub fn thread_atexit<P: Pages>(head: &AtomicUsize, pages: &P, node: usize, f: usize, obj: usize, dso: usize) {
    let n = pages.words(node, 4);
    for (slot, v) in n.iter().zip([w(f), w(obj), w(dso), w(head.load(Acquire))]) {
        slot.store(v, Relaxed);
    }
    head.store(node, Release);
}

/// Run the destructors on the list headed at `head`, most recent first (including ones they
/// register), handing each node back through `free`.
pub fn run_thread_dtors<P: Pages>(head: &AtomicUsize, pages: &P, call: &mut impl CCall, mut free: impl FnMut(usize)) {
    loop {
        let node = head.load(Acquire);
        if node == 0 {
            return;
        }
        let n = pages.words(node, 4);
        let word = |k: usize| n.get(k).map_or(0, |v| usize::try_from(v.load(Acquire)).unwrap_or(0));
        let (f, obj, next) = (word(0), word(1), word(3));
        head.store(next, Release);
        call.call3(f, obj, 0, 0);
        free(node);
    }
}
