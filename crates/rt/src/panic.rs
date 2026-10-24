//! The panic path of the shipped products: a diagnostic, then abort. A panic in the library is
//! a bug (the library never panics on any input), so the message names the source location and
//! nothing else; the `#[panic_handler]` attribute itself is in the product crate.

use core::panic::PanicInfo;

use liella_kernel::sys::Sys;

use crate::fatal;

/// Report `info` and abort.
pub fn panicked<S: Sys>(sys: &S, info: &PanicInfo<'_>, trap: impl FnOnce()) -> ! {
    let mut digits = [0u8; 20];
    match info.location() {
        Some(l) => {
            let line = fatal::decimal(u64::from(l.line()), &mut digits);
            fatal::fatal(sys, &[b"liella: internal error at ", l.file().as_bytes(), b":", line, b"\n"], trap)
        }
        None => fatal::fatal(sys, &[b"liella: internal error\n"], trap),
    }
}
