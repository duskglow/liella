//! The Name Service Switch of the process (spec §16.1; D-16.1, D-16.2): the configuration from
//! `/etc/nsswitch.conf` (re-read when the file changes, as the contract says, and overridden per
//! database by `__nss_configure_lookup`), `host.conf`'s `multi`, and the lookups of the user,
//! group, `ethers`, netgroup and `aliases` databases through the engine (`liella::names::nss`).
//! The netdb databases are in `crate::netdb_compat`, `getaddrinfo` in `crate::gai`.
//!
//! **The sources** (D-16.1). A lookup asks the sources of its database's line in order, through
//! [`liella::names::nss::run`]: `files` (and `compat` for `passwd`, `group` and `shadow`) is
//! `crate::nss_files`; `dns` answers the `hosts` database (`crate::resolv_compat`, phase 10
//! part 4) when the lookup's [`Ctx`] carries a resolver, and is unavailable otherwise (and for
//! every other database); any other source is a module of the NSS module ABI (F27, phase 14)
//! and unavailable until then.
//!
//! **Reloading.** Every lookup checks the configuration file's identity (device, inode, size and
//! change times); a different file, or one that appeared or disappeared, is read again. Without
//! the file every database uses its default (the manual). `host.conf` is checked the same way,
//! by the host lookups ([`Ctx::with_multi`]); calls that consult no database (numeric
//! `getaddrinfo` and `getnameinfo`) check neither file ([`Ctx::bare`]).
//! `__nss_configure_lookup` replaces one database's line until the process ends, whatever the
//! file says later (observed).
//!
//! **Enumerations** (`getpwent`, `gethostent`, ...) read the `files` source when the database's
//! line names it (`files` or `compat`), and are empty otherwise: the other built-in source
//! cannot enumerate.
//!
//! **Testability.** Every lookup takes a [`Ctx`]: the files to read ([`Paths`], fixtures in the
//! tests) and the configuration to use ([`Config`], built from text in the tests).

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering::{Acquire, Relaxed, Release}};

use liella::error::{Errno, Result};
use liella::mem::TryVec;
use liella::names::cache::Category;
use liella::names::db;
use liella::names::nss::{self, Answer, Config, Database, LookupError, Service, Spec, DATABASES, SPEC_BYTES};
use liella::users::{self, Group, Gshadow, Passwd, Shadow};
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_rt::sync::RawMutex;

use crate::nss_files::{self as files, Key};
use crate::result_slots::Buf;

/// The files the switch and its `files` source read (NUL-terminated paths).
#[derive(Clone, Copy, Debug)]
pub struct Paths<'a> {
    /// `nsswitch.conf`.
    pub nsswitch: &'a [u8],
    /// `host.conf`.
    pub host_conf: &'a [u8],
    /// `gai.conf`.
    pub gai_conf: &'a [u8],
    /// `hosts`.
    pub hosts: &'a [u8],
    /// `networks`.
    pub networks: &'a [u8],
    /// `services`.
    pub services: &'a [u8],
    /// `protocols`.
    pub protocols: &'a [u8],
    /// `rpc`.
    pub rpc: &'a [u8],
    /// `ethers`.
    pub ethers: &'a [u8],
    /// `netgroup`.
    pub netgroup: &'a [u8],
    /// `aliases`.
    pub aliases: &'a [u8],
    /// `passwd`.
    pub passwd: &'a [u8],
    /// `group`.
    pub group: &'a [u8],
    /// `shadow`.
    pub shadow: &'a [u8],
    /// `gshadow`.
    pub gshadow: &'a [u8],
}

/// The system's files.
pub const SYSTEM: Paths<'static> = Paths {
    nsswitch: b"/etc/nsswitch.conf\0",
    host_conf: b"/etc/host.conf\0",
    gai_conf: b"/etc/gai.conf\0",
    hosts: b"/etc/hosts\0",
    networks: b"/etc/networks\0",
    services: b"/etc/services\0",
    protocols: b"/etc/protocols\0",
    rpc: b"/etc/rpc\0",
    ethers: files::ETHERS,
    netgroup: b"/etc/netgroup\0",
    aliases: b"/etc/aliases\0",
    passwd: files::PASSWD,
    group: files::GROUP,
    shadow: files::SHADOW,
    gshadow: files::GSHADOW,
};

// ------------------------------------------------------------------------------ watched files

/// A configuration file read once and again when it changes: its identity when last read.
///
/// While the name-service cache is in use and validated less than a second ago (D-16.10), the
/// system's `nsswitch.conf`, `host.conf` and `gai.conf` are not `stat`ed again until its next
/// validation (`crate::nscache::NsCache::config_epoch`): the cache's window is theirs too, so
/// that an answer it serves and the configuration a lookup uses agree.
#[derive(Debug)]
pub struct Watched {
    lock: RawMutex,
    loaded: AtomicBool,
    /// Present (1) or missing (2), device, inode, size, change and modification times.
    stamp: [AtomicU64; 8],
    /// The cache's validation epoch when the file was last checked (`u64::MAX`: none).
    epoch: AtomicU64,
}

impl Default for Watched {
    fn default() -> Watched {
        Watched::new()
    }
}

impl Watched {
    /// Never read.
    pub const fn new() -> Watched {
        Watched { lock: RawMutex::new(), loaded: AtomicBool::new(false), stamp: [const { AtomicU64::new(0) }; 8], epoch: AtomicU64::new(u64::MAX) }
    }

    /// Under the lock: when `path` was never read, or (`recheck`) changed since, `load` with its
    /// text (`None` when it cannot be read); then `read`.
    pub fn with<S: Sys, T>(&self, sys: &S, path: &[u8], recheck: bool, load: impl FnOnce(Option<&[u8]>), read: impl FnOnce() -> T) -> T {
        let _g = self.lock.lock(sys);
        let loaded = self.loaded.load(Relaxed);
        let epoch = if loaded && recheck && crate::nscache::covers(path) { crate::nscache::NSCACHE.config_epoch(sys) } else { None };
        let skip = epoch.is_some_and(|e| e == self.epoch.load(Relaxed));
        if !loaded || (recheck && !skip) {
            self.epoch.store(epoch.unwrap_or(u64::MAX), Relaxed);
            let st = KStr::from_nul_terminated(path).and_then(|p| sys.fstatat(Fd::CWD, p, 0));
            let u = |v: i64| u64::from_ne_bytes(v.to_ne_bytes());
            let now = match st {
                Ok(s) => [1, s.dev, s.ino, u(s.size), u(s.ctime.sec), u(s.ctime.nsec), u(s.mtime.sec), u(s.mtime.nsec)],
                Err(_) => [2, 0, 0, 0, 0, 0, 0, 0],
            };
            if !loaded || self.stamp.iter().zip(now).any(|(a, b)| a.load(Relaxed) != b) {
                let text = liella::system::read_file(sys, path).ok();
                load(text.as_deref());
                for (a, b) in self.stamp.iter().zip(now) {
                    a.store(b, Relaxed);
                }
                self.loaded.store(true, Relaxed);
            }
        }
        read()
    }
}

// ------------------------------------------------------------------------------ the switch

const SLOT: usize = SPEC_BYTES + 1;
const TABLE: usize = DATABASES * SLOT;

/// The process's switch configuration: the file's lines and `__nss_configure_lookup`'s
/// overrides, each database's line encoded in atomics (S9).
pub struct Switch {
    file: Watched,
    lines: Buf<TABLE>,
    overrides: Buf<TABLE>,
    override_lock: RawMutex,
    /// Whether `__nss_configure_lookup` was ever called (until then the overrides are empty and
    /// are not decoded per lookup).
    overridden: AtomicBool,
}

impl Default for Switch {
    fn default() -> Switch {
        Switch::new()
    }
}

fn store(buf: &Buf<TABLE>, c: &Config) {
    let mut out = [0u8; TABLE];
    for (chunk, db) in out.chunks_exact_mut(SLOT).zip(Database::ALL) {
        if let (Some(spec), Some((flag, rest))) = (c.line(db), chunk.split_first_mut()) {
            *flag = 1;
            rest.copy_from_slice(&spec.encode());
        }
    }
    buf.store(&out);
}

fn load(buf: &Buf<TABLE>, into: &mut Config) {
    let mut bytes = [0u8; TABLE];
    buf.load(&mut bytes);
    for (chunk, db) in bytes.chunks_exact(SLOT).zip(Database::ALL) {
        if let Some((1, rest)) = chunk.split_first() {
            into.set(db, Spec::decode(rest));
        }
    }
}

impl Switch {
    /// Nothing read, nothing overridden.
    pub const fn new() -> Switch {
        Switch { file: Watched::new(), lines: Buf::new(), overrides: Buf::new(), override_lock: RawMutex::new(), overridden: AtomicBool::new(false) }
    }

    /// The configuration now: the file `path` (re-read when it changed), then the overrides.
    pub fn config<S: Sys>(&self, sys: &S, path: &[u8]) -> Config {
        self.config_checked(sys, path, true)
    }

    /// The configuration as last read (read now if it never was), without checking the file
    /// (the name-service cache's account stamp, whose own record covers the file's identity).
    pub fn config_as_read<S: Sys>(&self, sys: &S, path: &[u8]) -> Config {
        self.config_checked(sys, path, false)
    }

    fn config_checked<S: Sys>(&self, sys: &S, path: &[u8], recheck: bool) -> Config {
        let mut c = self.file.with(
            sys,
            path,
            recheck,
            |text| store(&self.lines, &text.map(Config::parse).unwrap_or(Config::EMPTY)),
            || {
                let mut c = Config::EMPTY;
                load(&self.lines, &mut c);
                c
            },
        );
        if self.overridden.load(Acquire) {
            let _g = self.override_lock.lock(sys);
            load(&self.overrides, &mut c);
        }
        c
    }

    /// Whether `__nss_configure_lookup` was ever called (the name-service cache then stays out
    /// of every lookup, D-16.10: the overrides are this process's alone).
    pub fn overridden(&self) -> bool {
        self.overridden.load(Acquire)
    }

    /// `__nss_configure_lookup(db, text)`: `db`'s sources are `text` from now on. `EINVAL` for
    /// an unknown database or a source list that does not parse (D-16.1).
    pub fn configure<S: Sys>(&self, sys: &S, db: &[u8], text: &[u8]) -> Result<()> {
        let db = Database::from_name(db).ok_or(Errno::EINVAL)?;
        let spec = Spec::parse(text).ok_or(Errno::EINVAL)?;
        let _g = self.override_lock.lock(sys);
        let mut c = Config::EMPTY;
        load(&self.overrides, &mut c);
        c.set(db, Some(spec));
        store(&self.overrides, &c);
        self.overridden.store(true, Release);
        Ok(())
    }
}

/// The process's switch.
pub static SWITCH: Switch = Switch::new();

/// `host.conf`'s `multi` (off unless the file says `multi on`).
#[derive(Debug)]
pub struct HostConf {
    file: Watched,
    multi: AtomicBool,
}

impl Default for HostConf {
    fn default() -> HostConf {
        HostConf::new()
    }
}

/// `multi on` in a `host.conf` text (`host.conf(5)`; the last `multi` line counts; the other
/// keywords concern DNS answers and are part 4's).
pub fn multi_of(text: &[u8]) -> bool {
    let mut on = false;
    for line in text.split(|&c| c == b'\n') {
        let mut w = db::Words::of(line);
        if w.next() == Some(b"multi") {
            on = w.next().is_some_and(|v| v.eq_ignore_ascii_case(b"on"));
        }
    }
    on
}

impl HostConf {
    /// Never read.
    pub const fn new() -> HostConf {
        HostConf { file: Watched::new(), multi: AtomicBool::new(false) }
    }

    /// `multi` now (the file `path` re-read when it changed).
    pub fn multi<S: Sys>(&self, sys: &S, path: &[u8]) -> bool {
        self.file.with(sys, path, true, |t| self.multi.store(t.is_some_and(multi_of), Relaxed), || self.multi.load(Relaxed))
    }
}

/// The process's `host.conf`.
pub static HOST_CONF: HostConf = HostConf::new();

// ------------------------------------------------------------------------------ lookups

/// What one lookup needs: the kernel, the files, the configuration, `host.conf`'s `multi`, and
/// for host lookups the resolver of the `dns` source and the locale of IDNA texts (part 4).
#[derive(Clone, Copy, Debug)]
pub struct Ctx<'a, S: Sys> {
    /// The kernel.
    pub sys: &'a S,
    /// The files.
    pub paths: &'a Paths<'a>,
    /// The switch configuration.
    pub config: Config,
    /// `multi on`.
    pub multi: bool,
    /// The resolver of the `dns` source (`None`: the source is unavailable).
    pub dns: Option<&'a crate::resolv_compat::Resolver>,
    /// The locale whose character set `AI_IDN`, `AI_CANONIDN` and `NI_IDN` texts are in
    /// (`None`: ASCII only).
    pub locale: Option<&'a liella::locale::Locale>,
    /// The name-service cache's memory (D-16.10; `None`: the lookups are not cached). Only a
    /// context with the system's files uses it ([`Ctx::cached`]).
    pub cache: Option<&'a dyn crate::nscache::Memory>,
}

impl<'a, S: Sys> Ctx<'a, S> {
    /// The process's configuration with the files `paths` (`multi` off: only host lookups by
    /// name read `host.conf`, through [`Ctx::with_multi`]).
    pub fn with_paths(sys: &'a S, paths: &'a Paths<'a>) -> Ctx<'a, S> {
        Ctx { sys, paths, config: SWITCH.config(sys, paths.nsswitch), multi: false, dns: None, locale: None, cache: None }
    }

    /// This context with the name-service cache (D-16.10), which serves it only while it reads
    /// the system's files.
    pub fn cached(self, memory: &'a dyn crate::nscache::Memory) -> Ctx<'a, S> {
        Ctx { cache: Some(memory), ..self }
    }

    /// The process's configuration and the system's files.
    pub fn system(sys: &'a S) -> Ctx<'a, S> {
        Ctx::with_paths(sys, &SYSTEM)
    }

    /// This context with `host.conf`'s `multi` (the file re-read when it changed), for host
    /// lookups.
    pub fn with_multi(self) -> Ctx<'a, S> {
        Ctx { multi: HOST_CONF.multi(self.sys, self.paths.host_conf), ..self }
    }

    /// A context that reads no configuration file, for a call that consults no database
    /// (`getaddrinfo` and `getnameinfo` with numeric arguments: `crate::gai::uses_switch`,
    /// `crate::gai::ni_uses_switch`). Its configuration is empty, so it must not be used for a
    /// lookup.
    pub fn bare(sys: &'a S, paths: &'a Paths<'a>) -> Ctx<'a, S> {
        Ctx { sys, paths, config: Config::EMPTY, multi: false, dns: None, locale: None, cache: None }
    }

    /// The sources of `db`.
    pub fn spec(&self, db: Database) -> Spec {
        self.config.spec(db)
    }

    /// Whether `db` is enumerated (its line names the `files` source).
    pub fn enumerates(&self, db: Database) -> bool {
        self.spec(db).reads_files()
    }

    /// One lookup of `db` answered by the `files` source with the first line of `file` that
    /// `pred` accepts (a copy); every other source is unavailable (see the module
    /// documentation). `compat` is `files` for `passwd`, `group` and `shadow`.
    pub fn line(&self, db: Database, file: &[u8], mut pred: impl FnMut(&[u8]) -> bool) -> Answer<TryVec<u8>> {
        let compat = matches!(db, Database::Passwd | Database::Group | Database::Shadow);
        nss::run(
            &self.spec(db),
            |svc| match svc {
                Service::Files => files::find_line(self.sys, file, &mut pred),
                Service::Compat if compat => files::find_line(self.sys, file, &mut pred),
                _ => Err(LookupError::Unavailable),
            },
            |_, _| Ok(()),
        )
    }
}

/// A lookup's answer as the user-database functions report it (D-17.3): the entry, `None` for
/// none (not found, unavailable), or the error (a file that cannot be read, `EAGAIN` for a
/// temporary failure).
pub fn found<T>(a: Answer<T>) -> Result<Option<T>> {
    match a {
        Ok(v) => Ok(Some(v)),
        Err(LookupError::NotFound | LookupError::NoData | LookupError::Unavailable) => Ok(None),
        Err(LookupError::TryAgain) => Err(Errno::EAGAIN),
        Err(LookupError::Os(e)) => Err(e),
    }
}

/// A user or group key's kind and bytes for the name-service cache.
fn key_parts<'a>(key: Key<'a>, id: &'a mut [u8; 4]) -> (u8, &'a [u8]) {
    match key {
        Key::Name(n) => (1, n),
        Key::Id(i) => {
            *id = i.to_le_bytes();
            (2, id.as_slice())
        }
    }
}

/// The user record for `key` (`getpwnam`, `getpwuid`), given to `f`.
pub fn passwd<S: Sys, T>(ctx: &Ctx<'_, S>, key: Key<'_>, f: impl FnOnce(&Passwd<'_>) -> T) -> Answer<T> {
    let mut id = [0u8; 4];
    let (kind, part) = key_parts(key, &mut id);
    let line = crate::nscache::line(ctx, Category::Passwd, kind, &[part], || passwd_line(ctx, key))?;
    users::passwd(&line).map(|p| f(&p)).ok_or(LookupError::NotFound)
}

/// The sources' passwd line for `key`.
fn passwd_line<S: Sys>(ctx: &Ctx<'_, S>, key: Key<'_>) -> Answer<TryVec<u8>> {
    ctx.line(Database::Passwd, ctx.paths.passwd, |l| users::passwd(l).is_some_and(|p| files::passwd_is(&p, key)))
}

/// [`passwd`] with the process's configuration and the system's files, through the name-service
/// cache (D-16.10): the switch configuration is read only when the cache cannot answer.
pub fn passwd_cached<S: Sys, T>(sys: &S, memory: &dyn crate::nscache::Memory, key: Key<'_>, f: impl FnOnce(&Passwd<'_>) -> T) -> Answer<T> {
    let mut id = [0u8; 4];
    let (kind, part) = key_parts(key, &mut id);
    let line = crate::nscache::line_system(sys, memory, Category::Passwd, kind, &[part], || passwd_line(&Ctx::system(sys), key))?;
    users::passwd(&line).map(|p| f(&p)).ok_or(LookupError::NotFound)
}

/// The shadow record of `name` (`getspnam`).
pub fn shadow<S: Sys, T>(ctx: &Ctx<'_, S>, name: &[u8], f: impl FnOnce(&Shadow<'_>) -> T) -> Answer<T> {
    let line = ctx.line(Database::Shadow, ctx.paths.shadow, |l| users::shadow(l).is_some_and(|s| !users::is_compat(s.name) && s.name == name))?;
    users::shadow(&line).map(|s| f(&s)).ok_or(LookupError::NotFound)
}

/// The shadow group record of `name` (`getsgnam`).
pub fn gshadow<S: Sys, T>(ctx: &Ctx<'_, S>, name: &[u8], f: impl FnOnce(&Gshadow<'_>) -> T) -> Answer<T> {
    let line = ctx.line(Database::Gshadow, ctx.paths.gshadow, |l| users::gshadow(l).is_some_and(|g| !users::is_compat(g.name) && g.name == name))?;
    users::gshadow(&line).map(|g| f(&g)).ok_or(LookupError::NotFound)
}

/// Merge the group line `more` into `kept` (`[SUCCESS=merge]`): when both name the same group
/// (name and GID), `more`'s members are appended (repeats kept, as `nsswitch.conf(5)` says);
/// otherwise `kept` stays.
fn merge_group(kept: &mut TryVec<u8>, more: TryVec<u8>) -> Result<()> {
    let (Some(a), Some(b)) = (users::group(kept), users::group(&more)) else { return Ok(()) };
    if a.name != b.name || a.gid != b.gid {
        return Ok(());
    }
    let mut line = TryVec::new();
    line.extend_from_slice(a.name)?;
    line.push(b':')?;
    line.extend_from_slice(a.passwd)?;
    line.push(b':')?;
    let mut gid = liella::net::Text::<12>::new();
    gid.push_dec(u64::from(a.gid));
    line.extend_from_slice(gid.as_bytes())?;
    line.push(b':')?;
    for (i, m) in a.members.iter().chain(b.members.iter()).enumerate() {
        if i > 0 {
            line.push(b',')?;
        }
        line.extend_from_slice(m)?;
    }
    *kept = line;
    Ok(())
}

/// The group record for `key` (`getgrnam`, `getgrgid`), merged across sources as the rules say.
pub fn group<S: Sys, T>(ctx: &Ctx<'_, S>, key: Key<'_>, f: impl FnOnce(&Group<'_>) -> T) -> Answer<T> {
    let mut id = [0u8; 4];
    let (kind, part) = key_parts(key, &mut id);
    let line = crate::nscache::line(ctx, Category::Group, kind, &[part], || group_line(ctx, key))?;
    users::group(&line).map(|g| f(&g)).ok_or(LookupError::NotFound)
}

/// The sources' group line for `key`, merged as the rules say.
fn group_line<S: Sys>(ctx: &Ctx<'_, S>, key: Key<'_>) -> Answer<TryVec<u8>> {
    let pred = |l: &[u8]| users::group(l).is_some_and(|g| files::group_is(&g, key));
    let file = ctx.paths.group;
    nss::run(
        &ctx.spec(Database::Group),
        |svc| match svc {
            Service::Files | Service::Compat => files::find_line(ctx.sys, file, pred),
            _ => Err(LookupError::Unavailable),
        },
        merge_group,
    )
}

/// [`group`] with the process's configuration and the system's files, through the name-service
/// cache (D-16.10): the switch configuration is read only when the cache cannot answer.
pub fn group_cached<S: Sys, T>(sys: &S, memory: &dyn crate::nscache::Memory, key: Key<'_>, f: impl FnOnce(&Group<'_>) -> T) -> Answer<T> {
    let mut id = [0u8; 4];
    let (kind, part) = key_parts(key, &mut id);
    let line = crate::nscache::line_system(sys, memory, Category::Group, kind, &[part], || group_line(&Ctx::system(sys), key))?;
    users::group(&line).map(|g| f(&g)).ok_or(LookupError::NotFound)
}

/// The GIDs of `user`'s groups, `base` first, one per group (`getgrouplist`, `initgroups`): through
/// the `initgroups` database (or `group`'s sources when it has no line), each source that
/// succeeds adding its groups (D-16.1). A source that fails adds nothing. The groups the sources
/// list (without `base`) go through the name-service cache.
pub fn group_list<S: Sys>(ctx: &Ctx<'_, S>, user: &[u8], base: u32) -> Result<TryVec<u32>> {
    let found = crate::nscache::gid_list(ctx, user, || listed_groups(ctx, user))?;
    let mut out = TryVec::new();
    out.push(base)?;
    for &g in found.iter() {
        if g != base {
            out.push(g)?;
        }
    }
    Ok(out)
}

/// The GIDs the sources list for `user`, one per group, in their order.
fn listed_groups<S: Sys>(ctx: &Ctx<'_, S>, user: &[u8]) -> Result<TryVec<u32>> {
    let (spec, inherited) = ctx.config.resolve(Database::Initgroups);
    let mut out = TryVec::new();
    let mut failed = None;
    nss::gather(&spec, inherited, |svc| match svc {
        Service::Files | Service::Compat => {
            let text = files::read(ctx.sys, ctx.paths.group)?;
            match files::add_groups(&text, user, &mut out) {
                Ok(true) => Ok(()),
                Ok(false) => Err(LookupError::NotFound),
                Err(e) => {
                    failed = Some(e);
                    Err(LookupError::Os(e))
                }
            }
        }
        _ => Err(LookupError::Unavailable),
    });
    match failed {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

// ------------------------------------------------------------------------------ ethers (§15.7)

/// The Ethernet address of `name` (`ether_hostton`).
pub fn ether_by_name<S: Sys>(ctx: &Ctx<'_, S>, name: &[u8]) -> Answer<[u8; 6]> {
    nss::run(
        &ctx.spec(Database::Ethers),
        |svc| match svc {
            Service::Files => files::ether_name(ctx.sys, ctx.paths.ethers, name),
            _ => Err(LookupError::Unavailable),
        },
        |_, _| Ok(()),
    )
}

/// The host name of the Ethernet address `a` (`ether_ntohost`).
pub fn ether_by_addr<S: Sys>(ctx: &Ctx<'_, S>, a: [u8; 6]) -> Answer<TryVec<u8>> {
    nss::run(
        &ctx.spec(Database::Ethers),
        |svc| match svc {
            Service::Files => files::ether_addr(ctx.sys, ctx.paths.ethers, a),
            _ => Err(LookupError::Unavailable),
        },
        |_, _| Ok(()),
    )
}

// ------------------------------------------------------------------------------ netgroups (§16.5)

/// The triples of a netgroup and of the netgroups it names, expanded.
#[derive(Debug, Default)]
pub struct Triples {
    /// The fields' text.
    text: TryVec<u8>,
    /// Per triple, each field's offset and length in `text` (`None`: empty, any value).
    items: TryVec<[Option<(usize, usize)>; 3]>,
}

impl Triples {
    /// The number of triples.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there is none.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The `i`th triple.
    pub fn get(&self, i: usize) -> Option<[Option<&[u8]>; 3]> {
        let t = self.items.get(i)?;
        Some(t.map(|f| f.and_then(|(at, n)| self.text.get(at..at.checked_add(n)?))))
    }

    fn push(&mut self, t: [Option<&[u8]>; 3]) -> Result<()> {
        let mut item = [None; 3];
        for (slot, f) in item.iter_mut().zip(t) {
            if let Some(f) = f {
                *slot = Some((self.text.len(), f.len()));
                self.text.extend_from_slice(f)?;
            }
        }
        self.items.push(item)
    }
}

/// The triples of `group`: its own, then those of the netgroups it names, breadth first, each
/// group once (a loop ends). `NotFound` when `group` itself is unknown; a member group that is
/// unknown adds nothing.
pub fn netgroup<S: Sys>(ctx: &Ctx<'_, S>, group: &[u8]) -> Answer<Triples> {
    let mut out = Triples::default();
    let mut queue: TryVec<TryVec<u8>> = TryVec::new();
    queue.push(TryVec::from_slice(group)?)?;
    let mut next = 0usize;
    while let Some(name) = queue.get(next) {
        let name = TryVec::from_slice(name)?;
        let found = ctx.line(Database::Netgroup, ctx.paths.netgroup, |l| db::netgroup(l, &name).is_some());
        let line = match found {
            Ok(l) => l,
            Err(e) if next == 0 => return Err(e),
            Err(_) => {
                next = next.saturating_add(1);
                continue;
            }
        };
        for m in db::netgroup(&line, &name).into_iter().flatten() {
            match m {
                db::NetMember::Triple(t) => out.push(t)?,
                db::NetMember::Group(g) => {
                    if !queue.iter().any(|q| q.as_slice() == g) {
                        queue.push(TryVec::from_slice(g)?)?;
                    }
                }
            }
        }
        next = next.saturating_add(1);
    }
    Ok(out)
}

/// `innetgr(group, host, user, domain)`: whether a triple of `group` (expanded) matches; a
/// `None` argument matches any field (see `liella::names::db::field_matches`).
pub fn in_netgroup<S: Sys>(ctx: &Ctx<'_, S>, group: &[u8], host: Option<&[u8]>, user: Option<&[u8]>, domain: Option<&[u8]>) -> bool {
    let Ok(t) = netgroup(ctx, group) else { return false };
    (0..t.len()).filter_map(|i| t.get(i)).any(|[h, u, d]| db::field_matches(h, host, true) && db::field_matches(u, user, false) && db::field_matches(d, domain, true))
}

/// The process-wide netgroup enumeration (`setnetgrent`, `getnetgrent`, `endnetgrent`): the
/// group's name and the position (P-scope). Each step expands the group again (netgroup files
/// are small; nothing is kept between calls but the name).
pub struct NetgroupCursor {
    lock: RawMutex,
    len: AtomicU64,
    at: AtomicU64,
    name: Buf<1024>,
}

impl Default for NetgroupCursor {
    fn default() -> NetgroupCursor {
        NetgroupCursor::new()
    }
}

impl NetgroupCursor {
    /// No group.
    pub const fn new() -> NetgroupCursor {
        NetgroupCursor { lock: RawMutex::new(), len: AtomicU64::new(0), at: AtomicU64::new(0), name: Buf::new() }
    }

    /// `setnetgrent(group)`: whether the group is known (an empty one included). A name longer
    /// than 1023 bytes names no group this source knows.
    pub fn set<S: Sys>(&self, ctx: &Ctx<'_, S>, group: &[u8]) -> bool {
        let _g = self.lock.lock(ctx.sys);
        self.at.store(0, Relaxed);
        self.len.store(0, Relaxed);
        if group.is_empty() || group.len() > 1023 || netgroup(ctx, group).is_err() {
            return false;
        }
        self.name.store(group);
        self.len.store(u64::try_from(group.len()).unwrap_or(0), Relaxed);
        true
    }

    /// `endnetgrent()`.
    pub fn end<S: Sys>(&self, sys: &S) {
        let _g = self.lock.lock(sys);
        self.len.store(0, Relaxed);
        self.at.store(0, Relaxed);
    }

    /// The next triple, given to `f`, which may refuse it (`false`: no room, the position stays).
    /// `Ok(None)` at the end or without a group.
    pub fn next<S: Sys, T>(&self, ctx: &Ctx<'_, S>, f: impl FnOnce([Option<&[u8]>; 3]) -> Option<T>) -> Result<Option<Option<T>>> {
        let _g = self.lock.lock(ctx.sys);
        let n = usize::try_from(self.len.load(Relaxed)).unwrap_or(0);
        if n == 0 {
            return Ok(None);
        }
        let mut name = [0u8; 1024];
        let name = name.get_mut(..n).ok_or(Errno::EINVAL)?;
        self.name.load(name);
        let t = match netgroup(ctx, name) {
            Ok(t) => t,
            Err(LookupError::Os(e)) => return Err(e),
            Err(_) => return Ok(None),
        };
        let at = usize::try_from(self.at.load(Relaxed)).unwrap_or(usize::MAX);
        let Some(triple) = t.get(at) else { return Ok(None) };
        let r = f(triple);
        if r.is_some() {
            self.at.store(u64::try_from(at.saturating_add(1)).unwrap_or(u64::MAX), Relaxed);
        }
        Ok(Some(r))
    }
}

/// The process's netgroup enumeration.
pub static NETGROUP_CURSOR: NetgroupCursor = NetgroupCursor::new();

// ------------------------------------------------------------------------------ aliases (§16.6)

/// The `aliases` entry `name` (ASCII case ignored), given to `f`.
pub fn alias<S: Sys, T>(ctx: &Ctx<'_, S>, name: &[u8], f: impl FnOnce(&db::Alias<'_>) -> T) -> Answer<T> {
    let entry = nss::run(
        &ctx.spec(Database::Aliases),
        |svc| match svc {
            Service::Files => files::alias_entry(ctx.sys, ctx.paths.aliases, name),
            _ => Err(LookupError::Unavailable),
        },
        |_, _| Ok(()),
    )?;
    match db::alias_at(&entry, 0) {
        Some((Some(a), _)) => Ok(f(&a)),
        _ => Err(LookupError::NotFound),
    }
}
