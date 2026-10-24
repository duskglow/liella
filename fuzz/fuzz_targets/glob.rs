//! glob patterns (spec §14.8, D-14.3): brace expansion, tilde expansion, and the directory walk
//! with `fnmatch` over a simulated file system (never the real one), with every flag `glob`
//! knows. Also `glob_pattern_p` and `fnmatch` on the pattern itself.
//!
//! Input: flags (little-endian `u16`), a byte (bit 0: `C.UTF-8`; bit 1: the error function
//! aborts), then `pattern NUL tree`, where the tree lists paths one per line, a trailing `/`
//! marking a directory, `@` a symbolic link.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::error::{Errno, Result};
use liella::mem::TryVec;
use liella::pattern::{fnmatch, glob};
use liella_fuzz::Input;

const S_IFDIR: u32 = 0o040_000;
const S_IFREG: u32 = 0o100_000;
const S_IFLNK: u32 = 0o120_000;
const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;
const DT_LNK: u8 = 10;

/// At most this many `{` in a pattern: brace expansion is exponential in them by definition.
const MAX_BRACES: usize = 8;

struct Tree {
    /// (path without a trailing slash, mode).
    entries: Vec<(Vec<u8>, u32)>,
    abort: bool,
    opened: usize,
}

impl Tree {
    fn new(text: &[u8]) -> Tree {
        let mut entries: Vec<(Vec<u8>, u32)> = Vec::new();
        for line in text.split(|&c| c == b'\n').take(256) {
            let (path, mode) = if let Some(p) = line.strip_suffix(b"/") {
                (p, S_IFDIR)
            } else if let Some(p) = line.strip_suffix(b"@") {
                (p, S_IFLNK)
            } else {
                (line, S_IFREG)
            };
            if path.is_empty() {
                continue;
            }
            // Every parent is a directory.
            for (i, &c) in path.iter().enumerate() {
                if c == b'/' && i > 0 && !entries.iter().any(|(p, _)| p.as_slice() == &path[..i]) {
                    entries.push((path[..i].to_vec(), S_IFDIR));
                }
            }
            entries.push((path.to_vec(), mode));
        }
        Tree { entries, abort: false, opened: 0 }
    }

    fn mode(&self, path: &[u8]) -> Option<u32> {
        let p = path.strip_suffix(b"/").unwrap_or(path);
        let p = p.strip_prefix(b"./").unwrap_or(p);
        if p.is_empty() || p == b"." || p == b"/" {
            return Some(S_IFDIR);
        }
        self.entries.iter().find(|(e, _)| e.as_slice() == p).map(|(_, m)| *m)
    }
}

impl glob::Access for Tree {
    type Dir = std::vec::IntoIter<(Vec<u8>, u8)>;

    fn open(&mut self, path: &[u8]) -> Result<Self::Dir> {
        self.opened += 1;
        if self.opened > 4096 {
            return Err(Errno::EMFILE);
        }
        if self.mode(path) != Some(S_IFDIR) {
            return Err(Errno::ENOTDIR);
        }
        let p = path.strip_suffix(b"/").unwrap_or(path);
        let p = p.strip_prefix(b"./").unwrap_or(p);
        let prefix: Vec<u8> = if p.is_empty() || p == b"." { Vec::new() } else { [p, b"/"].concat() };
        let mut list = vec![(b".".to_vec(), DT_DIR), (b"..".to_vec(), DT_DIR)];
        for (e, m) in &self.entries {
            if let Some(rest) = e.strip_prefix(prefix.as_slice())
                && !rest.is_empty() && !rest.contains(&b'/') {
                    list.push((rest.to_vec(), match *m {
                        S_IFDIR => DT_DIR,
                        S_IFLNK => DT_LNK,
                        _ => DT_REG,
                    }));
                }
        }
        Ok(list.into_iter())
    }

    fn read(&mut self, dir: &mut Self::Dir, name: &mut TryVec<u8>) -> Result<Option<u8>> {
        let Some((n, t)) = dir.next() else { return Ok(None) };
        name.clear();
        name.extend_from_slice(&n)?;
        Ok(Some(t))
    }

    fn close(&mut self, _dir: Self::Dir) {}

    fn lstat(&mut self, path: &[u8]) -> Result<u32> {
        self.mode(path).ok_or(Errno::ENOENT)
    }

    fn stat(&mut self, path: &[u8]) -> Result<u32> {
        match self.mode(path) {
            Some(S_IFLNK) => Ok(S_IFREG),
            Some(m) => Ok(m),
            None => Err(Errno::ENOENT),
        }
    }

    fn error(&mut self, _path: &[u8], _e: Errno) -> bool {
        self.abort
    }

    fn home(&mut self, user: Option<&[u8]>) -> Result<Option<TryVec<u8>>> {
        match user {
            None => Ok(Some(TryVec::from_slice(b"/home/me")?)),
            Some(b"root") => Ok(Some(TryVec::from_slice(b"/root")?)),
            Some(u) if u.len() < 8 => Ok(Some(TryVec::from_slice(&[b"/home/", u].concat())?)),
            Some(_) => Ok(None),
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let flags = i32::from(input.u16()) & glob::KNOWN_FLAGS;
    let opts = input.byte();
    let pattern = input.part();
    let tree_text = input.rest();
    if pattern.iter().filter(|&&c| c == b'{').count() > MAX_BRACES {
        return;
    }
    let loc = liella_fuzz::locale(opts & 1 != 0);
    let mut tree = Tree::new(tree_text);
    tree.abort = opts & 2 != 0;

    let _ = glob::is_magic(pattern, true);
    let _ = glob::is_magic(pattern, false);
    if let Ok(found) = glob::glob(pattern, flags, &loc, &mut tree) {
        assert!(!found.paths.is_empty(), "success has paths");
    }
    // The pattern against the tree's names, as fnmatch.
    for fl in [0, 1, 2, 4, 8, 16, 32, 1 | 4 | 32] {
        if let Ok(p) = fnmatch::Pattern::new(pattern, fl, &loc) {
            for (e, _) in tree.entries.iter().take(32) {
                let _ = p.matches(e, &loc);
            }
        }
        let _ = fnmatch::fnmatch(pattern, &tree_text[..tree_text.len().min(256)], fl, &loc);
    }
});
