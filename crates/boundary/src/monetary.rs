//! `strfmon` glue (spec §7.5; D-7.11): the locale's `LC_MONETARY` conventions, the amounts
//! from the C `va_list` and the current rounding mode, for `liella::monetary`. Only the C ABI
//! translation lives here; the format language is `liella::monetary::format`.

use liella::error::KResult;
use liella::monetary::{self, Amounts, Conventions};
use liella::num::float::{Float, Format};
use liella_abi::va::VaSource;
use liella_kernel::machine::Machine;

use crate::locale;
use crate::sys::CSys;
use crate::va_source::CVa;

/// The amounts of a C `va_list`: each conversion takes the next `double`, or with `L` the next
/// x87 `long double`.
struct Args(CVa);

impl Amounts for Args {
    fn amount(&mut self, long_double: bool) -> Float {
        if long_double {
            Float::unpack(self.0.long_double(), Format::F80)
        } else {
            Float::unpack(u128::from(self.0.double()), Format::F64)
        }
    }
}

/// `strfmon_l(s, max, loc, fmt, ap)` (`s` the caller's `max` bytes; `loc` 0: the calling
/// thread's locale; `ap` the address of the C `va_list`): the length written, the terminator
/// excluded.
pub fn vstrfmon(s: KResult<&mut [u8]>, loc: usize, fmt: KResult<&[u8]>, ap: usize) -> KResult<usize> {
    let (buf, fmt) = (s?, fmt?);
    let l = if loc == 0 { locale::current() } else { locale::of(loc) };
    // ARCH(x86_64): rounding mode read from MXCSR; FPCR on aarch64 (A-9)
    let mode = liella_legacy::num_compat::rounding(CSys.mxcsr());
    monetary::format(buf, fmt, &Conventions::of(l), &mut Args(CVa(ap)), mode)
}
