//! NUL-terminated strings for the kernel (spec §0.4 Path, H1, ⚑ KERN).
//!
//! The kernel reads paths, argument strings, extended-attribute names and similar values as
//! NUL-terminated byte strings. This is the one place in the library where that
//! representation exists; everywhere else text has an explicit length (decision D-H1).

use crate::errno::{Errno, KResult};

/// `PATH_MAX`: the kernel's limit for one path argument, including the terminator (⚑ KERN).
pub const PATH_MAX: usize = 4096;
/// `NAME_MAX`: the kernel's limit for one path component (⚑ KERN).
pub const NAME_MAX: usize = 255;

/// A borrowed byte string that ends with exactly one NUL and contains no other NUL.
///
/// Two forms. A measured one views a slice. An unmeasured one (`from_user`) holds only the
/// address of C text the caller passed and a function that measures it on demand: a call that
/// only forwards the path to the kernel (`as_ptr`) never reads the text in user space, so an
/// unreadable pointer reaches the kernel and gets its `EFAULT`, as with the reference library.
/// `with_nul`, `bytes` and `len` measure (and read) the text.
#[derive(Clone, Copy)]
pub struct KStr<'a>(Repr<'a>);

#[derive(Clone, Copy)]
enum Repr<'a> {
    Slice(&'a [u8]),
    /// Address of the first byte, and the measuring function (the boundary's, which owns the
    /// contract that the address is a C string for `'a`).
    User(*const u8, fn(*const u8) -> &'a [u8]),
}

impl PartialEq for KStr<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.with_nul() == other.with_nul()
    }
}

impl Eq for KStr<'_> {}

impl core::fmt::Debug for KStr<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Repr::Slice(b) => f.debug_tuple("KStr").field(&b).finish(),
            Repr::User(p, _) => f.debug_tuple("KStr").field(&p).finish(),
        }
    }
}

impl<'a> KStr<'a> {
    /// View `bytes`, which must end with its only NUL; otherwise `EINVAL`.
    ///
    /// The search for an earlier NUL is `<[u8]>::contains`, which core runs a word at a time:
    /// a byte-by-byte `position` cost about 5 instructions per byte, 10k for one 2 KiB
    /// `mkstemp` template, on every attempt (MKSTEMP-LONG), and execvp builds one per PATH
    /// element (EXECVP-PATH).
    pub fn from_nul_terminated(bytes: &'a [u8]) -> KResult<KStr<'a>> {
        match bytes.split_last() {
            Some((&0, rest)) if !rest.contains(&0) => Ok(KStr(Repr::Slice(bytes))),
            _ => Err(Errno::EINVAL),
        }
    }

    /// View `bytes`, which must end with a NUL, when the caller has already ensured the rest
    /// holds none (it was built from parts known to be NUL-free); otherwise `EINVAL`. Only the
    /// last byte is checked, so a caller that extends one name a component at a time and looks
    /// each prefix up pays O(1) per lookup instead of a scan of the whole name (REALPATH-DEPTH:
    /// realpath in deep trees). A NUL the caller let through is not a memory-safety problem
    /// (the kernel stops at it); debug builds check for one.
    pub fn from_nul_free_terminated(bytes: &'a [u8]) -> KResult<KStr<'a>> {
        match bytes.split_last() {
            Some((0, body)) => {
                debug_assert!(!body.contains(&0), "from_nul_free_terminated: interior NUL");
                Ok(KStr(Repr::Slice(bytes)))
            }
            _ => Err(Errno::EINVAL),
        }
    }

    /// C text at `p` that has not been read: `measure(p)` returns its bytes with the
    /// terminator, and is called only when `with_nul`, `bytes` or `len` is. Only the boundary
    /// builds these (from a C pointer, with its own measuring function); `p` is never
    /// dereferenced here.
    pub fn from_user(p: *const u8, measure: fn(*const u8) -> &'a [u8]) -> KStr<'a> {
        KStr(Repr::User(p, measure))
    }

    /// The bytes including the terminator.
    pub fn with_nul(self) -> &'a [u8] {
        match self.0 {
            Repr::Slice(b) => b,
            Repr::User(p, measure) => measure(p),
        }
    }

    /// The bytes without the terminator.
    pub fn bytes(self) -> &'a [u8] {
        let all = self.with_nul();
        match all.split_last() {
            Some((_, rest)) => rest,
            None => all,
        }
    }

    /// Length without the terminator.
    pub fn len(self) -> usize {
        self.bytes().len()
    }

    /// True for the empty string.
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// The address of the first byte, for handing to the kernel. Deriving an address from a
    /// reference is safe; only the boundary passes it on. Reads nothing.
    pub fn as_ptr(self) -> *const u8 {
        match self.0 {
            Repr::Slice(b) => b.as_ptr(),
            Repr::User(p, _) => p,
        }
    }
}

/// A NUL-terminated byte string of at most `N - 1` bytes, held inline.
#[derive(Clone)]
pub struct KStrBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
}

/// A path: at most `PATH_MAX - 1` bytes plus the terminator.
pub type KPathBuf = KStrBuf<PATH_MAX>;

impl<const N: usize> KStrBuf<N> {
    /// Copy `text` and terminate it.
    ///
    /// * An interior NUL is `EINVAL`: the kernel would silently stop at it (⚑ KERN).
    /// * `N - 1` bytes or more is `ENAMETOOLONG`, the kernel's own answer for paths (⚑ KERN).
    pub fn new(text: &[u8]) -> KResult<Self> {
        if text.contains(&0) {
            return Err(Errno::EINVAL);
        }
        if text.len() >= N {
            return Err(Errno::ENAMETOOLONG);
        }
        let mut buf = [0u8; N];
        if let Some(dst) = buf.get_mut(..text.len()) {
            dst.copy_from_slice(text);
        }
        Ok(KStrBuf { buf, len: text.len() })
    }

    /// The terminated view.
    pub fn as_kstr(&self) -> KStr<'_> {
        KStr(Repr::Slice(self.buf.get(..=self.len).unwrap_or(&[0])))
    }
}

impl<const N: usize> core::fmt::Debug for KStrBuf<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("KStrBuf").field(&self.as_kstr()).finish()
    }
}
