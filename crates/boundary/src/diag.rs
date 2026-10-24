//! The library's opt-in diagnostic channel (D-2.9, owner ruling #422).
//!
//! `LIELLA_DIAG=<topic>[,<topic>...]` in the environment (`all` for every topic; unknown names
//! and empty items are ignored) turns topics on. It is ignored in secure-execution mode
//! (`AT_SECURE`). Default: off, and the library writes nothing.
//!
//! The variable is read LAZILY, once: the first time a diagnostic call site is reached (a rare
//! failure path), not at process start. Startup and every common path are exactly as they are
//! without the channel (owner condition: no cost when disabled). The answer is cached in
//! [`TOPICS`] and used from then on, so a later `setenv`/`unsetenv` of `LIELLA_DIAG` has no
//! effect once a diagnostic site has run.
//!
//! A diagnostic is one line on descriptor 2, `<program>: liella: <topic>: <text>`, written with
//! one gathered `write` (no allocation, no stdio), for rare events only (a failure episode's
//! start and its recovery, never each event). A new topic is one [`Topic`] constant, one
//! [`NAMES`] entry and one [`note`] call on its rare path.
//!
//! Topics: `timers` (the notification threads of `SIGEV_THREAD` timers, D-23.3).

use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use liella_rt::fatal;

use crate::sys::CSys;
use crate::{conv, ffi, process};

/// A diagnostic topic: one bit of [`TOPICS`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Topic(u32);

impl Topic {
    /// `SIGEV_THREAD` timers' notification threads (D-23.3): a failure episode begins / ends.
    pub(crate) const TIMERS: Topic = Topic(1);
}

/// Each topic's name in `LIELLA_DIAG` and in its lines.
const NAMES: [(&[u8], Topic); 1] = [(b"timers", Topic::TIMERS)];

/// [`TOPICS`] before `LIELLA_DIAG` has been read (no topic uses this bit).
const UNREAD: u32 = 1 << 31;

/// The longest `LIELLA_DIAG` value read; a longer one is ignored (every topic off).
const VALUE_MAX: usize = 256;

/// The topics turned on (bits of [`Topic`]), or [`UNREAD`]. Touched by diagnostic call sites
/// only, which sit on rare paths.
static TOPICS: AtomicU32 = AtomicU32::new(UNREAD);

/// The topics a `LIELLA_DIAG` value names: comma-separated topic names, `all` for every one;
/// unknown names and empty items are ignored.
pub(crate) fn topics(value: &[u8]) -> u32 {
    let mut on = 0u32;
    for item in value.split(|&b| b == b',') {
        for (name, t) in NAMES {
            if item == b"all" || item == name {
                on |= t.0;
            }
        }
    }
    on
}

/// The topics on: the cached answer, or (the first time) `LIELLA_DIAG` read now, except in
/// secure-execution mode. Thread context only (it takes the environment writers' lock); the
/// call sites are a helper thread's failure path, never a signal handler.
fn on() -> u32 {
    let t = TOPICS.load(Relaxed);
    if t & UNREAD == 0 {
        return t;
    }
    let mut buf = [0u8; VALUE_MAX];
    let read = if process::secure() {
        0
    } else {
        process::getenv_into(b"LIELLA_DIAG", &mut buf).and_then(|n| buf.get(..n)).map_or(0, topics)
    };
    // Two first callers read the same variable; the first answer stored is the one used.
    match TOPICS.compare_exchange(UNREAD, read, Relaxed, Relaxed) {
        Ok(_) => read,
        Err(cur) => cur,
    }
}

/// Turn exactly the topics `value` names on (`selftest`: a test sets the answer instead of the
/// environment).
#[cfg(feature = "selftest")]
pub(crate) fn set(value: &[u8]) {
    TOPICS.store(topics(value), Relaxed);
}

/// Report `text` under `topic` when it is on: one line on descriptor 2. Called on rare paths
/// only (cold, out of line, so the caller's common path stays as it was).
#[cold]
#[inline(never)]
pub(crate) fn note(topic: Topic, text: &[u8]) {
    if on() & topic.0 == 0 {
        return;
    }
    let name = NAMES.iter().find(|(_, t)| *t == topic).map_or(&b""[..], |(n, _)| *n);
    let prog = conv::process_text(ffi::program_invocation_short_name.load(Relaxed));
    let sep: &[u8] = if prog.is_empty() { b"" } else { b": " };
    fatal::message(&CSys, &[prog, sep, b"liella: ", name, b": ", text, b"\n"]);
}
