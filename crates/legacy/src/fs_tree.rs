//! The C shapes of the tree walks and temporary names (spec §13.3, §13.9; D-13.4, D-13.13):
//! `nftw`/`ftw` over `liella::fs::walk`, the `mktemp` convention, and the stable sort the
//! comparison callbacks of `scandir` and `fts` use. The `fts` family is in [`crate::fts`].
//!
//! * `FTW_CHDIR` is the one place a walk changes the working directory (D-13.4: the contract
//!   makes it observable): the walk itself stays descriptor-relative, and this layer follows it
//!   with `fchdir` (into each directory it enters, back to the parent when it leaves), then
//!   returns to the caller's working directory, also when the walk ends early or fails.
//! * `nopenfd` is accepted and ignored (D-13.4, P-lift): there is no depth limit.
//! * Paths of any length are walked (D-13.13); the walk fails with `ENAMETOOLONG` only for a
//!   path the callback's `int` offsets cannot express.

use core::ffi::c_void;

use core::cell::Cell;

use liella::error::{Errno, Result};
use liella::fs::temp;
use liella::fs::walk::{self, Action, Event, Kind, Options};
use liella::mem::TryVec;
use liella_abi::callback::{FtwFn, NftwFn, SlotCompareFn};
use liella_abi::layout::struct_FTW;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Stat, O_CLOEXEC, O_DIRECTORY, O_PATH};

/// `FTW_PHYS`.
pub const FTW_PHYS: i32 = 1;
/// `FTW_MOUNT`.
pub const FTW_MOUNT: i32 = 2;
/// `FTW_CHDIR`.
pub const FTW_CHDIR: i32 = 4;
/// `FTW_DEPTH`.
pub const FTW_DEPTH: i32 = 8;
/// `FTW_ACTIONRETVAL`.
pub const FTW_ACTIONRETVAL: i32 = 16;

/// The `FTW_*` type code of a kind (`ftw` reports a dangling link as `FTW_NS`).
fn code(kind: Kind, ftw: bool) -> i32 {
    match kind {
        Kind::File => 0,
        Kind::Dir => 1,
        Kind::DirUnreadable => 2,
        Kind::NoStat => 3,
        Kind::Symlink => 4,
        Kind::DirPost => 5,
        Kind::Dangling if ftw => 3,
        Kind::Dangling => 6,
    }
}

/// What the callback's return value asks for: without `FTW_ACTIONRETVAL` any non-zero value
/// ends the walk and is returned; with it `FTW_CONTINUE` (0), `FTW_STOP` (1, returned),
/// `FTW_SKIP_SUBTREE` (2), `FTW_SKIP_SIBLINGS` (3), and any other value ends the walk and is
/// returned (as the record shows for 99).
fn action(r: i32, retval: bool) -> Action {
    match (retval, r) {
        (_, 0) => Action::Continue,
        (true, 2) => Action::SkipSubtree,
        (true, 3) => Action::SkipSiblings,
        (_, v) => Action::Stop(v),
    }
}

/// Run a walk, following it with the working directory if `chdir`, and return to the caller's
/// working directory at the end.
fn run<S: Sys>(sys: &S, path: &[u8], opt: Options, chdir: bool, visit: &mut dyn FnMut(&walk::Visit<'_>) -> i32, retval: bool, st: &mut WalkState) -> Result<i32> {
    let orig = if chdir { Some(sys.openat(Fd::CWD, KStr::from_nul_terminated(b".\0")?, O_PATH | O_DIRECTORY | O_CLOEXEC, 0)?) } else { None };
    st.orig = orig;
    let r = walk::walk_levels(sys, &mut st.levels, path, opt, &mut |ev| match ev {
        Event::Enter(fd) => {
            if orig.is_some() {
                sys.fchdir(fd)?;
            }
            Ok(Action::Continue)
        }
        Event::Leave(to) => {
            if let Some(o) = orig {
                sys.fchdir(to.unwrap_or(o))?;
            }
            Ok(Action::Continue)
        }
        Event::Visit(v) => Ok(action(visit(&v), retval)),
    });
    if let Some(o) = st.orig.take() {
        let back = sys.fchdir(o);
        let _ = sys.close(o);
        if r.is_ok() {
            back?;
        }
    }
    r
}

/// What a walk holds that must be given back if a callback throws (OD-6 revised): its open
/// directories and the caller's working directory (`FTW_CHDIR`).
pub struct WalkState {
    /// The open directories.
    pub levels: walk::Levels,
    /// The caller's working directory, while the walk follows it with its own.
    pub orig: Option<Fd>,
}

impl Default for WalkState {
    fn default() -> WalkState {
        WalkState { levels: walk::Levels::new(Fd::CWD), orig: None }
    }
}

/// A callback threw out of a walk (OD-6 revised): its directories are closed and the caller's
/// working directory restored.
pub fn unwound<S: Sys>(sys: &S, st: &mut WalkState) {
    st.levels.close_all(sys);
    if let Some(o) = st.orig.take() {
        let _ = sys.fchdir(o);
        let _ = sys.close(o);
    }
}

/// `nftw(path, fn, nopenfd, flags)`: 0, the callback's value that ended the walk, or the error.
pub fn nftw<S: Sys>(sys: &S, path: &[u8], f: NftwFn, flags: i32, st: &mut WalkState) -> Result<i32> {
    let opt = Options { physical: flags & FTW_PHYS != 0, mount: flags & FTW_MOUNT != 0, depth: flags & FTW_DEPTH != 0 };
    let zero = Stat::default();
    let mut visit = |v: &walk::Visit<'_>| {
        let mut w = struct_FTW { base: i32::try_from(v.base).unwrap_or(i32::MAX), level: i32::try_from(v.level).unwrap_or(i32::MAX) };
        let st: *const Stat = v.stat.unwrap_or(&zero);
        f(v.path.as_ptr().cast(), st.cast::<c_void>(), code(v.kind, false), core::ptr::from_mut(&mut w).cast())
    };
    run(sys, path, opt, flags & FTW_CHDIR != 0, &mut visit, flags & FTW_ACTIONRETVAL != 0, st)
}

/// `ftw(path, fn, nopenfd)`: `nftw` without flags (links followed), a dangling link reported as
/// `FTW_NS`.
pub fn ftw<S: Sys>(sys: &S, path: &[u8], f: FtwFn, st: &mut WalkState) -> Result<i32> {
    let zero = Stat::default();
    let mut visit = |v: &walk::Visit<'_>| {
        let st: *const Stat = v.stat.unwrap_or(&zero);
        f(v.path.as_ptr().cast(), st.cast::<c_void>(), code(v.kind, true))
    };
    run(sys, path, Options::default(), false, &mut visit, false, st)
}

/// `mktemp(template)`: a name that does not exist, nothing created. On failure the template
/// becomes the empty string (mktemp(3)) and the error is returned for `errno`.
pub fn mktemp<S: Sys>(sys: &S, template: &mut [u8]) -> Option<Errno> {
    match temp::make(sys, Fd::CWD, template, 0, temp::Create::Name) {
        Ok(_) => None,
        Err(e) => {
            if let Some(first) = template.first_mut() {
                *first = 0;
            }
            Some(e)
        }
    }
}

/// Sort `items` stably by a C comparison of slot values: `cmp` receives the addresses of two
/// variables holding `key(a)` and `key(b)` (`const struct dirent **`, `const FTSENT **`).
pub fn sort_slots<T: Copy>(items: &mut TryVec<T>, key: impl Fn(&T) -> usize, cmp: SlotCompareFn) -> Result<()> {
    sort_slots_in(items, key, cmp, &mut None, &Cell::new(true))
}

/// [`sort_slots`] with its buffer in `spare`; `items_hold` is true while `items` holds every
/// element (else `spare` does), for a caller that must recover when `cmp` throws (OD-6 revised).
pub fn sort_slots_in<T: Copy>(items: &mut TryVec<T>, key: impl Fn(&T) -> usize, cmp: SlotCompareFn, spare: &mut Option<TryVec<T>>, items_hold: &Cell<bool>) -> Result<()> {
    let n = items.len();
    if n < 2 {
        return Ok(());
    }
    let mut buf: TryVec<T> = TryVec::with_capacity(n)?;
    buf.extend_from_slice(items)?;
    let tmp = spare.insert(buf);
    let le = |a: &T, b: &T| {
        let (ka, kb) = (key(a), key(b));
        let pa: *const usize = &ka;
        let pb: *const usize = &kb;
        cmp(pa.cast(), pb.cast()) <= 0
    };
    let mut width = 1usize;
    let mut src_is_items = true;
    while width < n {
        {
            let (src, dst): (&[T], &mut [T]) = if src_is_items { (items.as_slice(), tmp.as_mut_slice()) } else { (tmp.as_slice(), items.as_mut_slice()) };
            let mut lo = 0usize;
            while lo < n {
                let mid = lo.saturating_add(width).min(n);
                let hi = lo.saturating_add(width.saturating_mul(2)).min(n);
                let (mut i, mut j, mut k) = (lo, mid, lo);
                while k < hi {
                    let take_left = match (src.get(i), src.get(j)) {
                        (Some(a), Some(b)) if i < mid && j < hi => le(a, b),
                        _ => i < mid,
                    };
                    let v = if take_left { src.get(i) } else { src.get(j) };
                    if let (Some(v), Some(d)) = (v, dst.get_mut(k)) {
                        *d = *v;
                    }
                    if take_left {
                        i = i.saturating_add(1);
                    } else {
                        j = j.saturating_add(1);
                    }
                    k = k.saturating_add(1);
                }
                lo = hi;
            }
        }
        src_is_items = !src_is_items;
        items_hold.set(src_is_items);
        width = width.saturating_mul(2);
    }
    if !src_is_items {
        items.as_mut_slice().copy_from_slice(tmp.as_slice());
        items_hold.set(true);
    }
    Ok(())
}
