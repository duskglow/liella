//! The `B*` speed codes of the C `struct termios` (spec §19.1 ⚑ HIST; D-19.1): `cfgetispeed`,
//! `cfgetospeed`, `cfsetispeed`, `cfsetospeed`, `cfsetspeed` and `cfmakeraw`, on the caller's
//! structure. The terminal itself takes integer rates (`liella::term`); the conversion is
//! `liella_abi::pack::{termios, termios_in}`.
//!
//! The structure holds one speed code, in `c_cflag`'s `CBAUD` bits: the output speed, which is
//! also the input speed. `cfsetispeed` with a speed other than 0 therefore sets both, and the
//! getters ignore the `CIBAUD` bits (as the reference does). The one input speed of its own is 0,
//! "the same as the output speed" when applied (POSIX), which `cfgetispeed` reports as `B0`; it is
//! recorded in `c_ispeed` ([`ISPEED_ZERO`]). `c_ospeed` and `c_ispeed` otherwise hold the rates,
//! which count for the code `BOTHER` (`liella::term::Termios`).

use liella::error::{Errno, Result};
use liella::term::{self, BOTHER, CBAUD, CIBAUD, CS8, RAW_CFLAG_OFF, RAW_IFLAG_OFF, RAW_LFLAG_OFF, RAW_OFLAG_OFF, VMIN, VTIME};
use liella_abi::layout::struct_termios;
use liella_abi::pack::ISPEED_ZERO;

/// Whether `v` is a speed code the structure can hold: a standard one or `BOTHER`.
fn is_code(v: u32) -> bool {
    v == BOTHER || term::code_rate(v).is_some()
}

/// `cfgetospeed`: the output speed code.
pub fn ospeed(t: &struct_termios) -> u32 {
    t.c_cflag & CBAUD
}

/// `cfgetispeed`: `B0` after `cfsetispeed(t, 0)`, else the output speed code.
pub fn ispeed(t: &struct_termios) -> u32 {
    if t.c_ispeed == ISPEED_ZERO { 0 } else { t.c_cflag & CBAUD }
}

/// Make `code` the structure's speed code (its rate in `c_ospeed`; `BOTHER` keeps the rate).
fn set_code(t: &mut struct_termios, code: u32) {
    t.c_cflag = (t.c_cflag & !CBAUD) | code;
    if let Some(rate) = term::code_rate(code) {
        t.c_ospeed = rate;
    }
}

/// `cfsetospeed`: a speed code or `BOTHER`; anything else is `EINVAL` and changes nothing. An
/// input speed of 0 set before stays.
pub fn set_ospeed(t: &mut struct_termios, v: u32) -> Result<()> {
    if !is_code(v) {
        return Err(Errno::EINVAL);
    }
    set_code(t, v);
    Ok(())
}

/// `cfsetispeed`: 0 makes the input speed follow the output speed; another speed code (or
/// `BOTHER`) is the speed of both directions; anything else is `EINVAL`.
pub fn set_ispeed(t: &mut struct_termios, v: u32) -> Result<()> {
    if v == 0 {
        t.c_ispeed = ISPEED_ZERO;
        return Ok(());
    }
    both(t, v)
}

/// Both speeds as `code`.
fn both(t: &mut struct_termios, code: u32) -> Result<()> {
    if !is_code(code) {
        return Err(Errno::EINVAL);
    }
    set_code(t, code);
    t.c_cflag &= !CIBAUD;
    t.c_ispeed = t.c_ospeed;
    Ok(())
}

/// `cfsetspeed`: both speeds, given as a speed code or as the rate of one (9600 is `B9600`).
/// `BOTHER` and other numbers are `EINVAL`.
pub fn set_speed(t: &mut struct_termios, v: u32) -> Result<()> {
    let code = if term::code_rate(v).is_some() { v } else { term::rate_code(v).ok_or(Errno::EINVAL)? };
    both(t, code)
}

/// `cfmakeraw` (`liella::term::Termios::make_raw`), on the C structure: the speeds and the
/// other control characters are kept.
pub fn make_raw(t: &mut struct_termios) {
    t.c_iflag &= !RAW_IFLAG_OFF;
    t.c_oflag &= !RAW_OFLAG_OFF;
    t.c_cflag = (t.c_cflag & !RAW_CFLAG_OFF) | CS8;
    t.c_lflag &= !RAW_LFLAG_OFF;
    if let Some(v) = t.c_cc.get_mut(VMIN) {
        *v = 1;
    }
    if let Some(v) = t.c_cc.get_mut(VTIME) {
        *v = 0;
    }
}
