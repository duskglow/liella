//! Plural-Forms rules of GNU message catalogs (spec §7.7; the GNU gettext manual, "Additional
//! functions for plural forms"; `spec/behaviour/catalogs.md`, "Plural forms").
//!
//! A catalog's header entry may carry `Plural-Forms: nplurals=N; plural=EXPR;`. `EXPR` is a C-like
//! expression in `n` (an `unsigned long`): `?:` (right-associative), `||`, `&&`, `==`, `!=`, `<`,
//! `>`, `<=`, `>=`, `+`, `-`, `*`, `/`, `%`, unary `!`, parentheses and decimal constants, with C's
//! precedence, evaluated in 64-bit unsigned arithmetic that wraps.
//!
//! The expression is compiled once into a postfix program ([`compile`]) held as bytes, so a caller
//! can keep it anywhere bytes can be kept, and evaluated with an explicit stack ([`evaluate`]):
//! neither step recurses, so 4000 nested parentheses cost no machine stack.
//!
//! **Never trapping** (decision; the reference library is killed by `SIGFPE`): a division or
//! remainder by zero gives no value, and neither does any operation on no value; `&&`, `||` and
//! `?:` discard such a value where C would not have evaluated it (`n != 0 && 10 / n`, `n ? 10 / n :
//! 0`). An evaluation that ends without a value uses the default rule `n != 1`, the rule of a
//! catalog without a usable `Plural-Forms`.
//!
//! **Unusable rules** (as observed): a missing header field, a missing or non-numeric `nplurals`, a
//! missing `plural` (a blank between a setting's name and its `=` counts as missing), or an
//! expression that does not parse (unary minus included) select the default rule `nplurals=2;
//! plural=n != 1;`. A constant beyond 64 bits does not parse (decision).

use crate::error::Errno;
use crate::mem::TryVec;

/// Push `n`.
const OP_N: u8 = 0;
/// Push the following 8 bytes (little-endian).
const OP_CONST: u8 = 1;
const OP_NOT: u8 = 2;
const OP_MUL: u8 = 3;
const OP_DIV: u8 = 4;
const OP_REM: u8 = 5;
const OP_ADD: u8 = 6;
const OP_SUB: u8 = 7;
const OP_LT: u8 = 8;
const OP_GT: u8 = 9;
const OP_LE: u8 = 10;
const OP_GE: u8 = 11;
const OP_EQ: u8 = 12;
const OP_NE: u8 = 13;
const OP_AND: u8 = 14;
const OP_OR: u8 = 15;
/// `c ? a : b` (pops `b`, `a`, `c`).
const OP_COND: u8 = 16;

/// Operator-stack markers of the parser.
const MARK_PAREN: u8 = 0x80;
const MARK_QUESTION: u8 = 0x81;
const MARK_COLON: u8 = 0x82;

/// The program layout: `u64 nplurals`, `u32 stack depth`, then the postfix code.
const HEAD: usize = 12;

/// The binding strength of an operator (higher binds tighter); `?:` is 0.
fn precedence(op: u8) -> u8 {
    match op {
        OP_NOT => 7,
        OP_MUL | OP_DIV | OP_REM => 6,
        OP_ADD | OP_SUB => 5,
        OP_LT | OP_GT | OP_LE | OP_GE => 4,
        OP_EQ | OP_NE => 3,
        OP_AND => 2,
        OP_OR => 1,
        _ => 0,
    }
}

/// One token of an expression.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tok {
    N,
    Num(u64),
    Op(u8),
    Bang,
    Question,
    Colon,
    Open,
    Close,
}

/// The token at the start of `s` (blanks skipped) and the rest; `Ok(None)` at the end of the
/// expression (`;`, a newline or the end of the text).
fn token(s: &[u8]) -> Result<Option<(Tok, &[u8])>, ()> {
    let s = s.get(s.iter().position(|&b| b != b' ' && b != b'\t').unwrap_or(s.len())..).unwrap_or(&[]);
    let Some(&c) = s.first() else { return Ok(None) };
    let next = s.get(1).copied().unwrap_or(0);
    let two = |t: Tok| Ok(Some((t, s.get(2..).unwrap_or(&[]))));
    let one = |t: Tok| Ok(Some((t, s.get(1..).unwrap_or(&[]))));
    match (c, next) {
        (b';' | b'\n', _) => Ok(None),
        (b'n', _) => one(Tok::N),
        (b'0'..=b'9', _) => {
            let digits = s.iter().take_while(|b| b.is_ascii_digit()).count();
            let mut v: u64 = 0;
            for &d in s.get(..digits).unwrap_or(&[]) {
                v = v.checked_mul(10).and_then(|v| v.checked_add(u64::from(d.wrapping_sub(b'0')))).ok_or(())?;
            }
            Ok(Some((Tok::Num(v), s.get(digits..).unwrap_or(&[]))))
        }
        (b'|', b'|') => two(Tok::Op(OP_OR)),
        (b'&', b'&') => two(Tok::Op(OP_AND)),
        (b'=', b'=') => two(Tok::Op(OP_EQ)),
        (b'!', b'=') => two(Tok::Op(OP_NE)),
        (b'<', b'=') => two(Tok::Op(OP_LE)),
        (b'>', b'=') => two(Tok::Op(OP_GE)),
        (b'<', _) => one(Tok::Op(OP_LT)),
        (b'>', _) => one(Tok::Op(OP_GT)),
        (b'+', _) => one(Tok::Op(OP_ADD)),
        (b'-', _) => one(Tok::Op(OP_SUB)),
        (b'*', _) => one(Tok::Op(OP_MUL)),
        (b'/', _) => one(Tok::Op(OP_DIV)),
        (b'%', _) => one(Tok::Op(OP_REM)),
        (b'!', _) => one(Tok::Bang),
        (b'?', _) => one(Tok::Question),
        (b':', _) => one(Tok::Colon),
        (b'(', _) => one(Tok::Open),
        (b')', _) => one(Tok::Close),
        _ => Err(()),
    }
}

/// Emit `op` (an operator or a `:` marker) into the postfix code.
fn emit(code: &mut TryVec<u8>, op: u8) -> Result<(), Errno> {
    code.push(if op == MARK_COLON { OP_COND } else { op })
}

/// Translate an infix expression to postfix code (a shunting-yard pass: no recursion).
/// `Err(None)` for a syntax error, `Err(Some(e))` for an allocation failure.
fn to_postfix(mut s: &[u8], code: &mut TryVec<u8>) -> Result<(), Option<Errno>> {
    let mut ops: TryVec<u8> = TryVec::new();
    let mut operand = true;
    while let Some((t, rest)) = token(s).map_err(|()| None)? {
        s = rest;
        match (operand, t) {
            (true, Tok::N) => {
                code.push(OP_N)?;
                operand = false;
            }
            (true, Tok::Num(v)) => {
                code.push(OP_CONST)?;
                code.extend_from_slice(&v.to_le_bytes())?;
                operand = false;
            }
            (true, Tok::Bang) => ops.push(OP_NOT)?,
            (true, Tok::Open) => ops.push(MARK_PAREN)?,
            (false, Tok::Op(op)) => {
                // Pop what binds at least as tightly (all binary operators are left-associative);
                // markers stop the popping.
                while let Some(&top) = ops.as_slice().last() {
                    if top >= MARK_PAREN || precedence(top) < precedence(op) {
                        break;
                    }
                    emit(code, top)?;
                    ops.pop();
                }
                ops.push(op)?;
                operand = true;
            }
            (false, Tok::Question) => {
                while let Some(&top) = ops.as_slice().last() {
                    if top >= MARK_PAREN {
                        break;
                    }
                    emit(code, top)?;
                    ops.pop();
                }
                ops.push(MARK_QUESTION)?;
                operand = true;
            }
            (false, Tok::Colon) => {
                // Close the innermost open `?`: everything above it is complete.
                loop {
                    match ops.pop() {
                        Some(MARK_QUESTION) => break,
                        Some(MARK_PAREN) | None => return Err(None),
                        Some(op) => emit(code, op)?,
                    }
                }
                ops.push(MARK_COLON)?;
                operand = true;
            }
            (false, Tok::Close) => {
                loop {
                    match ops.pop() {
                        Some(MARK_PAREN) => break,
                        Some(MARK_QUESTION) | None => return Err(None),
                        Some(op) => emit(code, op)?,
                    }
                }
            }
            _ => return Err(None),
        }
    }
    if operand {
        return Err(None);
    }
    while let Some(op) = ops.pop() {
        if op == MARK_PAREN || op == MARK_QUESTION {
            return Err(None);
        }
        emit(code, op)?;
    }
    Ok(())
}

/// The stack depth `code` needs, or `None` if it is not a well-formed program of one value.
fn depth(code: &[u8]) -> Option<u32> {
    let (mut d, mut max, mut i) = (0u32, 0u32, 0usize);
    while let Some(&op) = code.get(i) {
        i = i.checked_add(1)?;
        match op {
            OP_N => d = d.checked_add(1)?,
            OP_CONST => {
                d = d.checked_add(1)?;
                i = i.checked_add(8)?;
            }
            OP_NOT => {
                if d < 1 {
                    return None;
                }
            }
            OP_COND => d = d.checked_sub(3)?.checked_add(1)?,
            _ => d = d.checked_sub(2)?.checked_add(1)?,
        }
        max = max.max(d);
    }
    (d == 1 && i == code.len()).then_some(max)
}

/// Compile the rule of a catalog's header entry: the program [`evaluate`] runs, or `Ok(None)`
/// when the header has no usable `Plural-Forms` (the default rule applies).
pub fn compile(header: &[u8]) -> Result<Option<TryVec<u8>>, Errno> {
    let Some((nplurals, expr)) = field(header) else { return Ok(None) };
    let mut code = TryVec::new();
    match to_postfix(expr, &mut code) {
        Ok(()) => {}
        Err(None) => return Ok(None),
        Err(Some(e)) => return Err(e),
    }
    let Some(d) = depth(code.as_slice()) else { return Ok(None) };
    let mut prog = TryVec::with_capacity(HEAD.saturating_add(code.len()))?;
    prog.extend_from_slice(&nplurals.to_le_bytes())?;
    prog.extend_from_slice(&d.to_le_bytes())?;
    prog.extend_from_slice(code.as_slice())?;
    Ok(Some(prog))
}

/// Whether `a` starts with `b`, letters compared without regard to case.
fn starts_ci(a: &[u8], b: &[u8]) -> bool {
    a.get(..b.len()).is_some_and(|p| p.eq_ignore_ascii_case(b))
}

/// The text after the first `key` in `s`.
fn after<'a>(s: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let at = s.windows(key.len()).position(|w| w == key)?;
    s.get(at.checked_add(key.len())?..)
}

/// The `nplurals` value and the `plural` expression text of the header's `Plural-Forms` field, or
/// `None` if either is missing or `nplurals` does not start with a number. As observed: the field
/// is found anywhere in the header, its name in any case; on its line, the settings are the text
/// after the first `nplurals=` and the first `plural=`, in either order (a blank after `=` is
/// skipped; one before `=` leaves the setting missing; `Xnplurals=2` and `nplurals=2x` count).
fn field(header: &[u8]) -> Option<(u64, &[u8])> {
    const NAME: &[u8] = b"plural-forms:";
    let at = (0..header.len()).find(|&i| header.get(i..).is_some_and(|t| starts_ci(t, NAME)))?;
    let value = header.get(at.checked_add(NAME.len())?..)?;
    let value = value.get(..value.iter().position(|&b| b == b'\n').unwrap_or(value.len()))?;
    let blanks = |v: &[u8]| v.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
    let v = after(value, b"nplurals=")?;
    let v = v.get(blanks(v)..)?;
    let digits = v.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let mut nplurals: u64 = 0;
    for &d in v.get(..digits)? {
        nplurals = nplurals.checked_mul(10)?.checked_add(u64::from(d.wrapping_sub(b'0')))?;
    }
    let e = after(value, b"plural=")?;
    Some((nplurals, e.get(..e.iter().position(|&b| b == b';').unwrap_or(e.len()))?))
}

/// The form index for `n` under the default rule `n != 1`.
fn default_index(n: u64) -> u64 {
    u64::from(n != 1)
}

fn word(prog: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(prog.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

/// The plural form index for `n`: the rule's value, or 0 when it is not below `nplurals` (as
/// observed). `prog` is a program [`compile`] made, or `None` for the default rule. An evaluation
/// without a value (a division by zero) uses the default rule (see the module documentation).
pub fn evaluate(prog: Option<&[u8]>, n: u64) -> u64 {
    let Some(prog) = prog else { return default_index(n) };
    let nplurals = word(prog, 0).unwrap_or(2);
    // The most common rule, `n != 1`, without the interpreter (its value always exists).
    let index = if prog.get(HEAD..) == Some(&NOT_ONE[..]) { u64::from(n != 1) } else { run(prog, n).unwrap_or_else(|| default_index(n)) };
    if index < nplurals { index } else { 0 }
}

/// The code [`compile`] makes for `n != 1` (`OP_CONST` takes a little-endian word).
const NOT_ONE: [u8; 11] = [OP_N, OP_CONST, 1, 0, 0, 0, 0, 0, 0, 0, OP_NE];

/// The deepest stack [`run`] keeps in a local array (real rules need a handful of levels); a
/// deeper program gets its stack from the heap.
const INLINE_DEPTH: usize = 16;

/// Run the postfix code: the value, or `None` (no value, or a malformed program).
fn run(prog: &[u8], n: u64) -> Option<u64> {
    let d = usize::try_from(u32::from_le_bytes(prog.get(8..HEAD)?.try_into().ok()?)).ok()?;
    let code = prog.get(HEAD..)?;
    // One slot per level (`compile` checked the program needs `d`); `None` is "no value". No
    // allocation for the usual depth (GETTEXT-3: one per plural lookup before).
    if d <= INLINE_DEPTH {
        let mut inline = [None; INLINE_DEPTH];
        return execute(code, n, inline.get_mut(..d)?);
    }
    let mut heap: TryVec<Option<u64>> = TryVec::new();
    heap.resize(d, None).ok()?;
    execute(code, n, heap.as_mut_slice())
}

/// [`run`]'s loop over `code` with `stack` (its length: the most levels the program may use; a
/// program that needs more has no value).
fn execute(code: &[u8], n: u64, stack: &mut [Option<u64>]) -> Option<u64> {
    let mut sp = 0usize;
    let mut i = 0usize;
    while let Some(&op) = code.get(i) {
        i = i.checked_add(1)?;
        let mut pop = || -> Option<Option<u64>> {
            sp = sp.checked_sub(1)?;
            stack.get(sp).copied()
        };
        let v = match op {
            OP_N => Some(n),
            OP_CONST => {
                let c = word(code, i)?;
                i = i.checked_add(8)?;
                Some(c)
            }
            OP_NOT => pop()?.map(|a| u64::from(a == 0)),
            OP_COND => {
                let (b, a, c) = (pop()?, pop()?, pop()?);
                match c {
                    None => None,
                    Some(0) => b,
                    Some(_) => a,
                }
            }
            _ => {
                let (b, a) = (pop()?, pop()?);
                binary(op, a, b)
            }
        };
        *stack.get_mut(sp)? = v;
        sp = sp.checked_add(1)?;
    }
    sp = sp.checked_sub(1)?;
    stack.get(sp).copied()?
}

/// A binary operation; `&&` and `||` discard a missing right operand where C would not have
/// evaluated it.
fn binary(op: u8, a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match op {
        OP_AND => match a? {
            0 => Some(0),
            _ => b.map(|b| u64::from(b != 0)),
        },
        OP_OR => match a? {
            0 => b.map(|b| u64::from(b != 0)),
            _ => Some(1),
        },
        _ => {
            let (a, b) = (a?, b?);
            match op {
                OP_MUL => Some(a.wrapping_mul(b)),
                OP_DIV => a.checked_div(b),
                OP_REM => a.checked_rem(b),
                OP_ADD => Some(a.wrapping_add(b)),
                OP_SUB => Some(a.wrapping_sub(b)),
                OP_LT => Some(u64::from(a < b)),
                OP_GT => Some(u64::from(a > b)),
                OP_LE => Some(u64::from(a <= b)),
                OP_GE => Some(u64::from(a >= b)),
                OP_EQ => Some(u64::from(a == b)),
                OP_NE => Some(u64::from(a != b)),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn idx(rule: &str, n: u64) -> u64 {
        let h = format!("Content-Type: text/plain; charset=UTF-8\nPlural-Forms: {rule}\n");
        let p = compile(h.as_bytes()).unwrap();
        evaluate(p.as_ref().map(|p| p.as_slice()), n)
    }

    #[test]
    fn real_rules() {
        let ru = "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);";
        let got: Vec<u64> = [0, 1, 2, 5, 11, 21, 22, 25, 111].iter().map(|&n| idx(ru, n)).collect();
        assert_eq!(got, [2, 0, 1, 2, 2, 0, 1, 2, 2]);
        assert_eq!(idx("nplurals=2; plural=n>1;", 0), 0);
        assert_eq!(idx("nplurals=2; plural=n>1;", 2), 1);
        assert_eq!(idx("nplurals=4; plural=1+2*n%3;", 1), 3);
        assert_eq!(idx("nplurals=3; plural=!n+!!n*2;", 0), 1);
        assert_eq!(idx("nplurals=3; plural=n==1?0:n==2?1:2;", 2), 1);
        assert_eq!(idx("nplurals=2; plural=n<2||n>10&&n<20;", 11), 1);
        assert_eq!(idx("nplurals=2; plural=n==010;", 10), 1);
        assert_eq!(idx("nplurals=2; plural=n==4294967297;", 4_294_967_297), 1);
        assert_eq!(idx("nplurals= 2 ;  plural= n == 1 ? 1 : 0 ;", 1), 1);
        assert_eq!(idx("  nplurals = 2 ;  plural = n == 1 ? 1 : 0 ;", 1), 0, "a blank before = leaves the setting missing");
        assert_eq!(idx("Xnplurals=2x; Xplural=n==1?1:0;", 1), 1);
        assert_eq!(idx("plural=n>1; nplurals=2;", 2), 1);
        assert_eq!(idx("nplurals=3; plural=n-5;", 7), 2);
        assert_eq!(idx("nplurals=3; plural=n-5;", 3), 0, "wraps to a huge value, beyond nplurals");
    }

    /// GETTEXT-3: `n != 1` takes the shortcut (its code is exactly [`NOT_ONE`]) and answers as the
    /// interpreter does; programs deeper than the local stack run on a heap stack, the others on
    /// the local one, with the same answers either side of the boundary.
    #[test]
    fn shortcut_and_stacks() {
        let h = "Content-Type: text/plain; charset=UTF-8\nPlural-Forms: nplurals=2; plural=n != 1;\n";
        let p = compile(h.as_bytes()).unwrap().unwrap();
        assert_eq!(&p.as_slice()[HEAD..], &NOT_ONE[..], "the compiled code of n != 1");
        for n in [0, 1, 2, 3, 11, 100, u64::MAX] {
            assert_eq!(evaluate(Some(p.as_slice()), n), u64::from(n != 1), "shortcut, n={n}");
            assert_eq!(run(p.as_slice(), n), Some(u64::from(n != 1)), "interpreter, n={n}");
        }
        for depth in [INLINE_DEPTH - 2, INLINE_DEPTH - 1, INLINE_DEPTH, INLINE_DEPTH + 1, 3 * INLINE_DEPTH] {
            // `0+(0+(...(n)...))!=1`: one stack level per `0+(`.
            let expr = format!("{}n{}!=1", "0+(".repeat(depth), ")".repeat(depth));
            let rule = format!("nplurals=2; plural={expr};");
            let p = compile(format!("Content-Type: text/plain; charset=UTF-8\nPlural-Forms: {rule}\n").as_bytes()).unwrap().unwrap();
            let d = usize::try_from(u32::from_le_bytes(p.as_slice()[8..HEAD].try_into().unwrap())).unwrap();
            assert!(d > depth, "depth {d} for {depth} levels");
            for n in [0, 1, 2, 7] {
                assert_eq!(idx(&rule, n), u64::from(n != 1), "{depth} levels, n={n}");
            }
        }
    }

    #[test]
    fn unusable_rules_fall_back_to_n_ne_1() {
        for rule in ["nplurals=2; plural=n !=;", "nplurals=x; plural=n!=1;", "nplurals=2;", "plural=n!=1;", "nplurals=2; plural=-n;", "nplurals=2; plural=n>99999999999999999999;"] {
            assert_eq!(idx(rule, 1), 0, "{rule}");
            assert_eq!(idx(rule, 0), 1, "{rule}");
        }
        assert_eq!(evaluate(None, 5), 1);
    }

    #[test]
    fn never_traps() {
        assert_eq!(idx("nplurals=2; plural=n/0;", 5), 1, "the default rule");
        assert_eq!(idx("nplurals=2; plural=n%0;", 1), 0);
        assert_eq!(idx("nplurals=3; plural=10/n;", 0), 1);
        assert_eq!(idx("nplurals=3; plural=10/n;", 5), 2);
        assert_eq!(idx("nplurals=3; plural=n!=0 && 10/n>1;", 0), 0, "&& does not evaluate its right side");
        assert_eq!(idx("nplurals=3; plural=n==0 ? 2 : 10/n;", 0), 2);
        let deep = format!("nplurals=2; plural={}n{}!=1;", "(".repeat(4000), ")".repeat(4000));
        assert_eq!(idx(&deep, 1), 0);
        assert_eq!(idx(&deep, 2), 1);
    }
}
