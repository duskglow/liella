//! Static initializers of spec Appendix C (`PTHREAD_MUTEX_INITIALIZER`, ...) as Liella defines
//! them: the bytes a C program's static initializer stores, which the runtime must accept as a
//! valid object of the type (spec §22: statically initialised objects need no init call).
//!
//! Each value is built from the member offsets of the typed `layout` definitions and the kind
//! constants of spec §22, not copied from the reference table; the Appendix C tests compare the
//! two.

#![allow(clippy::indexing_slicing)]

use core::mem::{offset_of, size_of};

use crate::layout::{mbstate_t, once_flag, pthread_cond_t, pthread_mutex_t, pthread_once_t, pthread_rwlock_t};

/// `PTHREAD_MUTEX_NORMAL`/`PTHREAD_MUTEX_TIMED_NP`, `..._RECURSIVE_NP`, `..._ERRORCHECK_NP`,
/// `..._ADAPTIVE_NP` (spec §22.3 mutex kinds).
const KIND_RECURSIVE: u8 = 1;
const KIND_ERRORCHECK: u8 = 2;
const KIND_ADAPTIVE: u8 = 3;
/// `PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP` (spec §22.4).
const RWLOCK_PREFER_WRITER_NONRECURSIVE: u8 = 2;

const MUTEX_SIZE: usize = size_of::<pthread_mutex_t>();
const RWLOCK_SIZE: usize = size_of::<pthread_rwlock_t>();

const fn mutex_of_kind(kind: u8) -> [u8; MUTEX_SIZE] {
    let mut b = [0u8; MUTEX_SIZE];
    b[offset_of!(pthread_mutex_t, __data.__kind)] = kind;
    b
}

/// `PTHREAD_MUTEX_INITIALIZER`: all zero (a normal mutex, unlocked).
pub const PTHREAD_MUTEX_INITIALIZER: [u8; MUTEX_SIZE] = mutex_of_kind(0);
/// `PTHREAD_RECURSIVE_MUTEX_INITIALIZER_NP`.
pub const PTHREAD_RECURSIVE_MUTEX_INITIALIZER_NP: [u8; MUTEX_SIZE] = mutex_of_kind(KIND_RECURSIVE);
/// `PTHREAD_ERRORCHECK_MUTEX_INITIALIZER_NP`.
pub const PTHREAD_ERRORCHECK_MUTEX_INITIALIZER_NP: [u8; MUTEX_SIZE] = mutex_of_kind(KIND_ERRORCHECK);
/// `PTHREAD_ADAPTIVE_MUTEX_INITIALIZER_NP`.
pub const PTHREAD_ADAPTIVE_MUTEX_INITIALIZER_NP: [u8; MUTEX_SIZE] = mutex_of_kind(KIND_ADAPTIVE);

/// `PTHREAD_RWLOCK_INITIALIZER`: all zero (reader-preferring, unlocked).
pub const PTHREAD_RWLOCK_INITIALIZER: [u8; RWLOCK_SIZE] = [0; RWLOCK_SIZE];
/// `PTHREAD_RWLOCK_WRITER_NONRECURSIVE_INITIALIZER_NP`: the preference in `__flags`.
pub const PTHREAD_RWLOCK_WRITER_NONRECURSIVE_INITIALIZER_NP: [u8; RWLOCK_SIZE] = {
    let mut b = [0u8; RWLOCK_SIZE];
    b[offset_of!(pthread_rwlock_t, __data.__flags)] = RWLOCK_PREFER_WRITER_NONRECURSIVE;
    b
};

/// `PTHREAD_COND_INITIALIZER`: all zero.
pub const PTHREAD_COND_INITIALIZER: [u8; size_of::<pthread_cond_t>()] = [0; size_of::<pthread_cond_t>()];
/// `PTHREAD_ONCE_INIT`: 0 (not yet run).
pub const PTHREAD_ONCE_INIT: [u8; size_of::<pthread_once_t>()] = [0; size_of::<pthread_once_t>()];
/// `ONCE_FLAG_INIT` (C11): 0.
pub const ONCE_FLAG_INIT: [u8; size_of::<once_flag>()] = [0; size_of::<once_flag>()];
/// `mbstate_t` initialised with `{0}`: the initial conversion state.
pub const MBSTATE_T: [u8; size_of::<mbstate_t>()] = [0; size_of::<mbstate_t>()];
