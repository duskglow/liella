//! Locale files (spec Appendix D.1, D-7.7): the locale archive, compiled category files and the
//! alias file, found through a simulated file system as `setlocale`/`newlocale` find them, then
//! used: `nl_langinfo` items, character classes and case maps, multibyte decoding, collation
//! (`strcoll`, `strxfrm`), `strftime` with `LC_TIME` (eras included), and bracket expressions of
//! regular expressions and `fnmatch` patterns in the locale.
//!
//! Locale data is `'static` in the library (mapped for the life of the process), so the input
//! is leaked once per run within `liella_fuzz::LEAK_BUDGET`.
//!
//! Input: a flag byte (bit 0: `LOCPATH` set), then binary parts, each a little-endian `u32`
//! length and its bytes: the requested name, `locale.alias`, `locale-archive`, then the twelve
//! category files in `Cat::ALL` order (an empty part is a missing file).
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::ctype;
use liella::locale::file::Cat;
use liella::locale::{self, Locale, Source};
use liella::pattern::{fnmatch, regex};
use liella::text::{self, Decoded, Encoding, MbState};
use liella::time::format::{self as tf, Tm};
use liella_fuzz::Input;

struct Files {
    alias: &'static [u8],
    archive: &'static [u8],
    cats: [&'static [u8]; 12],
    locpath: bool,
}

impl Source for Files {
    fn file(&self, path: &[u8]) -> Option<&'static [u8]> {
        let found = if path == locale::ALIAS_FILE {
            self.alias
        } else if path == locale::ARCHIVE {
            self.archive
        } else {
            Cat::ALL.iter().position(|c| path.ends_with(c.file_name())).map_or(&[][..], |i| self.cats[i])
        };
        (!found.is_empty()).then_some(found)
    }

    fn locpath(&self) -> Option<&[u8]> {
        self.locpath.then_some(b"/fuzz/a::/fuzz/b")
    }

    fn charmap(&self, _name: &[u8]) -> Option<&'static liella::charmap::Charmap> {
        None
    }
}

struct Zone;

impl tf::Env for Zone {
    fn zone_name(&self, dst: bool) -> &[u8] {
        if dst { b"FDT" } else { b"FST" }
    }
    fn seconds(&self, _tm: &Tm<'_>) -> i128 {
        1_700_000_000
    }
}

fn use_locale(loc: &Locale) {
    for cat in Cat::ALL {
        for i in 0..160 {
            let _ = loc.item(cat, i);
            let _ = loc.string(cat, i);
        }
    }
    let _ = (loc.codeset(), loc.radix(), loc.thousands_sep(), loc.mb_cur_max(), loc.needs_charmap());
    for b in 0..=255u8 {
        let _ = ctype::classes(b, loc);
        let _ = ctype::to_upper(b, loc);
        let _ = ctype::to_lower(b, loc);
    }
    for c in [0u32, 0x41, 0xe9, 0x3b1, 0x4e00, 0x1f600, 0x10_ffff, 0x11_0000, u32::MAX] {
        let _ = ctype::wclasses(c, loc);
        let _ = text::display_width(c, loc);
        for name in [&b"alpha"[..], b"upper", b"punct", b"blank"] {
            if let Some(w) = ctype::wctype(name, loc) {
                let _ = ctype::is_wclass(c, w, loc);
            }
        }
        for name in [&b"toupper"[..], b"tolower"] {
            if let Some(t) = ctype::Trans::in_locale(name, loc) {
                let _ = ctype::wtrans(c, t, loc);
            }
        }
    }
    let enc = Encoding::of(loc);
    let samples: [&[u8]; 5] = [b"abc", b"ABC", b"\xc3\xa9t\xc3\xa9", b"a\xffb", b"\xe4\xb8\x80z"];
    for s in samples {
        let mut st = MbState::INITIAL;
        let mut at = 0usize;
        while at < s.len() {
            match text::decode(&mut st, &s[at..], enc) {
                Decoded::Char(_, n) if n > 0 => at += n,
                _ => break,
            }
        }
        for t in samples {
            let _ = loc.compare_bytes(s, t);
        }
        let _ = loc.xfrm_bytes(s).take(4096).count();
    }
    let wide: [&[i32]; 3] = [&[0x61, 0x62], &[0xe9, 0x41], &[0x4e00, -1]];
    for a in wide {
        for b in wide {
            let _ = loc.compare_wide(a, b);
        }
        let _ = loc.xfrm_wide(a).take(4096).count();
    }
    let tm = Tm { sec: 5, min: 4, hour: 13, mday: 2, mon: 9, year: 126, wday: 5, yday: 274, isdst: 0, gmtoff: 0, zone: Some(b"UTC") };
    let mut out = [0u8; 1024];
    for fmt in [&b"%c|%x|%X|%r|%p|%a|%A|%b|%B|%h"[..], b"%Ec|%EC|%Ex|%EX|%Ey|%EY", b"%Od|%Oe|%OH|%OI|%Om|%OM|%OS|%Ou|%OU|%OV|%Ow|%OW|%Oy"] {
        let _ = tf::strftime(&mut out, fmt, &tm, loc, &Zone);
    }
    for pat in [&b"[[:alpha:]][a-z]"[..], b"[[=a=]][[.a.]]", b"[^[:space:]]*"] {
        if let Ok(p) = regex::compile(pat, regex::regcomp_syntax(1), loc) {
            let _ = regex::exec(&p, loc, b"aZ\xc3\xa9 b", regex::Window { first: 0, last: 6, backward: false }, &regex::Options::default(), true);
        }
        let _ = fnmatch::fnmatch(pat, b"ab", 0, loc);
    }
}

fuzz_target!(|data: &[u8]| {
    let Some(data) = liella_fuzz::leak(data) else { return };
    let mut input = Input::new(data);
    let flags = input.byte();
    let name: &'static [u8] = input.sized();
    let alias = input.sized();
    let archive = input.sized();
    let cats: [&'static [u8]; 12] = core::array::from_fn(|_| input.sized());
    let files = Files { alias, archive, cats, locpath: flags & 1 != 0 };

    // Each category on its own (as `setlocale` with one category), then the whole locale.
    let mut loc = Locale::C;
    for cat in Cat::ALL {
        if let Some(d) = locale::find(&files, cat, name)
            && let Some(l) = loc.with(cat, name, d) {
                loc = l;
            }
    }
    if let Some(l) = locale::attach_charmap(&files, loc) {
        use_locale(&l);
    }
    // The archive and a category file read directly.
    if let Some(a) = locale::archive::Archive::parse(archive)
        && let Some(rec) = a.lookup(name) {
            for cat in Cat::ALL {
                if let Some(f) = rec.category(cat).and_then(locale::file::Category::parse) {
                    let _ = f.magic();
                }
            }
        }
});
