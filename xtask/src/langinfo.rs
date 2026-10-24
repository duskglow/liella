//! `cargo xtask locale-gen [--check]`: generate the built-in locales' `nl_langinfo` items
//! (`crates/core/src/locale/builtin_items.rs`) from `spec/abi/langinfo.tsv` (D-7.10).
//!
//! The generated module holds one pool of bytes (every distinct item string once; a string that
//! occurs inside another is not stored again), one compact record per item of "C" (`W` word,
//! `S` offset and length in the pool, `No` nothing recorded) and, for "C.UTF-8", only the items
//! whose value differs from "C": it is "C" with a few items changed. The statics the locale
//! code reads, one `[R]` per category and locale indexed by item number, are expanded from
//! those records by `const fn`s at compile time (no build script, no start-up work). The records
//! hold no pointers (a word, or an offset and length into the pool), so the tables need no dynamic
//! relocations and stay shared read-only; `Tab::Packed` turns a record into an `Item` on read.
//!
//! Rules of the expansion (they are what `builtin_items_equal_the_spec_rows` tests):
//! * a table has `highest recorded index + 1` entries; an index the table does not record
//!   (glibc's own class and collation tables) holds no data, the empty slice;
//! * a string value is stored as the spec records it, and when its last byte is not a NUL one
//!   is added, so a reader that scans a C string stays inside the item.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

const TSV: &str = "spec/abi/langinfo.tsv";
const OUT: &str = "crates/core/src/locale/builtin_items.rs";

/// One item's recorded value.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Rec {
    Word(u32),
    Bytes(Vec<u8>),
    Nothing,
}

fn parse_hex(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err(format!("odd hex length: {s}"));
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).map_err(|e| format!("{s}: {e}")))
        .collect()
}

fn rec(kind: &str, value: &str) -> Result<Rec, String> {
    match kind {
        "W" => value.parse().map(Rec::Word).map_err(|e| format!("word {value}: {e}")),
        "P" => {
            let mut b = parse_hex(value)?;
            if b.is_empty() {
                return Ok(Rec::Nothing);
            }
            if b.last() != Some(&0) {
                b.push(0);
            }
            Ok(Rec::Bytes(b))
        }
        k => Err(format!("kind {k}")),
    }
}

/// The byte pool: longest strings first, a string already inside the pool costs nothing.
struct Pool {
    bytes: Vec<u8>,
}

impl Pool {
    fn find(&self, s: &[u8]) -> Option<usize> {
        self.bytes.windows(s.len()).position(|w| w == s)
    }

    fn build(all: &[&Vec<u8>]) -> Pool {
        let mut sorted: Vec<&Vec<u8>> = all.to_vec();
        sorted.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
        sorted.dedup();
        let mut p = Pool { bytes: Vec::new() };
        for s in sorted {
            if p.find(s).is_none() {
                p.bytes.extend_from_slice(s);
            }
        }
        p
    }
}

fn literal(bytes: &[u8]) -> String {
    let mut s = String::from("*b\"");
    for &b in bytes {
        match b {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            0x20..=0x7e => s.push(char::from(b)),
            _ => {
                let _ = write!(s, "\\x{b:02x}");
            }
        }
    }
    s.push('"');
    s
}

fn generate(root: &Path) -> Result<String, String> {
    let text = fs::read_to_string(root.join(TSV)).map_err(|e| format!("{TSV}: {e}"))?;
    // category -> index -> (C, C.UTF-8)
    let mut cats: BTreeMap<u32, BTreeMap<usize, (Rec, Rec)>> = BTreeMap::new();
    for (n, line) in text.lines().enumerate().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 6 {
            return Err(format!("{TSV}:{}: {} fields", n + 1, f.len()));
        }
        let cat: u32 = f[0].parse().map_err(|e| format!("{TSV}:{}: {e}", n + 1))?;
        let index: usize = f[1].parse().map_err(|e| format!("{TSV}:{}: {e}", n + 1))?;
        let both = (rec(f[3], f[4])?, rec(f[3], f[5])?);
        if cats.entry(cat).or_default().insert(index, both).is_some() {
            return Err(format!("{TSV}:{}: duplicate row", n + 1));
        }
    }
    let strings: Vec<&Vec<u8>> = cats
        .values()
        .flat_map(|t| t.values())
        .flat_map(|(c, u)| [c, u])
        .filter_map(|r| if let Rec::Bytes(b) = r { Some(b) } else { None })
        .collect();
    let pool = Pool::build(&strings);
    if pool.bytes.len() > usize::from(u16::MAX) {
        return Err("pool does not fit 16-bit offsets".into());
    }
    let r = |x: &Rec| -> String {
        match x {
            Rec::Word(v) => format!("W({v})"),
            Rec::Nothing => "No".to_string(),
            Rec::Bytes(b) => format!("S({},{})", pool.find(b).unwrap_or(0), b.len()),
        }
    };

    let mut w = String::new();
    w.push_str("//! GENERATED by `cargo xtask locale-gen` from `spec/abi/langinfo.tsv` (D-7.10): do not edit.\n");
    w.push_str("//!\n//! The built-in locales' `nl_langinfo` items: a byte pool, one record per item of \"C\", the\n");
    w.push_str("//! items in which \"C.UTF-8\" differs, and the per-category `[R]` statics expanded from them\n");
    w.push_str("//! at compile time. See `xtask/src/langinfo.rs` for the rules.\n\n");
    w.push_str("use super::{Item, Tab};\n\n");
    w.push_str("/// One recorded item: a word, a string (offset and length in `POOL`) or nothing. No pointers: the\n/// tables of these are plain data in the shared library (no dynamic relocations, LOCALE-ITEMS-RELOC).\n");
    w.push_str("#[derive(Clone, Copy, Debug)]\npub enum R {\n    W(u32),\n    S(u16, u16),\n    No,\n}\nuse R::{No, S, W};\n\n");
    let _ = writeln!(w, "#[rustfmt::skip]\nstatic POOL: [u8; {}] = {};\n", pool.bytes.len(), literal(&pool.bytes));
    w.push_str(
        "impl R {\n    /// The item this record stands for.\n    #[inline]\n    #[allow(clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects)]\n    pub const fn item(self) -> Item {\n        match self {\n            R::W(v) => Item::Word(v),\n            R::S(o, l) => Item::Bytes(POOL.split_at(o as usize).1.split_at(l as usize).0),\n            R::No => Item::Bytes(&[]),\n        }\n    }\n}\n\n",
    );
    w.push_str(
        "/// `base` with the records of `delta` (index, record) replaced.\n/// The allow: a const fn loop, so `?`/checked forms are unavailable; `i < delta.len()` bounds the `+= 1`.\n#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]\nconst fn patched<const N: usize>(base: [R; N], delta: &[(usize, R)]) -> [R; N] {\n    let mut t = base;\n    let mut i = 0;\n    while i < delta.len() {\n        t[delta[i].0] = delta[i].1;\n        i += 1;\n    }\n    t\n}\n\n",
    );

    for (cat, items) in &cats {
        let len = items.keys().max().map_or(0, |m| m + 1);
        let mut c = vec![Rec::Nothing; len];
        let mut u = vec![Rec::Nothing; len];
        for (i, (a, b)) in items {
            c[*i] = a.clone();
            u[*i] = b.clone();
        }
        let _ = writeln!(w, "#[rustfmt::skip]\nconst B_{cat}: [R; {len}] = [");
        for chunk in c.chunks(8) {
            let line: Vec<String> = chunk.iter().map(&r).collect();
            let _ = writeln!(w, "    {},", line.join(", "));
        }
        w.push_str("];\n");
        let delta: Vec<String> = (0..len).filter(|&i| c[i] != u[i]).map(|i| format!("({i}, {})", r(&u[i]))).collect();
        let _ = writeln!(w, "#[rustfmt::skip]\npub(crate) static C_{cat}: [R; {len}] = B_{cat};");
        let _ = writeln!(w, "#[rustfmt::skip]\npub(crate) static C_UTF8_{cat}: [R; {len}] = patched(B_{cat}, &[{}]);\n", delta.join(", "));
    }

    w.push_str("/// The item table of category number `cat` (the C number; 6 is `LC_ALL`, none) of \"C\" or \"C.UTF-8\".\n");
    w.push_str("pub(crate) fn items(utf8: bool, cat: i32) -> Tab {\n    match (utf8, cat) {\n");
    for cat in cats.keys() {
        let _ = writeln!(w, "        (false, {cat}) => Tab::Packed(&C_{cat}),\n        (true, {cat}) => Tab::Packed(&C_UTF8_{cat}),");
    }
    w.push_str("        _ => Tab::Items(&[]),\n    }\n}\n");
    Ok(w)
}

pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let text = generate(root)?;
    let path = root.join(OUT);
    if check {
        let have = fs::read_to_string(&path).map_err(|e| format!("{OUT}: {e}"))?;
        if have != text {
            return Err(format!("{OUT} is stale: run `cargo xtask locale-gen`"));
        }
        println!("locale-gen: {OUT} is current");
        return Ok(());
    }
    fs::write(&path, text).map_err(|e| format!("{OUT}: {e}"))?;
    println!("locale-gen: wrote {OUT}");
    Ok(())
}
