//! The environment contract table (ENVIRON-TABLE-CONTRACT, D-1.12; docs/interfaces.md, "The
//! environment contract"): an opt-in Liella extension in which a hashed index of the
//! environment's names, not a scan of `environ`, answers `getenv`, misses included.
//!
//! The index maps a name's hash to the position of its first entry in the array. Slots are one
//! word each, `hash << 32 | (index + 1)` (0: empty), linear probing, at most half full. The
//! slots are the caller's (the products allocate them when the contract is turned on, and grow
//! them by a new allocation; an old one is never reused, so a reader still probing it reads
//! valid memory). Readers take no lock: a sequence word (even and non-zero: usable; odd or 0:
//! being changed or not built) brackets each lookup, and a lookup that saw it change answers
//! [`Answer::Unknown`] (the caller scans). Writers ([`Store::build`], [`Store::appended`],
//! [`Store::invalidate`]) run with the environment writers' lock held, so a lookup never builds.
//!
//! A hit is re-read at its entry (an entry whose name no longer matches falls through to the
//! next slot), so a broken contract gives stale answers, never another variable's value. A
//! lookup in an array other than the one the table was built for answers [`Answer::Unknown`].

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering::{Acquire, Relaxed, Release}, fence};

use crate::environ::matches;
use crate::mb_compat::ReadAt;
use crate::search_c::CWords;

/// The fewest slots a table uses (512 bytes).
pub const MIN_SLOTS: usize = 64;

/// The answer of a lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The first entry for the name: its index in the array and its address.
    Found(usize, usize),
    /// No entry for the name.
    Absent,
    /// The table cannot answer (not built, being changed, or built for another array): scan.
    Unknown,
}

/// The table's state words, held by the caller (statics in the products).
pub struct Store<'a> {
    /// The sequence word (even and non-zero: usable).
    pub seq: &'a AtomicU64,
    /// Entries indexed: the array's length.
    pub n: &'a AtomicUsize,
    /// The array the table was built for or last updated for.
    pub env: &'a AtomicUsize,
}

/// FNV-1a over `name`, 32 bits.
pub fn hash(name: &[u8]) -> u32 {
    name.iter().fold(0x811c_9dc5_u32, |h, &b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

/// The slots a table of `n` entries needs (a power of two, at least twice `n`); `None` past
/// what a slot can index.
pub fn slots_for(n: usize) -> Option<usize> {
    if n >= 0xffff_fff0 {
        return None;
    }
    n.checked_mul(2)?.checked_next_power_of_two().map(|c| c.max(MIN_SLOTS))
}

/// The name part of an entry's text (before its first `=`; all of it without one).
fn name_of(text: &[u8]) -> &[u8] {
    text.iter().position(|&b| b == b'=').map_or(text, |e| text.get(..e).unwrap_or(text))
}

fn pack(h: u32, idx: usize) -> usize {
    usize::try_from(h).unwrap_or(0).wrapping_shl(32) | idx.wrapping_add(1)
}

/// The hash a slot holds.
fn slot_hash(v: usize) -> u32 {
    u32::try_from(v.wrapping_shr(32)).unwrap_or(0)
}

/// The array index a slot holds.
fn slot_index(v: usize) -> usize {
    (v & 0xffff_ffff).wrapping_sub(1)
}

/// The first slot probed for `h` in `len` slots (a power of two).
fn start(h: u32, len: usize) -> usize {
    usize::try_from(h).unwrap_or(0) & len.wrapping_sub(1)
}

impl Store<'_> {
    /// Mark the table out of date (writers only); lookups scan until the next build.
    pub fn invalidate(&self) {
        let s = self.seq.load(Relaxed);
        if s & 1 == 0 {
            self.seq.store(s.wrapping_add(1), Relaxed);
            fence(Release);
        }
    }

    /// Whether the table is usable now.
    pub fn built(&self) -> bool {
        let s = self.seq.load(Acquire);
        s != 0 && s & 1 == 0
    }

    /// Insert `idx` under `h` unless an earlier entry for the same name is there. False when the
    /// slots are full.
    fn insert(slots: &[AtomicUsize], env: usize, h: u32, idx: usize, name: &[u8], mem: &impl CWords) -> bool {
        let len = slots.len();
        let mut i = start(h, len);
        for _ in 0..len {
            let Some(slot) = slots.get(i) else { return false };
            let v = slot.load(Relaxed);
            if v == 0 {
                slot.store(pack(h, idx), Relaxed);
                return true;
            }
            if slot_hash(v) == h {
                let other = mem.word(env, slot_index(v));
                if other != 0 && name_of(mem.text(other)) == name {
                    return true; // the earlier entry answers
                }
            }
            i = i.wrapping_add(1) & len.wrapping_sub(1);
        }
        false
    }

    /// Build the table for the array at `env` in `slots` (a power of two of them; writers' lock
    /// held). False, with the table left unusable (lookups scan), when the slots are too few
    /// for the array (`slots_for`).
    pub fn build(&self, slots: &[AtomicUsize], env: usize, mem: &impl CWords) -> bool {
        self.invalidate();
        let n = crate::environ::count(env, mem);
        if slots_for(n).is_none_or(|need| slots.len() < need) || !slots.len().is_power_of_two() {
            return false;
        }
        slots.iter().for_each(|s| s.store(0, Relaxed));
        for idx in 0..n {
            let name = name_of(mem.text(mem.word(env, idx)));
            if !Self::insert(slots, env, hash(name), idx, name, mem) {
                return false;
            }
        }
        self.n.store(n, Relaxed);
        self.env.store(env, Relaxed);
        self.seq.store(self.seq.load(Relaxed).wrapping_add(1), Release);
        true
    }

    /// After a writer call that replaced no entry (writers' lock held; the array, now at `env`,
    /// is the table's own, perhaps moved by that call): index the one entry it appended, if it
    /// appended one. False when the change is not that (an entry removed, or no room): rebuild.
    pub fn appended(&self, slots: &[AtomicUsize], env: usize, mem: &impl CWords) -> bool {
        if !self.built() || env == 0 {
            return false;
        }
        let n = self.n.load(Relaxed);
        let last = mem.word(env, n);
        if last == 0 {
            // The same length: nothing was added (`setenv` without overwrite of a set name).
            return n == 0 || mem.word(env, n.wrapping_sub(1)) != 0;
        }
        if mem.word(env, n.saturating_add(1)) != 0 || slots_for(n.saturating_add(1)).is_none_or(|need| slots.len() < need) {
            return false;
        }
        self.invalidate();
        let name = name_of(mem.text(last));
        if !Self::insert(slots, env, hash(name), n, name, mem) {
            return false;
        }
        self.n.store(n.saturating_add(1), Relaxed);
        self.env.store(env, Relaxed);
        self.seq.store(self.seq.load(Relaxed).wrapping_add(1), Release);
        true
    }

    /// Look `name` (hash `h`) up in the array at `env`. `slots` is called once the sequence
    /// word has been read, for the slots that sequence published.
    pub fn lookup<'s>(&self, slots: impl FnOnce() -> &'s [AtomicUsize], env: usize, name: &[u8], h: u32, mem: &impl CWords, read: &mut impl ReadAt<u8>) -> Answer {
        let s1 = self.seq.load(Acquire);
        if s1 == 0 || s1 & 1 == 1 || env != self.env.load(Relaxed) {
            return Answer::Unknown;
        }
        let slots = slots();
        let len = slots.len();
        let limit = name.len().saturating_add(1);
        let mut i = start(h, len);
        let mut answer = Answer::Unknown;
        for _ in 0..len {
            let v = slots.get(i).map_or(0, |s| s.load(Relaxed));
            if v == 0 {
                answer = Answer::Absent;
                break;
            }
            if slot_hash(v) == h {
                let idx = slot_index(v);
                let p = mem.word(env, idx);
                if p != 0 && read.read_at(p, limit).is_some_and(|t| matches(t, name)) {
                    answer = Answer::Found(idx, p);
                    break;
                }
            }
            i = i.wrapping_add(1) & len.wrapping_sub(1);
        }
        fence(Acquire);
        if self.seq.load(Relaxed) != s1 { Answer::Unknown } else { answer }
    }
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::as_conversions, clippy::unwrap_used, clippy::missing_asserts_for_indexing)]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use super::*;

    /// Words and strings in one arena, addressed from 8.
    struct Mem(Vec<u8>);

    impl Mem {
        fn put(&mut self, s: &[u8]) -> usize {
            let a = self.0.len();
            self.0.extend_from_slice(s);
            self.0.push(0);
            while self.0.len() % 8 != 0 {
                self.0.push(0);
            }
            a
        }
        fn array(&mut self, ptrs: &[usize], room: usize) -> usize {
            let a = self.0.len();
            for p in ptrs {
                self.0.extend_from_slice(&p.to_le_bytes());
            }
            self.0.resize(a + 8 * (ptrs.len() + 1 + room), 0);
            a
        }
    }

    impl CWords for Mem {
        fn word(&self, addr: usize, i: usize) -> usize {
            let o = addr + 8 * i;
            self.0.get(o..o + 8).map_or(0, |b| usize::from_le_bytes(b.try_into().unwrap_or([0; 8])))
        }
        fn set_word(&mut self, addr: usize, i: usize, v: usize) {
            let o = addr + 8 * i;
            if let Some(b) = self.0.get_mut(o..o + 8) {
                b.copy_from_slice(&v.to_le_bytes());
            }
        }
        fn alloc(&mut self, _words: usize) -> Option<usize> {
            None
        }
        fn free(&mut self, _addr: usize) {}
        fn text(&self, addr: usize) -> &[u8] {
            let t = self.0.get(addr..).unwrap_or(&[]);
            &t[..t.iter().position(|&b| b == 0).unwrap_or(t.len())]
        }
    }

    struct R<'a>(&'a Mem);
    impl ReadAt<u8> for R<'_> {
        fn read_at(&mut self, addr: usize, limit: usize) -> Option<&[u8]> {
            let t = self.0.text(addr);
            t.get(..limit.min(t.len()))
        }
    }

    fn slots(n: usize) -> Vec<AtomicUsize> {
        (0..n).map(|_| AtomicUsize::new(0)).collect()
    }

    #[test]
    fn sizes_are_powers_of_two_at_least_twice_the_entries() {
        assert_eq!(slots_for(0), Some(MIN_SLOTS));
        assert_eq!(slots_for(32), Some(64));
        assert_eq!(slots_for(33), Some(128));
        assert_eq!(slots_for(2000), Some(4096));
        assert_eq!(slots_for(usize::MAX), None);
    }

    #[test]
    fn first_entry_answers_misses_are_absent_and_appends_are_seen() {
        let (seq, n, envw) = (AtomicU64::new(0), AtomicUsize::new(0), AtomicUsize::new(0));
        let st = Store { seq: &seq, n: &n, env: &envw };
        let mut m = Mem(std::vec![0; 8]);
        let names: Vec<std::string::String> = (0..300).map(|k| std::format!("V{k}=x{k}")).collect();
        let mut ptrs: Vec<usize> = names.iter().map(|s| m.put(s.as_bytes())).collect();
        ptrs.push(m.put(b"V7=dup"));
        let env = m.array(&ptrs, 4);
        let s = slots(slots_for(301).unwrap());
        let look = |name: &[u8], m: &Mem| st.lookup(|| &s[..], env, name, hash(name), m, &mut R(m));
        assert_eq!(look(b"V7", &m), Answer::Unknown);
        // Too few slots: not built, lookups scan.
        assert!(!st.build(&slots(64), env, &m));
        assert_eq!(look(b"V7", &m), Answer::Unknown);
        assert!(st.build(&s, env, &m));
        assert_eq!(look(b"V7", &m), Answer::Found(7, ptrs[7]));
        assert_eq!(look(b"V299", &m), Answer::Found(299, ptrs[299]));
        assert_eq!(look(b"V", &m), Answer::Absent);
        assert_eq!(look(b"NOPE", &m), Answer::Absent);
        // Another array: the table does not answer.
        assert_eq!(st.lookup(|| &s[..], env + 8, b"V7", hash(b"V7"), &m, &mut R(&m)), Answer::Unknown);
        // Nothing appended: still usable.
        assert!(st.appended(&s, env, &m));
        let q = m.put(b"NEW=1");
        m.set_word(env, 301, q);
        // The contract: not seen until the writer reports it.
        assert_eq!(look(b"NEW", &m), Answer::Absent);
        assert!(st.appended(&s, env, &m));
        assert_eq!(look(b"NEW", &m), Answer::Found(301, q));
        // An entry removed (the array shorter) is not an append.
        m.set_word(env, 301, 0);
        assert!(!st.appended(&s, env, &m));
        st.invalidate();
        assert_eq!(look(b"V7", &m), Answer::Unknown);
        // A slot stored behind the table's back (a broken contract): the entry is re-read, so the
        // stale index never answers for another name.
        assert!(st.build(&s, env, &m));
        let z = m.put(b"ZZ=1");
        m.set_word(env, 7, z);
        assert_eq!(look(b"V7", &m), Answer::Absent);
    }
}
