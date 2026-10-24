//! The catalog directories gettext tries for a locale name (spec §7.7; Appendix D.2;
//! `spec/behaviour/catalogs.md`, "Locale name variants").
//!
//! For `language[_territory][.codeset][@modifier]` the directories are every combination that
//! keeps the language, most specific first: the modifier is dropped last, then the territory, then
//! the codeset (the order of Appendix D.1's weights). A codeset is tried as written and then
//! normalized, which differs from D.1: letters lowercased, everything but letters and digits
//! removed, and `iso` prefixed when only digits remain (`8859-1` is `iso88591`); the normalized
//! form is tried once when it equals the written one. An empty codeset counts as absent.
//!
//! **Path components** (decision, a security default): a locale name that contains a `/` gives no
//! directory, and a directory name that is empty, `.` or `..`, or longer than [`MAX_NAME`] is
//! never tried, so a locale name (from `LANGUAGE`, an alias, or the locale) cannot reach outside
//! the bound directory. The reference library uses such names as paths (`../sel/it` and `/it`
//! find `<dir>/sel/it`).

use crate::locale::name::{self, MAX_NAME};

/// A directory name built in a fixed buffer.
struct Buf {
    b: [u8; MAX_NAME],
    len: usize,
}

impl Buf {
    fn push(&mut self, s: &[u8]) -> bool {
        let Some(end) = self.len.checked_add(s.len()) else { return false };
        let Some(dst) = self.b.get_mut(self.len..end) else { return false };
        dst.copy_from_slice(s);
        self.len = end;
        true
    }

    fn as_bytes(&self) -> &[u8] {
        self.b.get(..self.len).unwrap_or(&[])
    }
}

/// Whether `s` may be used as one component of a path (see the module documentation).
pub fn safe_component(s: &[u8]) -> bool {
    !s.is_empty() && s != b"." && s != b".." && !s.contains(&b'/') && s.len() < MAX_NAME
}

/// The normalized codeset (see the module documentation) into `out`; its length.
fn normalize(codeset: &[u8], out: &mut [u8; MAX_NAME]) -> usize {
    let mut tmp = [0u8; MAX_NAME];
    let n = name::normalize_codeset(codeset, &mut tmp);
    let digits = tmp.get(..n).unwrap_or(&[]).iter().all(u8::is_ascii_digit);
    let mut b = Buf { b: [0; MAX_NAME], len: 0 };
    if digits && n > 0 {
        b.push(b"iso");
    }
    b.push(tmp.get(..n).unwrap_or(&[]));
    if let Some(dst) = out.get_mut(..b.len) {
        dst.copy_from_slice(b.as_bytes());
    }
    b.len
}

/// Call `f` with each directory name for `locale`, most specific first, until it returns `Some`.
pub fn each<T>(locale: &[u8], mut f: impl FnMut(&[u8]) -> Option<T>) -> Option<T> {
    if locale.len() >= MAX_NAME || locale.contains(&b'/') {
        return None;
    }
    let parts = name::parse(locale);
    let codeset = parts.codeset.filter(|c| !c.is_empty());
    let mut norm = [0u8; MAX_NAME];
    let norm_len = codeset.map_or(0, |c| normalize(c, &mut norm));
    let norm = norm.get(..norm_len).unwrap_or(&[]);
    for mask in (0u8..8).rev() {
        let modifier = if mask & 4 != 0 {
            match parts.modifier {
                Some(m) => Some(m),
                None => continue,
            }
        } else {
            None
        };
        let territory = if mask & 2 != 0 {
            match parts.territory {
                Some(t) => Some(t),
                None => continue,
            }
        } else {
            None
        };
        let forms: [Option<&[u8]>; 2] = if mask & 1 != 0 {
            match codeset {
                Some(c) if c != norm => [Some(c), Some(norm)],
                Some(c) => [Some(c), None],
                None => continue,
            }
        } else {
            [None, None]
        };
        for (k, cs) in forms.iter().enumerate() {
            if k == 1 && cs.is_none() {
                continue;
            }
            let mut b = Buf { b: [0; MAX_NAME], len: 0 };
            let mut ok = b.push(parts.language);
            if let Some(t) = territory {
                ok &= b.push(b"_") && b.push(t);
            }
            if let Some(c) = cs {
                ok &= b.push(b".") && b.push(c);
            }
            if let Some(m) = modifier {
                ok &= b.push(b"@") && b.push(m);
            }
            if ok && safe_component(b.as_bytes()) {
                if let Some(r) = f(b.as_bytes()) {
                    return Some(r);
                }
            }
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn all(locale: &str) -> Vec<String> {
        let mut v = Vec::new();
        each::<()>(locale.as_bytes(), |d| {
            v.push(String::from_utf8(d.to_vec()).unwrap());
            None
        });
        v
    }

    #[test]
    fn order_as_observed() {
        assert_eq!(
            all("xx_YY.ISO-8859-1@mod"),
            [
                "xx_YY.ISO-8859-1@mod", "xx_YY.iso88591@mod", "xx_YY@mod", "xx.ISO-8859-1@mod", "xx.iso88591@mod", "xx@mod",
                "xx_YY.ISO-8859-1", "xx_YY.iso88591", "xx_YY", "xx.ISO-8859-1", "xx.iso88591", "xx"
            ]
        );
        assert_eq!(all("xx_YY.8859-1"), ["xx_YY.8859-1", "xx_YY.iso88591", "xx_YY", "xx.8859-1", "xx.iso88591", "xx"]);
        assert_eq!(all("xx.utf8"), ["xx.utf8", "xx"]);
        assert_eq!(all("xx_YY.@mod"), ["xx_YY@mod", "xx@mod", "xx_YY", "xx"]);
        assert_eq!(all("_YY"), ["_YY"]);
        assert_eq!(all("xx_YY_ZZ"), ["xx_YY_ZZ", "xx"]);
    }

    #[test]
    fn unsafe_components_are_never_tried() {
        assert!(all("../sel/it").is_empty());
        assert!(all("/it").is_empty());
        assert!(all("..").is_empty());
        assert_eq!(all("..@x"), ["..@x", ".@x", "@x"], "single components; the reductions `..`, `.` and `` are not tried");
    }
}
