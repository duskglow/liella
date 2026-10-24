//! The floating-point environment (spec §10.6). The logic is `liella_rt::fpu` (it needs the
//! machine's MXCSR and x87 access, D-10.5); this module names it for users of `liella-math`, whose
//! functions take the rounding direction as an explicit argument (rule C2) rather than reading it.

pub use liella_rt::fpu::{
    clear, disable, enable, enabled, get_env, get_mode, hold, raise, raise_c, restore_flags, rounding, set, set_env, set_mode,
    set_rounding, test, update, x87_rounding, Env, Flags, Mode, Rounding, X87_DEFAULT_CONTROL,
};
