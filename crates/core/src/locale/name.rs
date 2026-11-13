//! Locale names (spec Appendix D.1): `language[_territory][.codeset][@modifier]`, codeset
//! normalization, the candidates tried for a name, aliases, and composite `LC_ALL` names.

use super::file::Cat;

/// The longest locale name handled (longer names are rejected).
pub const MAX_NAME: usize = 256;

/// The parts of a locale name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts<'a> {
    /// The language (always present).
    pub language: &'a [u8],
    /// `_territory`, without the underscore.
    pub territory: Option<&'a [u8]>,
    /// `.codeset`, without the dot.
    pub codeset: Option<&'a [u8]>,
    /// `@modifier`, without the at sign.
    pub modifier: Option<&'a [u8]>,
}

/// Split a name into its parts. The language ends at the first `_`, `.` or `@`; the territory
/// at `.` or `@`; the codeset at `@`.
pub fn parse(name: &[u8]) -> Parts<'_> {
    let (rest, modifier) = match name.iter().position(|&b| b == b'@') {
        Some(i) => (name.get(..i).unwrap_or(&[]), name.get(i.saturating_add(1)..)),
        None => (name, None),
    };
    let (rest, codeset) = match rest.iter().position(|&b| b == b'.') {
        Some(i) => (rest.get(..i).unwrap_or(&[]), rest.get(i.saturating_add(1)..)),
        None => (rest, None),
    };
    let (language, territory) = match rest.iter().position(|&b| b == b'_') {
        Some(i) => (rest.get(..i).unwrap_or(&[]), rest.get(i.saturating_add(1)..)),
        None => (rest, None),
    };
    Parts { language, territory, codeset, modifier }
}

/// The normalized codeset: letters lowercased, every other character that is not a digit
/// removed (`UTF-8` → `utf8`). Returns the length written (at most `out.len()`).
pub fn normalize_codeset(codeset: &[u8], out: &mut [u8]) -> usize {
    let mut n = 0;
    for &b in codeset {
        if b.is_ascii_alphanumeric() {
            if let Some(o) = out.get_mut(n) {
                *o = b.to_ascii_lowercase();
                n = n.saturating_add(1);
            }
        }
    }
    n
}

/// Whether two codeset names denote the same codeset (equal once normalized).
pub fn same_codeset(a: &[u8], b: &[u8]) -> bool {
    // Equal names normalize alike.
    if a == b {
        return true;
    }
    let mut x = [0u8; MAX_NAME];
    let mut y = [0u8; MAX_NAME];
    let (n, m) = (normalize_codeset(a, &mut x), normalize_codeset(b, &mut y));
    x.get(..n) == y.get(..m)
}

/// A candidate name, built in a fixed buffer.
#[derive(Clone, Copy)]
pub struct Candidate {
    buf: [u8; MAX_NAME],
    len: usize,
}

impl Candidate {
    /// The name.
    pub fn as_bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }

    fn push(&mut self, bytes: &[u8]) -> bool {
        let Some(dst) = self.buf.get_mut(self.len..self.len.saturating_add(bytes.len())) else { return false };
        dst.copy_from_slice(bytes);
        self.len = self.len.saturating_add(bytes.len());
        true
    }
}

/// The candidates for `parts`, most specific first (Appendix D.1): with the present
/// components weighted modifier 4, territory 2, codeset 1, in descending order of the kept
/// components' total; a codeset in its literal form, then its normalized form when that
/// differs. Candidates that are not a single path component ([`component`]: empty, `.`, `..`)
/// are skipped. Calls `f` for each until it returns `Some`.
pub fn candidates<T>(parts: &Parts<'_>, mut f: impl FnMut(&[u8]) -> Option<T>) -> Option<T> {
    let mut norm = [0u8; MAX_NAME];
    let norm_len = parts.codeset.map_or(0, |c| normalize_codeset(c, &mut norm));
    let norm = norm.get(..norm_len).unwrap_or(&[]);
    for mask in (0u8..8).rev() {
        let modifier = if mask & 4 != 0 { match parts.modifier { Some(m) => Some(m), None => continue } } else { None };
        let territory = if mask & 2 != 0 { match parts.territory { Some(t) => Some(t), None => continue } } else { None };
        let codesets: [Option<&[u8]>; 2] = if mask & 1 != 0 {
            match parts.codeset {
                Some(c) if c != norm => [Some(c), Some(norm)],
                Some(c) => [Some(c), None],
                None => continue,
            }
        } else {
            [None, None]
        };
        for (k, codeset) in codesets.iter().enumerate() {
            if k == 1 && codeset.is_none() {
                continue;
            }
            let mut c = Candidate { buf: [0; MAX_NAME], len: 0 };
            let mut ok = c.push(parts.language);
            if let Some(t) = territory {
                ok &= c.push(b"_") && c.push(t);
            }
            if let Some(cs) = codeset {
                ok &= c.push(b".") && c.push(cs);
            }
            if let Some(m) = modifier {
                ok &= c.push(b"@") && c.push(m);
            }
            // A candidate is a path component (`dir/<candidate>/LC_*`): an empty one, `.` or
            // `..` would name the directory itself or its parent (D-7.7).
            if ok && component(c.as_bytes()) {
                if let Some(r) = f(c.as_bytes()) {
                    return Some(r);
                }
            }
        }
    }
    None
}

/// The archive's form of a name: the codeset normalized (`de_DE.UTF-8` → `de_DE.utf8`).
pub fn archive_form(parts: &Parts<'_>) -> Candidate {
    let mut c = Candidate { buf: [0; MAX_NAME], len: 0 };
    c.push(parts.language);
    if let Some(t) = parts.territory {
        c.push(b"_");
        c.push(t);
    }
    if let Some(cs) = parts.codeset {
        let mut norm = [0u8; MAX_NAME];
        let n = normalize_codeset(cs, &mut norm);
        c.push(b".");
        c.push(norm.get(..n).unwrap_or(&[]));
    }
    if let Some(m) = parts.modifier {
        c.push(b"@");
        c.push(m);
    }
    c
}

/// Whether a name may be looked up: not empty, not too long, no `/`, not `.` or `..`
/// (Appendix D.1; D-7.7: a locale name is never resolved as a path, owner ruling 2026-10-04).
pub fn acceptable(name: &[u8]) -> bool {
    name.len() < MAX_NAME && component(name)
}

/// Whether `s` can stand as one path component naming an entry inside a directory: not
/// empty, no `/`, not `.` or `..`.
pub fn component(s: &[u8]) -> bool {
    !s.is_empty() && !s.contains(&b'/') && s != b"." && s != b".."
}

/// The value of `alias` in an alias file's text (`alias value` lines, `#` comments), matched
/// without regard to case.
///
/// Almost every line is not the alias, so a line is first rejected by its first non-blank byte
/// (a key is never empty and starts there); only a line whose first byte can start `alias` is
/// parsed. Lines are found with [`newline`], a word at a time. The answer is that of parsing
/// every line.
pub fn lookup_alias<'t>(text: &'t [u8], alias: &[u8]) -> Option<&'t [u8]> {
    let first = alias.first()?.to_ascii_lowercase();
    let mut rest = text;
    while !rest.is_empty() {
        let end = newline(rest);
        let (line, tail) = rest.split_at(end);
        rest = tail.get(1..).unwrap_or(&[]);
        let lead = line.iter().take_while(|b| b.is_ascii_whitespace()).count();
        if line.get(lead).is_none_or(|b| b.to_ascii_lowercase() != first) {
            continue;
        }
        let line = line.split(|&b| b == b'#').next().unwrap_or(&[]);
        let mut fields = line.split(|b| b.is_ascii_whitespace()).filter(|f| !f.is_empty());
        let (Some(key), Some(value)) = (fields.next(), fields.next()) else { continue };
        if key.eq_ignore_ascii_case(alias) {
            return Some(value);
        }
    }
    None
}

/// The index of the first newline in `s`, or its length: eight bytes per step (a byte of a word
/// XOR a newline is zero exactly where the word has a newline).
fn newline(s: &[u8]) -> usize {
    const ONES: u64 = 0x0101_0101_0101_0101;
    const HIGH: u64 = 0x8080_8080_8080_8080;
    let mut at = 0usize;
    let mut words = s.chunks_exact(8);
    for w in words.by_ref() {
        let Ok(bytes) = <[u8; 8]>::try_from(w) else { break };
        let x = u64::from_le_bytes(bytes) ^ (ONES * u64::from(b'\n'));
        if x.wrapping_sub(ONES) & !x & HIGH != 0 {
            return at.saturating_add(w.iter().position(|&b| b == b'\n').unwrap_or(0));
        }
        at = at.saturating_add(8);
    }
    at.saturating_add(words.remainder().iter().position(|&b| b == b'\n').unwrap_or(words.remainder().len()))
}

/// Parse a composite name (`LC_CTYPE=…;LC_NUMERIC=…;…`): the twelve names in [`Cat::ALL`]
/// order, or `None` unless every category is named exactly once.
pub fn parse_composite(name: &[u8]) -> Option<[&[u8]; 12]> {
    if !name.contains(&b'=') {
        return None;
    }
    let mut out: [Option<&[u8]>; 12] = [None; 12];
    for part in name.split(|&b| b == b';') {
        let i = part.iter().position(|&b| b == b'=')?;
        let (key, value) = (part.get(..i)?, part.get(i.saturating_add(1)..)?);
        let cat = Cat::ALL.iter().position(|c| c.name() == key)?;
        let slot = out.get_mut(cat)?;
        if slot.is_some() || value.is_empty() {
            return None;
        }
        *slot = Some(value);
    }
    let mut res: [&[u8]; 12] = [&[]; 12];
    for (r, o) in res.iter_mut().zip(out) {
        *r = o?;
    }
    Some(res)
}

/// Write the composite name of `names` (in [`Cat::ALL`] order) into `out`; the length, or
/// `None` if it does not fit. When all names agree, that one name.
pub fn composite(names: &[&[u8]; 12], out: &mut [u8]) -> Option<usize> {
    let mut n = 0usize;
    let mut put = |b: &[u8]| -> Option<()> {
        let dst = out.get_mut(n..n.checked_add(b.len())?)?;
        dst.copy_from_slice(b);
        n = n.checked_add(b.len())?;
        Some(())
    };
    if names.iter().all(|x| *x == names[0]) {
        put(names[0])?;
        return Some(n);
    }
    for (k, (cat, name)) in Cat::ALL.iter().zip(names).enumerate() {
        if k > 0 {
            put(b";")?;
        }
        put(cat.name())?;
        put(b"=")?;
        put(name)?;
    }
    Some(n)
}
