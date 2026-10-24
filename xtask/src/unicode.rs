//! `cargo xtask unicode-gen [--check]`: generate the Unicode tables of the built-in `C.UTF-8`
//! locale and of the internal Unicode API from `spec/unicode/15.1.0` (IP-19, D-5.4, D-X.4).
//!
//! The derivation rules are those `spec/tools/ucd_derive.py` established against the reference
//! system's compiled `C.utf8`: classes, simple case maps and widths agree for every code point.
//!
//! Generated files: `crates/core/src/unicode/tables.rs` and `crates/core/src/names/dns/idna_tables.rs`
//! (the UTS #46 tables, D-16.4). With `--check`, nothing is written; the
//! command fails if a file would change.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

const MAX: u32 = 0x11_0000;
const BLOCK: u32 = 256;
const OUT: &str = "crates/core/src/unicode/tables.rs";

/// The `<ctype.h>` class masks (spec Appendix C.9: `_ISbit(0..11)`).
const fn isbit(b: u16) -> u16 {
    if b < 8 { (1 << b) << 8 } else { (1 << b) >> 8 }
}
const UPPER: u16 = isbit(0);
const LOWER: u16 = isbit(1);
const ALPHA: u16 = isbit(2);
const DIGIT: u16 = isbit(3);
const XDIGIT: u16 = isbit(4);
const SPACE: u16 = isbit(5);
const PRINT: u16 = isbit(6);
const GRAPH: u16 = isbit(7);
const BLANK: u16 = isbit(8);
const CNTRL: u16 = isbit(9);
const PUNCT: u16 = isbit(10);
const ALNUM: u16 = isbit(11);

struct Record {
    cat: String,
    bidi: String,
    ccc: u8,
    decomp: Option<(bool, Vec<u32>)>,
    upper: Option<u32>,
    lower: Option<u32>,
    title: Option<u32>,
}

fn hex(s: &str) -> Result<u32, String> {
    u32::from_str_radix(s.trim(), 16).map_err(|e| format!("bad code point `{s}`: {e}"))
}

fn read(dir: &Path, name: &str) -> Result<String, String> {
    fs::read_to_string(dir.join(name)).map_err(|e| format!("{name}: {e}"))
}

/// `cp` or `a..b` ; property — as sets per property value.
fn ranges(dir: &Path, name: &str) -> Result<HashMap<String, BTreeSet<u32>>, String> {
    let mut out: HashMap<String, BTreeSet<u32>> = HashMap::new();
    for line in read(dir, name)?.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut f = line.split(';').map(str::trim);
        let (Some(cps), Some(prop)) = (f.next(), f.next()) else { continue };
        let (a, b) = match cps.split_once("..") {
            Some((a, b)) => (hex(a)?, hex(b)?),
            None => (hex(cps)?, hex(cps)?),
        };
        out.entry(prop.to_string()).or_default().extend(a..=b);
    }
    Ok(out)
}

fn unicode_data(dir: &Path) -> Result<BTreeMap<u32, Record>, String> {
    let mut out = BTreeMap::new();
    let mut first: Option<u32> = None;
    for line in read(dir, "UnicodeData.txt")?.lines() {
        let f: Vec<&str> = line.split(';').collect();
        if f.len() < 15 {
            return Err(format!("UnicodeData.txt: short line `{line}`"));
        }
        let cp = hex(f[0])?;
        let opt = |s: &str| if s.is_empty() { Ok(None) } else { hex(s).map(Some) };
        let decomp = if f[5].is_empty() {
            None
        } else {
            let compat = f[5].starts_with('<');
            let body = if compat { f[5].split_once('>').map_or("", |x| x.1) } else { f[5] };
            Some((compat, body.split_whitespace().map(hex).collect::<Result<Vec<_>, _>>()?))
        };
        let rec = || -> Result<Record, String> {
            Ok(Record {
                cat: f[2].to_string(),
                bidi: f[4].to_string(),
                ccc: f[3].parse().map_err(|e| format!("ccc of {cp:04X}: {e}"))?,
                decomp: decomp.clone(),
                upper: opt(f[12])?,
                lower: opt(f[13])?,
                title: opt(f[14])?,
            })
        };
        if f[1].ends_with(", First>") {
            first = Some(cp);
        } else if f[1].ends_with(", Last>") {
            let a = first.take().ok_or("UnicodeData.txt: Last without First")?;
            for c in a..=cp {
                out.insert(c, rec()?);
            }
        } else {
            out.insert(cp, rec()?);
        }
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Prop {
    class: u16,
    /// Bit 0: `combining`; bit 1: `combining_level3`.
    extra: u8,
    width: i8,
    upper: i32,
    lower: i32,
    title: i32,
}

fn props(dir: &Path, ud: &BTreeMap<u32, Record>) -> Result<Vec<Prop>, String> {
    let core = ranges(dir, "DerivedCoreProperties.txt")?;
    let empty = BTreeSet::new();
    let get = |m: &HashMap<String, BTreeSet<u32>>, k: &str| -> BTreeSet<u32> { m.get(k).unwrap_or(&empty).clone() };
    let alphabetic = get(&core, "Alphabetic");
    let uppercase = get(&core, "Uppercase");
    let lowercase = get(&core, "Lowercase");
    let eaw = ranges(dir, "EastAsianWidth.txt")?;
    let wide: BTreeSet<u32> = get(&eaw, "W").union(&get(&eaw, "F")).copied().collect();
    let hst = ranges(dir, "HangulSyllableType.txt")?;
    let jamo0: BTreeSet<u32> = get(&hst, "V").union(&get(&hst, "T")).copied().collect();
    let pcm = get(&ranges(dir, "PropList.txt")?, "Prepended_Concatenation_Mark");
    let nobreak = [0x00A0u32, 0x2007, 0x202F];
    let delta = |to: Option<u32>, c: u32| -> i32 {
        to.map_or(0, |t| i32::try_from(i64::from(t) - i64::from(c)).unwrap_or(0))
    };
    let mut out = Vec::with_capacity(MAX as usize);
    for c in 0..MAX {
        let r = ud.get(&c);
        let g = r.map_or("Cn", |r| r.cat.as_str());
        let mut class = 0u16;
        let (up, lo, ti) = r.map_or((None, None, None), |r| (r.upper, r.lower, r.title));
        if lo.is_some_and(|l| l != c) || uppercase.contains(&c) {
            class |= UPPER;
        }
        if up.is_some_and(|u| u != c) || lowercase.contains(&c) {
            class |= LOWER;
        }
        let ascii_digit = (0x30..=0x39).contains(&c);
        if alphabetic.contains(&c) || (g == "Nd" && !ascii_digit) {
            class |= ALPHA;
        }
        if ascii_digit {
            class |= DIGIT;
        }
        if ascii_digit || (0x41..=0x46).contains(&c) || (0x61..=0x66).contains(&c) {
            class |= XDIGIT;
        }
        let breaking_space = g == "Zs" && !nobreak.contains(&c);
        if breaking_space || g == "Zl" || g == "Zp" || (0x09..=0x0D).contains(&c) {
            class |= SPACE;
        }
        if breaking_space || c == 0x09 {
            class |= BLANK;
        }
        if matches!(g, "Cc" | "Zl" | "Zp") {
            class |= CNTRL;
        }
        if !matches!(g, "Cc" | "Zl" | "Zp" | "Cs" | "Cn") {
            class |= PRINT;
            if !breaking_space {
                class |= GRAPH;
            }
        }
        if class & (ALPHA | DIGIT) != 0 {
            class |= ALNUM;
        }
        if class & GRAPH != 0 && class & ALNUM == 0 {
            class |= PUNCT;
        }
        let mut extra = 0u8;
        if matches!(g, "Mn" | "Mc" | "Me") {
            extra |= 1;
            if r.map_or(0, |r| r.ccc) < 200 {
                extra |= 2;
            }
        }
        let width: i8 = if c == 0 {
            0
        } else if matches!(g, "Cc" | "Cs" | "Zl" | "Zp" | "Cn") {
            -1
        } else if c == 0x00AD || pcm.contains(&c) {
            1
        } else if matches!(g, "Mn" | "Me" | "Cf") || jamo0.contains(&c) {
            0
        } else if wide.contains(&c) || ((0x2E80..=0xA4CF).contains(&c) && c != 0x303F) {
            2
        } else {
            1
        };
        out.push(Prop { class, extra, width, upper: delta(up, c), lower: delta(lo, c), title: delta(ti, c) });
    }
    Ok(out)
}

/// One SpecialCasing.txt entry: (code point, lower, title, upper).
type Special = (u32, Vec<u32>, Vec<u32>, Vec<u32>);

/// Unconditional full mappings of SpecialCasing.txt.
fn special_casing(dir: &Path) -> Result<Vec<Special>, String> {
    let mut out = Vec::new();
    for line in read(dir, "SpecialCasing.txt")?.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(';').map(str::trim).collect();
        // A fifth non-empty field is a condition (language or context): handled in code.
        if f.len() > 5 && !f[4].is_empty() {
            continue;
        }
        let seq = |s: &str| s.split_whitespace().map(hex).collect::<Result<Vec<_>, _>>();
        out.push((hex(f[0])?, seq(f[1])?, seq(f[2])?, seq(f[3])?));
    }
    Ok(out)
}

/// Full case folding (status C and F) where it differs from the code point.
fn case_folding(dir: &Path) -> Result<Vec<(u32, Vec<u32>)>, String> {
    let mut out = Vec::new();
    for line in read(dir, "CaseFolding.txt")?.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(';').map(str::trim).collect();
        if f.len() >= 3 && (f[1] == "C" || f[1] == "F") {
            out.push((hex(f[0])?, f[2].split_whitespace().map(hex).collect::<Result<Vec<_>, _>>()?));
        }
    }
    Ok(out)
}

/// Full compatibility decompositions (recursive; Hangul syllables are algorithmic and left
/// out), with whether any step was a compatibility mapping.
fn decompositions(ud: &BTreeMap<u32, Record>) -> Vec<(u32, bool, Vec<u32>)> {
    fn expand(ud: &BTreeMap<u32, Record>, c: u32, out: &mut Vec<u32>, compat: &mut bool) {
        match ud.get(&c).and_then(|r| r.decomp.as_ref()) {
            Some((k, parts)) => {
                *compat |= *k;
                for &p in parts {
                    expand(ud, p, out, compat);
                }
            }
            None => out.push(c),
        }
    }
    let mut res = Vec::new();
    for (&c, r) in ud {
        if r.decomp.is_some() {
            let mut v = Vec::new();
            let mut compat = false;
            expand(ud, c, &mut v, &mut compat);
            res.push((c, compat, v));
        }
    }
    res
}

fn generate(dir: &Path) -> Result<String, String> {
    let ud = unicode_data(dir)?;
    let props = props(dir, &ud)?;
    // Records, deduplicated; blocks of 256 code points, deduplicated.
    let mut ids: HashMap<Prop, u16> = HashMap::new();
    let mut records: Vec<Prop> = Vec::new();
    let mut per_cp = Vec::with_capacity(props.len());
    for p in &props {
        let id = *ids.entry(*p).or_insert_with(|| {
            records.push(*p);
            u16::try_from(records.len() - 1).unwrap_or(u16::MAX)
        });
        per_cp.push(id);
    }
    let mut block_ids: HashMap<&[u16], u16> = HashMap::new();
    let mut blocks: Vec<&[u16]> = Vec::new();
    let mut stage1 = Vec::new();
    for chunk in per_cp.chunks(BLOCK as usize) {
        let id = *block_ids.entry(chunk).or_insert_with(|| {
            blocks.push(chunk);
            u16::try_from(blocks.len() - 1).unwrap_or(u16::MAX)
        });
        stage1.push(id);
    }
    let mut s = String::new();
    let w = &mut s;
    writeln!(w, "// @generated by `cargo xtask unicode-gen` from spec/unicode/15.1.0 (Unicode 15.1.0).").ok();
    writeln!(w, "// Do not edit. Derivation rules: xtask/src/unicode.rs (D-5.4). Data: Unicode License v3,").ok();
    writeln!(w, "// spec/unicode/15.1.0/license.txt.").ok();
    writeln!(w, "#![allow(clippy::unreadable_literal)]\n").ok();
    writeln!(w, "use super::Props;\n").ok();
    writeln!(w, "/// Code points per stage-2 block.").ok();
    writeln!(w, "pub(crate) const BLOCK_SHIFT: u32 = 8;\n").ok();
    writeln!(w, "/// Stage 1: the block of each 256 code points.").ok();
    writeln!(w, "pub(crate) static STAGE1: [u16; {}] = [", stage1.len()).ok();
    for row in stage1.chunks(24) {
        writeln!(w, "    {},", row.iter().map(u16::to_string).collect::<Vec<_>>().join(", ")).ok();
    }
    writeln!(w, "];\n").ok();
    writeln!(w, "/// Stage 2: the record of each code point within its block.").ok();
    writeln!(w, "pub(crate) static STAGE2: [[u16; 256]; {}] = [", blocks.len()).ok();
    for b in &blocks {
        writeln!(w, "    [").ok();
        for row in b.chunks(32) {
            writeln!(w, "        {},", row.iter().map(u16::to_string).collect::<Vec<_>>().join(", ")).ok();
        }
        writeln!(w, "    ],").ok();
    }
    writeln!(w, "];\n").ok();
    writeln!(w, "/// The distinct property records.").ok();
    writeln!(w, "pub(crate) static RECORDS: [Props; {}] = [", records.len()).ok();
    for r in &records {
        writeln!(w, "    Props {{ class: {:#06x}, extra: {}, width: {}, upper: {}, lower: {}, title: {} }},", r.class, r.extra, r.width, r.upper, r.lower, r.title).ok();
    }
    writeln!(w, "];\n").ok();
    // Variable-length sequences share one pool.
    let mut pool: Vec<u32> = Vec::new();
    let mut put = |v: &[u32]| -> (u32, u8) {
        let start = u32::try_from(pool.len()).unwrap_or(u32::MAX);
        pool.extend_from_slice(v);
        (start, u8::try_from(v.len()).unwrap_or(u8::MAX))
    };
    let special = special_casing(dir)?;
    writeln!(w, "/// Unconditional full case mappings (SpecialCasing.txt): code point, then (start, len) in").ok();
    writeln!(w, "/// `POOL` of its lowercase, titlecase and uppercase. Sorted by code point.").ok();
    writeln!(w, "pub(crate) static SPECIAL: [(u32, [(u32, u8); 3]); {}] = [", special.len()).ok();
    for (c, lo, ti, up) in &special {
        let (a, b, d) = (put(lo), put(ti), put(up));
        writeln!(w, "    ({c:#x}, [({}, {}), ({}, {}), ({}, {})]),", a.0, a.1, b.0, b.1, d.0, d.1).ok();
    }
    writeln!(w, "];\n").ok();
    let fold = case_folding(dir)?;
    writeln!(w, "/// Full case folding (CaseFolding.txt, status C and F): code point, (start, len) in `POOL`.").ok();
    writeln!(w, "pub(crate) static FOLD: [(u32, (u32, u8)); {}] = [", fold.len()).ok();
    for (c, v) in &fold {
        let (a, n) = put(v);
        writeln!(w, "    ({c:#x}, ({a}, {n})),").ok();
    }
    writeln!(w, "];\n").ok();
    let dec = decompositions(&ud);
    writeln!(w, "/// Full decompositions (recursive), Hangul excepted: code point, compatibility, (start, len)").ok();
    writeln!(w, "/// in `POOL`. Sorted by code point.").ok();
    writeln!(w, "pub(crate) static DECOMP: [(u32, bool, (u32, u8)); {}] = [", dec.len()).ok();
    for (c, compat, v) in &dec {
        let (a, n) = put(v);
        writeln!(w, "    ({c:#x}, {compat}, ({a}, {n})),").ok();
    }
    writeln!(w, "];\n").ok();
    writeln!(w, "/// Canonical combining classes other than 0, as sorted ranges (first, last, class).").ok();
    let mut ccc: Vec<(u32, u32, u8)> = Vec::new();
    for (&c, r) in &ud {
        if r.ccc == 0 {
            continue;
        }
        match ccc.last_mut() {
            Some(l) if l.1 + 1 == c && l.2 == r.ccc => l.1 = c,
            _ => ccc.push((c, c, r.ccc)),
        }
    }
    writeln!(w, "pub(crate) static CCC: [(u32, u32, u8); {}] = [", ccc.len()).ok();
    for (a, b, k) in &ccc {
        writeln!(w, "    ({a:#x}, {b:#x}, {k}),").ok();
    }
    writeln!(w, "];\n").ok();
    writeln!(w, "/// The sequence pool.").ok();
    writeln!(w, "pub(crate) static POOL: [u32; {}] = [", pool.len()).ok();
    for row in pool.chunks(12) {
        writeln!(w, "    {},", row.iter().map(|c| format!("{c:#x}")).collect::<Vec<_>>().join(", ")).ok();
    }
    writeln!(w, "];").ok();
    Ok(s)
}

// ------------------------------------------------------------------------------ IDNA (D-16.4)

// The IDNA tables (UTS #46 15.1.0), derived here from the pinned public files only (COLD-TABLE-IDNA):
//
// * `IdnaMappingTable.txt`: the status of every code point, the IDNA2008 flag (`NV8`/`XV8`) and the mapping;
// * `UnicodeData.txt`: Bidi_Class (RFC 5893), General_Category M* (the leading-mark rule of UTS #46 4.1 step 4),
//   canonical decompositions (NFC step of UTS #46 4);
// * `ArabicShaping.txt`: Joining_Type (RFC 5892 A.1), with the derived rule "unlisted Mn, Me, Cf are T";
// * `DerivedNormalizationProps.txt`: Full_Composition_Exclusion (which decompositions compose back).
//
// Form (decisions in D-16.12):
// * One two-level trie answers every per-code-point property in a single lookup: the 16-bit property word is
//   `status | excluded << 4 | bidi << 5 | joining << 9 | mark << 12`; level 2 holds palette indices (bytes),
//   level 1 holds block numbers; blocks are deduplicated and the block size is the one giving the least data.
// * Mappings are runs (start, length, delta) when consecutive code points map to consecutive single code points,
//   and a deduplicated pool of code point strings otherwise.
// * Decompositions: sorted keys with end offsets into one data array; compositions: one packed u64 per pair.
const IDNA_OUT: &str = "crates/core/src/names/dns/idna_tables.rs";

/// Status codes of the generated table (`idna.rs` reads them).
fn idna_status(s: &str) -> Result<u8, String> {
    Ok(match s {
        "valid" => 0,
        "ignored" => 1,
        "mapped" => 2,
        "deviation" => 3,
        "disallowed" => 4,
        "disallowed_STD3_valid" => 5,
        "disallowed_STD3_mapped" => 6,
        _ => return Err(format!("IdnaMappingTable.txt: unknown status `{s}`")),
    })
}

/// Bidi class codes (`idna.rs`: L=0 R=1 AL=2 EN=3 ES=4 ET=5 AN=6 CS=7 NSM=8 BN=9 ON=10, others 11).
fn bidi_code(c: &str) -> u8 {
    match c {
        "L" => 0,
        "R" => 1,
        "AL" => 2,
        "EN" => 3,
        "ES" => 4,
        "ET" => 5,
        "AN" => 6,
        "CS" => 7,
        "NSM" => 8,
        "BN" => 9,
        "ON" => 10,
        _ => 11,
    }
}

/// Joining_Type codes: U=0 T=1 L=2 R=3 D=4 C=5.
fn join_code(t: &str) -> Result<u8, String> {
    Ok(match t {
        "U" => 0,
        "T" => 1,
        "L" => 2,
        "R" => 3,
        "D" => 4,
        "C" => 5,
        _ => return Err(format!("ArabicShaping.txt: unknown joining type `{t}`")),
    })
}

fn idna_range(cps: &str) -> Result<(u32, u32), String> {
    match cps.split_once("..") {
        Some((a, b)) => Ok((hex(a)?, hex(b)?)),
        None => Ok((hex(cps)?, hex(cps)?)),
    }
}

/// Write `name` as a static array, `per` values per line.
fn idna_array<T: std::fmt::LowerHex>(w: &mut String, doc: &str, name: &str, ty: &str, vals: &[T], per: usize) {
    writeln!(w, "\n/// {doc}").ok();
    writeln!(w, "pub(crate) static {name}: [{ty}; {}] = [", vals.len()).ok();
    for line in vals.chunks(per) {
        w.push_str("   ");
        for v in line {
            write!(w, " 0x{v:x},").ok();
        }
        w.push('\n');
    }
    w.push_str("];\n");
}

/// The canonical (non-compatibility) full decomposition of `c`.
fn canonical_full(ud: &BTreeMap<u32, Record>, c: u32, out: &mut Vec<u32>) {
    match ud.get(&c).and_then(|r| r.decomp.as_ref()) {
        Some((false, d)) => {
            for &x in d {
                canonical_full(ud, x, out);
            }
        }
        _ => out.push(c),
    }
}

fn generate_idna(dir: &Path) -> Result<String, String> {
    let ud = unicode_data(dir)?;
    let excluded = ranges(dir, "DerivedNormalizationProps.txt")?
        .remove("Full_Composition_Exclusion")
        .ok_or("DerivedNormalizationProps.txt: no Full_Composition_Exclusion")?;
    let mut shaping: HashMap<u32, u8> = HashMap::new();
    for line in read(dir, "ArabicShaping.txt")?.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(';').map(str::trim).collect();
        let (Some(cp), Some(t)) = (f.first(), f.get(2)) else { return Err(format!("ArabicShaping.txt: `{line}`")) };
        shaping.insert(hex(cp)?, join_code(t)?);
    }

    // Status, IDNA2008 flag and mapping from the mapping table; the other properties from the UCD files.
    let mut status: Vec<Option<u8>> = vec![None; MAX as usize];
    let mut map: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for line in read(dir, "IdnaMappingTable.txt")?.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(';').map(str::trim).collect();
        let (Some(cps), Some(st)) = (f.first(), f.get(1)) else { return Err(format!("IdnaMappingTable.txt: `{line}`")) };
        let (a, b) = idna_range(cps)?;
        let code = idna_status(st)? | if f.get(3).is_some_and(|x| !x.is_empty()) { 0x10 } else { 0 };
        let m = f.get(2).map_or(Ok(Vec::new()), |m| m.split_whitespace().map(hex).collect::<Result<Vec<_>, _>>())?;
        for c in a..=b {
            let slot = status.get_mut(c as usize).ok_or("IdnaMappingTable.txt: code point out of range")?;
            if slot.replace(code).is_some() {
                return Err(format!("IdnaMappingTable.txt: {c:04X} listed twice"));
            }
            if !m.is_empty() {
                map.insert(c, m.clone());
            }
        }
    }
    let mut props: Vec<u16> = Vec::with_capacity(MAX as usize);
    for c in 0..MAX {
        let st = status[c as usize].ok_or_else(|| format!("IdnaMappingTable.txt: {c:04X} not listed"))?;
        let rec = ud.get(&c);
        let bidi = rec.map_or(0, |r| bidi_code(&r.bidi));
        let mark = rec.is_some_and(|r| r.cat.starts_with('M'));
        let join = match shaping.get(&c) {
            Some(&j) => j,
            None => u8::from(rec.is_some_and(|r| matches!(r.cat.as_str(), "Mn" | "Me" | "Cf"))),
        };
        props.push(u16::from(st) | u16::from(bidi) << 5 | u16::from(join) << 9 | u16::from(mark) << 12);
    }

    // The trie: palette of distinct property words, then the block size with the least data.
    let mut palette: Vec<u16> = props.clone();
    palette.sort_unstable();
    palette.dedup();
    if palette.len() > 256 {
        return Err(format!("IDNA property palette has {} entries (> 256)", palette.len()));
    }
    let index: HashMap<u16, u8> = palette.iter().enumerate().map(|(i, &p)| (p, i as u8)).collect();
    let bytes: Vec<u8> = props.iter().map(|p| index[p]).collect();
    let mut best: Option<(usize, u32, Vec<u16>, Vec<u8>)> = None;
    for shift in 4..=9u32 {
        let bs = 1usize << shift;
        let mut seen: HashMap<&[u8], u16> = HashMap::new();
        let (mut l1, mut l2): (Vec<u16>, Vec<u8>) = (Vec::new(), Vec::new());
        for blk in bytes.chunks(bs) {
            let n = u16::try_from(seen.len()).map_err(|_| "too many IDNA trie blocks")?;
            let id = *seen.entry(blk).or_insert_with(|| {
                l2.extend_from_slice(blk);
                n
            });
            l1.push(id);
        }
        let size = l1.len() * 2 + l2.len();
        if best.as_ref().is_none_or(|b| size < b.0) {
            best = Some((size, shift, l1, l2));
        }
    }
    let (trie_size, shift, l1, l2) = best.ok_or("no trie")?;

    // Mappings: delta runs and a string pool.
    let (mut starts, mut lens, mut args): (Vec<u32>, Vec<u8>, Vec<u32>) = (Vec::new(), Vec::new(), Vec::new());
    let mut pool: Vec<u32> = Vec::new();
    let mut pooled: HashMap<Vec<u32>, (usize, usize)> = HashMap::new();
    let mut maxmap = 0usize;
    for (&c, v) in &map {
        maxmap = maxmap.max(v.len());
        if let [t] = v[..] {
            let delta = t.wrapping_sub(c);
            let extends = match (starts.last(), lens.last(), args.last()) {
                (Some(&s), Some(&l), Some(&a)) => l != 0 && l < 255 && a == delta && s + u32::from(l) == c,
                _ => false,
            };
            if extends {
                if let Some(l) = lens.last_mut() {
                    *l += 1;
                }
            } else {
                starts.push(c);
                lens.push(1);
                args.push(delta);
            }
        } else {
            let (off, len) = *pooled.entry(v.clone()).or_insert_with(|| {
                let off = pool.len();
                pool.extend_from_slice(v);
                (off, v.len())
            });
            if off >= 1 << 24 {
                return Err("IDNA mapping pool too large".into());
            }
            starts.push(c);
            lens.push(0);
            args.push((off as u32) << 8 | len as u32);
        }
    }

    // Canonical decompositions (fully expanded, Hangul is algorithmic) and compositions.
    let (mut dkeys, mut dends, mut ddata): (Vec<u32>, Vec<u16>, Vec<u32>) = (Vec::new(), Vec::new(), Vec::new());
    let mut compose: Vec<u64> = Vec::new();
    for (&c, r) in &ud {
        let Some((false, d)) = &r.decomp else { continue };
        let mut full = Vec::new();
        canonical_full(&ud, c, &mut full);
        dkeys.push(c);
        ddata.extend_from_slice(&full);
        dends.push(u16::try_from(ddata.len()).map_err(|_| "IDNA decomposition data too large")?);
        if let [a, b] = d[..] {
            if !excluded.contains(&c) {
                compose.push(u64::from(a) << 42 | u64::from(b) << 21 | u64::from(c));
            }
        }
    }
    compose.sort_unstable();
    if compose.iter().any(|&k| k >> 63 != 0) || ud.keys().any(|&c| c >= 1 << 21) {
        return Err("IDNA composition key overflow".into());
    }

    let mut w = String::new();
    w.push_str("// @generated by `cargo xtask unicode-gen` from spec/unicode/15.1.0 (Unicode 15.1.0: IdnaMappingTable.txt,\n");
    w.push_str("// UnicodeData.txt, ArabicShaping.txt, DerivedNormalizationProps.txt). Do not edit. Rules: xtask/src/unicode.rs\n");
    w.push_str("// (D-16.4, COLD-TABLE-IDNA). Data: Unicode License v3, spec/unicode/15.1.0/license.txt.\n");
    w.push_str("//! UTS #46 tables (D-16.4): one property trie, mapping runs, decompositions, compositions.\n");
    w.push_str("#![allow(clippy::unreadable_literal)]\n");
    writeln!(w, "\n/// log2 of the trie block size (the size with the least data was chosen: {trie_size} bytes).").ok();
    writeln!(w, "pub(crate) const SHIFT: u32 = {shift};").ok();
    writeln!(w, "\n/// The longest mapping, in code points.\npub(crate) const MAX_MAP: usize = {maxmap};").ok();
    idna_array(&mut w, "Distinct property words: `status | excluded << 4 | bidi << 5 | joining << 9 | mark << 12`.", "PALETTE", "u16", &palette, 12);
    idna_array(&mut w, "Trie level 1: block number of each `1 << SHIFT` code points.", "TRIE1", "u16", &l1, 16);
    idna_array(&mut w, "Trie level 2: palette index of each code point of each block.", "TRIE2", "u8", &l2, 24);
    idna_array(&mut w, "Mapping runs, first code point (sorted).", "MAP_START", "u32", &starts, 12);
    idna_array(&mut w, "Mapping run length (a delta run) or 0 (one entry of MAP_POOL).", "MAP_LEN", "u8", &lens, 24);
    idna_array(&mut w, "Delta run: the wrapping delta of the target; pool entry: `offset << 8 | length`.", "MAP_ARG", "u32", &args, 12);
    idna_array(&mut w, "Mapping strings.", "MAP_POOL", "u32", &pool, 12);
    idna_array(&mut w, "Canonical decomposition: code points with one (sorted).", "DECOMP_KEY", "u32", &dkeys, 12);
    idna_array(&mut w, "Canonical decomposition: end offset in DECOMP_DATA of each key.", "DECOMP_END", "u16", &dends, 16);
    idna_array(&mut w, "Canonical decompositions, fully expanded.", "DECOMP_DATA", "u32", &ddata, 12);
    idna_array(&mut w, "Primary composites: `first << 42 | second << 21 | composite`, sorted.", "COMPOSE", "u64", &compose, 6);
    Ok(w)
}

fn write_out(root: &Path, out: &str, text: &str, check: bool) -> Result<(), String> {
    let path = root.join(out);
    let old = fs::read_to_string(&path).unwrap_or_default();
    if old == text {
        println!("unicode-gen: up to date ({out})");
        return Ok(());
    }
    if check {
        return Err(format!("unicode-gen: {out} is stale; run `cargo xtask unicode-gen`"));
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&path, text).map_err(|e| e.to_string())?;
    println!("unicode-gen: wrote {out}");
    Ok(())
}

pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let dir = root.join("spec/unicode/15.1.0");
    write_out(root, OUT, &generate(&dir)?, check)?;
    write_out(root, IDNA_OUT, &generate_idna(&dir)?, check)
}
