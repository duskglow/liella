//! Constructor and destructor order (phase 14 DL-11, plan/ph14/design.md §9, D-27.19). Pure logic
//! over object indices and dynamic arrays; the boundary (`boundary/src/ld.rs`) reads the arrays and
//! makes the calls.
//!
//! * [`init_order`]: the order objects are initialised in. Objects are indexed in load order (0 is
//!   the root: the program at start, the object named to `dlopen` later); `edges` are the
//!   `DT_NEEDED` links `(dependent, dependency)` in the order each object names them. The walk is
//!   depth first over the objects in *reverse* load order, each object placed after the objects it
//!   needs (post-order); a dependency already being walked (a cycle) is passed over; the root is never
//!   entered from another object and comes last. Probe-settled (tools/probe/dl/order.sh against the
//!   reference loader: a diamond, ties between siblings, a dependency loaded through a later sibling,
//!   two- and three-object cycles at start; DL-0b's record for a cycle opened by `dlopen`).
//! * [`fini_order`]: the reverse of [`init_order`] over every object loaded at the time (DL-0b's
//!   exit record: an object opened later is placed by its dependencies, not by when it was opened).
//! * [`hooks`]: an object's `DT_PREINIT_ARRAY` (the program's only: a shared object's is ignored, as
//!   by the reference), `DT_INIT`, `DT_INIT_ARRAY`, `DT_FINI_ARRAY`, `DT_FINI`.
//! * [`init_calls`] (`DT_INIT`, then `DT_INIT_ARRAY` in order) and [`fini_calls`] (`DT_FINI_ARRAY`
//!   in reverse, then `DT_FINI`): the functions to call for one object (ELF gABI "Initialization
//!   and Termination Functions").

use crate::elf::{dt, Dynamic};

/// The most objects one order covers.
pub const OBJECTS_MAX: usize = 64;

/// Why an order cannot be computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderError {
    /// More than [`OBJECTS_MAX`] objects.
    TooMany,
    /// An edge names an object outside `0..n`.
    BadEdge,
}

/// Walk state of one object.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    New,
    Active,
    Done,
}

/// The initialisation order of objects `0..n` (see the module documentation) into `out`; returns
/// the number of entries (`n`).
///
/// # Errors
///
/// [`OrderError`] for more than [`OBJECTS_MAX`] objects or an edge outside `0..n`.
pub fn init_order(n: usize, edges: &[(u8, u8)], out: &mut [u8; OBJECTS_MAX]) -> Result<usize, OrderError> {
    if n > OBJECTS_MAX {
        return Err(OrderError::TooMany);
    }
    if edges.iter().any(|&(a, b)| usize::from(a) >= n || usize::from(b) >= n) {
        return Err(OrderError::BadEdge);
    }
    let mut mark = [Mark::New; OBJECTS_MAX];
    if n == 0 {
        return Ok(0);
    }
    // The root is held back: never entered through an edge, placed last.
    if let Some(m) = mark.first_mut() {
        *m = Mark::Active;
    }
    let mut len = 0usize;
    let mut emit = |v: u8, out: &mut [u8; OBJECTS_MAX]| {
        if let Some(slot) = out.get_mut(len) {
            *slot = v;
            len = len.saturating_add(1);
        }
    };
    // The walk's stack: (object, index of the next edge to look at).
    let mut stack = [(0u8, 0usize); OBJECTS_MAX];
    for start in (1..n).rev() {
        let Ok(start) = u8::try_from(start) else { return Err(OrderError::TooMany) };
        if mark.get(usize::from(start)) != Some(&Mark::New) {
            continue;
        }
        let mut depth = 0usize;
        push(&mut stack, &mut depth, &mut mark, start);
        while let Some(&(v, from)) = depth.checked_sub(1).and_then(|t| stack.get(t)) {
            let next = edges.iter().enumerate().skip(from).find(|(_, e)| e.0 == v);
            match next {
                Some((k, &(_, w))) => {
                    if let Some(top) = depth.checked_sub(1).and_then(|t| stack.get_mut(t)) {
                        top.1 = k.saturating_add(1);
                    }
                    if mark.get(usize::from(w)) == Some(&Mark::New) {
                        push(&mut stack, &mut depth, &mut mark, w);
                    }
                }
                None => {
                    depth = depth.saturating_sub(1);
                    if let Some(m) = mark.get_mut(usize::from(v)) {
                        *m = Mark::Done;
                    }
                    emit(v, out);
                }
            }
        }
    }
    emit(0, out);
    Ok(len)
}

/// Push `v` on the walk's stack and mark it active.
fn push(stack: &mut [(u8, usize); OBJECTS_MAX], depth: &mut usize, mark: &mut [Mark; OBJECTS_MAX], v: u8) {
    if let Some(slot) = stack.get_mut(*depth) {
        *slot = (v, 0);
        *depth = depth.saturating_add(1);
    }
    if let Some(m) = mark.get_mut(usize::from(v)) {
        *m = Mark::Active;
    }
}

/// The finalisation order of objects `0..n`: [`init_order`] reversed.
///
/// # Errors
///
/// As [`init_order`].
pub fn fini_order(n: usize, edges: &[(u8, u8)], out: &mut [u8; OBJECTS_MAX]) -> Result<usize, OrderError> {
    let len = init_order(n, edges, out)?;
    if let Some(s) = out.get_mut(..len) {
        s.reverse();
    }
    Ok(len)
}

/// An array of function addresses in an object: run-time address and number of entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Array {
    /// Run-time address of the first entry.
    pub addr: u64,
    /// Number of 8-byte entries.
    pub len: u64,
}

/// An object's initialisation and termination functions (run-time addresses; 0 = none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hooks {
    /// `DT_PREINIT_ARRAY`: the program's only.
    pub preinit: Array,
    /// `DT_INIT`.
    pub init: u64,
    /// `DT_INIT_ARRAY`.
    pub init_array: Array,
    /// `DT_FINI_ARRAY`.
    pub fini_array: Array,
    /// `DT_FINI`.
    pub fini: u64,
}

/// The hooks of the object with dynamic array `d` and load bias `bias`; `program` says whether it
/// is the program (only the program's `DT_PREINIT_ARRAY` counts).
#[must_use]
pub fn hooks(d: &Dynamic<'_>, bias: u64, program: bool) -> Hooks {
    let at = |tag| d.get(tag).filter(|&v| v != 0).map_or(0, |v| v.wrapping_add(bias));
    let array = |addr, size| {
        let a = at(addr);
        if a == 0 {
            return Array::default();
        }
        Array { addr: a, len: d.get(size).unwrap_or(0) / 8 }
    };
    Hooks {
        preinit: if program { array(dt::PREINIT_ARRAY, dt::PREINIT_ARRAYSZ) } else { Array::default() },
        init: at(dt::INIT),
        init_array: array(dt::INIT_ARRAY, dt::INIT_ARRAYSZ),
        fini_array: array(dt::FINI_ARRAY, dt::FINI_ARRAYSZ),
        fini: at(dt::FINI),
    }
}

/// Whether an array entry names a function: 0 and all-ones are left out (D-27.19).
fn callable(f: u64) -> bool {
    f != 0 && f != u64::MAX
}

/// The functions to call to initialise one object: `DT_INIT` (`init`, 0 = none), then the entries
/// of its `DT_INIT_ARRAY` (`array`, already relocated) in order.
pub fn init_calls(init: u64, array: &[u64]) -> impl Iterator<Item = u64> + '_ {
    core::iter::once(init).chain(array.iter().copied()).filter(|&f| callable(f))
}

/// The functions to call to finalise one object: the entries of its `DT_FINI_ARRAY` (`array`) in
/// reverse, then `DT_FINI` (`fini`, 0 = none).
pub fn fini_calls(fini: u64, array: &[u64]) -> impl Iterator<Item = u64> + '_ {
    array.iter().rev().copied().chain(core::iter::once(fini)).filter(|&f| callable(f))
}
