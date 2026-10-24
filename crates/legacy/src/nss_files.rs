//! The `files` source (spec §16.1; D-16.1): every database read from its file, the file named by
//! a path parameter (NUL-terminated; [`crate::nss::SYSTEM`] names the system's files) so that the
//! tests read fixtures. The NSS engine (`crate::nss`) asks this source for the databases whose
//! `nsswitch.conf` line names `files` (or `compat` for `passwd`, `group` and `shadow`).
//!
//! A file that cannot be read is the source failing with that error (`unavail` for the rules,
//! the error reported to the caller: `ENOENT` when missing); a record that is not there is
//! `notfound`. The formats are `liella::names::db` (the netdb files), `liella::users` (the user
//! databases) and `liella::net::ether` (`ethers`).
//!
//! **User databases** (§17.3, §17.4). A lookup takes the first record that matches and skips
//! NIS compatibility entries (`+name`, `-name`: not users or groups of this source); the
//! enumerations (`crate::cursors`) return every line that parses, compatibility entries
//! included (they are lines of the file, as `fgetpwent` returns them).
//!
//! **Hosts.** A name or alias matches without regard to ASCII case. Without `multi on` in
//! `host.conf` the first matching line wins and lines are not merged (netdb.md); with it, every
//! matching line of the family asked for adds its addresses and names (the first line's name
//! is the official one), as `host.conf(5)` describes and the reference does (probe, D-16.2).

use liella::error::{Errno, Result};
use liella::mem::TryVec;
use liella::names::db;
use liella::names::nss::{Answer, LookupError};
use liella::names::{Family, Ip};
use liella::net::ether;
use liella::users::{self, Group, Gshadow, Passwd, Shadow};
use liella_kernel::sys::Sys;

/// The text of `file`, or the source's failure.
pub fn read<S: Sys>(sys: &S, file: &[u8]) -> Answer<TryVec<u8>> {
    liella::system::read_file(sys, file).map_err(LookupError::Os)
}

/// A copy of the first line of `file` that `pred` accepts (`db::lines`; reading stops there).
pub fn find_line<S: Sys>(sys: &S, file: &[u8], pred: impl FnMut(&[u8]) -> bool) -> Answer<TryVec<u8>> {
    liella::system::find_line(sys, file, pred).map_err(LookupError::Os)?.ok_or(LookupError::NotFound)
}

// ------------------------------------------------------------------------------ users and groups (§17.3, §17.4)

/// The `passwd` file of the system.
pub const PASSWD: &[u8] = b"/etc/passwd\0";
/// The `group` file.
pub const GROUP: &[u8] = b"/etc/group\0";
/// The `shadow` file.
pub const SHADOW: &[u8] = b"/etc/shadow\0";
/// The `gshadow` file.
pub const GSHADOW: &[u8] = b"/etc/gshadow\0";

/// What a user or group lookup searches for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key<'a> {
    /// A user or group name.
    Name(&'a [u8]),
    /// A UID or GID.
    Id(u32),
}

/// Whether the user record `p` is the one `key` asks for.
pub fn passwd_is(p: &Passwd<'_>, key: Key<'_>) -> bool {
    !users::is_compat(p.name)
        && match key {
            Key::Name(n) => p.name == n,
            Key::Id(i) => p.uid == i,
        }
}

/// Whether the group record `g` is the one `key` asks for.
pub fn group_is(g: &Group<'_>, key: Key<'_>) -> bool {
    !users::is_compat(g.name)
        && match key {
            Key::Name(n) => g.name == n,
            Key::Id(i) => g.gid == i,
        }
}

/// The user record for `key` in the passwd file `file`, given to `f` (`getpwnam`, `getpwuid`).
pub fn passwd_by<S: Sys, T>(sys: &S, file: &[u8], key: Key<'_>, f: impl FnOnce(&Passwd<'_>) -> T) -> Result<Option<T>> {
    let text = liella::system::read_file(sys, file)?;
    Ok(users::lines(&text).filter_map(users::passwd).find(|p| passwd_is(p, key)).map(|p| f(&p)))
}

/// The group record for `key` in the group file `file` (`getgrnam`, `getgrgid`).
pub fn group_by<S: Sys, T>(sys: &S, file: &[u8], key: Key<'_>, f: impl FnOnce(&Group<'_>) -> T) -> Result<Option<T>> {
    let text = liella::system::read_file(sys, file)?;
    Ok(users::lines(&text).filter_map(users::group).find(|g| group_is(g, key)).map(|g| f(&g)))
}

/// The shadow record of `name` in `file` (`getspnam`).
pub fn shadow_by<S: Sys, T>(sys: &S, file: &[u8], name: &[u8], f: impl FnOnce(&Shadow<'_>) -> T) -> Result<Option<T>> {
    let text = liella::system::read_file(sys, file)?;
    Ok(users::lines(&text).filter_map(users::shadow).find(|s| !users::is_compat(s.name) && s.name == name).map(|s| f(&s)))
}

/// The shadow group record of `name` in `file` (`getsgnam`).
pub fn gshadow_by<S: Sys, T>(sys: &S, file: &[u8], name: &[u8], f: impl FnOnce(&Gshadow<'_>) -> T) -> Result<Option<T>> {
    let text = liella::system::read_file(sys, file)?;
    Ok(users::lines(&text).filter_map(users::gshadow).find(|g| !users::is_compat(g.name) && g.name == name).map(|g| f(&g)))
}

/// Add to `out` the GIDs of the groups of `text` (a group file) that list `user`, one per group
/// (a GID two groups share is listed twice), skipping compatibility entries: whether any group
/// listed `user`.
pub fn add_groups(text: &[u8], user: &[u8], out: &mut TryVec<u32>) -> Result<bool> {
    let mut any = false;
    for g in users::lines(text).filter_map(users::group) {
        if !users::is_compat(g.name) && g.members.contains(user) {
            any = true;
            out.push(g.gid)?;
        }
    }
    Ok(any)
}

/// The GIDs of the groups `user` belongs to in the group file `file`, `base` first, never repeated
/// (`getgrouplist`, `initgroups` with the `files` source alone). A file that cannot be read
/// lists `base` alone.
pub fn group_list<S: Sys>(sys: &S, file: &[u8], user: &[u8], base: u32) -> Result<TryVec<u32>> {
    let text = liella::system::read_file(sys, file).unwrap_or_else(|_| TryVec::new());
    let mut out = TryVec::new();
    out.push(base)?;
    let mut found = TryVec::new();
    add_groups(&text, user, &mut found)?;
    for &g in found.iter() {
        if g != base {
            out.push(g)?;
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------------------ ethers (§15.7)

/// The `ethers` file of the system.
pub const ETHERS: &[u8] = b"/etc/ethers\0";

/// The address the `ethers` file `file` gives `name` (case-insensitive; the first entry wins).
pub fn ether_by_name<S: Sys>(sys: &S, file: &[u8], name: &[u8]) -> Option<[u8; 6]> {
    ether_name(sys, file, name).ok()
}

/// As [`ether_by_name`], with the source's outcome.
pub fn ether_name<S: Sys>(sys: &S, file: &[u8], name: &[u8]) -> Answer<[u8; 6]> {
    let text = read(sys, file)?;
    ether::entries(&text).find(|(_, h)| h.eq_ignore_ascii_case(name)).map(|(a, _)| a).ok_or(LookupError::NotFound)
}

/// The name the `ethers` file `file` gives `a` (the first entry wins).
pub fn ether_by_addr<S: Sys>(sys: &S, file: &[u8], a: [u8; 6]) -> Result<Option<TryVec<u8>>> {
    match ether_addr(sys, file, a) {
        Ok(n) => Ok(Some(n)),
        Err(LookupError::Os(e)) if e == Errno::ENOMEM => Err(e),
        Err(_) => Ok(None),
    }
}

/// As [`ether_by_addr`], with the source's outcome.
pub fn ether_addr<S: Sys>(sys: &S, file: &[u8], a: [u8; 6]) -> Answer<TryVec<u8>> {
    let text = read(sys, file)?;
    let (_, h) = ether::entries(&text).find(|(x, _)| *x == a).ok_or(LookupError::NotFound)?;
    Ok(TryVec::from_slice(h)?)
}

// ------------------------------------------------------------------------------ hosts (§16.3)

/// A host: its official name, its aliases and its addresses (of one family, or of both for an
/// `AF_UNSPEC` lookup with `multi on`).
#[derive(Debug)]
pub struct Host {
    /// The names, each followed by a NUL: the official name first.
    names: TryVec<u8>,
    /// The addresses, in order, without repeats.
    pub addrs: TryVec<Ip>,
}

impl Host {
    /// A host with the official name `name` and no address.
    pub fn new(name: &[u8]) -> Result<Host> {
        let mut h = Host { names: TryVec::new(), addrs: TryVec::new() };
        h.add_name(name)?;
        Ok(h)
    }

    /// The official name.
    pub fn name(&self) -> &[u8] {
        self.names.split(|&c| c == 0).next().unwrap_or(&[])
    }

    /// The aliases.
    pub fn aliases(&self) -> impl Iterator<Item = &[u8]> + Clone {
        let all = self.names.strip_suffix(b"\0").unwrap_or(&self.names);
        all.split(|&c| c == 0).skip(1)
    }

    /// Add an alias (or, first, the official name) unless the host has it already.
    pub fn add_name(&mut self, name: &[u8]) -> Result<()> {
        if !self.names.is_empty() && self.names.split(|&c| c == 0).any(|n| n == name) {
            return Ok(());
        }
        self.names.extend_from_slice(name)?;
        self.names.push(0)
    }

    /// Add an address unless the host has it already.
    pub fn add_addr(&mut self, a: Ip) -> Result<()> {
        if self.addrs.contains(&a) { Ok(()) } else { self.addrs.push(a) }
    }

    /// The family of the first address (the family of a host from one family's lookup).
    pub fn family(&self) -> Option<Family> {
        self.addrs.first().map(|a| a.family())
    }

    /// The host as bytes for the name-service cache (D-16.10): the names' length (4 bytes,
    /// little-endian) and bytes, then each address as its length (4 or 16) and bytes.
    pub fn encode(&self, out: &mut TryVec<u8>) -> Result<()> {
        out.extend_from_slice(&u32::try_from(self.names.len()).map_err(|_| Errno::EOVERFLOW)?.to_le_bytes())?;
        out.extend_from_slice(&self.names)?;
        for a in self.addrs.iter() {
            out.push(u8::try_from(a.bytes().len()).unwrap_or(0))?;
            out.extend_from_slice(a.bytes())?;
        }
        Ok(())
    }

    /// The host [`Host::encode`] gave `bytes` (`None`: not such bytes).
    pub fn decode(bytes: &[u8]) -> Option<Host> {
        let n = usize::try_from(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?)).ok()?;
        let names = bytes.get(4..n.checked_add(4)?)?;
        if names.last() != Some(&0) {
            return None;
        }
        let mut h = Host { names: TryVec::from_slice(names).ok()?, addrs: TryVec::new() };
        let mut rest = bytes.get(n.checked_add(4)?..)?;
        while let Some((&len, tail)) = rest.split_first() {
            let ip = match len {
                4 => Ip::V4(tail.get(..4)?.try_into().ok()?),
                16 => Ip::V6(tail.get(..16)?.try_into().ok()?),
                _ => return None,
            };
            h.addrs.push(ip).ok()?;
            rest = tail.get(usize::from(len)..)?;
        }
        Some(h)
    }
}

/// Add the names of a `hosts` line to `h`.
fn add_line(h: &mut Host, l: &db::HostLine<'_>) -> Result<()> {
    if !l.name.is_empty() {
        h.add_name(l.name)?;
    }
    for a in l.aliases.clone() {
        h.add_name(a)?;
    }
    h.add_addr(l.addr)
}

/// The host named `name` in the hosts text `text`, of `family`.
pub fn host_in(text: &[u8], name: &[u8], family: Family, multi: bool) -> Answer<Host> {
    let mut found: Option<Host> = None;
    for l in db::lines(text).filter_map(db::host_line) {
        if !family.admits(&l.addr) || !l.names(name) {
            continue;
        }
        match &mut found {
            None => {
                let mut h = Host::new(l.name)?;
                add_line(&mut h, &l)?;
                found = Some(h);
                if !multi {
                    break;
                }
            }
            Some(h) => add_line(h, &l)?,
        }
    }
    found.ok_or(LookupError::NotFound)
}

/// The host whose address is `a` in the hosts text `text` (the first line wins).
pub fn host_at(text: &[u8], a: Ip) -> Answer<Host> {
    let l = db::lines(text).filter_map(db::host_line).find(|l| l.addr == a).ok_or(LookupError::NotFound)?;
    let mut h = Host::new(l.name)?;
    add_line(&mut h, &l)?;
    Ok(h)
}

/// The host named `name` in the hosts file `file`.
pub fn host_by_name<S: Sys>(sys: &S, file: &[u8], name: &[u8], family: Family, multi: bool) -> Answer<Host> {
    host_in(&read(sys, file)?, name, family, multi)
}

/// The host whose address is `a` in the hosts file `file`.
pub fn host_by_addr<S: Sys>(sys: &S, file: &[u8], a: Ip) -> Answer<Host> {
    host_at(&read(sys, file)?, a)
}

// ------------------------------------------------------------------------------ the other netdb files

/// A copy of the `aliases` entry of `file` named `name` (ASCII case ignored): its text.
pub fn alias_entry<S: Sys>(sys: &S, file: &[u8], name: &[u8]) -> Answer<TryVec<u8>> {
    let text = read(sys, file)?;
    let mut at = 0usize;
    while let Some((e, next)) = db::alias_at(&text, at) {
        if e.is_some_and(|e| e.name.eq_ignore_ascii_case(name)) {
            return Ok(TryVec::from_slice(text.get(at..next).unwrap_or(&[]))?);
        }
        at = next;
    }
    Err(LookupError::NotFound)
}
