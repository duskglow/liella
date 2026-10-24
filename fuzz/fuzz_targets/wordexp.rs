//! wordexp (spec §14.10, D-14.5): the word parser, quoting, parameter expansion, arithmetic,
//! tilde expansion and field splitting over a simulated shell environment. Command
//! substitution never runs anything: the simulated shell answers with the command's own text.
//! Also the arithmetic evaluator on its own.
//!
//! Input: a flag byte (`WRDE_*` bits: `NOCMD`, `SHOWERR`, `UNDEF`; bit 7: `C.UTF-8`), then
//! `words NUL variables NUL positional parameters`, variables as `NAME=value` lines, parameters
//! separated by spaces.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::error::Result;
use liella::mem::TryVec;
use liella::pattern::wordexp::{self, Shell, ValBuf, Words};
use liella_fuzz::Input;

/// The most fields kept, and the most variables `${X:=w}` may set.
const MAX_FIELDS: usize = 1 << 16;
const MAX_VARS: usize = 256;

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
        } else if self.vars.len() < MAX_VARS {
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
        // Nothing runs: the "output" is the command text.
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
        if self.0 > MAX_FIELDS { Err(liella::error::Errno::ENOMEM) } else { Ok(()) }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let flags = input.byte();
    let words = input.part();
    let vars_text = input.part();
    let args_text = input.part();

    let vars = vars_text
        .split(|&c| c == b'\n')
        .filter_map(|l| {
            let eq = l.iter().position(|&c| c == b'=')?;
            Some((l[..eq].to_vec(), l[eq + 1..].to_vec()))
        })
        .take(MAX_VARS)
        .collect();
    let mut args: Vec<&[u8]> = vec![b"prog"];
    args.extend(args_text.split(|&c| c == b' ').filter(|a| !a.is_empty()).take(64));
    let mut env = Env { vars, args };
    let loc = liella_fuzz::locale(flags & 0x80 != 0);
    let wflags = i32::from(flags) & (wordexp::NOCMD | wordexp::SHOWERR | wordexp::UNDEF);
    let mut fields = Fields(0);
    if let Err(e) = wordexp::wordexp(words, wflags, &loc, &mut env, &mut fields) {
        assert!((1..=5).contains(&e.code()));
    }
    // The arithmetic evaluator on the words as an expression (`$((...))`'s body).
    let mut var = |name: &[u8], out: &mut ValBuf| -> Result<bool> {
        out.clear();
        if name.first() == Some(&b'x') {
            out.extend_from_slice(b"7")?;
            Ok(true)
        } else {
            Ok(false)
        }
    };
    let _ = wordexp::arith::evaluate(words, flags & 0x20 != 0, &mut var);
});
