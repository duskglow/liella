//! The C interface's process-wide logger (spec §26.1 ⚑ HIST; D-26.1): `openlog`, `closelog`,
//! `setlogmask` and the delivery of `syslog`'s formatted text, over `liella::log`.
//!
//! The C contract keeps one logger per process, and its rules are historical:
//! * **The identifier is kept by pointer** (§26.1 ⚑ HIST): the caller's string is read again for
//!   every message ([`Host::text`]). With none (no `openlog`, or after `closelog`) it is the
//!   program's short name. `openlog(NULL, ...)` selects the program's short name too, also after
//!   an earlier `openlog` with another identifier (openlog(3); owner ruling 2026-10-04: the
//!   reference keeps the identifier in force, a recorded difference).
//! * **`openlog`** replaces the options entirely. A facility with no bits outside the facility
//!   field becomes the default, `LOG_KERN` (0) included; any other value is ignored. With
//!   `LOG_NDELAY` it connects if not connected.
//! * **`closelog`** closes the connection and forgets the identifier; the options, the default
//!   facility and the mask stay.
//! * **`setlogmask(0)`** changes nothing; any other value is installed whole. The previous mask
//!   is returned. The initial mask passes every priority (`0xff`).
//! * **Priorities** with bits outside the priority and facility fields are logged with those
//!   bits cleared, after a record `syslog: unknown facility/priority: <hex>` at
//!   `LOG_AUTH|LOG_ERR` reports the value. A priority without a facility takes the default
//!   (`LOG_USER` until `openlog` sets another).
//! * **The connection is synchronised** (D-26.1): one lock serialises the settings, the
//!   connection and each delivery. A `fork` child keeps the connection (the parent's
//!   descriptor) and its own copy of the settings.

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering::Relaxed};

use liella::error::KResult;
use liella::log::{self, Channels, Link, Record, Stamp, FACMASK, LOG_AUTH, LOG_ERR, LOG_NDELAY, LOG_PID, PRIMASK};
use liella::mem::TryVec;
use liella_kernel::sys::Sys;
use liella_rt::sync::RawMutex;

/// What the logger needs from the process.
pub trait Host {
    /// The identifier `openlog` kept by pointer: the NUL-terminated text at `addr` (not 0).
    fn text(&self, addr: usize) -> &[u8];
    /// The program's short name.
    fn program(&self) -> &[u8];
    /// The local time now.
    fn stamp(&self) -> Stamp;
    /// Where records go.
    fn channels(&self) -> KResult<Channels<'_>>;
}

/// The process's logger.
#[derive(Debug)]
pub struct Global {
    lock: RawMutex,
    /// The identifier's address (0: none, the program name).
    ident: AtomicUsize,
    options: AtomicU32,
    facility: AtomicU32,
    mask: AtomicU32,
    fd: AtomicI32,
    stream: AtomicBool,
}

/// The one instance.
pub static GLOBAL: Global = Global::new();

impl Default for Global {
    fn default() -> Self {
        Self::new()
    }
}

impl Global {
    /// No identifier, no options, `LOG_USER`, every priority, not connected.
    pub const fn new() -> Global {
        Global {
            lock: RawMutex::new(),
            ident: AtomicUsize::new(0),
            options: AtomicU32::new(0),
            facility: AtomicU32::new(log::LOG_USER),
            mask: AtomicU32::new(0xff),
            fd: AtomicI32::new(-1),
            stream: AtomicBool::new(false),
        }
    }

    fn link(&self) -> Link {
        Link::from_raw(self.fd.load(Relaxed), self.stream.load(Relaxed))
    }

    fn set_link(&self, l: Link) {
        let (fd, stream) = l.raw();
        self.fd.store(fd, Relaxed);
        self.stream.store(stream, Relaxed);
    }

    /// `openlog(ident, options, facility)`; `ident` is the identifier's address (0: the program
    /// name).
    pub fn open<S: Sys, H: Host>(&self, sys: &S, host: &H, ident: usize, options: u32, facility: u32) {
        let _g = self.lock.lock(sys);
        self.ident.store(ident, Relaxed);
        self.options.store(options, Relaxed);
        if facility & !FACMASK == 0 {
            self.facility.store(facility, Relaxed);
        }
        if options & LOG_NDELAY != 0 && !self.link().is_open() {
            if let Ok(ch) = host.channels() {
                let mut l = Link::CLOSED;
                let _ = l.connect(sys, ch.socket);
                self.set_link(l);
            }
        }
    }

    /// `closelog`.
    pub fn close<S: Sys>(&self, sys: &S) {
        let _g = self.lock.lock(sys);
        let mut l = self.link();
        l.close(sys);
        self.set_link(l);
        self.ident.store(0, Relaxed);
    }

    /// `setlogmask(mask)`: the previous mask; 0 changes nothing.
    pub fn set_mask(&self, mask: u32) -> u32 {
        if mask == 0 { self.mask.load(Relaxed) } else { self.mask.swap(mask, Relaxed) }
    }

    /// Whether a message at `pri` is logged (the mask; only such messages are formatted).
    pub fn passes(&self, pri: u32) -> bool {
        log::passes(self.mask.load(Relaxed), pri)
    }

    /// Deliver the formatted `text` at `pri` (see the module documentation).
    pub fn log<S: Sys, H: Host>(&self, sys: &S, host: &H, pri: u32, text: &[u8]) {
        let _g = self.lock.lock(sys);
        let Ok(ch) = host.channels() else { return };
        let mut pri = pri;
        if pri & !(PRIMASK | FACMASK) != 0 {
            if let Ok(t) = unknown_text(pri) {
                self.deliver(sys, host, &ch, LOG_AUTH | LOG_ERR, t.as_slice());
            }
            pri &= PRIMASK | FACMASK;
        }
        if pri & FACMASK == 0 {
            pri |= self.facility.load(Relaxed);
        }
        self.deliver(sys, host, &ch, pri, text);
    }

    /// Under the lock: one record.
    fn deliver<S: Sys, H: Host>(&self, sys: &S, host: &H, ch: &Channels<'_>, pri: u32, text: &[u8]) {
        let options = self.options.load(Relaxed);
        let addr = self.ident.load(Relaxed);
        let ident = if addr == 0 { host.program() } else { host.text(addr) };
        let pid = (options & LOG_PID != 0).then(|| sys.getpid());
        let rec = Record { pri, stamp: host.stamp(), ident, pid, text };
        let mut l = self.link();
        let _ = log::emit(sys, &mut l, ch, options, &rec);
        self.set_link(l);
    }

    /// In a `fork` child: the lock is free (its holder, if any, is not in the child).
    pub fn after_fork_child(&self) {
        self.lock.reset();
    }
}

/// `syslog: unknown facility/priority: <pri in lower-case hexadecimal>`.
fn unknown_text(pri: u32) -> KResult<TryVec<u8>> {
    let mut t = TryVec::from_slice(b"syslog: unknown facility/priority: ")?;
    let mut digits = [0u8; 8];
    let mut n = 0usize;
    let mut v = pri;
    loop {
        if let Some(d) = digits.get_mut(n) {
            *d = b"0123456789abcdef".get(usize::try_from(v & 0xf).unwrap_or(0)).copied().unwrap_or(b'0');
        }
        n = n.saturating_add(1);
        v >>= 4;
        if v == 0 || n >= digits.len() {
            break;
        }
    }
    for d in digits.get(..n).unwrap_or(&[]).iter().rev() {
        t.push(*d)?;
    }
    Ok(t)
}
