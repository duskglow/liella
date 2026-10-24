//! `wordexp` ends in bounded time on the pinned fuzz timeouts (WORDEXP-TIMEOUT-2; spec §14.10).
//! The inputs (fuzz/seeds/wordexp/timeout-*) are the wordexp fuzz target's timeout artifacts:
//! pattern removal (`${1%$*}`, `${2%*.?[...*}`) with long patterns and values, and long runs of
//! `${1#...}`. Each call must end (any result, `WRDE_NOSPACE` included) within the bound. The
//! answers themselves are covered by the conformance and oracle suites.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, missing_docs)]

#[path = "support/timeout.rs"]
mod timeout;

use liella::error::Result;
use liella::locale::Locale;
use liella::mem::TryVec;
use liella::pattern::wordexp::{self, Shell, ValBuf, Words};

/// The fuzz target's simulated shell (fuzz/fuzz_targets/wordexp.rs), kept in step with it.
struct Env<'a> {
    vars: Vec<(Vec<u8>, Vec<u8>)>,
    args: Vec<&'a [u8]>,
}

impl Shell for Env<'_> {
    fn var(&mut self, name: &[u8], out: &mut ValBuf) -> Result<bool> {
        out.clear();
        match self.vars.iter().find(|(n, _)| n.as_slice() == name) {
            Some((_, v)) => {
                out.extend_from_slice(v)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
    fn set_var(&mut self, name: &[u8], value: &[u8]) -> Result<()> {
        if let Some(slot) = self.vars.iter_mut().find(|(n, _)| n.as_slice() == name) {
            slot.1 = value.to_vec();
        } else if self.vars.len() < 256 {
            self.vars.push((name.to_vec(), value.to_vec()));
        }
        Ok(())
    }
    fn arg_count(&mut self) -> usize {
        self.args.len().saturating_sub(1)
    }
    fn arg(&mut self, i: usize, out: &mut ValBuf) -> Result<bool> {
        out.clear();
        match self.args.get(i) {
            Some(a) => {
                out.extend_from_slice(a)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
    fn pid(&mut self) -> i32 {
        4242
    }
    fn home(&mut self, user: Option<&[u8]>, out: &mut ValBuf) -> Result<bool> {
        out.clear();
        match user {
            None => out.extend_from_slice(b"/home/me")?,
            Some(u) if u.len() < 8 => {
                out.extend_from_slice(b"/home/")?;
                out.extend_from_slice(u)?;
            }
            Some(_) => return Ok(false),
        }
        Ok(true)
    }
    fn run(&mut self, command: &[u8], _show_err: bool, out: &mut TryVec<u8>) -> Result<()> {
        out.clear();
        out.extend_from_slice(&command[..command.len().min(4096)])
    }
    fn stderr(&mut self, _text: &[u8]) {}
    fn glob(&mut self, pattern: &[u8], out: &mut TryVec<TryVec<u8>>) -> Result<()> {
        if pattern.contains(&b'*') {
            out.push(TryVec::from_slice(b"a.c")?)?;
            out.push(TryVec::from_slice(b"b.c")?)?;
        }
        Ok(())
    }
}

struct Fields(usize);

impl Words for Fields {
    fn push(&mut self, _word: &[u8]) -> Result<()> {
        self.0 += 1;
        if self.0 > 1 << 16 { Err(liella::error::Errno::ENOMEM) } else { Ok(()) }
    }
}

fn split_part(data: &mut &[u8]) -> Vec<u8> {
    match data.iter().position(|&b| b == 0) {
        Some(i) => {
            let head = data[..i].to_vec();
            *data = &data[i + 1..];
            head
        }
        None => std::mem::take(data).to_vec(),
    }
}

/// One fuzz-target call on `data`: the wordexp result code (0 = success) and the time taken by
/// wordexp and by the arithmetic evaluator.
fn replay(data: &[u8]) -> (i32, std::time::Duration, std::time::Duration) {
    let (&flags, mut rest) = data.split_first().unwrap_or((&0, &[]));
    let words = split_part(&mut rest);
    let vars_text = split_part(&mut rest);
    let args_text = split_part(&mut rest);
    let vars = vars_text
        .split(|&c| c == b'\n')
        .filter_map(|l| {
            let eq = l.iter().position(|&c| c == b'=')?;
            Some((l[..eq].to_vec(), l[eq + 1..].to_vec()))
        })
        .take(256)
        .collect();
    let mut args: Vec<&[u8]> = vec![b"prog"];
    args.extend(args_text.split(|&c| c == b' ').filter(|a| !a.is_empty()).take(64));
    let mut env = Env { vars, args };
    let loc = if flags & 0x80 != 0 { Locale::c_utf8(b"C.UTF-8") } else { Locale::C };
    let wflags = i32::from(flags) & (wordexp::NOCMD | wordexp::SHOWERR | wordexp::UNDEF);
    let mut fields = Fields(0);
    let t = std::time::Instant::now();
    let code = match wordexp::wordexp(&words, wflags, &loc, &mut env, &mut fields) {
        Ok(()) => 0,
        Err(e) => e.code(),
    };
    let tw = t.elapsed();
    let mut var = |name: &[u8], out: &mut ValBuf| -> Result<bool> {
        out.clear();
        if name.first() == Some(&b'x') {
            out.extend_from_slice(b"7")?;
            Ok(true)
        } else {
            Ok(false)
        }
    };
    let t = std::time::Instant::now();
    let _ = wordexp::arith::evaluate(&words, flags & 0x20 != 0, &mut var);
    (code, tw, t.elapsed())
}

const BOUND_SECS: u64 = 1;

/// Run `work` on its own thread and fail, rather than hang, if it has not ended after four bounds
/// (a wordexp that does not end would otherwise run until the harness kills the whole test binary).
fn bounded<T: Send + 'static>(what: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    let limit = timeout::limit(std::time::Duration::from_secs(4 * BOUND_SECS));
    let got = rx.recv_timeout(limit).ok();
    assert!(got.is_some(), "{what}: took over {:.1} s, over the bound (it did not end)", limit.as_secs_f64());
    got.unwrap()
}

#[test]
fn wordexp_ends_in_bounded_time_on_pinned_fuzz_inputs() {
    // Clauses: s14_10_c_1, s14_10_f_0 (pattern removal; WRDE_NOSPACE as the resource bound).
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds/wordexp");
    let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).filter(|n| n.starts_with("timeout-")).collect();
    names.sort();
    for name in &names {
        let data = std::fs::read(dir.join(name)).unwrap();
        let (code, tw, ta) = bounded(name, move || replay(&data));
        eprintln!("{name}: code {code} wordexp {tw:?} arith {ta:?}");
        timeout::assert_within(&format!("{name}: wordexp"), std::time::Duration::from_secs(BOUND_SECS), tw);
        timeout::assert_within(&format!("{name}: arith"), std::time::Duration::from_secs(BOUND_SECS), ta);
        assert!((0..=5).contains(&code), "{name}: code {code}");
    }
    assert!(names.len() >= 11, "the pinned timeout inputs are present ({})", names.len());
}

struct Collect(Vec<Vec<u8>>);

impl Words for Collect {
    fn push(&mut self, word: &[u8]) -> Result<()> {
        self.0.push(word.to_vec());
        Ok(())
    }
}

#[test]
fn suffix_removal_with_a_trailing_backslash_keeps_the_value_in_linear_time() {
    // Clauses: s14_10_c_1 (`${x%p}`, `${x%%p}`: a pattern ending in an unescaped `\` from an
    // expansion matches nothing, so the value is kept; timeout-25f0be39 had this shape).
    let mut long = vec![b'a'; 20_000];
    long.push(b'\\');
    for (word, prefix_op) in [(&b"${1%*$1}"[..], false), (b"${1%%*$1}", false), (b"${1#$1*}", true), (b"${1##$1*}", true)] {
        let (arg, w) = (long.clone(), word.to_vec());
        let (words, elapsed) = bounded(&String::from_utf8_lossy(word), move || {
            let mut env = Env { vars: Vec::new(), args: vec![b"prog", &arg] };
            let mut out = Collect(Vec::new());
            let t = std::time::Instant::now();
            wordexp::wordexp(&w, 0, &Locale::C, &mut env, &mut out).unwrap();
            (out.0, t.elapsed())
        });
        timeout::assert_within(&format!("{}: wordexp", String::from_utf8_lossy(word)), std::time::Duration::from_secs(BOUND_SECS), elapsed);
        assert_eq!(words, vec![long.clone()], "{} (prefix {prefix_op}): the value is kept", String::from_utf8_lossy(word));
    }
    // A pattern whose backslash escapes a following character still removes.
    let mut env = Env { vars: Vec::new(), args: vec![b"prog", b"x.c\\d"] };
    let mut out = Collect(Vec::new());
    wordexp::wordexp(b"${1%\\\\d}", 0, &Locale::C, &mut env, &mut out).unwrap();
    assert_eq!(out.0, vec![b"x.c".to_vec()]);
    // A pattern ending in a lone `\` matches nothing, also when the value is exactly the text before
    // it: `${2%$1}` with $1 = `a\` and $2 = `a` keeps `a` (a trailing `\` is not an empty match).
    for word in [&b"${2%$1}"[..], b"${2%%$1}", b"${2#$1}", b"${2##$1}"] {
        let mut env = Env { vars: Vec::new(), args: vec![b"prog", b"a\\", b"a"] };
        let mut out = Collect(Vec::new());
        wordexp::wordexp(word, 0, &Locale::C, &mut env, &mut out).unwrap();
        assert_eq!(out.0, vec![b"a".to_vec()], "{}: the value is kept", String::from_utf8_lossy(word));
    }
}
