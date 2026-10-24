//! C callback types (safe to call in Rust: the export signatures type them) and the
//! foreign-unwind policy. Finding F12.
//!
//! The callbacks of the search, sort, tree, `ftw`/`nftw`, `scandir` and `fts` functions are
//! plain `extern "C"` (OD-6 revised, D-14.6): a C++ exception one throws passes the library's
//! Rust frames, which have no landing pads (the products are `panic = "abort"`), by their
//! call-frame information and reaches the C caller, as in glibc; the library puts its state
//! back in order on the way (`boundary::posix::s22_threads::guarded`). As `"C-unwind"` types they would
//! get terminate landing pads, so the same throw would abort the process. The `glob`
//! callbacks stay `"C-unwind"` (a throw from them aborts: D-14.6).

use crate::ctypes::{c_int, c_void};

/// `__compar_fn_t`: `int (*)(const void *, const void *)`.
pub type CompareFn = extern "C" fn(*const c_void, *const c_void) -> c_int;
/// `__compar_d_fn_t`: `int (*)(const void *, const void *, void *)` (`qsort_r`, GNU order).
pub type CompareCtxFn = extern "C" fn(*const c_void, *const c_void, *mut c_void) -> c_int;
/// `__action_fn_t`: `void (*)(const void *node, VISIT, int depth)` (`twalk`).
pub type ActionFn = extern "C" fn(*const c_void, c_int, c_int);
/// `void (*)(const void *node, VISIT, void *closure)` (`twalk_r`).
pub type ActionCtxFn = extern "C" fn(*const c_void, c_int, *mut c_void);
/// `__free_fn_t`: `void (*)(void *)` (`tdestroy`).
pub type FreeFn = extern "C" fn(*mut c_void);
/// `__ftw_func_t`: `int (*)(const char *path, const struct stat *st, int typeflag)` (`ftw`).
pub type FtwFn = extern "C" fn(*const crate::ctypes::c_char, *const c_void, c_int) -> c_int;
/// `__nftw_func_t`: `int (*)(const char *path, const struct stat *st, int typeflag, struct FTW
/// *ftw)` (`nftw`).
pub type NftwFn = extern "C" fn(*const crate::ctypes::c_char, *const c_void, c_int, *mut c_void) -> c_int;
/// `int (*)(const struct dirent *)`: `scandir`'s filter.
pub type DirFilterFn = extern "C" fn(*const c_void) -> c_int;
/// `int (*)(const T **, const T **)`: the comparison of `scandir` (`T` = `struct dirent`) and
/// of `fts_open` (`T` = `FTSENT`), called with the addresses of two array slots.
pub type SlotCompareFn = extern "C" fn(*const *const c_void, *const *const c_void) -> c_int;
/// `int (*)(const char *epath, int eerrno)`: `glob`'s error function.
pub type GlobErrFn = extern "C-unwind" fn(*const crate::ctypes::c_char, c_int) -> c_int;
/// `void (*)(void *)`: `glob_t::gl_closedir`.
pub type GlobClosedirFn = extern "C-unwind" fn(*mut c_void);
/// `struct dirent *(*)(void *)`: `glob_t::gl_readdir`.
pub type GlobReaddirFn = extern "C-unwind" fn(*mut c_void) -> *mut c_void;
/// `void *(*)(const char *)`: `glob_t::gl_opendir`.
pub type GlobOpendirFn = extern "C-unwind" fn(*const crate::ctypes::c_char) -> *mut c_void;
/// `int (*)(const char *, struct stat *)`: `glob_t::gl_lstat` and `gl_stat`.
pub type GlobStatFn = extern "C-unwind" fn(*const crate::ctypes::c_char, *mut c_void) -> c_int;

/// `glob_t` (and `glob64_t`, the same layout on x86-64) with its `GLOB_ALTDIRFUNC` members
/// typed as callbacks: the layout of `layout::glob_t` (spec Appendix C), whose function members
/// are plain addresses there. A NULL member is `None`.
#[repr(C)]
pub struct GlobT {
    /// `gl_pathc`.
    pub gl_pathc: u64,
    /// `gl_pathv`.
    pub gl_pathv: usize,
    /// `gl_offs`.
    pub gl_offs: u64,
    /// `gl_flags`.
    pub gl_flags: c_int,
    /// `gl_closedir`.
    pub gl_closedir: Option<GlobClosedirFn>,
    /// `gl_readdir`.
    pub gl_readdir: Option<GlobReaddirFn>,
    /// `gl_opendir`.
    pub gl_opendir: Option<GlobOpendirFn>,
    /// `gl_lstat`.
    pub gl_lstat: Option<GlobStatFn>,
    /// `gl_stat`.
    pub gl_stat: Option<GlobStatFn>,
}

const _: () = assert!(size_of::<GlobT>() == size_of::<crate::layout::glob_t>() && align_of::<GlobT>() == align_of::<crate::layout::glob_t>());
