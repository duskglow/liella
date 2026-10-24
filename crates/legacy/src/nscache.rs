//! The integrated name-service cache of the process (D-16.10; it replaces nscd, D-16.5): speed
//! only, never opacity. The table's format is `liella::names::cache`; this module decides when
//! to use which table, what to watch, and who may write.
//!
//! **What is cached.** The answers of the built-in sources (`files`, `dns`) for hosts, passwd,
//! group (group lists included), services, protocols and networks, negative answers included
//! (not found, no data); never errors, `shadow`/`gshadow`, enumerations, lookups while
//! `__nss_configure_lookup` overrides are set, lookups whose thread changed `_res` (the
//! boundary does not offer the cache to them), or lookups through fixture paths (only a
//! context with the system's files and a [`Memory`] is cached: `Ctx::cached`). A key is the
//! category, the kind of query and the query exactly as the sources get it (a host name after
//! IDNA, an address with its family, a service with its protocol, a name or an ID); for hosts
//! also `host.conf`'s `multi` and a fingerprint of the `dns` source's resolver (its effective
//! configuration, which `LOCALDOMAIN` and `RES_OPTIONS` shape, and the text of the file
//! `HOSTALIASES` names). Everything that depends on the calling process runs after the cache
//! on every lookup: IDNA through the locale, `AI_ADDRCONFIG`, RFC 6724 sorting, the flags and
//! the packing.
//!
//! **Cold start.** A process's first cacheable lookup goes to the sources without the table
//! (they read the current files); the second attaches the table and validates. A process that
//! makes one lookup (most short-lived commands) pays nothing for the cache.
//!
//! **Validity.** Each table records the identity (device, inode, size, modification and change
//! times; present or not) of the watched files as last validated, and generations: one for
//! every category, one per category. An entry is used only under the generations it was
//! stored under and, for DNS data, within its TTL (at most an hour; TTL 0 is never stored;
//! negative answers per RFC 2308, at most five minutes). The first lookup of a process always
//! validates (it `stat`s the files before anything is served); later, the global files
//! (`nsswitch.conf`, `host.conf`, `resolv.conf`, `gai.conf`) and each category's database file
//! are checked at most once a second per process (the monotonic clock of the vDSO), and a
//! difference from the record moves the generations (all of them, or the category's) and
//! updates the record. Other processes of the user see the generations move without `stat`
//! calls of their own. While validation is that recent, the switch's configuration files are
//! not `stat`ed on each lookup either (`crate::nss::Watched`), so the cache's window is also
//! theirs; `resolv.conf`'s reload of `_res` (D-16.3) keeps its own check. An answer obtained
//! while a change was not yet seen is stored under the generations read before the sources
//! were asked, so it dies with them.
//!
//! **Tables.** The shared region of the effective user, `<dir>/liella-nscache-<euid>`
//! (`/dev/shm/` in the products), when it can be trusted; otherwise a private table in
//! anonymous memory. The region is created `O_RDWR|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC`, mode
//! 0600 (then `fchmod` 0600, against a umask, and sized to [`cache::CAP_BYTES`]), or opened
//! `O_NOFOLLOW` (and `O_NONBLOCK`, so a FIFO cannot block); `fstat` must show a regular file
//! owned by the effective UID with mode exactly 0600 and a table's size within the cap. Any
//! mismatch: the private table, and the file is neither trusted nor modified. The private
//! table is also used when `AT_SECURE` is set or the real and effective user or group IDs
//! differ. A credential change ([`NsCache::credentials_changed`], from the setxid broadcast,
//! D-17.7), and any successful `unshare` or `setns` (a user namespace changes the credentials, a
//! mount namespace what the region's path names), drops the mapping and re-keys before the next
//! lookup. A process whose effective UID is the overflow UID (65534: what a user namespace shows
//! for any unmapped owner) keeps a private table, and so does any process outside the initial
//! user namespace (`/proc/self/ns/user`): there its IDs are the namespace's while `/dev/shm` is
//! shared by every namespace, so a region named for its "UID 0" would carry another account's
//! name (validation T13). The region's descriptor is kept above the standard descriptors
//! (`liella::io::above_standard`), so a program that closed its output never writes into the
//! region (validation T12). The region's size cannot be sealed (a file in
//! `/dev/shm` carries `F_SEAL_SEAL`; `F_ADD_SEALS` fails with `EPERM`, kernel 6.8), so a same-user
//! process that truncates it makes the processes that map it fault (`SIGBUS`). Readers keep a count while
//! they use a mapping; dropping it waits for the count to reach zero, so no thread reads a
//! mapping after it is gone. Writers hold the process's lock and, for the region, an OFD lock
//! on its file (released by the kernel if the writer dies); a writer that cannot get it soon
//! does not write. A region whose header is not this account's (layout, size, owner, boot ID,
//! account stamp) or that a writer left half-changed is reinitialised under that lock.
//!
//! **Switches.** [`NsCache::enable`] (the native `liella_nscache_enable`; `LIELLA_NSCACHE=0`
//! at start, ignored under `AT_SECURE`): disabled, a lookup costs one relaxed atomic load and
//! nothing is mapped, `stat`ed or kept. [`NsCache::set_category`], [`NsCache::invalidate`]
//! (which also moves the region's generations and makes the next lookup validate at once) and
//! [`NsCache::status`].

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering::{AcqRel, Acquire, Relaxed, Release}};

use liella::error::{Errno, Result};
use liella::mem::TryVec;
use liella::names::cache::{self, Category, File, FileId, Found, Stamp, Stored, Table, CATEGORIES};
use liella::names::nss::{Answer, LookupError};
use liella::names::Ip;
use liella_kernel::fd::Fd;
use liella_kernel::ops::FcntlLock;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Flock, Timespec, CLOCK_MONOTONIC, CLOCK_REALTIME, F_UNLCK, F_WRLCK, O_CLOEXEC, O_CREAT, O_EXCL, O_NOCTTY, O_NOFOLLOW, O_NONBLOCK, O_RDWR, SEEK_SET, S_IFMT, S_IFREG};
use liella_rt::sync::RawMutex;

use crate::nss::{Ctx, SYSTEM};
use crate::nss_files::{Host, Key};
use crate::resolv_compat::{Resolver, PATH_RESCONF};
use crate::result_slots::Buf;

/// The memory the cache's tables live in, supplied by the boundary (the address-based
/// conversions are its category *slice*).
pub trait Memory: core::fmt::Debug {
    /// Map `len` bytes of the regular file `fd` shared and read-write: the address.
    fn map_shared(&self, fd: Fd, len: usize) -> Option<usize>;
    /// Map `len` bytes of private anonymous memory (zero, reserved lazily): the address.
    fn map_private(&self, len: usize) -> Option<usize>;
    /// Unmap a mapping [`Memory::map_shared`] or [`Memory::map_private`] made.
    fn unmap(&self, addr: usize, len: usize);
    /// The first `n` words of the mapping at `addr` that `map_shared` or `map_private` made
    /// with at least `n` words, as atomics. Contract: the caller uses the slice only while the
    /// mapping stays mapped (this module: while it holds a reader count, see the module
    /// documentation).
    fn words(&self, addr: usize, n: usize) -> &[AtomicU64];
}

/// The longest positive DNS lifetime kept (seconds).
pub const POSITIVE_CAP: u64 = 3600;
/// The longest negative DNS lifetime kept (seconds; RFC 2308 suggests one to three hours, the
/// cache keeps a missing name for at most five minutes).
pub const NEGATIVE_CAP: u64 = 300;
/// The validation window (nanoseconds).
pub const WINDOW_NS: u64 = 1_000_000_000;

/// Status modes (`liella_nscache_status`): disabled.
pub const MODE_OFF: u32 = 0;
/// Enabled; nothing mapped yet (no lookup since the start, a re-key or `enable`).
pub const MODE_IDLE: u32 = 1;
/// The private table.
pub const MODE_PRIVATE: u32 = 2;
/// The user's shared region.
pub const MODE_SHARED: u32 = 3;
/// Enabled but no table could be made (no memory): lookups go to the sources.
pub const MODE_UNAVAILABLE: u32 = 4;

/// Why the table is private (or missing): the shared region is used.
pub const REASON_NONE: u32 = 0;
/// No region location (a test build, or one that did not configure it).
pub const REASON_NO_REGION: u32 = 1;
/// `AT_SECURE`, or real and effective user or group IDs that differ.
pub const REASON_SECURE: u32 = 2;
/// The region file is not to be trusted (owner, mode, type, size, a symbolic link).
pub const REASON_REFUSED: u32 = 3;
/// The region could not be opened, created, sized, locked or mapped.
pub const REASON_UNAVAILABLE: u32 = 4;

const ON: u8 = 1;
/// The UID a user namespace shows for an unmapped owner (`/proc/sys/kernel/overflowuid`'s
/// default).
const OVERFLOW_UID: u32 = 65534;
const OFF: u8 = 2;
/// The bit of the reader count that announces a mapping being dropped.
const CLOSING: u64 = 1 << 63;

// ------------------------------------------------------------------------------ lifetimes

/// How long an answer may be kept, as the sources report it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Life {
    /// No DNS data: until a watched file changes.
    #[default]
    Files,
    /// DNS data: the smallest TTL seen (seconds).
    Dns(u32),
    /// DNS data whose lifetime nothing gave (a missing response, a negative answer without an
    /// `SOA`): not kept.
    Unknown,
}

impl Life {
    /// A DNS response's TTL (`liella::names::dns::host::ttl`) joins the answer.
    pub fn dns(&mut self, ttl: Option<u32>) {
        *self = match (*self, ttl) {
            (Life::Unknown, _) | (_, None) => Life::Unknown,
            (Life::Files, Some(t)) => Life::Dns(t),
            (Life::Dns(a), Some(t)) => Life::Dns(a.min(t)),
        };
    }

    /// The lifetime to store (`None`: do not store).
    pub fn ttl(self, negative: bool) -> Option<u64> {
        match self {
            Life::Files => Some(cache::FOREVER),
            Life::Dns(0) | Life::Unknown => None,
            Life::Dns(t) => Some(u64::from(t).min(if negative { NEGATIVE_CAP } else { POSITIVE_CAP })),
        }
    }
}

// ------------------------------------------------------------------------------ the process's cache

/// What `liella_nscache_status` reports.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Status {
    /// Enabled.
    pub enabled: bool,
    /// The enabled categories (bit `1 << n` for category `n`).
    pub categories: u32,
    /// `MODE_*`.
    pub mode: u32,
    /// `REASON_*`.
    pub reason: u32,
    /// The shared region's owner (`u64::MAX`: no region).
    pub owner: u64,
    /// The sum of the table's generations (0: no table): it moves with every invalidation.
    pub generation: u64,
    /// Answers served from the table.
    pub hits: u64,
    /// Lookups the table could not answer (the sources were asked).
    pub misses: u64,
    /// `stat` calls made to validate.
    pub stats: u64,
    /// Answers stored.
    pub stores: u64,
}

/// One lookup's view of the table: what it reads and writes under.
#[derive(Clone, Copy, Debug)]
struct Session {
    cat: Category,
    gens: (u64, u64),
    now: u64,
    attach: u64,
}

/// The process's cache (S9: atomics; the lock serialises attaching, dropping and writing).
pub struct NsCache {
    enabled: AtomicU8,
    cats_off: AtomicU32,
    lock: RawMutex,
    mode: AtomicU32,
    reason: AtomicU32,
    addr: AtomicUsize,
    words: AtomicUsize,
    fd: AtomicI32,
    owner: AtomicU64,
    stamp: [AtomicU64; 4],
    readers: AtomicU64,
    cred: AtomicU64,
    attached: AtomicU64,
    /// When the global files (0) and each category's file (1 + n) were last validated
    /// (monotonic nanoseconds; 0: never).
    checked: [AtomicU64; CATEGORIES + 1],
    /// The table's generations as last seen.
    seen: [AtomicU64; CATEGORIES + 1],
    /// Moves whenever validation ran or a generation was seen to move (`crate::nss::Watched`).
    epoch: AtomicU64,
    /// The identities of `passwd` and `nsswitch.conf` taken while attaching, for the first
    /// validation (consumed by it; `pre_valid` says whether they are there).
    pre: [AtomicU64; 10],
    pre_valid: AtomicBool,
    /// Cacheable lookups seen: the first one of a process goes to the sources (see
    /// [`NsCache::session`]).
    lookups: AtomicU32,
    hits: AtomicU64,
    misses: AtomicU64,
    stats: AtomicU64,
    stores: AtomicU64,
    secure: AtomicBool,
    dir: Buf<256>,
    dir_len: AtomicUsize,
}

impl core::fmt::Debug for NsCache {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NsCache").field("enabled", &self.enabled).field("mode", &self.mode).field("addr", &self.addr).finish_non_exhaustive()
    }
}

impl Default for NsCache {
    fn default() -> NsCache {
        NsCache::new()
    }
}

/// A reader's hold on the current mapping.
struct Reader<'a>(&'a AtomicU64);

impl Drop for Reader<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Release);
    }
}

fn mono_ns<S: Sys>(sys: &S) -> u64 {
    let t = sys.clock_gettime(CLOCK_MONOTONIC).unwrap_or_default();
    u64::try_from(t.sec).unwrap_or(0).saturating_mul(1_000_000_000).saturating_add(u64::try_from(t.nsec).unwrap_or(0)).max(1)
}

fn real_secs<S: Sys>(sys: &S) -> u64 {
    u64::try_from(sys.clock_gettime(CLOCK_REALTIME).unwrap_or_default().sec).unwrap_or(0)
}

/// The system's path of a watched file.
fn path_of(f: File) -> &'static [u8] {
    match f {
        File::Nsswitch => SYSTEM.nsswitch,
        File::HostConf => SYSTEM.host_conf,
        File::ResolvConf => PATH_RESCONF,
        File::GaiConf => SYSTEM.gai_conf,
        File::Hosts => SYSTEM.hosts,
        File::Passwd => SYSTEM.passwd,
        File::Group => SYSTEM.group,
        File::Services => SYSTEM.services,
        File::Protocols => SYSTEM.protocols,
        File::Networks => SYSTEM.networks,
    }
}

/// The identity of the file at `path` now.
fn identity<S: Sys>(sys: &S, path: &[u8]) -> FileId {
    match KStr::from_nul_terminated(path).and_then(|p| sys.fstatat(Fd::CWD, p, 0)) {
        Ok(s) => cache::file_id(s.dev, s.ino, s.size, (s.mtime.sec, s.mtime.nsec), (s.ctime.sec, s.ctime.nsec)),
        Err(_) => cache::MISSING,
    }
}

/// The inode number of the initial user namespace (`PROC_USER_INIT_INO`, ⚑ KERN: fixed since
/// Linux 3.8; nsfs reports it for `/proc/self/ns/user` in the initial namespace).
const INITIAL_USER_NS_INO: u64 = 0xEFFF_FFFD;

/// Whether the process is in the initial user namespace (`/proc/self/ns/user`, followed). Only
/// there are the IDs the kernel shows the process the IDs files in `/dev/shm` are owned by
/// for everyone: in another namespace "UID 0" may be any user, and a region named for it would
/// carry another account's name in the shared directory (validation T13). Unknown (no
/// `/proc`) counts as another namespace.
fn initial_user_ns<S: Sys>(sys: &S) -> bool {
    KStr::from_nul_terminated(b"/proc/self/ns/user\0").and_then(|p| sys.fstatat(Fd::CWD, p, 0)).is_ok_and(|s| s.ino == INITIAL_USER_NS_INO)
}

/// Take the region's OFD lock, trying `tries` times 0.5 ms apart: whether it is held.
fn lock_region<S: Sys>(sys: &S, fd: Fd, tries: u32) -> bool {
    for i in 0..tries {
        let mut fl = Flock::new(F_WRLCK, i16::try_from(SEEK_SET).unwrap_or(0), 0, 0);
        match sys.fcntl_lock(fd, FcntlLock::OfdSetLk, &mut fl) {
            Ok(()) => return true,
            Err(e) if e == Errno::EAGAIN || e == Errno::EACCES => {}
            Err(_) => return false,
        }
        if i.saturating_add(1) < tries {
            let _ = sys.clock_nanosleep(CLOCK_MONOTONIC, 0, &Timespec { sec: 0, nsec: 500_000 }, None);
        }
    }
    false
}

fn unlock_region<S: Sys>(sys: &S, fd: Fd) {
    let mut fl = Flock::new(F_UNLCK, i16::try_from(SEEK_SET).unwrap_or(0), 0, 0);
    let _ = sys.fcntl_lock(fd, FcntlLock::OfdSetLk, &mut fl);
}

/// The boot ID (`/proc/sys/kernel/random/boot_id`, 32 hexadecimal digits), or zero.
fn boot_id<S: Sys>(sys: &S) -> [u64; 2] {
    // One `read` (the file is 37 bytes).
    let mut text = [0u8; 64];
    let Ok(fd) = KStr::from_nul_terminated(b"/proc/sys/kernel/random/boot_id\0").and_then(|p| sys.openat(Fd::CWD, p, liella_kernel::uapi::O_RDONLY | O_CLOEXEC, 0)) else { return [0, 0] };
    let n = sys.read(fd, &mut text).unwrap_or(0);
    let _ = sys.close(fd);
    let text = text.get(..n).unwrap_or(&[]);
    let mut out = [0u64; 2];
    let mut n = 0usize;
    for d in text.iter().filter_map(|&c| char::from(c).to_digit(16)) {
        if let Some(w) = out.get_mut(n >> 4) {
            *w = w.wrapping_shl(4) | u64::from(d);
        }
        n = n.saturating_add(1);
    }
    out
}

impl NsCache {
    /// Enabled, nothing mapped, no region location.
    pub const fn new() -> NsCache {
        NsCache {
            enabled: AtomicU8::new(ON),
            cats_off: AtomicU32::new(0),
            lock: RawMutex::new(),
            mode: AtomicU32::new(MODE_IDLE),
            reason: AtomicU32::new(REASON_NONE),
            addr: AtomicUsize::new(0),
            words: AtomicUsize::new(0),
            fd: AtomicI32::new(-1),
            owner: AtomicU64::new(u64::MAX),
            stamp: [const { AtomicU64::new(0) }; 4],
            readers: AtomicU64::new(0),
            cred: AtomicU64::new(1),
            attached: AtomicU64::new(0),
            checked: [const { AtomicU64::new(0) }; CATEGORIES + 1],
            seen: [const { AtomicU64::new(0) }; CATEGORIES + 1],
            epoch: AtomicU64::new(1),
            pre: [const { AtomicU64::new(0) }; 10],
            pre_valid: AtomicBool::new(false),
            lookups: AtomicU32::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            stats: AtomicU64::new(0),
            stores: AtomicU64::new(0),
            secure: AtomicBool::new(false),
            dir: Buf::new(),
            dir_len: AtomicUsize::new(0),
        }
    }

    // -------------------------------------------------------------- configuration

    /// At process start: disabled by the environment (`LIELLA_NSCACHE=0`, already ignored by
    /// the caller under `AT_SECURE`), secure-execution mode, and the shared regions' directory
    /// (with its final `/`; `None`: private tables only).
    pub fn start(&self, disabled: bool, secure: bool, dir: Option<&[u8]>) {
        self.secure.store(secure, Relaxed);
        self.set_region_dir(dir);
        self.enabled.store(if disabled { OFF } else { ON }, Release);
    }

    /// Set the shared regions' directory (the products: `/dev/shm/`; the tests, through the
    /// boundary's `selftest`, a private one). The next lookup attaches anew.
    pub fn set_region_dir(&self, dir: Option<&[u8]>) {
        match dir {
            Some(d) if d.len() < 200 && !d.contains(&0) => {
                self.dir.store(d);
                self.dir_len.store(d.len(), Release);
            }
            _ => self.dir_len.store(0, Release),
        }
        self.credentials_changed();
    }

    /// The path of the region of `euid` (NUL-terminated) in `buf`: its length.
    fn region_path(&self, euid: u32, buf: &mut [u8; 256]) -> Option<usize> {
        let n = self.dir_len.load(Acquire);
        if n == 0 {
            return None;
        }
        self.dir.load(buf.get_mut(..n)?);
        let mut t = liella::net::Text::<48>::new();
        t.push_all(b"liella-nscache-");
        t.push_dec(u64::from(euid));
        let end = n.checked_add(t.as_bytes().len())?;
        buf.get_mut(n..end)?.copy_from_slice(t.as_bytes());
        *buf.get_mut(end)? = 0;
        end.checked_add(1)
    }

    /// Keep the identities of `passwd` and `nsswitch.conf` taken while attaching.
    fn prestat(&self, files: &[(File, FileId); 2]) {
        for (slot, v) in self.pre.iter().zip(files.iter().flat_map(|(_, id)| id.iter())) {
            slot.store(*v, Relaxed);
        }
        self.pre_valid.store(true, Release);
    }

    /// The identity of `f` taken while attaching, if this is the first validation after it.
    fn prestatted(&self, f: File) -> Option<FileId> {
        let at = match f {
            File::Passwd => 0usize,
            File::Nsswitch => 5,
            _ => return None,
        };
        self.pre_valid.load(Acquire).then(|| core::array::from_fn(|k| self.pre.get(at.saturating_add(k)).map_or(0, |a| a.load(Relaxed))))
    }

    fn stamp(&self) -> Stamp {
        let w = |i: usize| self.stamp.get(i).map_or(0, |a| a.load(Relaxed));
        Stamp { owner: w(0), boot: [w(1), w(2)], account: w(3) }
    }

    fn set_stamp(&self, s: &Stamp) {
        for (a, v) in self.stamp.iter().zip([s.owner, s.boot[0], s.boot[1], s.account]) {
            a.store(v, Relaxed);
        }
    }

    // -------------------------------------------------------------- the native interface

    /// Enable or disable the cache for the process: whether it was enabled. Disabling drops the
    /// table (nothing stays mapped).
    pub fn enable<S: Sys>(&self, sys: &S, mem: &dyn Memory, on: bool) -> bool {
        let _l = self.lock.lock(sys);
        let was = self.enabled.swap(if on { ON } else { OFF }, AcqRel) == ON;
        if !on {
            self.detach(sys, mem);
            for c in &self.checked {
                c.store(0, Relaxed);
            }
        }
        self.epoch.fetch_add(1, AcqRel);
        was
    }

    /// Enable or disable one category: whether it was enabled.
    pub fn set_category(&self, c: Category, on: bool) -> bool {
        let old = if on { self.cats_off.fetch_and(!c.bit(), AcqRel) } else { self.cats_off.fetch_or(c.bit(), AcqRel) };
        old & c.bit() == 0
    }

    /// Invalidate every category (`None`) or one: the table's generations move (the shared
    /// region's too, so every process of the user drops those entries), and the next lookup
    /// validates at once, whatever the time since the last check.
    pub fn invalidate<S: Sys>(&self, sys: &S, mem: &dyn Memory, which: Option<Category>) {
        let _l = self.lock.lock(sys);
        match which {
            None => {
                for c in &self.checked {
                    c.store(0, Relaxed);
                }
            }
            Some(c) => {
                if let Some(t) = self.checked.get(c.index().saturating_add(1)) {
                    t.store(0, Relaxed);
                }
                if let Some(t) = self.checked.first() {
                    t.store(0, Relaxed);
                }
            }
        }
        self.epoch.fetch_add(1, AcqRel);
        let Some(_r) = self.enter() else { return };
        if let Some(t) = self.table(mem) {
            let _ = self.write(sys, &t, |t| match which {
                None => t.bump(true, cache::ALL_CATEGORIES),
                Some(c) => t.bump(false, c.bit()),
            });
        }
    }

    /// The cache's state.
    pub fn status(&self, mem: &dyn Memory) -> Status {
        let enabled = self.enabled.load(Acquire) == ON;
        let mode = if enabled { self.mode.load(Acquire) } else { MODE_OFF };
        let generation = self.enter().and_then(|_r| self.table(mem).and_then(|t| t.all_gens())).map_or(0, |g| g.iter().fold(0u64, |s, &v| s.wrapping_add(v)));
        Status {
            enabled,
            categories: cache::ALL_CATEGORIES & !self.cats_off.load(Relaxed),
            mode,
            reason: self.reason.load(Relaxed),
            owner: if mode == MODE_SHARED { self.owner.load(Relaxed) } else { u64::MAX },
            generation,
            hits: self.hits.load(Relaxed),
            misses: self.misses.load(Relaxed),
            stats: self.stats.load(Relaxed),
            stores: self.stores.load(Relaxed),
        }
    }

    // -------------------------------------------------------------- hooks

    /// The process's credentials changed (the setxid family, `setgroups`; D-17.7): the next
    /// lookup drops the mapping and attaches for the new identity.
    pub fn credentials_changed(&self) {
        self.cred.fetch_add(1, AcqRel);
    }

    /// In a `fork` child (one thread): no other thread holds the lock or a reader count, and the
    /// region's descriptor is shared with the parent (and so is its OFD lock), so the next
    /// lookup attaches anew.
    pub fn after_fork_child(&self) {
        self.lock.reset();
        self.readers.store(0, Release);
        self.credentials_changed();
    }

    /// While the cache is in use and validated the global files less than a second ago: the
    /// epoch that names that validation (`crate::nss::Watched` skips its own `stat` while the
    /// epoch it last checked under is current).
    pub fn config_epoch<S: Sys>(&self, sys: &S) -> Option<u64> {
        if self.enabled.load(Relaxed) != ON {
            return None;
        }
        let m = self.mode.load(Relaxed);
        if (m != MODE_PRIVATE && m != MODE_SHARED) || self.attached.load(Relaxed) != self.cred.load(Relaxed) {
            return None;
        }
        let t = self.checked.first().map_or(0, |c| c.load(Relaxed));
        if t == 0 || mono_ns(sys).saturating_sub(t) >= WINDOW_NS {
            return None;
        }
        Some(self.epoch.load(Acquire))
    }

    // -------------------------------------------------------------- tables

    fn enter(&self) -> Option<Reader<'_>> {
        let r = self.readers.fetch_add(1, Acquire);
        let g = Reader(&self.readers);
        (r & CLOSING == 0).then_some(g)
    }

    /// The current table (the caller holds a [`Reader`]).
    fn table<'m>(&self, mem: &'m dyn Memory) -> Option<Table<'m>> {
        let addr = self.addr.load(Acquire);
        if addr == 0 {
            return None;
        }
        Table::new(mem.words(addr, self.words.load(Acquire)))
    }

    /// Drop the current table (the caller holds the lock and no [`Reader`]).
    fn detach<S: Sys>(&self, sys: &S, mem: &dyn Memory) {
        self.readers.fetch_or(CLOSING, AcqRel);
        while self.readers.load(Acquire) & !CLOSING != 0 {
            sys.sched_yield();
        }
        let addr = self.addr.swap(0, AcqRel);
        let words = self.words.swap(0, AcqRel);
        if addr != 0 {
            mem.unmap(addr, words.saturating_mul(8));
        }
        let fd = self.fd.swap(-1, AcqRel);
        if fd >= 0 {
            let _ = sys.close(Fd::new(fd));
        }
        self.owner.store(u64::MAX, Relaxed);
        self.mode.store(MODE_IDLE, Release);
        self.readers.fetch_and(!CLOSING, Release);
    }

    /// Map a table for the current credentials (the caller holds the lock and no [`Reader`]).
    fn attach<S: Sys>(&self, sys: &S, mem: &dyn Memory) {
        let cred = self.cred.load(Acquire);
        self.detach(sys, mem);
        let euid = sys.geteuid();
        let (mode, reason) = match self.open_shared(sys, mem, euid) {
            Ok(()) => (MODE_SHARED, REASON_NONE),
            Err(why) if self.make_private(sys, mem, euid) => (MODE_PRIVATE, why),
            Err(why) => (MODE_UNAVAILABLE, why),
        };
        for c in &self.checked {
            c.store(0, Relaxed);
        }
        if let Some(g) = self.enter().and_then(|_r| self.table(mem).and_then(|t| t.all_gens())) {
            for (s, v) in self.seen.iter().zip(g) {
                s.store(v, Relaxed);
            }
        }
        self.reason.store(reason, Relaxed);
        self.mode.store(mode, Release);
        self.epoch.fetch_add(1, AcqRel);
        self.attached.store(cred, Release);
    }

    /// The private table.
    fn make_private<S: Sys>(&self, _sys: &S, mem: &dyn Memory, euid: u32) -> bool {
        let Some(addr) = mem.map_private(cache::CAP_BYTES) else { return false };
        let stamp = Stamp { owner: u64::from(euid), ..Stamp::default() };
        let Some(t) = Table::new(mem.words(addr, cache::CAP_WORDS)) else {
            mem.unmap(addr, cache::CAP_BYTES);
            return false;
        };
        let _ = t.begin();
        t.init(&stamp, false);
        t.end();
        self.set_stamp(&stamp);
        self.words.store(cache::CAP_WORDS, Release);
        self.addr.store(addr, Release);
        true
    }

    /// The user's shared region (see the module documentation): `Err` with the reason it is
    /// not used.
    fn open_shared<S: Sys>(&self, sys: &S, mem: &dyn Memory, euid: u32) -> core::result::Result<(), u32> {
        let mut buf = [0u8; 256];
        let n = self.region_path(euid, &mut buf).ok_or(REASON_NO_REGION)?;
        // The overflow UID is what a user namespace shows for every unmapped owner: a file it
        // seems to own may belong to anyone, so such a process keeps a private table.
        if self.secure.load(Relaxed) || euid == OVERFLOW_UID || sys.getuid() != euid || sys.getgid() != sys.getegid() {
            return Err(REASON_SECURE);
        }
        // Inside a user namespace the name and the owner check would use that namespace's IDs,
        // while `/dev/shm` is shared with every namespace: such a process keeps a private table.
        if !initial_user_ns(sys) {
            return Err(REASON_SECURE);
        }
        let path = KStr::from_nul_terminated(buf.get(..n).unwrap_or(&[])).map_err(|_| REASON_UNAVAILABLE)?;
        // The region usually exists: open it first (`O_NONBLOCK` so that a FIFO cannot block);
        // create it only when it is missing, exclusively, so that nothing else is ever taken.
        let refused = |e: Errno| e == Errno::ELOOP || e == Errno::EACCES || e == Errno::EPERM || e == Errno::ENXIO || e == Errno::EISDIR;
        let (fd, created) = match sys.openat(Fd::CWD, path, O_RDWR | O_NOFOLLOW | O_NONBLOCK | O_NOCTTY | O_CLOEXEC, 0) {
            Ok(fd) => (fd, false),
            Err(e) if e == Errno::ENOENT => match sys.openat(Fd::CWD, path, O_RDWR | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600) {
                Ok(fd) => (fd, true),
                Err(e) if refused(e) || e == Errno::EEXIST => return Err(if e == Errno::EEXIST { REASON_UNAVAILABLE } else { REASON_REFUSED }),
                Err(_) => return Err(REASON_UNAVAILABLE),
            },
            Err(e) if refused(e) => return Err(REASON_REFUSED),
            Err(_) => return Err(REASON_UNAVAILABLE),
        };
        // Kept across calls: never one of the standard descriptors, or a program that closed its
        // output would write into the region (validation T12).
        let fd = match liella::io::above_standard(sys, fd) {
            Ok(fd) => fd,
            Err(_) => {
                if created {
                    // Not sized yet: other processes would wait for it forever.
                    let _ = sys.unlinkat(Fd::CWD, path, 0);
                }
                return Err(REASON_UNAVAILABLE);
            }
        };
        let r = self.map_region(sys, mem, fd, created, euid);
        if r.is_err() {
            let _ = sys.close(fd);
        }
        r
    }

    fn map_region<S: Sys>(&self, sys: &S, mem: &dyn Memory, fd: Fd, created: bool, euid: u32) -> core::result::Result<(), u32> {
        if created {
            // Ours, just made: its mode exactly 0600 whatever the umask, its size the cap.
            sys.fchmod(fd, 0o600).map_err(|_| REASON_UNAVAILABLE)?;
            sys.ftruncate(fd, i64::try_from(cache::CAP_BYTES).unwrap_or(0)).map_err(|_| REASON_UNAVAILABLE)?;
        }
        let st = sys.fstat(fd).map_err(|_| REASON_UNAVAILABLE)?;
        if st.mode & S_IFMT != S_IFREG || st.uid != euid || st.mode & 0o7777 != 0o600 {
            return Err(REASON_REFUSED);
        }
        let size = usize::try_from(st.size).map_err(|_| REASON_REFUSED)?;
        if size == 0 {
            // Another process has just created it and not yet sized it.
            return Err(REASON_UNAVAILABLE);
        }
        if size > cache::CAP_BYTES || size & 4095 != 0 || cache::geometry(size >> 3).is_none() {
            return Err(REASON_REFUSED);
        }
        let addr = mem.map_shared(fd, size).ok_or(REASON_UNAVAILABLE)?;
        let words = size >> 3;
        let boot = boot_id(sys);
        // The files the account stamp rests on, as they are now (the first validation reuses
        // these `stat`s).
        let files = [(File::Passwd, identity(sys, path_of(File::Passwd))), (File::Nsswitch, identity(sys, path_of(File::Nsswitch)))];
        self.stats.fetch_add(2, Relaxed);
        self.prestat(&files);
        let owner = u64::from(euid);
        let ok = match Table::new(mem.words(addr, words)) {
            Some(t) => {
                // The stamp the table holds is this account's without a lookup when it was
                // verified against these very files (a user database edited since, an account
                // deleted and its UID reused, changes their identity) and only `files` answers
                // passwd lookups. Otherwise the account's name is looked up.
                let recorded = t.stamp_files();
                let cheap = recorded == Some([files[0].1, files[1].1]) && passwd_from_files(sys);
                let stamp = match t.account() {
                    Some(a) if cheap => Stamp { owner, boot, account: a },
                    _ => Stamp { owner, boot, account: account(sys, euid) },
                };
                let good = t.valid(&stamp) && recorded == Some([files[0].1, files[1].1]);
                if good {
                    self.set_stamp(&stamp);
                    true
                } else {
                    // Not this account's table, left half-changed, or its stamp verified against
                    // other files: under the lock, start afresh or record the files.
                    let locked = lock_region(sys, fd, 100);
                    if locked {
                        let ok = t.valid(&stamp);
                        let consistent = t.begin();
                        if !ok || !consistent {
                            t.init(&stamp, !created);
                        }
                        t.set_stamp_files(&[files[0].1, files[1].1]);
                        t.end();
                        unlock_region(sys, fd);
                        self.set_stamp(&stamp);
                    }
                    locked
                }
            }
            None => false,
        };
        if !ok {
            mem.unmap(addr, size);
            return Err(REASON_UNAVAILABLE);
        }
        self.owner.store(owner, Relaxed);
        self.fd.store(fd.raw(), Release);
        self.words.store(words, Release);
        self.addr.store(addr, Release);
        Ok(())
    }

    /// Change the table under the writers' locks (the caller holds the process's lock and a
    /// [`Reader`]): `false` when the region's lock could not be had soon (nothing written). A
    /// table that is not valid, or that a writer left half-changed, is reinitialised first.
    fn write<S: Sys>(&self, sys: &S, t: &Table<'_>, f: impl FnOnce(&Table<'_>)) -> bool {
        let fd = self.fd.load(Acquire);
        if fd >= 0 && !lock_region(sys, Fd::new(fd), 20) {
            return false;
        }
        let stamp = self.stamp();
        let ok = t.valid(&stamp);
        let consistent = t.begin();
        if !ok || !consistent {
            t.init(&stamp, true);
        }
        f(t);
        t.end();
        if fd >= 0 {
            unlock_region(sys, Fd::new(fd));
        }
        true
    }

    /// Validate the files of `cat` (and the global ones) when they are due: `false` when a
    /// change could not be recorded (the lookup then goes to the sources).
    fn validate<S: Sys>(&self, sys: &S, mem: &dyn Memory, cat: Category) -> bool {
        let now = mono_ns(sys);
        let ci = cat.index().saturating_add(1);
        let due = |i: usize| {
            let t = self.checked.get(i).map_or(0, |c| c.load(Relaxed));
            t == 0 || now.saturating_sub(t) >= WINDOW_NS
        };
        let (global, own) = (due(0), due(ci));
        if !global && !own {
            return true;
        }
        let mut ids: [(File, FileId); 5] = [(File::Nsswitch, cache::MISSING); 5];
        let mut n = 0usize;
        let files = File::GLOBAL.iter().copied().filter(|_| global).chain(core::iter::once(cat.file()).filter(|_| own));
        let mut stats = 0u64;
        for (slot, f) in ids.iter_mut().zip(files) {
            *slot = match self.prestatted(f) {
                Some(id) => (f, id),
                None => {
                    stats = stats.saturating_add(1);
                    (f, identity(sys, path_of(f)))
                }
            };
            n = n.saturating_add(1);
        }
        self.pre_valid.store(false, Release);
        self.stats.fetch_add(stats, Relaxed);
        let ids = ids.get(..n).unwrap_or(&[]);
        let differs = |t: &Table<'_>| ids.iter().any(|(f, id)| t.file(*f).is_none_or(|rec| rec != *id));
        let changed = match self.enter().and_then(|_r| self.table(mem).map(|t| differs(&t))) {
            Some(c) => c,
            None => return false,
        };
        if changed {
            let _l = self.lock.lock(sys);
            let Some(_r) = self.enter() else { return false };
            let Some(t) = self.table(mem) else { return false };
            let written = self.write(sys, &t, |t| {
                let (mut all, mut mask) = (false, 0u32);
                for (f, id) in ids {
                    let rec = t.file_in_change(*f);
                    if rec == *id {
                        continue;
                    }
                    // A file never recorded (a fresh table) is recorded without a move.
                    if rec != [0; 5] {
                        match f.category() {
                            None => all = true,
                            Some(c) => mask |= c.bit(),
                        }
                    }
                    t.set_file(*f, id);
                }
                t.bump(all, mask);
            });
            if !written {
                return false;
            }
        }
        if global {
            if let Some(c) = self.checked.first() {
                c.store(now, Relaxed);
            }
            self.epoch.fetch_add(1, AcqRel);
        }
        if own {
            if let Some(c) = self.checked.get(ci) {
                c.store(now, Relaxed);
            }
        }
        true
    }

    /// Start a lookup of `cat`: `None` when the cache does not serve it (disabled, the category
    /// disabled, overrides set, no table, a change that could not be recorded, a busy table).
    fn session<S: Sys>(&self, sys: &S, mem: &dyn Memory, cat: Category) -> Option<Session> {
        if self.enabled.load(Relaxed) != ON {
            return None;
        }
        if self.cats_off.load(Relaxed) & cat.bit() != 0 || crate::nss::SWITCH.overridden() {
            return None;
        }
        // A process's first cacheable lookup goes to the sources, without the table: they read
        // the current files, and a process that makes only that one lookup (most short-lived
        // commands) pays nothing for the cache. The second attaches and validates.
        if self.mode.load(Acquire) == MODE_IDLE && self.lookups.fetch_add(1, Relaxed) == 0 {
            return None;
        }
        if self.attached.load(Acquire) != self.cred.load(Acquire) || self.mode.load(Acquire) == MODE_IDLE {
            let _l = self.lock.lock(sys);
            if self.enabled.load(Relaxed) != ON {
                return None;
            }
            if self.attached.load(Acquire) != self.cred.load(Acquire) || self.mode.load(Acquire) == MODE_IDLE {
                self.attach(sys, mem);
            }
        }
        let attach = self.attached.load(Acquire);
        let m = self.mode.load(Acquire);
        if m != MODE_PRIVATE && m != MODE_SHARED {
            return None;
        }
        if !self.validate(sys, mem, cat) {
            return None;
        }
        let stamp = self.stamp();
        let valid = self.enter().and_then(|_r| self.table(mem).map(|t| t.valid(&stamp)))?;
        if !valid {
            // Damaged meanwhile (or by someone else): the next writer starts it afresh.
            let _l = self.lock.lock(sys);
            let _r = self.enter()?;
            let t = self.table(mem)?;
            if !self.write(sys, &t, |_| {}) {
                return None;
            }
        }
        let _r = self.enter()?;
        let t = self.table(mem)?;
        let all = t.all_gens()?;
        let mut moved = false;
        for (s, g) in self.seen.iter().zip(all) {
            if s.swap(g, Relaxed) != g {
                moved = true;
            }
        }
        if moved {
            self.epoch.fetch_add(1, AcqRel);
        }
        let gens = (all[0], *all.get(cat.index().saturating_add(1))?);
        Some(Session { cat, gens, now: real_secs(sys), attach })
    }

    fn get(&self, mem: &dyn Memory, s: &Session, key: &[u8], hash: u64, out: &mut TryVec<u8>) -> Found {
        let Some(_r) = self.enter() else { return Found::Busy };
        if self.attached.load(Acquire) != s.attach {
            return Found::Busy;
        }
        let Some(t) = self.table(mem) else { return Found::Busy };
        let mut buf = TryVec::new();
        t.get(key, hash, s.gens, s.now, &mut buf, out).unwrap_or(Found::Busy)
    }

    fn put<S: Sys>(&self, sys: &S, mem: &dyn Memory, s: &Session, key: &[u8], value: &[u8], ttl: u64) {
        let hash = cache::hash(0, key);
        let _l = self.lock.lock(sys);
        let Some(_r) = self.enter() else { return };
        if self.attached.load(Acquire) != s.attach {
            return;
        }
        let Some(t) = self.table(mem) else { return };
        let stored = Stored { category: s.cat, gens: s.gens, time: s.now, ttl };
        let mut put = false;
        if self.write(sys, &t, |t| put = t.put(key, hash, &stored, value)) && put {
            self.stores.fetch_add(1, Relaxed);
        }
    }
}

/// The process's cache.
pub static NSCACHE: NsCache = NsCache::new();

/// The account stamp of `euid`: its user-database name (looked up without the cache) and UID.
fn account<S: Sys>(sys: &S, euid: u32) -> u64 {
    crate::nss::passwd(&Ctx::system(sys), Key::Id(euid), |p| cache::account(p.name, euid)).unwrap_or_else(|_| cache::account(b"", euid))
}

/// Whether only the `files` source (or `compat`, which is `files` here) answers passwd lookups,
/// so that the user database is the `passwd` file whose identity the account stamp records.
fn passwd_from_files<S: Sys>(sys: &S) -> bool {
    // As read: when `nsswitch.conf` changed since the stamp was verified, its recorded identity
    // differs anyway and the account is looked up.
    let spec = crate::nss::SWITCH.config_as_read(sys, SYSTEM.nsswitch).spec(liella::names::nss::Database::Passwd);
    // Other modules (`systemd`, `sss`, ...) cannot answer before the NSS module ABI (F27, phase 14):
    // when they can, a line naming one must make this false.
    spec.steps().iter().all(|s| matches!(s.service, liella::names::nss::Service::Files | liella::names::nss::Service::Compat | liella::names::nss::Service::Module))
}

/// Whether `path` is one of the switch's configuration files whose checks the cache's window
/// covers (`crate::nss::Watched`): `nsswitch.conf`, `host.conf`, `gai.conf`. `resolv.conf`
/// keeps its own check: it decides when `_res` is read again (D-16.3).
pub fn covers(path: &[u8]) -> bool {
    path == SYSTEM.nsswitch || path == SYSTEM.host_conf || path == SYSTEM.gai_conf
}

// ------------------------------------------------------------------------------ lookups

/// The answer of `cat` for `key` from the table, or from the sources (`fill`, which reports
/// how long its answer may be kept), stored if it may be.
pub fn lookup<S: Sys, V>(
    sys: &S,
    mem: &dyn Memory,
    cat: Category,
    key: &[u8],
    fill: impl FnOnce(&mut Life) -> Answer<V>,
    encode: impl FnOnce(&V, &mut TryVec<u8>) -> Result<()>,
    decode: impl FnOnce(&[u8]) -> Option<V>,
) -> Answer<V> {
    let c = &NSCACHE;
    let Some(s) = c.session(sys, mem, cat) else { return fill(&mut Life::Files) };
    let hash = cache::hash(0, key);
    let mut out = TryVec::new();
    if c.get(mem, &s, key, hash, &mut out) == Found::Hit {
        // A value this code cannot read is a miss: the sources answer, and replace it.
        let hit = match out.split_first() {
            Some((0, v)) => decode(v).map(Ok),
            Some((1, _)) => Some(Err(LookupError::NotFound)),
            Some((2, _)) => Some(Err(LookupError::NoData)),
            _ => None,
        };
        if let Some(a) = hit {
            c.hits.fetch_add(1, Relaxed);
            return a;
        }
    }
    c.misses.fetch_add(1, Relaxed);
    let mut life = Life::Files;
    let a = fill(&mut life);
    let (tag, negative) = match &a {
        Ok(_) => (0u8, false),
        Err(LookupError::NotFound) => (1, true),
        Err(LookupError::NoData) => (2, true),
        Err(_) => return a,
    };
    let Some(ttl) = life.ttl(negative) else { return a };
    let mut val = TryVec::new();
    if val.push(tag).is_err() {
        return a;
    }
    if let Ok(v) = &a {
        if encode(v, &mut val).is_err() {
            return a;
        }
    }
    c.put(sys, mem, &s, key, &val, ttl);
    a
}

/// The memory of a context the cache may serve: it has one, and reads the system's files.
fn cached<'a, S: Sys>(ctx: &Ctx<'a, S>) -> Option<&'a dyn Memory> {
    let p = ctx.paths;
    let system = [(p.nsswitch, SYSTEM.nsswitch), (p.host_conf, SYSTEM.host_conf), (p.gai_conf, SYSTEM.gai_conf), (p.hosts, SYSTEM.hosts), (p.networks, SYSTEM.networks), (p.services, SYSTEM.services), (p.protocols, SYSTEM.protocols), (p.passwd, SYSTEM.passwd), (p.group, SYSTEM.group)]
        .iter()
        .all(|(a, b)| a == b);
    ctx.cache.filter(|_| system)
}

/// A key: the category, the kind of query, and its parts (each with its length).
pub fn key(cat: Category, kind: u8, parts: &[&[u8]]) -> Result<TryVec<u8>> {
    let mut k = TryVec::new();
    k.push(u8::try_from(cat.index()).unwrap_or(0))?;
    k.push(kind)?;
    for p in parts {
        k.extend_from_slice(&u32::try_from(p.len()).unwrap_or(u32::MAX).to_le_bytes())?;
        k.extend_from_slice(p)?;
    }
    Ok(k)
}

/// A database line through the cache (`fill`: the sources' line).
pub fn line<S: Sys>(ctx: &Ctx<'_, S>, cat: Category, kind: u8, parts: &[&[u8]], fill: impl FnOnce() -> Answer<TryVec<u8>>) -> Answer<TryVec<u8>> {
    let Some(mem) = cached(ctx) else { return fill() };
    let Ok(k) = key(cat, kind, parts) else { return fill() };
    lookup(ctx.sys, mem, cat, &k, |_| fill(), |v, out| out.extend_from_slice(v), |b| TryVec::from_slice(b).ok())
}

/// A database line of the system's files through the cache, without a context (the caller
/// builds one in `fill` only when the cache cannot answer).
pub fn line_system<S: Sys>(sys: &S, mem: &dyn Memory, cat: Category, kind: u8, parts: &[&[u8]], fill: impl FnOnce() -> Answer<TryVec<u8>>) -> Answer<TryVec<u8>> {
    let Ok(k) = key(cat, kind, parts) else { return fill() };
    lookup(sys, mem, cat, &k, |_| fill(), |v, out| out.extend_from_slice(v), |b| TryVec::from_slice(b).ok())
}

/// A group list through the cache (`fill`: the GIDs the sources list, without the base).
pub fn gid_list<S: Sys>(ctx: &Ctx<'_, S>, user: &[u8], fill: impl FnOnce() -> Result<TryVec<u32>>) -> Result<TryVec<u32>> {
    let Some(mem) = cached(ctx) else { return fill() };
    let Ok(k) = key(Category::Group, 9, &[user]) else { return fill() };
    let r = lookup(
        ctx.sys,
        mem,
        Category::Group,
        &k,
        |_| fill().map_err(LookupError::Os),
        |v, out| {
            out.push(1)?;
            for g in v.iter() {
                out.extend_from_slice(&g.to_le_bytes())?;
            }
            Ok(())
        },
        |b| {
            let mut v = TryVec::new();
            for c in b.get(1..)?.chunks_exact(4) {
                v.push(u32::from_le_bytes(c.try_into().ok()?)).ok()?;
            }
            Some(v)
        },
    );
    match r {
        Ok(v) => Ok(v),
        Err(LookupError::Os(e)) => Err(e),
        Err(_) => Ok(TryVec::new()),
    }
}

/// A host through the cache: by name (`Some(name)`, with the family asked) or by address.
pub fn host<S: Sys>(ctx: &Ctx<'_, S>, name: Option<&[u8]>, family: u8, addr: Option<Ip>, fill: impl FnOnce(&mut Life) -> Answer<Host>) -> Answer<Host> {
    let Some(mem) = cached(ctx) else { return fill(&mut Life::Files) };
    let fp = ctx.dns.map_or(0, fingerprint).to_le_bytes();
    let k = match (name, addr) {
        (Some(n), _) => key(Category::Hosts, 1, &[&[family, u8::from(ctx.multi)], &fp, n]),
        (None, Some(a)) => key(Category::Hosts, 2, &[&[u8::from(ctx.multi)], &fp, a.bytes()]),
        (None, None) => return fill(&mut Life::Files),
    };
    let Ok(k) = k else { return fill(&mut Life::Files) };
    lookup(ctx.sys, mem, Category::Hosts, &k, fill, |h, out| h.encode(out), Host::decode)
}

/// A fingerprint of what the `dns` source answers with: the resolver's effective configuration
/// and the `HOSTALIASES` text.
pub fn fingerprint(r: &Resolver) -> u64 {
    let h = cache::mix(config_fingerprint(&r.conf), r.aliases.as_deref().map_or(0, |t| cache::hash(3, t)));
    cache::finish(h)
}

/// A fingerprint of a resolver configuration (also how the boundary sees that a program changed
/// `_res`).
pub fn config_fingerprint(c: &liella::names::dns::conf::Config) -> u64 {
    let mut h = cache::hash(1, &[]);
    for s in c.servers.iter() {
        h = cache::mix(h, cache::hash(u64::from(s.port) << 32 | u64::from(s.scope), s.ip.bytes()));
    }
    for d in c.search.iter() {
        h = cache::mix(h, cache::hash(2, d));
    }
    for (a, m) in c.sort.iter() {
        h = cache::mix(h, u64::from(u32::from_be_bytes(*a)) << 32 | u64::from(u32::from_be_bytes(*m)));
    }
    for v in [u64::from(c.ndots), u64::from(c.timeout), u64::from(c.attempts), c.options] {
        h = cache::mix(h, v);
    }
    cache::finish(h)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn lifetimes() {
        assert_eq!(Life::Files.ttl(false), Some(cache::FOREVER));
        assert_eq!(Life::Files.ttl(true), Some(cache::FOREVER));
        let mut l = Life::Files;
        l.dns(Some(7200));
        assert_eq!(l.ttl(false), Some(POSITIVE_CAP), "positive answers: an hour at most");
        assert_eq!(l.ttl(true), Some(NEGATIVE_CAP), "negative answers: five minutes at most");
        l.dns(Some(30));
        l.dns(Some(60));
        assert_eq!(l.ttl(false), Some(30), "the smallest TTL of the responses");
        l.dns(Some(0));
        assert_eq!(l.ttl(false), None, "TTL 0 is never stored");
        let mut u = Life::Files;
        u.dns(None);
        u.dns(Some(60));
        assert_eq!(u.ttl(true), None, "a response without a lifetime");
    }

    #[test]
    fn keys_keep_their_parts_apart() {
        let a = key(Category::Services, 2, &[b"ab", b"c"]).unwrap();
        let b = key(Category::Services, 2, &[b"a", b"bc"]).unwrap();
        let c = key(Category::Protocols, 2, &[b"ab", b"c"]).unwrap();
        assert_ne!(a.as_slice(), b.as_slice());
        assert_ne!(a.as_slice(), c.as_slice());
    }
}
