//! Regular-expression glue (spec §14.9; D-14.4): `regex_t`, the POSIX, GNU and BSD interfaces
//! over `liella::pattern::regex`.
//!
//! A compiled `regex_t` names a caller-heap block (`buffer`, `used` and `allocated` its length):
//! the interned handle of the locale the pattern was compiled in (8 bytes, D-7.8), the length
//! of the bytes every match contains (`regex::required_string`; 4 bytes, `u32::MAX` for
//! none), whether they are the whole pattern or begin every match (4 bytes, 1, 2 or 0) and the bytes padded to a
//! multiple of 4, then the serialised program (`Program::as_bytes`: little-endian words, the
//! automaton and the matcher's tables, all derived once here). Searches read it in place
//! (`ProgRef::from_bytes`): nothing is copied or derived per call, and the block is never
//! written after `regcomp`, so threads may search with one `regex_t` at once.
//! `regexec` finds a plain string's earliest occurrence directly, and answers no match at once
//! when the required bytes do not occur in the window (`regex::prefilter`, D-14.10). Searches
//! match in that locale, so a later `setlocale` does not change a compiled pattern. `regfree`
//! frees it (and, as the GNU interface specifies, `fastmap` and `translate`).
//!
//! The bit-field word at offset 56 (`can_be_null`, `regs_allocated`, `fastmap_accurate`,
//! `no_sub`, `not_bol`, `not_eol`, `newline_anchor`) is laid out low bit first (x86-64 psABI).
//!
//! Decisions (listed in the phase report): an unknown error code has a message (`Unknown
//! error`) instead of aborting; `re_exec` without a compiled pattern returns 0; an invalid
//! argument (a NULL or uncompiled `regex_t`, a NULL subject) is `REG_BADPAT` from `regexec` and
//! -2 from the GNU searches; a subject longer than `regoff_t` can express is `REG_ESPACE` (-2);
//! the work budget of a search with back-references is `re_max_failures` × 65536 thread steps.

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering::Relaxed;

use liella::error::KResult;
use liella::mem::TryVec;
use liella::pattern::regex::{self, syntax, Match, Options, Prefilter, ProgRef, ReError, ScratchMem, ScratchNeed, ScratchSource, Window};
use liella_abi::ctypes::{c_char, c_int, c_void};
use liella_abi::layout::{regex_t, struct_re_registers};
use liella_legacy::regex_compat as bsd;

use crate::{caller_alloc, conv, data, locale};

const REG_BADPAT: c_int = 2;
const REG_ESPACE: c_int = 12;
const REG_NOTBOL: c_int = 1;
const REG_NOTEOL: c_int = 2;
const REG_STARTEND: c_int = 4;

const CAN_BE_NULL: u8 = 1;
const REGS_SHIFT: u8 = 1;
const REGS_MASK: u8 = 3 << REGS_SHIFT;
const FASTMAP_ACCURATE: u8 = 1 << 3;
const NO_SUB: u8 = 1 << 4;
const NOT_BOL: u8 = 1 << 5;
const NOT_EOL: u8 = 1 << 6;
const NEWLINE_ANCHOR: u8 = 1 << 7;

const REGS_UNALLOCATED: u8 = 0;
const REGS_REALLOCATE: u8 = 1;

fn bits(re: &regex_t) -> u8 {
    re.can_be_null.first().copied().unwrap_or(0)
}

fn set_bits(re: &mut regex_t, v: u8) {
    if let Some(b) = re.can_be_null.first_mut() {
        *b = v;
    }
}

fn budget() -> u64 {
    let f = data::re_max_failures.load(Relaxed);
    u64::try_from(f).unwrap_or(0).max(1).saturating_mul(regex::BUDGET_PER_FAILURE)
}

/// A unique number for each compiled pattern (D-14.12): the lazy-DFA cache key, written into the
/// block head so a reused block address cannot be mistaken for an earlier pattern's. Starts at 1
/// so 0 marks "none"; its wrap after 2^64 compilations is unreachable. Nothing reads it yet.
static SERIAL: AtomicU64 = AtomicU64::new(1);

/// The fixed block head (D-14.12): the interned locale handle, the per-regcomp serial, the
/// required-bytes length (`u32::MAX`: none) and whether they are the whole pattern.
const HEAD: usize = 24;

/// The block head bytes for its fields (handle, serial, required length, whole flag).
fn write_head(handle: u64, serial: u64, n: u32, whole: u32) -> [u8; HEAD] {
    let mut h = [0u8; HEAD];
    let (a, r) = h.split_at_mut(8);
    let (b, r) = r.split_at_mut(8);
    let (c, d) = r.split_at_mut(4);
    a.copy_from_slice(&handle.to_le_bytes());
    b.copy_from_slice(&serial.to_le_bytes());
    c.copy_from_slice(&n.to_le_bytes());
    d.copy_from_slice(&whole.to_le_bytes());
    h
}

/// Read the block head: the locale handle, the serial, the required-bytes length and whole flag,
/// and the rest of the block (the lead bytes and the program).
fn read_head(bytes: &[u8]) -> Result<(usize, u64, u32, u32, &[u8]), ReError> {
    // One fixed-size split, then fixed-offset reads of the head array: no per-field bounds check.
    let (head, rest) = bytes.split_first_chunk::<HEAD>().ok_or(ReError::BadPat)?;
    let w = |at: usize| head.get(at..).and_then(|t| t.first_chunk::<8>()).copied().ok_or(ReError::BadPat);
    let handle = u64::from_le_bytes(w(0)?);
    let serial = u64::from_le_bytes(w(8)?);
    let [n0, n1, n2, n3, w0, w1, w2, w3] = w(16)?;
    Ok((usize::try_from(handle).map_err(|_| ReError::BadPat)?, serial, u32::from_le_bytes([n0, n1, n2, n3]), u32::from_le_bytes([w0, w1, w2, w3]), rest))
}

/// Compile `pattern` in the current locale into a new block: its address and length, and the
/// program's facts.
fn compile_block(pattern: &[u8], syn: u64) -> Result<(usize, usize, usize, bool), ReError> {
    let handle = locale::current_handle();
    let prog = regex::compile(pattern, syn, locale::by_handle(handle))?;
    // An end-anchored literal `L$` (RE-SO-3) stores `L` and a '\n' as its lead, whole flag 3
    // (`regex::end_prefilter` settles its searches); any other pattern its required bytes.
    // Flags 1 and 3 promise a pattern without subexpressions (`gnu_search` fills the registers of
    // a prefilter match without the program, D-14.17).
    let no_sub = prog.nsub() == 0;
    let end = regex::end_literal(&prog, locale::by_handle(handle)).filter(|_| no_sub).and_then(|mut l| l.push(b'\n').ok().map(|()| l));
    let is_end = end.is_some();
    let lead = match end {
        Some(l) => Some((l, false)),
        None => regex::required_string(&prog, locale::by_handle(handle)),
    };
    let lead_len =lead.as_ref().map_or(Some(0), |(p, _)| p.len().div_ceil(4).checked_mul(4)).ok_or(ReError::ESpace)?;
    let words = prog.as_bytes();
    let len = words.len().checked_add(HEAD).and_then(|n| n.checked_add(lead_len)).ok_or(ReError::ESpace)?;
    let mut bytes = TryVec::with_capacity(len).map_err(|_| ReError::ESpace)?;
    let h = u64::try_from(handle).map_err(|_| ReError::ESpace)?;
    let n = lead.as_ref().map_or(Some(u32::MAX), |(p, _)| u32::try_from(p.len()).ok().filter(|&n| n != u32::MAX)).ok_or(ReError::ESpace)?;
    // 1 = the whole pattern, 2 = they begin every match (`regex::leads_every_match`), 3 = `L$`
    // (with the '\n'), 0 = inside.
    let whole = match &lead {
        _ if is_end => 3,
        Some((_, true)) if no_sub => 1,
        Some((p, false)) if regex::leads_every_match(&prog, locale::by_handle(handle), p) => 2,
        _ => 0,
    };
    let serial = SERIAL.fetch_add(1, Relaxed);
    bytes.extend_from_slice(&write_head(h, serial, n, whole)).map_err(|_| ReError::ESpace)?;
    if let Some((p, _)) = &lead {
        bytes.extend_from_slice(p).map_err(|_| ReError::ESpace)?;
        while bytes.len() % 4 != 0 {
            bytes.push(0).map_err(|_| ReError::ESpace)?;
        }
    }
    bytes.extend_from_slice(words).map_err(|_| ReError::ESpace)?;
    let addr = caller_alloc::dup_bytes(&bytes).map_err(|_| ReError::ESpace)?;
    Ok((addr, len, prog.nsub(), prog.can_be_null()))
}

/// A compiled block's parts: the locale handle, the per-regcomp serial (the lazy DFA's key),
/// the required bytes (if any) and whether they are the whole pattern, whether they begin every
/// match, whether they are an end-anchored literal and a '\n' (`regex::end_prefilter`), and the
/// program's words.
type BlockParts = (usize, u64, Option<(&'static [u8], bool)>, bool, bool, &'static [u8]);

fn block_parts(addr: usize, len: usize) -> Result<BlockParts, ReError> {
    let bytes = conv::regex_bytes(addr, len).ok_or(ReError::BadPat)?;
    let (handle, serial, n, whole, rest) = read_head(bytes)?;
    if n == u32::MAX {
        return Ok((handle, serial, None, false, false, rest));
    }
    let n = usize::try_from(n).map_err(|_| ReError::BadPat)?;
    let (lead, rest) = rest.split_at_checked(n.div_ceil(4).checked_mul(4).ok_or(ReError::BadPat)?).ok_or(ReError::BadPat)?;
    Ok((handle, serial, lead.get(..n).map(|l| (l, whole == 1)), whole == 2, whole == 3, rest))
}

/// The prefilter's answer for a block's lead (`regex::prefilter`, or `regex::end_prefilter` for
/// an end-anchored literal).
fn pre_answer(lead: Option<(&[u8], bool)>, leading: bool, end: bool, subject: &[u8], first: usize, opts: &Options) -> Prefilter {
    match lead {
        Some((l, _)) if end => regex::end_prefilter(l, subject, first, opts, conv::find_slice),
        _ => regex::prefilter(lead, leading, subject, first, opts, conv::find_slice),
    }
}

/// The locale and program of a compiled block.
fn load_block(addr: usize, len: usize) -> Result<(&'static liella::locale::Locale, ProgRef<'static>), ReError> {
    let (handle, _, _, _, _, rest) = block_parts(addr, len)?;
    let prog = ProgRef::from_bytes(rest).ok_or(ReError::BadPat)?;
    Ok((locale::by_handle(handle), prog))
}

fn load(re: &regex_t) -> Result<(&'static liella::locale::Locale, ProgRef<'static>), ReError> {
    load_block(re.buffer, usize::try_from(re.used).map_err(|_| ReError::BadPat)?)
}

fn store(re: &mut regex_t, block: (usize, usize, usize, bool), syn: u64) {
    let (addr, len, nsub, null) = block;
    re.buffer = addr;
    re.allocated = u64::try_from(len).unwrap_or(0);
    re.used = re.allocated;
    re.syntax = i64::from_ne_bytes(syn.to_ne_bytes());
    re.re_nsub = u64::try_from(nsub).unwrap_or(0);
    set_bits(re, if null { CAN_BE_NULL } else { 0 });
}

// ------------------------------------------------------------------------------ POSIX

/// `regcomp`: 0 or the error code.
pub fn regcomp(preg: Option<&mut regex_t>, pattern: KResult<&[u8]>, cflags: c_int) -> c_int {
    let (Some(re), Ok(pat)) = (preg, pattern) else { return REG_BADPAT };
    let syn = regex::regcomp_syntax(cflags);
    match compile_block(pat, syn) {
        Ok(block) => {
            store(re, block, syn);
            re.fastmap = 0;
            re.translate = 0;
            let mut b = bits(re);
            if cflags & regex::cflags::NOSUB != 0 {
                b |= NO_SUB;
            }
            if cflags & regex::cflags::NEWLINE != 0 {
                b |= NEWLINE_ANCHOR;
            }
            set_bits(re, b);
            0
        }
        Err(e) => regex::regcomp_code(e),
    }
}

/// `regexec`: 0, `REG_NOMATCH`, or an error code.
pub fn regexec(preg: Option<&regex_t>, string: usize, nmatch: usize, pmatch: usize, eflags: c_int) -> c_int {
    let Some(re) = preg else { return REG_BADPAT };
    // Parse the block once (D-14.12 fixed-cost fold): the lead for the prefilter and the program
    // words for the engine come from one `block_parts`, not two.
    let (handle, serial, lead, leading, end, rest) = match usize::try_from(re.used).map_err(|_| ReError::BadPat).and_then(|n| block_parts(re.buffer, n)) {
        Ok(p) => p,
        Err(e) => return e.code(),
    };
    let no_sub = bits(re) & NO_SUB != 0;
    let want = if no_sub { 0 } else { nmatch };
    let startend = eflags & REG_STARTEND != 0;
    let view = if startend { want.max(1) } else { want };
    let pm = if view == 0 { None } else { conv::regmatch_array(pmatch, view) };
    let (so, eo, subject) = if startend {
        let Some(first) = pm.as_ref().and_then(|p| p.first()) else { return REG_BADPAT };
        let (Ok(so), Ok(eo)) = (usize::try_from(first.rm_so), usize::try_from(first.rm_eo)) else { return regex::NOMATCH };
        if so > eo {
            return regex::NOMATCH;
        }
        let Some(s) = conv::regex_bytes(string, eo) else { return REG_BADPAT };
        (so, eo, s)
    } else {
        let Some(s) = conv::regex_subject_z(string) else { return REG_BADPAT };
        (0, s.len(), s)
    };
    if i32::try_from(eo).is_err() {
        return REG_ESPACE;
    }
    let opts = Options { not_bol: eflags & REG_NOTBOL != 0, not_eol: eflags & REG_NOTEOL != 0, newline_anchor: bits(re) & NEWLINE_ANCHOR != 0, budget: budget(), ..Options::default() };
    // The window ends where the subject does (`eo`): a plain string's earliest occurrence is its
    // match, and without the required bytes there is none.
    // When the bytes begin every match, the engine starts at their first occurrence (RE-SEARCH-2).
    let pre = pre_answer(lead, leading, end, subject, so, &opts);
    let first = if let Prefilter::From(k) = pre { k } else { so };
    let r = match pre {
        Prefilter::Match(start, end) => Ok(Some(Match { start, end, groups: regex::Groups::new() })),
        Prefilter::NoMatch => Ok(None),
        Prefilter::From(_) | Prefilter::Engine => match ProgRef::from_bytes(rest) {
            Some(prog) => {
                let mut src = ThreadScratch::acquire();
                let (loc, window) = (locale::by_handle(handle), Window { first, last: eo, backward: false });
                if want == 0 {
                    // Existence only (D-14.13): the lazy DFA, keyed by the block's serial; no
                    // offsets are reported, so the match's are never read.
                    regex::exists_with(&prog, loc, subject, window, &opts, serial, &mut src).map(|found| found.then(|| Match { start: 0, end: 0, groups: regex::Groups::new() }))
                } else {
                    // Positions (D-14.16): the DFA's span passes, then the subexpressions on the span.
                    regex::search_with(&prog, loc, subject, window, &opts, want > 1, serial, &mut src)
                }
            }
            None => Err(ReError::BadPat),
        },
    };
    match r {
        Err(e) => e.code(),
        Ok(None) => regex::NOMATCH,
        Ok(Some(m)) => {
            if let Some(p) = pm {
                for (i, slot) in p.iter_mut().take(want).enumerate() {
                    let (s, e) = entry(&m, i);
                    slot.rm_so = s;
                    slot.rm_eo = e;
                }
            }
            0
        }
    }
}

/// The calling thread's scratch for searches too large for the stack (D-14.11): one block per
/// element type in the thread's result table, allocated on first need, grown (to a power of
/// two) when too small, kept between calls and freed when the thread exits; so a thread's
/// steady state allocates nothing, and what it keeps is bounded by its largest search.
///
/// A search claims the thread's scratch for its duration (D-14.12): a re-entrant search (a
/// signal handler that calls a regex function during one) gets an unclaimed guard whose scratch
/// is `None`, so it falls back to per-call storage instead of aliasing the first search's blocks.
struct ThreadScratch {
    /// The claim was tried (on the first need for scratch, so a search that stays on its stack
    /// never touches the busy flag).
    tried: bool,
    /// The thread's records slot while this search holds its scratch.
    slot: Option<&'static crate::tb::RecordsSlot>,
    /// The thread's result table, fetched once per search (not once per block).
    table: Option<&'static crate::users::Table>,
}

impl ThreadScratch {
    /// The guard for one search; it claims the thread's scratch on first need and releases the
    /// claim when dropped.
    fn acquire() -> Self {
        ThreadScratch { tried: false, slot: None, table: None }
    }

    /// The thread's result table when this search holds the thread's scratch (claiming it and
    /// fetching the table on the first call only).
    fn table(&mut self) -> Option<&'static crate::users::Table> {
        if !self.tried {
            self.tried = true;
            self.slot = crate::users::scratch_claim();
            self.table = self.slot.and_then(|s| crate::users::table_of(s).ok());
        }
        self.table
    }
}

impl Drop for ThreadScratch {
    fn drop(&mut self) {
        if let Some(s) = self.slot {
            crate::users::scratch_release(s);
        }
    }
}

/// The u32 words of the lazy-DFA state store (D-14.12): a fixed 32 KiB per thread.
const DFA_STORE_U32S: usize = 8192;

fn thread_array<T: conv::CPod>(table: &crate::users::Table, kind: usize, n: usize) -> Option<&'static mut [T]> {
    let bytes = n.checked_mul(size_of::<T>())?;
    let addr = crate::users::block_addr_in(table, kind, bytes.max(1)).ok()?;
    conv::own_array(addr, n)
}

impl ScratchSource for ThreadScratch {
    fn scratch(&mut self, need: &ScratchNeed) -> Option<ScratchMem<'_>> {
        let table = self.table()?;
        use crate::users::{REGEX_I32, REGEX_U32, REGEX_U64, REGEX_USIZE};
        Some(ScratchMem {
            u32s: thread_array(table, REGEX_U32, need.u32s)?,
            u64s: thread_array(table, REGEX_U64, need.u64s)?,
            i32s: thread_array(table, REGEX_I32, need.i32s)?,
            usizes: thread_array(table, REGEX_USIZE, need.usizes)?,
        })
    }

    fn dfa_store(&mut self) -> Option<&mut [u32]> {
        let table = self.table()?;
        thread_array::<u32>(table, crate::users::REGEX_DFA, DFA_STORE_U32S)
    }
}

/// Entry `i` of a match: 0 the whole match, then the subexpressions, -1 beyond them.
fn entry(m: &Match, i: usize) -> (i32, i32) {
    let off = |v: i64| i32::try_from(v).unwrap_or(-1);
    match i.checked_sub(1) {
        None => (i32::try_from(m.start).unwrap_or(-1), i32::try_from(m.end).unwrap_or(-1)),
        Some(g) => m.groups.as_slice().get(g).map_or((-1, -1), |&(s, e)| (off(s), off(e))),
    }
}

/// `regerror`: the message's length plus one; as much of it as fits, terminated, in `buf`.
pub fn regerror(code: c_int, buf: KResult<Option<&mut [u8]>>) -> usize {
    let msg = regex::message_z(code);
    if let Ok(Some(b)) = buf {
        if let Some((last, room)) = b.split_last_mut() {
            let n = room.len().min(msg.len().saturating_sub(1));
            for (d, s) in room.iter_mut().zip(msg.iter()).take(n) {
                *d = *s;
            }
            match room.get_mut(n) {
                Some(z) => *z = 0,
                None => *last = 0,
            }
        }
    }
    msg.len()
}

/// `regfree`.
pub fn regfree(preg: Option<&mut regex_t>) {
    let Some(re) = preg else { return };
    // The block is `regcomp`'s; `fastmap` and `translate` are malloc'ed by the GNU contract.
    caller_alloc::release(re.buffer);
    caller_alloc::release(re.fastmap);
    caller_alloc::release(re.translate);
    re.buffer = 0;
    re.allocated = 0;
    re.used = 0;
    re.fastmap = 0;
    re.translate = 0;
}

// ------------------------------------------------------------------------------ GNU

/// `re_set_syntax`: the previous syntax.
pub fn re_set_syntax(syn: u64) -> u64 {
    data::re_syntax_options.swap(syn, Relaxed)
}

/// `re_compile_pattern`: NULL, or the error's message.
pub fn re_compile_pattern(pattern: KResult<&[u8]>, buffer: Option<&mut regex_t>) -> *const c_char {
    let msg = |e: ReError| regex::message_z(e.code()).as_ptr().cast::<c_char>();
    let (Some(b), Ok(pat)) = (buffer, pattern) else { return msg(ReError::BadPat) };
    let syn = data::re_syntax_options.load(Relaxed);
    match compile_block(pat, syn) {
        Ok(block) => {
            // A buffer from an earlier compilation is replaced (GNU: it is malloc'ed storage).
            caller_alloc::release(b.buffer);
            store(b, block, syn);
            let mut v = bits(b) | NEWLINE_ANCHOR;
            if syn & syntax::NO_SUB != 0 {
                v |= NO_SUB;
            }
            set_bits(b, v);
            core::ptr::null()
        }
        Err(e) => msg(e),
    }
}

/// `re_compile_fastmap`: 0, or -2 for a buffer that holds no compiled pattern. The fastmap (when
/// there is one) marks the bytes a match can begin with (every byte when a match can be empty);
/// the marks are a superset, never missing a possible start.
pub fn re_compile_fastmap(buffer: Option<&mut regex_t>) -> c_int {
    let Some(b) = buffer else { return -2 };
    let Ok((_, prog)) = load(b) else { return -2 };
    if let Some(map) = conv::regex_bytes_mut(b.fastmap, 256) {
        for (c, x) in (0u8..=255).zip(map.iter_mut()) {
            *x = u8::from(prog.can_start_with(c));
        }
        set_bits(b, bits(b) | FASTMAP_ACCURATE);
    }
    0
}

/// `re_set_registers`.
pub fn re_set_registers(buffer: Option<&mut regex_t>, regs: Option<&mut struct_re_registers>, num: u32, starts: usize, ends: usize) {
    let (Some(b), Some(r)) = (buffer, regs) else { return };
    let v = bits(b) & !REGS_MASK;
    if num == 0 {
        set_bits(b, v);
        r.num_regs = 0;
        r.start = 0;
        r.end = 0;
    } else {
        set_bits(b, v | (REGS_REALLOCATE << REGS_SHIFT));
        r.num_regs = num;
        r.start = starts;
        r.end = ends;
    }
}

/// The arguments of one GNU search: two texts (the second may be empty), where it starts and
/// how far, where matching must stop, and whether it is anchored at `start` (`re_match`).
#[derive(Clone, Copy)]
pub struct GnuArgs {
    /// First text and its length.
    pub s1: (usize, i32),
    /// Second text and its length.
    pub s2: (usize, i32),
    /// Start position.
    pub start: i32,
    /// Range (negative: backward).
    pub range: i32,
    /// Matching stops here.
    pub stop: i32,
    /// `re_match`: the match must start at `start`; the result is its length.
    pub anchored: bool,
}

fn text(addr: usize, len: i32) -> Option<&'static [u8]> {
    let n = usize::try_from(len).ok()?;
    if n == 0 { Some(&[]) } else { conv::regex_bytes(addr, n) }
}

/// `re_search`, `re_search_2`, `re_match`, `re_match_2`: the start (or the length matched),
/// -1 for no match, -2 for an error.
pub fn gnu_search(buffer: Option<&mut regex_t>, a: GnuArgs, regs: Option<&mut struct_re_registers>) -> i32 {
    let Some(b) = buffer else { return -2 };
    let Ok((handle, serial, lead, leading, end, rest)) = usize::try_from(b.used).map_err(|_| ReError::BadPat).and_then(|n| block_parts(b.buffer, n)) else { return -2 };
    // A negative size is no match, -1, as glibc does (T26: ptx passes `limit - cursor`, which
    // can be negative, and treats -2 as fatal).
    if a.s1.1 < 0 || a.s2.1 < 0 {
        return -1;
    }
    let (Some(t1), Some(t2)) = (text(a.s1.0, a.s1.1), text(a.s2.0, a.s2.1)) else { return -2 };
    let joined;
    let whole: &[u8] = if t2.is_empty() {
        t1
    } else if t1.is_empty() {
        t2
    } else {
        let mut v = TryVec::new();
        if v.extend_from_slice(t1).and_then(|()| v.extend_from_slice(t2)).is_err() {
            return -2;
        }
        joined = v;
        &joined
    };
    let total = whole.len();
    if i32::try_from(total).is_err() {
        return -2;
    }
    let Ok(start) = usize::try_from(a.start) else { return -1 };
    if start > total {
        return -1;
    }
    let stop = usize::try_from(a.stop).unwrap_or(0).min(total);
    let subject = whole.get(..stop).unwrap_or(whole);
    let window = if a.anchored {
        Window { first: start, last: start, backward: false }
    } else if a.range >= 0 {
        let last = i64::try_from(start).unwrap_or(0).saturating_add(i64::from(a.range));
        Window { first: start, last: usize::try_from(last).unwrap_or(total).min(total), backward: false }
    } else {
        let first = i64::try_from(start).unwrap_or(0).saturating_add(i64::from(a.range)).max(0);
        Window { first: usize::try_from(first).unwrap_or(0), last: start, backward: true }
    };
    let window = Window { last: window.last.min(subject.len()), ..window };
    if window.first > subject.len() {
        return -1;
    }
    let v = bits(b);
    let opts = Options { not_bol: v & NOT_BOL != 0, not_eol: v & NOT_EOL != 0, newline_anchor: v & NEWLINE_ANCHOR != 0, budget: budget(), ..Options::default() };
    let fill = regs.is_some() && v & NO_SUB == 0;
    // The required bytes first, as regexec (D-14.10; RE-SEARCH): absent from the text a match
    // could lie in (`subject[window.first..]`) = no match; a plain string's earliest occurrence
    // is its match when it starts in a forward window (and none starts there otherwise). A
    // backward window and a translate table leave the search to the engine.
    let lead = if b.translate == 0 { lead } else { None };
    // When the bytes begin every match, a forward search starts at their first occurrence
    // (RE-SEARCH-2: on input where every line matches, the prefilter's scan then replaces the
    // engine's scan of the bytes before it instead of adding to it).
    let pre = pre_answer(lead, leading, end, subject, window.first, &opts);
    #[cfg(feature = "selftest")]
    {
        use regex::paths::{hit, Path};
        let usable = !window.backward && !matches!(pre, Prefilter::Match(s, _) | Prefilter::From(s) if s > window.last);
        hit(match pre {
            Prefilter::NoMatch => Path::GnuNoMatch,
            Prefilter::Match(..) if usable => Path::GnuMatch,
            Prefilter::From(_) if usable => Path::GnuFrom,
            _ => Path::GnuEngine,
        });
    }
    let (m, window) = match pre {
        Prefilter::NoMatch => return -1,
        Prefilter::Match(_, _) | Prefilter::From(_) if window.backward => (None, window),
        Prefilter::Match(start, _) | Prefilter::From(start) if start > window.last => return -1,
        Prefilter::Match(start, end) => (Some(Match { start, end, groups: regex::Groups::new() }), window),
        Prefilter::From(first) => (None, Window { first, ..window }),
        Prefilter::Engine => (None, window),
    };
    // The program only for the engine (as `regexec`): a prefilter match is a pattern without
    // subexpressions (compile_block's flags 1 and 3), so its registers need no program.
    let (m, nsub) = match m {
        Some(m) => (m, 0),
        None => {
            let Some(prog) = ProgRef::from_bytes(rest) else { return -2 };
            let mut src = ThreadScratch::acquire();
            match regex::search_with(&prog, locale::by_handle(handle), subject, window, &opts, fill, serial, &mut src) {
                Err(_) => return -2,
                Ok(None) => return -1,
                Ok(Some(m)) => (m, prog.nsub()),
            }
        }
    };
    if let (true, Some(r)) = (fill, regs) {
        if fill_registers(b, r, &m, nsub).is_err() {
            return -2;
        }
    }
    let at = if a.anchored { m.end.saturating_sub(m.start) } else { m.start };
    i32::try_from(at).unwrap_or(-2)
}

/// Store a match in the registers, allocating or growing them as `regs_allocated` says.
fn fill_registers(b: &mut regex_t, r: &mut struct_re_registers, m: &Match, nsub: usize) -> Result<(), ()> {
    let need = nsub.checked_add(1).ok_or(())?;
    let mode = (bits(b) & REGS_MASK) >> REGS_SHIFT;
    let alloc_n = need.checked_add(1).ok_or(())?;
    let have = usize::try_from(r.num_regs).map_err(|_| ())?;
    if mode == REGS_UNALLOCATED {
        let (st, en) = (caller_alloc::calloc(alloc_n, 4).expose_provenance(), caller_alloc::calloc(alloc_n, 4).expose_provenance());
        if st == 0 || en == 0 {
            caller_alloc::release(st);
            caller_alloc::release(en);
            return Err(());
        }
        r.start = st;
        r.end = en;
        r.num_regs = u32::try_from(alloc_n).map_err(|_| ())?;
        set_bits(b, (bits(b) & !REGS_MASK) | (REGS_REALLOCATE << REGS_SHIFT));
    } else if mode == REGS_REALLOCATE && have < need {
        let bytes = alloc_n.checked_mul(4).ok_or(())?;
        // The arrays came from this allocator (GNU contract: malloc'ed); a failed `realloc`
        // leaves the old array in place.
        let st = caller_alloc::realloc(core::ptr::with_exposed_provenance_mut::<c_void>(r.start), bytes).expose_provenance();
        if st == 0 {
            return Err(());
        }
        r.start = st;
        let en = caller_alloc::realloc(core::ptr::with_exposed_provenance_mut::<c_void>(r.end), bytes).expose_provenance();
        if en == 0 {
            return Err(());
        }
        r.end = en;
        r.num_regs = u32::try_from(alloc_n).map_err(|_| ())?;
    }
    let n = usize::try_from(r.num_regs).map_err(|_| ())?;
    let (Some(starts), Some(ends)) = (conv::regoff_array(r.start, n), conv::regoff_array(r.end, n)) else { return Err(()) };
    for (i, (s, e)) in starts.iter_mut().zip(ends.iter_mut()).enumerate() {
        let (a, z) = entry(m, i);
        *s = a;
        *e = z;
    }
    Ok(())
}

// ------------------------------------------------------------------------------ BSD

/// `re_comp`: NULL on success (or for `re_comp(NULL)` with a pattern compiled), else a message.
pub fn re_comp(pattern: KResult<&[u8]>) -> *mut c_char {
    let _g = bsd::LOCK.lock();
    let Ok(pat) = pattern else {
        return if bsd::PATTERN.load(Relaxed) != 0 { core::ptr::null_mut() } else { bsd::NO_PREVIOUS.as_ptr().cast::<c_char>().cast_mut() };
    };
    let old = bsd::PATTERN.swap(0, Relaxed);
    caller_alloc::release(old);
    bsd::PATTERN_LEN.store(0, Relaxed);
    match compile_block(pat, data::re_syntax_options.load(Relaxed)) {
        Ok((addr, len, _, _)) => {
            bsd::PATTERN.store(addr, Relaxed);
            bsd::PATTERN_LEN.store(len, Relaxed);
            core::ptr::null_mut()
        }
        Err(e) => regex::message_z(e.code()).as_ptr().cast::<c_char>().cast_mut(),
    }
}

/// `re_exec`: 1 if the string matches the pattern `re_comp` compiled, 0 if not (or if there is
/// none), -1 if the search failed.
pub fn re_exec(string: KResult<&[u8]>) -> c_int {
    let _g = bsd::LOCK.lock();
    let (addr, len) = (bsd::PATTERN.load(Relaxed), bsd::PATTERN_LEN.load(Relaxed));
    let (Ok(s), true) = (string, addr != 0) else { return 0 };
    let Ok((loc, prog)) = load_block(addr, len) else { return -1 };
    let opts = Options { newline_anchor: true, budget: budget(), ..Options::default() };
    match regex::exec_in(&prog, loc, s, Window { first: 0, last: s.len(), backward: false }, &opts, false) {
        Ok(Some(_)) => 1,
        Ok(None) => 0,
        Err(_) => -1,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn block_head_round_trips() {
        // D-14.12: the serial and the required-bytes fields survive write -> read, and the rest
        // of the block is returned untouched. A wrong offset in either helper would break this.
        for &(handle, serial, n, whole) in &[(0usize, 1u64, u32::MAX, 0u32), (0x5a5a, 42, 3, 1), (usize::MAX >> 1, u64::MAX, 0, 0)] {
            let mut block = write_head(u64::try_from(handle).unwrap(), serial, n, whole).to_vec();
            block.extend_from_slice(&[9, 8, 7, 6]);
            let (rh, rs, rn, rw, rest) = read_head(&block).unwrap();
            assert_eq!((rh, rs, rn, rw), (handle, serial, n, whole));
            assert_eq!(rest, &[9u8, 8, 7, 6][..]);
        }
        // A block shorter than the head is rejected, not read out of bounds.
        assert!(read_head(&[0u8; HEAD - 1]).is_err());
    }
}
