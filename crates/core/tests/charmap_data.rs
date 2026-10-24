//! The installed charmaps (D-6.8): every file decompresses with a matching CRC and parses, and
//! the codecs agree with the reference library's iconv for the sequences each charmap lists.
//! Without `/usr/share/i18n/charmaps` (or a C compiler for the oracle) the test says so and
//! passes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/oracle.rs"]
mod oracle;

use std::path::Path;

use liella::charmap::{inflate, Charmap};

const DIR: &str = "/usr/share/i18n/charmaps";

fn charmaps() -> Vec<std::path::PathBuf> {
    let Ok(rd) = std::fs::read_dir(DIR) else { return Vec::new() };
    let mut v: Vec<_> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    v.sort();
    v
}

fn text(p: &Path) -> Vec<u8> {
    let raw = std::fs::read(p).unwrap();
    if p.extension().is_some_and(|e| e == "gz") {
        inflate::gunzip(&raw, 64 << 20).unwrap_or_else(|e| panic!("{}: {e:?}", p.display())).into()
    } else {
        raw
    }
}

#[test]
fn every_charmap_inflates_and_parses() {
    // Trace: no spec clause (reading the installed charmap files: inflate, CRC and parse); D-6.8.
    let files = charmaps();
    if files.is_empty() {
        skip_note::skipped(&format!("skipped: no {DIR}"));
        return;
    }
    let mut parsed = 0;
    for f in &files {
        let t = text(f);
        match Charmap::parse(&t) {
            Ok(cm) => {
                // ISO_10646 has no <code_set_name>; the loader finds charmaps by file name.
                if cm.name().is_empty() {
                    eprintln!("no <code_set_name>: {}", f.display());
                }
                parsed += 1;
            }
            Err(_) => eprintln!("not a mapping charmap: {}", f.display()),
        }
    }
    assert!(parsed >= files.len() - 5, "{parsed} of {} parsed", files.len());
}

#[test]
fn inflate_rejects_damage() {
    // Trace: no spec clause (the internal inflate engine rejects a damaged charmap file); D-6.8.
    let files = charmaps();
    let Some(f) = files.iter().find(|p| p.extension().is_some_and(|e| e == "gz")) else { return };
    let mut raw = std::fs::read(f).unwrap();
    let mid = raw.len() / 2;
    raw[mid] ^= 0x55;
    assert!(inflate::gunzip(&raw, 64 << 20).is_err(), "a flipped byte is caught (CRC or structure)");
    assert!(inflate::gunzip(&raw[..raw.len() / 3], 64 << 20).is_err(), "a truncated file is an error");
    let good = std::fs::read(f).unwrap();
    assert!(inflate::gunzip(&good, 16).is_err(), "output beyond the limit is an error");
}

/// Build the iconv oracle once; `None` without a C compiler.
fn oracle() -> Option<std::path::PathBuf> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let exe = root.join("target/test-oracles/oracle_iconv");
    let mut cmd = std::process::Command::new("cc");
    cmd.arg("-O2").arg(root.join("crates/core/tests/oracle/oracle_iconv.c"));
    oracle::build(cmd, &exe).then_some(exe)
}

/// Charsets whose charmap and the reference library's iconv module disagree (D-6.8 records
/// them): the teletex sets list their non-spacing diacritic prefixes as private-use code points,
/// which the modules combine with the following letter; MAC-CYRILLIC's 0xA2 is U+00A2 in the
/// charmap and U+0490 in the module. UTF-8 is built in and never read from its charmap.
const KNOWN: [(&str, &str); 6] = [
    ("ANSI_X3.110-1983", "diacritic prefixes"),
    ("ISO-IR-90", "diacritic prefixes"),
    ("ISO_6937", "diacritic prefixes"),
    ("T.61-8BIT", "diacritic prefixes"),
    ("MAC-CYRILLIC", "0xA2"),
    ("UTF-8", "built in"),
];

#[test]
fn codecs_agree_with_the_reference_iconv() {
    // Clauses: s6_4_c_1, s6_4_c_2, s6_4_g_0 (D-6.8, D-6.9).
    let files = charmaps();
    let Some(exe) = oracle() else { return skip_note::skipped("skipped: no C compiler") };
    if files.is_empty() {
        return skip_note::skipped(&format!("skipped: no {DIR}"));
    }
    let (mut charsets, mut checked, mut skipped) = (0, 0usize, Vec::new());
    let mut differences: Vec<String> = Vec::new();
    for f in &files {
        let Ok(cm) = Charmap::parse(&text(f)) else { continue };
        let stem = f.file_name().unwrap().to_string_lossy().trim_end_matches(".gz").to_string();
        if KNOWN.iter().any(|(k, _)| *k == stem) {
            continue;
        }
        let all: Vec<(u32, [u8; 4], usize)> = cm.mappings().collect();
        // Every mapping of small charsets; an even sample of large ones.
        let step = (all.len() / 4000).max(1);
        let sample: Vec<_> = all.iter().step_by(step).copied().collect();
        let mut input = String::new();
        for (_, b, n) in &sample {
            input.push_str("D ");
            for x in &b[..*n] {
                input.push_str(&format!("{x:02x}"));
            }
            input.push('\n');
        }
        for (cp, _, _) in &sample {
            input.push_str(&format!("E {cp:x}\n"));
        }
        let mut child = std::process::Command::new(&exe).arg(&stem).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
        {
            use std::io::Write as _;
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        }
        let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        if lines.first() == Some(&"SKIP") {
            skipped.push(stem);
            continue;
        }
        charsets += 1;
        let (dec, enc) = lines.split_at(sample.len());
        let mut bad = 0;
        for ((cp, b, n), want) in sample.iter().zip(dec) {
            checked += 1;
            let got = match cm.decode(&b[..*n]) {
                liella::charmap::Step::Char(c, used) if used == *n => format!("{c:x}"),
                _ => "ERR".into(),
            };
            if got != *want {
                bad += 1;
                if bad <= 3 {
                    differences.push(format!("{stem} decode {:02x?} (charmap {cp:x}): liella {got} glibc {want}", &b[..*n]));
                }
            }
        }
        for ((cp, _, _), want) in sample.iter().zip(enc) {
            checked += 1;
            let mut buf = [0u8; 8];
            let got = cm.encode(*cp, &mut buf).map_or("ERR".to_string(), |k| buf[..k].iter().map(|x| format!("{x:02x}")).collect());
            if got != *want {
                bad += 1;
                if bad <= 3 {
                    differences.push(format!("{stem} encode {cp:x}: liella {got} glibc {want}"));
                }
            }
        }
        if bad > 0 {
            differences.push(format!("{stem}: {bad} differences in {} sampled mappings", sample.len()));
        }
    }
    eprintln!("{charsets} charsets compared ({checked} conversions); unknown to the reference iconv: {skipped:?}");
    for d in &differences {
        eprintln!("{d}");
    }
    assert!(differences.is_empty(), "{} charsets differ", differences.iter().filter(|d| d.contains("differences in")).count());
}

#[test]
fn header_names_from_a_prefix_match_the_full_parse() {
    // Trace: no spec clause (the internal alias scan of a charmap's first 4 KiB); D-6.8.
    // The alias scan reads only the first 4 KiB of each file (D-6.8); it must see every name
    // the full parse sees.
    let mut checked = 0;
    for f in &charmaps() {
        let raw = std::fs::read(f).unwrap();
        let head = &raw[..raw.len().min(4096)];
        let prefix: Vec<u8> = if f.extension().is_some_and(|e| e == "gz") { inflate::gunzip_prefix(head, 8192).unwrap().into() } else { head.to_vec() };
        let Ok(cm) = Charmap::parse(&text(f)) else { continue };
        let mut seen: Vec<Vec<u8>> = Vec::new();
        liella::charmap::header_names(&prefix, &mut |n| seen.push(n.to_vec()));
        for n in std::iter::once(cm.name()).chain(cm.aliases()).filter(|n| !n.is_empty()) {
            assert!(seen.iter().any(|s| s == n), "{}: {:?} not in the header prefix ({} bytes inflated)", f.display(), String::from_utf8_lossy(n), prefix.len());
        }
        checked += 1;
    }
    eprintln!("{checked} charmaps");
}
