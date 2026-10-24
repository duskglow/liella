//! The process-wide logger's glue (spec §26.1; D-26.1): the identifier read through its
//! pointer, the program name, the local time (the zone last selected, as `localtime_r`: a
//! change of `TZ` takes effect with `tzset`), the channels, and `syslog`'s formatting. The
//! logger itself is `liella_legacy::log_compat` over `liella::log`.
//!
//! **Formatting.** A message the mask discards is not formatted at all (§26.1). The text is the
//! `printf` expansion (`%m` included, from `errno` at entry), with the fortified forms' `%n`
//! check for the checking entry points. A NULL format logs nothing. Nothing here writes
//! `errno` (X3: the kernel's results never reach it), so `syslog` leaves it as it was, whatever
//! became of the message (§26.1: it never reports failure).

use core::sync::atomic::Ordering::Acquire;

use liella::error::KResult;
use liella::log::{Channels, Stamp};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::CLOCK_REALTIME;
use liella_legacy::log_compat::{Host, GLOBAL};

use crate::sys::CSys;
use crate::{conv, ffi, printf, zone};

/// The process as the logger sees it.
struct Process;

impl Host for Process {
    fn text(&self, addr: usize) -> &[u8] {
        conv::session_text(addr)
    }

    fn program(&self) -> &[u8] {
        conv::process_text(ffi::program_invocation_short_name.load(Acquire))
    }

    fn stamp(&self) -> Stamp {
        let now = CSys.clock_gettime(CLOCK_REALTIME).map_or(0, |t| t.sec);
        let byte = |v: i32| u8::try_from(v).unwrap_or(0);
        zone::localtime_r(now).map_or(Stamp::default(), |tm| Stamp { month: byte(tm.tm_mon), day: byte(tm.tm_mday), hour: byte(tm.tm_hour), min: byte(tm.tm_min), sec: byte(tm.tm_sec) })
    }

    fn channels(&self) -> KResult<Channels<'_>> {
        #[cfg(feature = "selftest")]
        if let Some((socket, console, wait_ns)) = crate::selftest::syslog_override() {
            return Ok(Channels {
                socket: liella_kernel::path::KStr::from_nul_terminated(socket)?,
                console: liella_kernel::path::KStr::from_nul_terminated(console)?,
                stderr: liella_kernel::fd::Fd::new(2),
                wait_ns,
            });
        }
        Channels::system()
    }
}

/// `openlog(ident, option, facility)`; `ident` is the caller's string's address (0: NULL).
pub fn openlog(ident: usize, option: i32, facility: i32) {
    GLOBAL.open(&CSys, &Process, ident, conv::bits(option), conv::bits(facility));
}

/// `closelog`.
pub fn closelog() {
    GLOBAL.close(&CSys);
}

/// `setlogmask(mask)`: the previous mask.
pub fn setlogmask(mask: i32) -> i32 {
    i32::from_ne_bytes(GLOBAL.set_mask(conv::bits(mask)).to_ne_bytes())
}

/// `vsyslog(pri, fmt, ap)` and the checking forms (`flag` > 0: a `%n` in writable memory
/// aborts).
pub fn vsyslog(pri: i32, flag: i32, fmt: KResult<&[u8]>, ap: usize) {
    let pri = conv::bits(pri);
    if !GLOBAL.passes(pri) {
        return;
    }
    let Ok(fmt) = fmt else { return };
    if let Ok(text) = printf::collect_chk(fmt, flag, ap) {
        GLOBAL.log(&CSys, &Process, pri, text.as_slice());
    }
}

/// A `fork` child: the logger's lock is free; the connection is the parent's, shared.
pub(crate) fn after_fork_child() {
    GLOBAL.after_fork_child();
}
