//! `locale::name::lookup_alias` against a plain reference: the line-by-line parse the function
//! must agree with (D-7.7: the alias is consulted for a name the archive and directories do not
//! hold, whatever the file holds). The scan rejects a line by its first non-blank byte and finds lines a word at a
//! time; this pins that neither changes an answer.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]

use liella::locale::name::lookup_alias;

/// The definition: split into lines, cut at `#`, the first two blank-separated fields are the
/// key and the value, the key is matched without regard to case.
fn reference<'t>(text: &'t [u8], alias: &[u8]) -> Option<&'t [u8]> {
    for line in text.split(|&b| b == b'\n') {
        let line = line.split(|&b| b == b'#').next().unwrap_or(&[]);
        let mut fields = line.split(|b| b.is_ascii_whitespace()).filter(|f| !f.is_empty());
        let (Some(key), Some(value)) = (fields.next(), fields.next()) else { continue };
        if key.eq_ignore_ascii_case(alias) {
            return Some(value);
        }
    }
    None
}

const FILE: &[u8] = b"# Locale name alias data base.\n#\n\n\
bokmal\t\tnb_NO.ISO-8859-1\n\
japanese.euc\tja_JP.eucJP\n\
  indented  de_DE.UTF-8   # trailing comment\n\
English\ten_US.ISO-8859-1\r\n\
ja_JP ja_JP.eucJP\n\
nokey#comment value\n\
novalue\n\
tab\x0bvt x\n\
\x0cformfeed ff_FF\n\
last nl_NL.ISO-8859-1";

#[test]
fn aliases_resolve_and_others_do_not() {
    // Trace: no spec clause (internal alias-file scan; D-7.7 order and results unchanged).
    assert_eq!(lookup_alias(FILE, b"bokmal"), Some(&b"nb_NO.ISO-8859-1"[..]));
    assert_eq!(lookup_alias(FILE, b"BOKMAL"), Some(&b"nb_NO.ISO-8859-1"[..]));
    assert_eq!(lookup_alias(FILE, b"japanese.euc"), Some(&b"ja_JP.eucJP"[..]));
    assert_eq!(lookup_alias(FILE, b"ja_jp"), Some(&b"ja_JP.eucJP"[..]));
    assert_eq!(lookup_alias(FILE, b"indented"), Some(&b"de_DE.UTF-8"[..]));
    assert_eq!(lookup_alias(FILE, b"english"), Some(&b"en_US.ISO-8859-1"[..]));
    assert_eq!(lookup_alias(FILE, b"formfeed"), Some(&b"ff_FF"[..]));
    assert_eq!(lookup_alias(FILE, b"last"), Some(&b"nl_NL.ISO-8859-1"[..]));
    // Not aliases: a name with a codeset, a comment, a key without value, the empty name.
    assert_eq!(lookup_alias(FILE, b"en_US.UTF-8"), None);
    assert_eq!(lookup_alias(FILE, b"nokey"), None);
    assert_eq!(lookup_alias(FILE, b"novalue"), None);
    assert_eq!(lookup_alias(FILE, b"Locale"), None);
    assert_eq!(lookup_alias(FILE, b""), None);
    assert_eq!(lookup_alias(b"", b"x"), None);
}

#[test]
fn scan_agrees_with_the_reference_on_generated_files() {
    // Trace: no spec clause (internal alias-file scan; D-7.7 order and results unchanged).
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    // Few distinct bytes, so keys collide, newlines fall at every offset of an 8-byte word,
    // and the blank/comment cases are common.
    let alphabet = b"aAbB_. \t\r\x0c\x0b#\n\n\n\x80\xff";
    let aliases: [&[u8]; 6] = [b"a", b"A", b"ab", b"b_.", b"a.b", b"\xff"];
    for _ in 0..4000 {
        let len = (next() % 90) as usize;
        let text: Vec<u8> = (0..len).map(|_| alphabet[(next() % alphabet.len() as u64) as usize]).collect();
        for alias in aliases {
            assert_eq!(lookup_alias(&text, alias), reference(&text, alias), "text {text:?} alias {alias:?}");
        }
    }
    // Every length and newline offset around the word size, with the alias on the last line.
    for pad in 0..40usize {
        let mut text = vec![b'x'; pad];
        text.extend_from_slice(b"\nkey value\n");
        assert_eq!(lookup_alias(&text, b"KEY"), Some(&b"value"[..]), "pad {pad}");
        text.truncate(text.len() - 1);
        assert_eq!(lookup_alias(&text, b"key"), Some(&b"value"[..]), "pad {pad} no final newline");
    }
}
