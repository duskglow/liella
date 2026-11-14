//! Library search (plan/ph14/design.md §4, phase 14 DL-8, D-27.16; clause s27_1_c_1_1): the order
//! for a name without `/` (`DT_RPATH` of the requester and its loaders when the requester has no
//! `DT_RUNPATH`, `LD_LIBRARY_PATH`, the requester's `DT_RUNPATH`, the cache, the default
//! directories), the dynamic string tokens, the `glibc-hwcaps` subdirectories and Liella's pinned
//! sonames (R14-3 (a)), with the secure-mode rules of spec/security/threat-model.md "The dynamic
//! loader" (rules 1, 2, 3, 5 and 9; R14-15, R14-18).
//!
//! Pure logic without allocation: [`search`] hands each candidate path, in order, to a callback that
//! tries to open it and says whether it found a usable object. The boundary (`boundary/src/ld.rs`)
//! opens and maps; tests record the paths.

use crate::arch::Arch;
use crate::cache::Cache;

/// The compile-time installation prefix (R14-3 (a)): `LIELLA_PREFIX` when the build sets it, else `/`
/// (the sole-libc layout, where the cache is `/etc/ld.so.cache` and the system directories are the
/// default ones).
pub const PREFIX: &str = match option_env!("LIELLA_PREFIX") {
    Some(p) => p,
    None => "/",
};

/// The directory below the prefix that holds Liella's libraries (R14-3 (a)): `LIELLA_LIBDIR` when
/// the build sets it, else `lib`. The reference packages set `usr/lib/x86_64-linux-gnu`, the
/// multiarch directory of their sysroot (docs/install-packages.md, RELEASE-PACKAGING-DEB).
pub const LIBDIR: &str = match option_env!("LIELLA_LIBDIR") {
    Some(d) => d,
    None => "lib",
};

/// The longest path the loader builds (Linux `PATH_MAX`, NUL included); a longer candidate is skipped.
pub const PATH_MAX: usize = 4096;

/// Liella's own sonames besides the loader (R14-3 (a), threat model rule 9): libc, libm and the
/// placeholder libraries (DL-1). They resolve to `<prefix>/<libdir>` ([`LIBDIR`]) before any search.
pub const PINNED: &[&str] =
    &["libc.so.6", "libm.so.6", "libpthread.so.0", "libdl.so.2", "librt.so.1", "libutil.so.1", "libanl.so.1", "libc_malloc_debug.so.0"];

/// A path being built, NUL-terminated on request.
pub struct PathBuf {
    buf: [u8; PATH_MAX],
    len: usize,
}

impl Default for PathBuf {
    fn default() -> Self {
        PathBuf { buf: [0; PATH_MAX], len: 0 }
    }
}

impl PathBuf {
    /// Empty the path.
    pub fn clear(&mut self) {
        self.len = 0;
    }
    /// Append `part`; false (and the path unusable) when it would not fit with a NUL.
    pub fn push(&mut self, part: &[u8]) -> bool {
        let Some(end) = self.len.checked_add(part.len()).filter(|&e| e < PATH_MAX) else {
            self.len = PATH_MAX;
            return false;
        };
        let Some(dst) = self.buf.get_mut(self.len..end) else { return false };
        dst.copy_from_slice(part);
        self.len = end;
        true
    }
    /// The path without its NUL (empty when it overflowed).
    pub fn as_bytes(&self) -> &[u8] {
        if self.len >= PATH_MAX { &[] } else { self.buf.get(..self.len).unwrap_or(&[]) }
    }
    /// The path with a NUL after it, for a system call (`None` when it overflowed).
    pub fn c_str(&mut self) -> Option<&[u8]> {
        if self.len >= PATH_MAX {
            return None;
        }
        *self.buf.get_mut(self.len)? = 0;
        self.buf.get(..=self.len)
    }
}

/// `<prefix>/<sub>` into `out` (no doubled `/` when the prefix is `/`).
pub fn prefixed(sub: &[u8], out: &mut PathBuf) -> bool {
    out.clear();
    out.push(PREFIX.trim_end_matches('/').as_bytes()) && out.push(b"/") && out.push(sub)
}

/// `<prefix>/<libdir>` ([`LIBDIR`]) into `out`, then `/<name>` when `name` is not empty: the
/// directory of Liella's own libraries, or one of them.
pub fn own_lib(name: &[u8], out: &mut PathBuf) -> bool {
    prefixed(LIBDIR.trim_matches('/').as_bytes(), out) && (name.is_empty() || (out.push(b"/") && out.push(name)))
}

/// One object of the requester chain: its `DT_RPATH`, its `DT_RUNPATH` and its `$ORIGIN` (the
/// directory of its resolved path; `None` when unknown).
#[derive(Clone, Copy, Debug, Default)]
pub struct Scope<'a> {
    /// `DT_RPATH`.
    pub rpath: Option<&'a [u8]>,
    /// `DT_RUNPATH`.
    pub runpath: Option<&'a [u8]>,
    /// `$ORIGIN`.
    pub origin: Option<&'a [u8]>,
}

/// What the search may use besides the requester chain.
#[derive(Clone, Copy, Debug)]
pub struct Ctx<'a> {
    /// `AT_SECURE` (R14-18): no environment input, no relative or empty element, `$ORIGIN` only in
    /// a trusted directory.
    pub secure: bool,
    /// `LD_LIBRARY_PATH` (ignored in secure mode).
    pub library_path: Option<&'a [u8]>,
    /// `LD_ORIGIN_PATH`: `$ORIGIN` when an object's own is unknown (ignored in secure mode).
    pub origin_path: Option<&'a [u8]>,
    /// `$PLATFORM` ([`Arch::platform`]).
    pub platform: &'a str,
    /// The supported `glibc-hwcaps` subdirectories, highest first ([`Arch::hwcaps`]).
    pub hwcaps: &'a [&'a str],
    /// The cache, when one was read and parsed.
    pub cache: Option<&'a Cache<'a>>,
}

/// Whether `name` is one of Liella's pinned sonames.
pub fn pinned(name: &[u8]) -> bool {
    PINNED.iter().any(|p| p.as_bytes() == name)
}

/// Whether `dir` is a trusted directory (threat model rule 5): `<prefix>/<libdir>` and, when the prefix
/// is `/`, exactly the default directories [`Arch::SYSTEM_DIRS`] (owner #4237). A trailing `/` is
/// ignored.
pub fn trusted<A: Arch>(dir: &[u8]) -> bool {
    let dir = match dir {
        [rest @ .., b'/'] if !rest.is_empty() => rest,
        d => d,
    };
    let mut own = PathBuf::default();
    (own_lib(b"", &mut own) && own.as_bytes() == dir) || (PREFIX == "/" && A::SYSTEM_DIRS.iter().any(|t| t.as_bytes() == dir))
}

/// The tokens and the value each expands to, in the order they are matched.
fn token(rest: &[u8]) -> Option<(&'static [u8], &[u8])> {
    for name in [&b"ORIGIN"[..], b"PLATFORM", b"LIB"] {
        if let Some(after) = rest.strip_prefix(b"{").and_then(|r| r.strip_prefix(name)).and_then(|r| r.strip_prefix(b"}")) {
            return Some((name, after));
        }
        if let Some(after) = rest.strip_prefix(name) {
            if !after.first().is_some_and(|&c| c.is_ascii_alphanumeric() || c == b'_') {
                return Some((name, after));
            }
        }
    }
    None
}

/// Expand the dynamic string tokens of `elem` (`$ORIGIN`, `$LIB`, `$PLATFORM`, braced or not) into
/// `out`, for an object whose `$ORIGIN` is `origin`. `false` when the element must be skipped: an
/// unknown token, `$ORIGIN` unknown, or in secure mode `$ORIGIN` outside a trusted directory (threat
/// model rule 2) or a result that is empty or relative (rule 3). Outside secure mode an empty
/// result means the current directory.
pub fn expand<A: Arch>(elem: &[u8], origin: Option<&[u8]>, ctx: &Ctx<'_>, out: &mut PathBuf) -> bool {
    out.clear();
    let mut rest = elem;
    while let Some(at) = rest.iter().position(|&b| b == b'$') {
        if !out.push(rest.get(..at).unwrap_or(&[])) {
            return false;
        }
        let Some((name, after)) = token(rest.get(at.saturating_add(1)..).unwrap_or(&[])) else { return false };
        let value: &[u8] = match name {
            b"ORIGIN" => match (origin, ctx.secure) {
                (Some(o), true) if trusted::<A>(o) => o,
                (_, true) => return false,
                (Some(o), false) => o,
                (None, false) => match ctx.origin_path {
                    Some(o) => o,
                    None => return false,
                },
            },
            b"LIB" => A::LIB.as_bytes(),
            _ => ctx.platform.as_bytes(),
        };
        if !out.push(value) {
            return false;
        }
        rest = after;
    }
    if !out.push(rest) {
        return false;
    }
    match out.as_bytes().first() {
        Some(b'/') => true,
        _ if ctx.secure => false,
        None => out.push(b"."),
        Some(_) => true,
    }
}

/// Try `dir`'s supported `glibc-hwcaps` subdirectories, highest first, then `dir` itself.
fn in_dir(dir: &[u8], name: &[u8], ctx: &Ctx<'_>, path: &mut PathBuf, try_open: &mut dyn FnMut(&mut PathBuf) -> bool) -> bool {
    let dir = match dir {
        [rest @ .., b'/'] if !rest.is_empty() => rest,
        d => d,
    };
    for h in ctx.hwcaps {
        path.clear();
        if path.push(dir) && path.push(b"/glibc-hwcaps/") && path.push(h.as_bytes()) && path.push(b"/") && path.push(name) && try_open(path) {
            return true;
        }
    }
    path.clear();
    path.push(dir) && path.push(b"/") && path.push(name) && try_open(path)
}

/// Each element of the search list `list` (separated by any byte of `seps`), expanded for `origin`,
/// tried as a directory.
fn in_list<A: Arch>(list: &[u8], seps: &[u8], origin: Option<&[u8]>, name: &[u8], ctx: &Ctx<'_>, try_open: &mut dyn FnMut(&mut PathBuf) -> bool) -> bool {
    let mut dir = PathBuf::default();
    let mut path = PathBuf::default();
    for elem in list.split(|b| seps.contains(b)) {
        if expand::<A>(elem, origin, ctx, &mut dir) && in_dir(dir.as_bytes(), name, ctx, &mut path, try_open) {
            return true;
        }
    }
    false
}

/// Search for the object `name` needed by `chain[0]` (`chain[1..]`: the objects that loaded it, up
/// to the program), handing each candidate path to `try_open` until it returns true. Returns
/// whether an object was found. A name with `/` is tried as given (after token expansion); the
/// loader's own name ([`Arch::INTERP`]) is the caller's (the loader is already loaded).
pub fn search<A: Arch>(name: &[u8], chain: &[Scope<'_>], ctx: &Ctx<'_>, try_open: &mut dyn FnMut(&mut PathBuf) -> bool) -> bool {
    let me = chain.first().copied().unwrap_or_default();
    let program_origin = chain.last().and_then(|s| s.origin);
    let env_path = if ctx.secure { None } else { ctx.library_path };
    if name.contains(&b'/') {
        let mut path = PathBuf::default();
        return expand::<A>(name, me.origin, ctx, &mut path) && try_open(&mut path);
    }
    if name.contains(&b'$') {
        return false;
    }
    if pinned(name) {
        // R14-3 (a): LD_LIBRARY_PATH may override outside secure mode (testing a libc build); then
        // `<prefix>/<libdir>` only, never the cache, RPATH/RUNPATH or another default directory.
        if env_path.is_some_and(|p| in_list::<A>(p, b":;", program_origin, name, ctx, try_open)) {
            return true;
        }
        let mut path = PathBuf::default();
        return own_lib(name, &mut path) && try_open(&mut path);
    }
    if me.runpath.is_none() {
        for s in chain.iter().filter(|s| s.runpath.is_none()) {
            if s.rpath.is_some_and(|r| in_list::<A>(r, b":", s.origin, name, ctx, try_open)) {
                return true;
            }
        }
    }
    if env_path.is_some_and(|p| in_list::<A>(p, b":;", program_origin, name, ctx, try_open)) {
        return true;
    }
    if me.runpath.is_some_and(|r| in_list::<A>(r, b":", me.origin, name, ctx, try_open)) {
        return true;
    }
    let mut path = PathBuf::default();
    if let Some(found) = ctx.cache.and_then(|c| c.lookup(name, A::CACHE_FLAGS, ctx.hwcaps)) {
        path.clear();
        if path.push(found) && try_open(&mut path) {
            return true;
        }
    }
    // The default directories: the system's when the prefix is `/` (they include `/lib`), else
    // `<prefix>/<libdir>` only (R14-3 (a)).
    if PREFIX == "/" {
        return A::SYSTEM_DIRS.iter().any(|d| in_dir(d.as_bytes(), name, ctx, &mut path, try_open));
    }
    let mut dir = PathBuf::default();
    own_lib(b"", &mut dir) && in_dir(dir.as_bytes(), name, ctx, &mut path, try_open)
}
