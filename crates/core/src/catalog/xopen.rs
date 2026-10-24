//! X/Open message catalogs (`.cat`; spec §7.8, Appendix D.2.2, D-7.6; POSIX `catopen`,
//! `catgets`, XBD 8.2 `NLSPATH`).
//!
//! [`Cat::parse`] validates a whole image before any lookup: the header, the native table, and
//! every used slot's string start. The image must end with a NUL (the caller copies a file that
//! does not), so every string is terminated within it. The second (big-endian) table is not read.
//!
//! [`each_path`] produces the files `catopen` tries for a name without `/`: the `NLSPATH` templates,
//! then the default templates (as observed). Conversions: `%N` the name, `%L` the locale name, `%l`
//! its language, `%t` its territory, `%c` its codeset (a `@modifier` stays with the last element
//! present, as observed), `%%` a `%`. An empty template is the name itself. A template with an
//! unknown conversion, or ending in a lone `%` (decision: the reference library abandons the whole
//! search there), is skipped.
//!
//! **Path components** (decision, a security default): a template is skipped when a locale value it
//! substitutes is `.`, `..` or contains a `/`, so `LANG` cannot name a directory outside the
//! template's. The reference library substitutes such values unchecked (`LANG=../x`).

/// The magic number (little-endian on x86_64).
const MAGIC: u32 = 0x9604_08de;
/// The size of the header.
pub const HEADER: usize = 12;
/// The longest path tried, with its terminator (`PATH_MAX`).
pub const PATH_MAX: usize = 4096;

/// The default templates, tried after `NLSPATH`'s.
const DEFAULTS: [&[u8]; 4] = [b"/usr/share/locale/%L/%N", b"/usr/share/locale/%L/LC_MESSAGES/%N", b"/usr/share/locale/%l/%N", b"/usr/share/locale/%l/LC_MESSAGES/%N"];

/// A validated catalog image.
#[derive(Clone, Copy, Debug)]
pub struct Cat<'a> {
    image: &'a [u8],
    plane_size: u32,
    plane_depth: u32,
    pool: usize,
}

fn u32_at(image: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(image.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

impl<'a> Cat<'a> {
    /// Validate `image` (ending with a NUL; `file_len` is the file's own length). `None` if it is
    /// not a catalog: shorter than the header, a wrong magic number (a big-endian writer's file
    /// included), a plane size or depth of 0, tables beyond the file, or a string starting outside
    /// the image.
    pub fn parse(image: &'a [u8], file_len: usize) -> Option<Cat<'a>> {
        let c = Cat::parse_header(image, file_len)?;
        let slots = c.pool.checked_sub(HEADER)? / 24;
        for s in 0..slots {
            let at = s.checked_mul(12)?.checked_add(HEADER)?;
            if u32_at(image, at)? != 0 && c.pool.checked_add(usize::try_from(u32_at(image, at.checked_add(8)?)?).ok()?)? >= image.len() {
                return None;
            }
        }
        Some(c)
    }

    /// An image [`Cat::parse`] accepted before: the header's checks only (every later access is
    /// bounds-checked anyway).
    pub fn parse_header(image: &'a [u8], file_len: usize) -> Option<Cat<'a>> {
        if image.last() != Some(&0) || file_len < HEADER || file_len > image.len() || u32_at(image, 0)? != MAGIC {
            return None;
        }
        let (plane_size, plane_depth) = (u32_at(image, 4)?, u32_at(image, 8)?);
        if plane_size == 0 || plane_depth == 0 {
            return None;
        }
        let slots = usize::try_from(plane_size).ok()?.checked_mul(usize::try_from(plane_depth).ok()?)?;
        let pool = slots.checked_mul(24)?.checked_add(HEADER)?;
        if pool > file_len {
            return None;
        }
        Some(Cat { image, plane_size, plane_depth, pool })
    }

    /// The offset of message `msg` of set `set` in the image (a NUL-terminated string), or `None`
    /// if the catalog has no such message. Every plane is searched at the message's slot.
    pub fn get(&self, set: i32, msg: i32) -> Option<usize> {
        let (Ok(set), Ok(msg)) = (u32::try_from(set), u32::try_from(msg)) else { return None };
        let key = set.wrapping_add(1);
        let index = key.wrapping_mul(msg).checked_rem(self.plane_size)?;
        for plane in 0..self.plane_depth {
            let slot = usize::try_from(plane.checked_mul(self.plane_size)?.checked_add(index)?).ok()?;
            let at = slot.checked_mul(12)?.checked_add(HEADER)?;
            if u32_at(self.image, at)? == key && u32_at(self.image, at.checked_add(4)?)? == msg {
                return self.pool.checked_add(usize::try_from(u32_at(self.image, at.checked_add(8)?)?).ok()?);
            }
        }
        None
    }
}

/// The `%l`, `%t` and `%c` parts of a locale name (see the module documentation).
fn split(locale: &[u8]) -> [&[u8]; 3] {
    let l_end = locale.iter().position(|&b| b == b'_' || b == b'.').unwrap_or(locale.len());
    let (lang, rest) = locale.split_at(l_end);
    let (terr, codeset) = match rest.first() {
        Some(b'_') => {
            let rest = rest.get(1..).unwrap_or(&[]);
            let t_end = rest.iter().position(|&b| b == b'.').unwrap_or(rest.len());
            let (t, c) = rest.split_at(t_end);
            (t, c.get(1..).unwrap_or(&[]))
        }
        Some(_) => (&[][..], rest.get(1..).unwrap_or(&[])),
        None => (&[][..], &[][..]),
    };
    [lang, terr, codeset]
}

fn unsafe_value(v: &[u8]) -> bool {
    v == b"." || v == b".." || v.contains(&b'/')
}

/// Expand `template` into `out`: the length, or `None` to skip the template.
fn expand(template: &[u8], name: &[u8], locale: &[u8], out: &mut [u8; PATH_MAX]) -> Option<usize> {
    if template.is_empty() {
        out.get_mut(..name.len())?.copy_from_slice(name);
        return (name.len() < PATH_MAX).then_some(name.len());
    }
    let [lang, terr, codeset] = split(locale);
    let mut n = 0usize;
    let mut put = |s: &[u8]| -> Option<()> {
        let end = n.checked_add(s.len())?;
        if end >= PATH_MAX {
            return None;
        }
        out.get_mut(n..end)?.copy_from_slice(s);
        n = end;
        Some(())
    };
    let mut i = 0usize;
    while let Some(&b) = template.get(i) {
        i = i.checked_add(1)?;
        if b != b'%' {
            put(&[b])?;
            continue;
        }
        let c = *template.get(i)?;
        i = i.checked_add(1)?;
        let value: &[u8] = match c {
            b'N' => name,
            b'L' => locale,
            b'l' => lang,
            b't' => terr,
            b'c' => codeset,
            b'%' => b"%",
            _ => return None,
        };
        if c != b'N' && c != b'%' && unsafe_value(value) {
            return None;
        }
        put(value)?;
    }
    Some(n)
}

/// Call `f` with each path `catopen` tries for `name` (without `/`), in order, until it returns
/// `Some`: each template of `nlspath` (unless unset or empty), then the default templates.
/// `locale` is the locale name the call uses (`C` when none).
pub fn each_path<T>(nlspath: Option<&[u8]>, name: &[u8], locale: &[u8], mut f: impl FnMut(&[u8]) -> Option<T>) -> Option<T> {
    let mut buf = [0u8; PATH_MAX];
    let from_env = nlspath.filter(|p| !p.is_empty()).into_iter().flat_map(|p| p.split(|&b| b == b':'));
    for template in from_env.chain(DEFAULTS.iter().copied()) {
        if let Some(n) = expand(template, name, locale, &mut buf) {
            if let Some(r) = f(buf.get(..n).unwrap_or(&[])) {
                return Some(r);
            }
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn paths(nlspath: Option<&str>, name: &str, locale: &str) -> Vec<String> {
        let mut v = Vec::new();
        each_path::<()>(nlspath.map(str::as_bytes), name.as_bytes(), locale.as_bytes(), |p| {
            v.push(String::from_utf8(p.to_vec()).unwrap());
            None
        });
        v
    }

    #[test]
    fn templates() {
        let p = paths(Some("/t/l=%l/t=%t/c=%c/%N:/t/%%/%N:/t/u%X/%N:/t/x%::"), "app", "de_DE.UTF-8@euro");
        assert_eq!(&p[..4], ["/t/l=de/t=DE/c=UTF-8@euro/app", "/t/%/app", "app", "app"]);
        assert_eq!(p[4], "/usr/share/locale/de_DE.UTF-8@euro/app");
        assert_eq!(paths(Some("/t/%l|%t|%c"), "a", "de_DE@euro")[0], "/t/de|DE@euro|");
        assert_eq!(paths(Some("/t/%l|%t|%c"), "a", "de@euro")[0], "/t/de@euro||");
        assert_eq!(paths(Some("/t/%l|%t|%c"), "a", "de_DE_X.Y")[0], "/t/de|DE_X|Y");
        assert_eq!(paths(Some(""), "a", "C").len(), 4, "an empty NLSPATH is unset");
        assert!(paths(Some("/t/in/%L/%N"), "a", "../x")[0].starts_with("/usr/"), "../x is never substituted");
        assert_eq!(paths(None, "a", "sub/de").len(), 0, "every default template substitutes the locale");
    }

    #[test]
    fn lookup() {
        // Plane size 3, depth 1: (1,1) at slot 2, (1,2) at slot 1, (2,1) at slot 0.
        let mut img = vec![0u8; 12 + 72];
        img[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        img[4..8].copy_from_slice(&3u32.to_le_bytes());
        img[8..12].copy_from_slice(&1u32.to_le_bytes());
        for (slot, set, msg, off) in [(2usize, 2u32, 1u32, 0u32), (1, 2, 2, 8), (0, 3, 1, 16)] {
            let at = 12 + slot * 12;
            img[at..at + 4].copy_from_slice(&set.to_le_bytes());
            img[at + 4..at + 8].copy_from_slice(&msg.to_le_bytes());
            img[at + 8..at + 12].copy_from_slice(&off.to_le_bytes());
        }
        img.extend_from_slice(b"one-one\0one-two\0two-one\0");
        let len = img.len();
        let c = Cat::parse(&img, len).unwrap();
        assert_eq!(c.get(1, 1), Some(84));
        assert_eq!(c.get(2, 1), Some(100));
        assert_eq!(c.get(3, 3), None);
        assert_eq!(c.get(-1, 1), None);
        let mut zero = img.clone();
        zero[4..8].copy_from_slice(&0u32.to_le_bytes());
        assert!(Cat::parse(&zero, len).is_none(), "plane size 0 is not a catalog (no division by zero)");
        assert!(Cat::parse(&img[..40], 40).is_none());
    }
}
