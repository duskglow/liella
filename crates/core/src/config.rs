//! Runtime configuration (spec §1.8; D-1.8) and the internal line reader for configuration
//! files (IP-21).
//!
//! **Tunables** are parsed once at startup from `GLIBC_TUNABLES` (`name=value:name=value`) into
//! a typed [`Tunables`] value. Names are `liella.*`; the `glibc.*` names that mean something in
//! this design are accepted as aliases. Unknown names and invalid values are ignored, as the
//! contract requires. In secure mode no tunable is honoured (the environment variable is also
//! removed, `rt::start::UNSECURE`; `spec/security/threat-model.md`, the `GLIBC_TUNABLES` row).
//! The full policy (honoured, ignored, refused) is `docs/tunables.md` (D-1.8, R15-6).
//!
//! The tunables that apply so far select the C layer's machine paths (P7, D-4.14):
//!
//! | Name | Alias | Value (range) |
//! |---|---|---|
//! | `liella.cpu.hwcaps` | `glibc.cpu.hwcaps` | comma-separated `-AVX2`, `-ERMS` (disable) or `AVX2`, `ERMS` |
//! | `liella.cpu.rep_movsb_threshold` | `glibc.cpu.x86_rep_movsb_threshold` | bytes, 0x100 or more |
//! | `liella.cpu.rep_stosb_threshold` | `glibc.cpu.x86_rep_stosb_threshold` | bytes, 1 or more |
//! | `liella.pthread.rseq` | `glibc.pthread.rseq` | 0 or 1; `0` disables the rseq registration (D-22.7, §22.9) |
//! | `liella.compat.rpc_reply_any_source` | (none) | 0 or 1; `1` lets the UDP RPC client take a reply from any source address (COMPAT-12, D-29.2) |
//! | `liella.malloc.perturb` | `glibc.malloc.perturb` | 0 to 255: `M_PERTURB` for the C `malloc` family (§3.2); overrides `MALLOC_PERTURB_` ([`Tunables::malloc_perturb`]) |
//!
//! **Syntax** (black-box probes of glibc 2.39, LEGACY-TUNABLES-POLICY, 2026-10-23):
//! * entries are `:`-separated, and a value runs from the first `=` to the next `:`. An entry
//!   without `=` is skipped, except the last one: when the text after the last `:` has no `=`
//!   (`a=1:`, `a=1:b`, `:`, `foo`), or when any entry has a second `=` (`a=b=c`, `p==5`,
//!   `foo=1=:p=7`), the whole string is malformed and no entry is honoured. An empty string is
//!   no entry.
//! * only spaces and tabs are leading blanks (form feed, vertical tab, CR and newline make the
//!   value malformed); a sign may precede `0x` (`+0x1a` is 26, `0x+1a` is malformed).
//! * diagnostics: [`warnings`] (glibc's `WARNING: ld.so: invalid GLIBC_TUNABLES ...` lines).
//! * a number is: spaces or tabs, an optional `+` or `-`, then `0x`/`0X` and hexadecimal
//!   digits, or `0` and octal digits, or decimal digits; the digit run may be empty (`=`, `=+`,
//!   `=0x` all read 0). Anything after the digits (`5x`, `7 `, `09`, `0xg`) or a value above
//!   2^64 - 1 makes the value malformed. `-0` is 0; any other negative value is out of range
//!   (glibc ignored `perturb=-1` and `x86_rep_movsb_threshold=-1` without a warning).
//! * a well-formed number outside the tunable's range is ignored, as is a malformed value; a
//!   later valid setting of a tunable replaces an earlier one.

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // slices are cut at positions found in the same slice

/// The parsed tunables. `None` means "not set": the built-in choice applies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tunables {
    /// `Some(false)` forbids the AVX2 scanners; `Some(true)` asks for them (honoured only if the
    /// CPU has AVX2).
    pub avx2: Option<bool>,
    /// The same for `rep movsb`/`rep stosb`.
    pub erms: Option<bool>,
    /// Bytes from which copies use `rep movsb`.
    pub rep_movsb_threshold: Option<usize>,
    /// Bytes from which fills use `rep stosb`.
    pub rep_stosb_threshold: Option<usize>,
    /// `Some(false)`: threads register no rseq area (§22.9).
    pub rseq: Option<bool>,
    /// COMPAT-12 (D-29.2): `Some(true)` makes the UDP RPC client take a reply with the
    /// call's xid from any source address, as the reference library does.
    pub rpc_reply_any_source: Option<bool>,
}

/// Which tunable a name denotes (its `liella.*` name or the `glibc.*` alias).
#[derive(Clone, Copy)]
enum Key {
    Hwcaps,
    MovsbThreshold,
    StosbThreshold,
    Rseq,
    Perturb,
    RpcReplyAnySource,
}

/// The tunable `name` stands for, or `None` for names that mean nothing in this design.
fn key_of(name: &[u8]) -> Option<Key> {
    Some(match name {
        b"liella.cpu.hwcaps" | b"glibc.cpu.hwcaps" => Key::Hwcaps,
        b"liella.cpu.rep_movsb_threshold" | b"glibc.cpu.x86_rep_movsb_threshold" => Key::MovsbThreshold,
        b"liella.cpu.rep_stosb_threshold" | b"glibc.cpu.x86_rep_stosb_threshold" => Key::StosbThreshold,
        b"liella.pthread.rseq" | b"glibc.pthread.rseq" => Key::Rseq,
        b"liella.malloc.perturb" | b"glibc.malloc.perturb" => Key::Perturb,
        b"liella.compat.rpc_reply_any_source" => Key::RpcReplyAnySource,
        _ => return None,
    })
}

/// A numeric value (module docs, "Syntax"): `None` when it is malformed or negative. The digit
/// run may be empty (0).
fn number(v: &[u8]) -> Option<u64> {
    // A negative value other than -0 is outside every tunable's range: ignored like a
    // malformed one, but without a warning ([`warnings`]).
    signed_number(v).and_then(|(negative, n)| (!negative || n == 0).then_some(n))
}

/// A well-formed number as its sign (`true` for `-`) and magnitude, or `None` when malformed
/// (module docs, "Syntax"; the malformed values are the ones glibc warns about).
fn signed_number(v: &[u8]) -> Option<(bool, u64)> {
    let start = v.iter().position(|&b| b != b' ' && b != b'\t').unwrap_or(v.len());
    let v = &v[start..];
    let (negative, v) = match v {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        _ => (false, v),
    };
    let (digits, radix) = match v {
        [b'0', b'x' | b'X', rest @ ..] => (rest, 16u32),
        [b'0', rest @ ..] => (rest, 8),
        _ => (v, 10),
    };
    let n = digits.iter().try_fold(0u64, |acc, &c| acc.checked_mul(u64::from(radix))?.checked_add(u64::from(char::from(c).to_digit(radix)?)))?;
    Some((negative, n))
}

/// The type and range of a tunable glibc 2.39 knows, as `ld.so --list-tunables` lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunableType {
    /// A number listed in decimal (a 32-bit integer).
    Int {
        /// The listed minimum.
        min: u64,
        /// The listed maximum.
        max: u64,
    },
    /// A number listed in hexadecimal (a size or 64-bit count).
    Size {
        /// The listed minimum.
        min: u64,
        /// The listed maximum.
        max: u64,
    },
    /// A string: never malformed.
    Str,
}

const I32: u64 = 0x7fff_ffff;
const fn int(min: u64, max: u64) -> TunableType {
    TunableType::Int { min, max }
}
const fn size(min: u64) -> TunableType {
    TunableType::Size { min, max: u64::MAX }
}

/// The 41 tunables of glibc 2.39 on x86_64 (`ld.so --list-tunables`, probed on keke,
/// LEGACY-TUNABLES-POLICY 2026-10-23). A malformed value of any numeric one draws a warning
/// ([`warnings`]); the dynamic loader of phase 14 reuses this table and the text.
pub const GLIBC_TUNABLE_TYPES: [(&[u8], TunableType); 41] = [
    (b"glibc.cpu.hwcap_mask", size(0)),
    (b"glibc.cpu.hwcaps", TunableType::Str),
    (b"glibc.cpu.plt_rewrite", int(0, 2)),
    (b"glibc.cpu.prefer_map_32bit_exec", int(0, 1)),
    (b"glibc.cpu.x86_data_cache_size", size(0)),
    (b"glibc.cpu.x86_ibt", TunableType::Str),
    (b"glibc.cpu.x86_non_temporal_threshold", TunableType::Size { min: 0x4040, max: 0x0fff_ffff_ffff_ffff }),
    (b"glibc.cpu.x86_rep_movsb_threshold", size(0x100)),
    (b"glibc.cpu.x86_rep_stosb_threshold", size(1)),
    (b"glibc.cpu.x86_shared_cache_size", size(0)),
    (b"glibc.cpu.x86_shstk", TunableType::Str),
    (b"glibc.elision.enable", int(0, 1)),
    (b"glibc.elision.skip_lock_after_retries", int(0, I32)),
    (b"glibc.elision.skip_lock_busy", int(0, I32)),
    (b"glibc.elision.skip_lock_internal_abort", int(0, I32)),
    (b"glibc.elision.skip_trylock_internal_abort", int(0, I32)),
    (b"glibc.elision.tries", int(0, I32)),
    (b"glibc.gmon.maxarcs", int(50, I32)),
    (b"glibc.gmon.minarcs", int(50, I32)),
    (b"glibc.malloc.arena_max", size(1)),
    (b"glibc.malloc.arena_test", size(1)),
    (b"glibc.malloc.check", int(0, 3)),
    (b"glibc.malloc.hugetlb", size(0)),
    (b"glibc.malloc.mmap_max", int(0, I32)),
    (b"glibc.malloc.mmap_threshold", size(0)),
    (b"glibc.malloc.mxfast", size(0)),
    (b"glibc.malloc.perturb", int(0, 255)),
    (b"glibc.malloc.tcache_count", size(0)),
    (b"glibc.malloc.tcache_max", size(0)),
    (b"glibc.malloc.tcache_unsorted_limit", size(0)),
    (b"glibc.malloc.top_pad", size(0)),
    (b"glibc.malloc.trim_threshold", size(0)),
    (b"glibc.mem.decorate_maps", int(0, 1)),
    (b"glibc.mem.tagging", int(0, 255)),
    (b"glibc.pthread.mutex_spin_count", int(0, 32767)),
    (b"glibc.pthread.rseq", int(0, 1)),
    (b"glibc.pthread.stack_cache_size", size(0)),
    (b"glibc.pthread.stack_hugetlb", int(0, 1)),
    (b"glibc.rtld.dynamic_sort", int(1, 2)),
    (b"glibc.rtld.nns", TunableType::Size { min: 1, max: 0x10 }),
    (b"glibc.rtld.optional_static_tls", size(0)),
];

/// The type of the glibc tunable `name` (exact bytes), or `None` for a name glibc does not know.
pub fn glibc_tunable_type(name: &[u8]) -> Option<TunableType> {
    if !name.starts_with(b"glibc.") {
        return None; // most names: rejected without a table walk
    }
    GLIBC_TUNABLE_TYPES.iter().find(|(n, _)| *n == name).map(|&(_, t)| t)
}

/// Whether a `GLIBC_TUNABLES` string is well formed (module docs, "Syntax"): the text after the
/// last `:` has an `=`, and no entry has a second `=`. The empty string is well formed.
fn well_formed(text: &[u8]) -> bool {
    let last = text.rsplit(|&b| b == b':').next().unwrap_or(&[]);
    text.is_empty() || (last.contains(&b'=') && text.split(|&b| b == b':').all(|e| e.iter().filter(|&&b| b == b'=').count() <= 1))
}

/// The diagnostics glibc 2.39 prints on standard error for `GLIBC_TUNABLES=text` (probe-settled,
/// TUNABLES-WARNING 2026-10-23): `emit` receives each line as pieces to write in order (no
/// allocation). A malformed string gives one line naming it whole and nothing else; otherwise
/// each entry naming a numeric glibc tunable with a malformed value gives one line, in order,
/// repeats included. Unknown names, string tunables and out-of-range numbers give none. The
/// caller decides when they apply (not in secure mode, not when a loader already printed them).
pub fn warnings(text: &[u8], mut emit: impl FnMut(&[&[u8]])) {
    const HEAD: &[u8] = b"WARNING: ld.so: invalid GLIBC_TUNABLES ";
    if !well_formed(text) {
        emit(&[HEAD, b"`", text, b"': ignored.\n"]);
        return;
    }
    for e in text.split(|&b| b == b':') {
        let Some(eq) = e.iter().position(|&b| b == b'=') else { continue };
        let (name, value) = (&e[..eq], &e[eq + 1..]);
        let numeric = matches!(glibc_tunable_type(name), Some(TunableType::Int { .. } | TunableType::Size { .. }));
        if numeric && signed_number(value).is_none() {
            emit(&[HEAD, b"value `", value, b"' for option `", name, b"': ignored.\n"]);
        }
    }
}

/// A numeric value inside `min..=max`, or `None` (malformed or out of range: both ignored).
fn ranged(v: &[u8], min: u64, max: u64) -> Option<u64> {
    number(v).filter(|n| (min..=max).contains(n))
}

/// The `name=value` entries of a `GLIBC_TUNABLES` string that name a tunable of this design
/// (`:`-separated). An entry without `=` is skipped; a malformed string yields no entry
/// (module docs, "Syntax").
fn entries(text: &[u8]) -> impl Iterator<Item = (Key, &[u8])> {
    let whole = well_formed(text);
    text.split(|&b| b == b':').filter(move |_| whole).filter_map(|e| {
        let eq = e.iter().position(|&b| b == b'=')?;
        Some((key_of(&e[..eq])?, &e[eq + 1..]))
    })
}

impl Tunables {
    /// Parse `GLIBC_TUNABLES`. In secure mode nothing is honoured. A later valid setting of a
    /// tunable replaces an earlier one; an invalid one leaves it alone.
    pub fn parse(text: &[u8], secure: bool) -> Tunables {
        let mut t = Tunables::default();
        if secure {
            return t;
        }
        for (key, value) in entries(text) {
            match key {
                Key::Hwcaps => {
                    for item in value.split(|&b| b == b',') {
                        let (on, name) = match item {
                            [b'-', rest @ ..] => (false, rest),
                            _ => (true, item),
                        };
                        // `_Usable` is the spelling older releases used for the same switch.
                        let name = match name.len().checked_sub(7) {
                            Some(cut) if name[cut..].eq_ignore_ascii_case(b"_Usable") => &name[..cut],
                            _ => name,
                        };
                        if name.eq_ignore_ascii_case(b"AVX2") {
                            t.avx2 = Some(on);
                        } else if name.eq_ignore_ascii_case(b"ERMS") {
                            t.erms = Some(on);
                        }
                    }
                }
                Key::MovsbThreshold => t.rep_movsb_threshold = threshold(value, 0x100).or(t.rep_movsb_threshold),
                Key::StosbThreshold => t.rep_stosb_threshold = threshold(value, 1).or(t.rep_stosb_threshold),
                Key::Rseq => t.rseq = ranged(value, 0, 1).map(|n| n != 0).or(t.rseq),
                Key::Perturb => {} // read on its own by `malloc_perturb`
                Key::RpcReplyAnySource => t.rpc_reply_any_source = ranged(value, 0, 1).map(|n| n != 0).or(t.rpc_reply_any_source),
            }
        }
        t
    }

    /// The tunable `liella.malloc.perturb` (alias `glibc.malloc.perturb`) in `GLIBC_TUNABLES`:
    /// the C `malloc` family's perturbation byte (`M_PERTURB`, §3.2), 0 to 255; other values
    /// are ignored, the last valid setting counts. In secure mode nothing is honoured.
    pub fn malloc_perturb(text: &[u8], secure: bool) -> Option<u8> {
        if secure {
            return None;
        }
        entries(text).filter(|(k, _)| matches!(k, Key::Perturb)).filter_map(|(_, v)| u8::try_from(ranged(v, 0, 255)?).ok()).last()
    }
}

/// A byte-count threshold of at least `min` (glibc's listed minimum for the tunable).
fn threshold(v: &[u8], min: u64) -> Option<usize> {
    usize::try_from(ranged(v, min, u64::MAX)?).ok()
}

/// Blanks inside configuration lines.
fn is_blank(b: &u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r')
}

/// The meaningful lines of a configuration file (IP-21): each line without its newline, with
/// a `#` comment and surrounding blanks (space, tab, CR) removed; blank results are skipped.
pub fn lines(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    text.split(|&b| b == b'\n').map(|l| trim(l.split(|&b| b == b'#').next().unwrap_or(l))).filter(|l| !l.is_empty())
}

/// `s` without leading and trailing blanks (space, tab, CR).
pub fn trim(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|b| !is_blank(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !is_blank(b)).map_or(start, |i| i + 1);
    &s[start..end]
}

/// The blank-separated fields of a line.
pub fn fields(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    line.split(is_blank).filter(|f| !f.is_empty())
}

#[cfg(test)]
mod tests {
    use super::Tunables;

    #[test]
    fn rpc_reply_any_source() {
        // Trace: no spec clause (COMPAT-12, D-29.2: honoured only outside a secure process).
        assert_eq!(Tunables::parse(b"liella.compat.rpc_reply_any_source=1", false).rpc_reply_any_source, Some(true));
        assert_eq!(Tunables::parse(b"liella.compat.rpc_reply_any_source=0", false).rpc_reply_any_source, Some(false));
        assert_eq!(Tunables::parse(b"liella.compat.rpc_reply_any_source=2", false).rpc_reply_any_source, None);
        assert_eq!(Tunables::parse(b"liella.compat.rpc_reply_any_source=1", true).rpc_reply_any_source, None);
    }

    #[test]
    fn malloc_perturb() {
        assert_eq!(Tunables::malloc_perturb(b"glibc.malloc.perturb=165", false), Some(165));
        assert_eq!(Tunables::malloc_perturb(b"glibc.malloc.perturb=165:glibc.malloc.mmap_threshold=32768", false), Some(165));
        assert_eq!(Tunables::malloc_perturb(b"liella.malloc.perturb=0x5a", false), Some(0x5a));
        assert_eq!(Tunables::malloc_perturb(b"glibc.malloc.perturb=300", false), None, "out of range: ignored");
        assert_eq!(Tunables::malloc_perturb(b"glibc.malloc.perturb=junk", false), None);
        assert_eq!(Tunables::malloc_perturb(b"glibc.malloc.perturb=1:glibc.malloc.perturb=junk", false), Some(1), "the last valid setting");
        assert_eq!(Tunables::malloc_perturb(b"glibc.malloc.perturb=165", true), None, "secure mode");
    }
}
