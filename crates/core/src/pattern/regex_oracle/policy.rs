//! Submatch selection: the one component of the matcher that decides which of the paths reaching
//! the same state is kept (D-14.4).
//!
//! The matcher (`exec`) runs every path of the automaton in parallel, position by position.
//! When two threads reach the same state (instruction and empty-iteration depth) at the same
//! position, their futures are identical, so a thread can be dropped as soon as, for every
//! possible future, another thread at that state ends with a better capture vector. A
//! [`Policy`] supplies the two decisions that make "better" concrete:
//!
//! * [`Policy::resets_on_iteration`]: whether a new iteration of a repetition clears the groups
//!   inside it;
//! * [`Policy::prefer`]: whether a candidate replaces the incumbent thread of one *comparison
//!   class* of a state. A state inside repetitions has one class per group range that a future
//!   iteration may overwrite (`mask`), plus the class of futures that overwrite nothing
//!   (`mask` `None`); the matcher keeps the best thread of each class, which is enough because
//!   every future belongs to exactly one class and, within a class, the final vectors compare as
//!   the current ones do with the masked groups ignored.
//!
//! [`Posix`] implements spec §14.9 (POSIX XBD 9.1, owner ruling 2026-10-03).
//!
//! **An alternation-priority mode (glibc's selection; designed for, not built).** It would be a
//! second implementation of this trait, chosen per pattern at compile time and passed to the
//! matcher instead of [`Posix`]: no reset on iteration (an inner group keeps its value from an
//! earlier iteration), and one class per state whose order is the order in which a depth-first
//! search would reach the threads (alternatives left to right, one more iteration before
//! stopping). That order is not a function of the captures, so the thread would also carry a
//! priority word: the matcher already visits split branches in a fixed order (`SPLIT`'s first
//! operand is the earlier alternative or the greedy choice), and the policy would receive that
//! branch index on each epsilon edge and renumber the priorities once per position. Nothing
//! else in the parser, the automaton or the matcher depends on the selection rule.

/// The submatch-selection rule the matcher is parameterised by.
pub(crate) trait Policy {
    /// Whether entering a new iteration of a repetition clears the groups inside it.
    fn resets_on_iteration(&self) -> bool;

    /// Whether candidate `cand` must replace incumbent `inc` (captures as `(so, eo)` pairs per
    /// group, -1 unset; `eo` -1 with `so` set: the group is open) for the futures that overwrite
    /// the groups of `mask` (`None`: no group is overwritten). Equal vectors keep the incumbent.
    fn prefer(&self, mask: Option<(u32, u32)>, cand: &[i32], inc: &[i32]) -> bool;
}

/// POSIX XBD 9.1 (spec §14.9): consistent with the whole match being the longest of the
/// leftmost matches, each subexpression, from left to right, matches the longest possible
/// string, a null string being longer than no match; a repeated group reports its last
/// iteration, and a group nested in a repetition whose last iteration did not take it reports
/// -1 (regexec's rule 3).
///
/// Compared group by group in the order of the opening parentheses: a group that takes part
/// beats one that does not; then the earlier start (the longest possible string for the earlier
/// subexpressions leaves this one starting where they end); then the longer match. A group that
/// is still open in both threads has the same end in every common future, so only its start is
/// compared.
pub(crate) struct Posix;

impl Policy for Posix {
    fn resets_on_iteration(&self) -> bool {
        true
    }

    fn prefer(&self, mask: Option<(u32, u32)>, cand: &[i32], inc: &[i32]) -> bool {
        let groups = cand.chunks_exact(2).zip(inc.chunks_exact(2));
        for (g, (c, i)) in (1u32..).zip(groups) {
            if mask.is_some_and(|(lo, hi)| lo <= g && g <= hi) {
                continue;
            }
            let (&[cs, ce], &[is, ie]) = (c, i) else { continue };
            match (cs >= 0, is >= 0) {
                (false, false) => continue,
                (true, false) => return true,
                (false, true) => return false,
                (true, true) => {}
            }
            if cs != is {
                return cs < is;
            }
            if ce < 0 || ie < 0 {
                continue;
            }
            if ce != ie {
                return ce > ie;
            }
        }
        false
    }
}
