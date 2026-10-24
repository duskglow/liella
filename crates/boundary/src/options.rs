//! Command-line parsing glue (spec §1.6, §1.6a; D-1.6, D-1.6a): the process-wide parser state
//! of the C functions, the caller's argument vector and option table, the exported `optind`,
//! `optarg`, `opterr` and `optopt`, and the diagnostics. The parsing itself is
//! `liella::getopt`, over a caller-owned state.
//!
//! Rewritten under the clean-room rules (implementation plan §1.4, item 2) from the spec and
//! `spec/behaviour/getopt.md`, with the open points settled by black-box probes.
//!
//! **State.** One parser state for the process, under a lock: the next element, the position
//! within it, the operands skipped and not yet moved, whether the options are exhausted, and
//! `optopt`. Each call reads `optind` and `opterr`, and writes `optind`, `optarg` (NULL when the
//! result has no argument) and `optopt` (the state's, which changes only on errors: setting the
//! variable does not change it). `optind = 0`, or the first call, starts a new parse at element
//! 1 and reads `POSIXLY_CORRECT`. Any other value differing from the one the last call left
//! continues from that element, forgetting the position within an element and the skipped
//! operands. A negative `optind` returns -1 and changes nothing.
//!
//! **Results.** An option letter returns the letter; a long option stores its index in
//! `*longind` (when given) and returns its `val`, or stores `val` in `*flag` and returns 0; an
//! operand returned in place returns 1 with `optarg` naming it. Errors return `'?'`, or `':'`
//! for a missing argument when the option string starts (after `+` or `-`) with `:`.
//!
//! **Diagnostics** go to `stderr` (as `err` writes them, `report::emit`), unless `opterr` is 0
//! or the option string asks for quiet, prefixed with `argv[0]` and `: `:
//! `invalid option -- 'c'`, `option requires an argument -- 'c'`, `unrecognized option 'TEXT'`,
//! `option 'TEXT' is ambiguous; possibilities: 'NAME' ...`, `option 'NAME' doesn't allow an
//! argument`, `option 'NAME' requires an argument`. TEXT is the option as written, NAME the
//! table's name; each is preceded by `--`, `-` or `-W ` as the option was written.
//!
//! **`getsubopt`** writes NUL over the `,` ending the suboption and, for a recognized one with a
//! value, over its `=` (spec §1.6a; the reference leaves the `=`: a recorded difference).
//! `*valuep` is the value (NULL without one) for a recognized suboption, the whole suboption
//! for an unrecognized one, and NULL when the text is empty (the reference leaves it unset).

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering::Relaxed};

use liella::getopt::{self, Args, Event, HasArg, LongOpt, Longs, Span, Spec, State};
use liella::mem::TryVec;
use liella_legacy::lock::SpinLock;

use crate::{conv, data, errno, process, report, tb};

/// Serializes the C functions' use of the state below.
static LOCK: SpinLock = SpinLock::new();
/// The process-wide parser state (`liella::getopt::State`), under [`LOCK`].
static NEXT: AtomicUsize = AtomicUsize::new(1);
static CURSOR: AtomicUsize = AtomicUsize::new(0);
static FIRST: AtomicUsize = AtomicUsize::new(1);
static LAST: AtomicUsize = AtomicUsize::new(1);
static ENDED: AtomicBool = AtomicBool::new(false);
static OPTOPT: AtomicI32 = AtomicI32::new(0);
/// A parse has started (the first call starts one whatever `optind` holds).
static STARTED: AtomicBool = AtomicBool::new(false);
/// `POSIXLY_CORRECT` was set when the parse started.
static POSIXLY: AtomicBool = AtomicBool::new(false);

/// The size of `struct option` (Appendix C): `name`, `has_arg`, `flag`, `val`.
const OPTION_SIZE: usize = 32;

fn load() -> State {
    State {
        next: NEXT.load(Relaxed),
        cursor: CURSOR.load(Relaxed),
        first: FIRST.load(Relaxed),
        last: LAST.load(Relaxed),
        ended: ENDED.load(Relaxed),
        dashes: false,
        optopt: OPTOPT.load(Relaxed),
    }
}

fn store(st: &State) {
    NEXT.store(st.next, Relaxed);
    CURSOR.store(st.cursor, Relaxed);
    FIRST.store(st.first, Relaxed);
    LAST.store(st.last, Relaxed);
    ENDED.store(st.ended, Relaxed);
    OPTOPT.store(st.optopt, Relaxed);
}

/// The caller's argument vector: `n` text addresses at `base`.
struct CArgs {
    base: usize,
    n: usize,
}

impl CArgs {
    /// The word holding element `i`'s address.
    fn word(&self, i: usize) -> Option<&'static AtomicUsize> {
        if i >= self.n {
            return None;
        }
        tb::c_word(self.base.checked_add(i.checked_mul(8)?)?)
    }

    /// The address of the bytes `s` names (0 when there is no element).
    fn addr(&self, s: Span) -> usize {
        self.word(s.index).map_or(0, |w| w.load(Relaxed).wrapping_add(s.start))
    }
}

impl Args for CArgs {
    fn count(&self) -> usize {
        self.n
    }
    fn text(&self, i: usize) -> &[u8] {
        self.word(i).map_or(&[], |w| conv::caller_text(w.load(Relaxed)))
    }
    fn swap(&mut self, i: usize, j: usize) {
        if let (Some(a), Some(b)) = (self.word(i), self.word(j)) {
            let t = a.load(Relaxed);
            a.store(b.load(Relaxed), Relaxed);
            b.store(t, Relaxed);
        }
    }
}

/// The caller's long-option table at an address.
struct CLongs(usize);

/// The `int` at `addr` (0 when unreadable).
fn int_at(addr: Option<usize>) -> i32 {
    addr.and_then(tb::c_int).map_or(0, |w| i32::from_ne_bytes(w.load(Relaxed).to_ne_bytes()))
}

/// Store `v` in the `int` at `addr` (nothing when there is none).
fn set_int(addr: usize, v: i32) {
    if let Some(w) = tb::c_int(addr) {
        w.store(u32::from_ne_bytes(v.to_ne_bytes()), Relaxed);
    }
}

impl Longs for CLongs {
    fn entry(&self, i: usize) -> Option<LongOpt<'_>> {
        let at = self.0.checked_add(i.checked_mul(OPTION_SIZE)?)?;
        let name = tb::c_word(at)?.load(Relaxed);
        if name == 0 {
            return None;
        }
        let flag = at.checked_add(16).and_then(tb::c_word).map_or(0, |w| w.load(Relaxed));
        Some(LongOpt { name: conv::caller_text(name), has_arg: HasArg::of(int_at(at.checked_add(8))), flag, val: int_at(at.checked_add(24)) })
    }
}

/// `getopt`, `getopt_long`, `getopt_long_only`, `__posix_getopt`: `argc` elements at `argv`;
/// `longopts` and `longind` are addresses (0: none).
#[allow(clippy::too_many_arguments)]
pub fn getopt(argc: i32, argv: usize, optstring: &[u8], longopts: usize, longind: usize, long_only: bool, posixly_correct: bool) -> i32 {
    let mut args = CArgs { base: argv, n: if argv == 0 { 0 } else { usize::try_from(argc).unwrap_or(0) } };
    let table = (longopts != 0).then_some(CLongs(longopts));
    let Ok(optind) = usize::try_from(data::optind.load(Relaxed)) else {
        data::optarg.store(0, Relaxed);
        return -1;
    };
    let (event, quiet) = {
        let _g = LOCK.lock();
        let mut st = load();
        if optind == 0 || !STARTED.load(Relaxed) {
            st = State::new(st.optopt);
            STARTED.store(true, Relaxed);
            POSIXLY.store(process::getenv(crate::startup_names::POSIXLY_CORRECT).is_some(), Relaxed);
        } else if optind != st.next {
            st.moved(optind);
        }
        let spec = Spec::new(optstring, posixly_correct || POSIXLY.load(Relaxed));
        let event = getopt::next(&mut st, &mut args, &spec, table.as_ref(), long_only);
        store(&st);
        data::optind.store(i32::try_from(st.next).unwrap_or(i32::MAX), Relaxed);
        data::optopt.store(st.optopt, Relaxed);
        (event, spec.quiet)
    };
    let (code, arg) = match event {
        Event::End => (-1, 0),
        Event::Short { letter, arg } => (i32::from(letter), arg.map_or(0, |a| args.addr(a))),
        Event::Long { entry, arg } => {
            set_int(longind, i32::try_from(entry).unwrap_or(i32::MAX));
            let code = match table.as_ref().and_then(|t| t.entry(entry)) {
                Some(e) if e.flag != 0 => {
                    set_int(e.flag, e.val);
                    0
                }
                Some(e) => e.val,
                None => 0,
            };
            (code, arg.map_or(0, |a| args.addr(a)))
        }
        Event::Operand(i) => (1, args.word(i).map_or(0, |w| w.load(Relaxed))),
        Event::MissingShort(_) | Event::MissingLong { .. } if quiet => (i32::from(b':'), 0),
        _ => {
            if !quiet && data::opterr.load(Relaxed) != 0 {
                diagnose(args.text(0), &args, table.as_ref(), event);
            }
            (i32::from(b'?'), 0)
        }
    };
    data::optarg.store(arg, Relaxed);
    code
}

/// Print the diagnostic for an error `event` (module documentation), prefixed with `prog`;
/// `argp_parse` reports its option errors through this too.
pub(crate) fn diagnose<A: Args + ?Sized, L: Longs + ?Sized>(prog: &[u8], args: &A, table: Option<&L>, event: Event) {
    let saved = errno::get();
    let name = |entry: usize| table.and_then(|t| t.entry(entry)).map_or(&[][..], |e| e.name);
    match event {
        Event::Invalid(c) => report::emit(&[prog, b": invalid option -- '", &[c], b"'\n"], true, saved),
        Event::MissingShort(c) => report::emit(&[prog, b": option requires an argument -- '", &[c], b"'\n"], true, saved),
        Event::Unrecognized { dash, text } => report::emit(&[prog, b": unrecognized option '", dash.text(), text.of(args), b"'\n"], true, saved),
        Event::NoArgument { dash, entry } => report::emit(&[prog, b": option '", dash.text(), name(entry), b"' doesn't allow an argument\n"], true, saved),
        Event::MissingLong { dash, entry } => report::emit(&[prog, b": option '", dash.text(), name(entry), b"' requires an argument\n"], true, saved),
        Event::Ambiguous { dash, text, name: typed, strict } => {
            let head: [&[u8]; 5] = [prog, b": option '", dash.text(), text.of(args), b"' is ambiguous; possibilities:"];
            let mut line: TryVec<u8> = TryVec::new();
            let mut ok = head.iter().try_for_each(|p| line.extend_from_slice(p)).is_ok();
            if let Some(t) = table {
                getopt::candidates(t, typed.of(args), strict, |e| {
                    let parts: [&[u8]; 4] = [b" '", dash.text(), e.name, b"'"];
                    ok &= parts.iter().try_for_each(|p| line.extend_from_slice(p)).is_ok();
                });
            }
            ok &= line.push(b'\n').is_ok();
            if ok {
                report::emit(&[line.as_slice()], true, saved);
            } else {
                // No memory for the list of possibilities: the line without it.
                let mut parts: [&[u8]; 6] = [b"\n"; 6];
                for (p, h) in parts.iter_mut().zip(head) {
                    *p = h;
                }
                report::emit(&parts, true, saved);
            }
        }
        Event::End | Event::Short { .. } | Event::Long { .. } | Event::Operand(_) => {}
    }
}

/// `getsubopt(&opts, tokens, &value)`: the addresses of the three arguments.
pub fn getsubopt(optionp: usize, tokens: usize, valuep: usize) -> i32 {
    let value = tb::c_word(valuep);
    let set_value = |v: usize| {
        if let Some(w) = value {
            w.store(v, Relaxed);
        }
    };
    let Some(cursor) = tb::c_word(optionp) else { return -1 };
    let start = cursor.load(Relaxed);
    let text = conv::caller_text_mut(start);
    let names = (0..usize::MAX).map_while(|i| {
        let w = tb::c_word(tokens.checked_add(i.checked_mul(8)?)?)?.load(Relaxed);
        (w != 0).then(|| conv::caller_text(w))
    });
    let Some(s) = getopt::subopt(text, names) else {
        set_value(0);
        return -1;
    };
    if let Some(comma) = text.get_mut(s.end) {
        *comma = 0;
    }
    let r = match s.token {
        Some(i) => {
            match s.value {
                Some(v) => {
                    if let Some(eq) = text.get_mut(s.name_end) {
                        *eq = 0;
                    }
                    set_value(start.wrapping_add(v));
                }
                None => set_value(0),
            }
            i32::try_from(i).unwrap_or(i32::MAX)
        }
        None => {
            set_value(start);
            -1
        }
    };
    cursor.store(start.wrapping_add(s.next), Relaxed);
    r
}
