//! The `p`-form exec search (spec §18.3; D-18.2) seen through the candidates it hands to `exec`
//! (EXECVP-PATH): one buffer serves the whole search, so every candidate must be exactly
//! `element/name` with nothing left from a longer one before it, across a long `PATH`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use liella::process::search;
use liella_kernel::errno::Errno;
use liella_kernel::path::{KStr, PATH_MAX};

/// Run the search over `path` for `name`, failing every candidate with `fail(candidate)`;
/// the candidates in order (with their terminators) and the result.
fn run(name: &[u8], path: &[u8], mut fail: impl FnMut(&[u8]) -> Errno) -> (Vec<Vec<u8>>, Errno) {
    let name = KStr::from_nul_terminated(name).unwrap();
    let mut seen = Vec::new();
    let mut exec = |c: KStr<'_>| {
        seen.push(c.with_nul().to_vec());
        fail(c.bytes())
    };
    let e = search(name, Some(path), &mut exec, None);
    (seen, e)
}

#[test]
fn candidates_are_exact_after_longer_ones() {
    // Trace: no clause ID (§18.3 is not parsed into spec/clauses.tsv); D-18.2.
    let long = format!("/nonexistent/{}", "m".repeat(240));
    let path = format!("{long}:/b::/usr/bin");
    let (seen, e) = run(b"prog\0", path.as_bytes(), |_| Errno::ENOENT);
    let want: Vec<Vec<u8>> = vec![format!("{long}/prog\0").into_bytes(), b"/b/prog\0".to_vec(), b"prog\0".to_vec(), b"/usr/bin/prog\0".to_vec()];
    assert_eq!(seen, want);
    assert_eq!(e, Errno::ENOENT);
}

#[test]
fn long_path_tries_every_element_and_remembers_eacces() {
    // Trace: no clause ID (§18.3 is not parsed into spec/clauses.tsv); D-18.2.
    let mut dirs: Vec<String> = (0..200).map(|k| format!("/nonexistent/stress/dir{k:03}")).collect();
    dirs.insert(7, "/noexec".to_string());
    let path = dirs.join(":");
    let (seen, e) = run(b"cbench-no-such-command\0", path.as_bytes(), |c| if c.starts_with(b"/noexec/") { Errno::EACCES } else { Errno::ENOENT });
    assert_eq!(seen.len(), 201);
    for (c, d) in seen.iter().zip(&dirs) {
        assert_eq!(c, &format!("{d}/cbench-no-such-command\0").into_bytes());
    }
    assert_eq!(e, Errno::EACCES);
}


/// The candidates a plain split of `path` on `:` gives for `name` (no terminator).
fn model(name: &[u8], path: &[u8]) -> Vec<Vec<u8>> {
    path.split(|&b| b == b':')
        .map(|el| {
            let mut c = el.to_vec();
            if !el.is_empty() {
                c.push(b'/');
            }
            c.extend_from_slice(name);
            c.push(0);
            c
        })
        .collect()
}

#[test]
fn split_matches_a_plain_split_at_every_length() {
    // Trace: no clause ID (§18.3 is not parsed into spec/clauses.tsv); D-18.2. EXECVP-PATH-2:
    // the separator search reads a word at a time, so elements of every length around a word,
    // and bytes next to ':' in value or with the high bit set, must split exactly as byte-wise.
    let fill = [b'a', b':' | 0x80, b';', b'9', 0x01, 0xff, 0x80];
    for i in 0..20usize {
        for j in 0..20usize {
            let a: Vec<u8> = (0..i).map(|k| fill[(k + j) % fill.len()]).collect();
            let b: Vec<u8> = (0..j).map(|k| fill[(k * 3 + i) % fill.len()]).collect();
            for path in [[&a[..], b":", &b[..]].concat(), [b":", &a[..], b":", &b[..], b":"].concat()] {
                let (seen, e) = run(b"prog\0", &path, |_| Errno::ENOENT);
                assert_eq!(seen, model(b"prog", &path), "path {path:?}");
                assert_eq!(e, Errno::ENOENT);
            }
        }
    }
}

#[test]
fn nul_in_an_element_stops_with_einval() {
    // Trace: no clause ID (§18.3 is not parsed into spec/clauses.tsv); D-18.2. Only a Rust
    // caller can pass one (a C `PATH` ends at its first NUL); the element is never tried. The
    // NUL sits in the word-wise part of the scan in one case and in the byte-wise tail in another.
    for path in [&b"/a:/b\0c:/d"[..], b"/a:/bbbbbbbbbbbbbbbbb\0:/d", b"/a:/bbbbbbbbbb\0bbbbbbbbbbbb:/d", b"/a:\0"] {
        let (seen, e) = run(b"prog\0", path, |_| Errno::ENOENT);
        assert_eq!(seen, vec![b"/a/prog\0".to_vec()], "path {path:?}");
        assert_eq!(e, Errno::EINVAL);
    }
}

#[test]
fn candidates_up_to_path_max_and_one_past() {
    // Trace: no clause ID (§18.3 is not parsed into spec/clauses.tsv); D-18.2. A candidate with
    // its terminator may fill PATH_MAX exactly; one byte more is ENAMETOOLONG, which ends the search.
    let name = |n: usize| [vec![b'n'; n], vec![0]].concat();
    let (seen, e) = run(&name(PATH_MAX - 1), b":/x", |_| Errno::ENOENT);
    assert_eq!(seen, vec![name(PATH_MAX - 1)]);
    assert_eq!(e, Errno::ENAMETOOLONG);
    let (seen, e) = run(&name(PATH_MAX), b":/x", |_| Errno::ENOENT);
    assert!(seen.is_empty());
    assert_eq!(e, Errno::ENAMETOOLONG);
    let dir = format!("/{}", "d".repeat(PATH_MAX - 10));
    let fits = format!("{dir}:{dir}d:/y");
    let (seen, e) = run(&name(PATH_MAX - dir.len() - 2), fits.as_bytes(), |_| Errno::ENOENT);
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].len(), PATH_MAX);
    assert_eq!(e, Errno::ENAMETOOLONG);
}
