//! Decoding a `va_list` by format (spec §12.6; D-12.4): the arguments a parsed format names are
//! read in order of their indices, each by its class, so that positional and sequential
//! formats read the same way.

use liella::error::KResult;
use liella::mem::InlineVec;
use liella::printf::{Arg, Class};
use liella_abi::va::VaSource;

/// The arguments `classes` describe, read from `src` in order. A registered type
/// (`register_printf_type`, D-12.8) is read by `user` (its number and object size), which runs
/// the type's own function on the same `va_list`.
pub fn read(classes: &[Class], src: &mut dyn VaSource, user: &mut dyn FnMut(u32, usize) -> KResult<Arg>, args: &mut InlineVec<Arg, 16>) -> KResult<()> {
    for c in classes {
        args.push(match *c {
            Class::Int => Arg::Int(src.int()),
            Class::Double => Arg::Double(src.double()),
            Class::LongDouble => Arg::LongDouble(src.long_double()),
            Class::User { ty, size } => user(ty, size)?,
        })?;
    }
    Ok(())
}
