//! The arithmetic lexer of `wordexp` (WORDEXP-FIXED): constants and operator spellings, including
//! the refusals, which the fast paths (direct operator match, one-pass decimal constants) must
//! keep. Clauses: s14_10_c_1, s14_10_c_2 (D-14.5).
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, missing_docs)]

use liella::error::Result;
use liella::pattern::wordexp::{self, ValBuf, WordError};

fn eval(text: &str) -> core::result::Result<i64, WordError> {
    let mut var = |name: &[u8], out: &mut ValBuf| -> Result<bool> {
        out.clear();
        if name == b"x" {
            out.extend_from_slice(b"7")?;
            Ok(true)
        } else {
            Ok(false)
        }
    };
    wordexp::arith::evaluate(text.as_bytes(), false, &mut var)
}

#[test]
fn constants_keep_their_radix_and_range() {
    // Clauses: s14_10_c_1 (D-14.5: constants, whole, in their radix).
    assert_eq!(eval("1234567"), Ok(1_234_567));
    assert_eq!(eval("9223372036854775807"), Ok(i64::MAX));
    assert_eq!(eval("9223372036854775808"), Err(WordError::Syntax));
    assert_eq!(eval("99999999999999999999"), Err(WordError::Syntax));
    assert_eq!(eval("010"), Ok(8));
    assert_eq!(eval("0x1F"), Ok(31));
    assert_eq!(eval("12ab"), Err(WordError::Syntax));
    assert_eq!(eval("12_"), Err(WordError::Syntax));
    assert_eq!(eval("08"), Err(WordError::Syntax));
    assert_eq!(eval("x*6"), Ok(42));
}

#[test]
fn operator_spellings_and_refusals() {
    // Clauses: s14_10_c_1 (D-14.5: the operators; assignment, ++, --, ** and , refused).
    assert_eq!(eval("3<4"), Ok(1));
    assert_eq!(eval("3>4"), Ok(0));
    assert_eq!(eval("3<=3"), Ok(1));
    assert_eq!(eval("3>=4"), Ok(0));
    assert_eq!(eval("1<<4"), Ok(16));
    assert_eq!(eval("256>>4"), Ok(16));
    assert_eq!(eval("3==3"), Ok(1));
    assert_eq!(eval("3!=3"), Ok(0));
    assert_eq!(eval("1&&2"), Ok(1));
    assert_eq!(eval("0||0"), Ok(0));
    assert_eq!(eval("6&3|8^1"), Ok(11));
    assert_eq!(eval("(1)?(2):(3)"), Ok(2));
    assert_eq!(eval("~0"), Ok(-1));
    assert_eq!(eval("!5"), Ok(0));
    assert_eq!(eval("-(-3)"), Ok(3));
    assert_eq!(eval(" 7 % 4 * 2 "), Ok(6));
    for bad in ["1<<=2", "1>>=2", "x+=1", "x-=1", "x*=1", "x/=1", "x%=1", "x&=1", "x^=1", "x|=1", "x=1", "1,2", "x++", "x--", "2**3", "1 @ 2", "1 )", "(1"] {
        assert_eq!(eval(bad), Err(WordError::Syntax), "{bad}");
    }
}
