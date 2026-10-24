//! Category *export*: the `export!` macro.
//!
//! ```ignore
//! export! { "GLIBC_2.2.5";
//! pub extern "C" fn read(fd: c_int, buf: *mut c_void, n: size_t) -> ssize_t { ... }
//! }
//! ```
//!
//! * With feature `export` (the shipped products only) the function gets
//!   `#[unsafe(no_mangle)]` and becomes a C symbol. Test builds leave it mangled, so tests can
//!   call every translation as an ordinary Rust function without interposing on the host C
//!   library (D-X.5).
//! * The version string names the symbol's version node (spec Appendix B). It is recorded in
//!   the documentation of the function; the products' version script is generated from
//!   `spec/abi/versions.tsv` (phase 4).

/// Define one exported C function (see the module documentation). `export! { runtime "V"; ... }`
/// exports only with feature `runtime`; `export! { asm "V"; ... }` never exports (the symbol is
/// the assembly's, D-4.22 COPY-FILL; the function is its tests' Rust twin); the process-runtime symbols (`__libc_start_main`, `exit`,
/// `__errno_location`, ...) that are meaningful only in a process Liella started.
/// `export! { libm "V"; ... }` exports only with feature `libm` (the `libm` product's own
/// symbols), and `export! { libc_libm "V"; ... }` with either feature (the math functions glibc
/// exports from both `libc.so.6` and `libm.so.6`, such as `frexp`; D-10.7).
#[macro_export]
macro_rules! export {
    (libm $version:literal; $(#[$meta:meta])* pub extern "C" fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block) => {
        $(#[$meta])*
        #[doc = concat!("C symbol `", stringify!($name), "@", $version, "` (the libm product: feature `libm`).")]
        // UNSAFE(export): a C symbol needs an unmangled name; names are unique per version node
        // (spec Appendix B).
        #[cfg_attr(feature = "libm", unsafe(no_mangle))]
        #[allow(unsafe_code)]
        // The names are the ABI's (glibc's mixed-case symbols), not ours to rename.
        #[allow(non_snake_case)]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $name($($arg: $ty),*) $(-> $ret)? $body
    };
    (libc_libm $version:literal; $(#[$meta:meta])* pub extern "C" fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block) => {
        $(#[$meta])*
        #[doc = concat!("C symbol `", stringify!($name), "@", $version, "` (both products: feature `export` or `libm`).")]
        // UNSAFE(export): a C symbol needs an unmangled name; names are unique per version node
        // (spec Appendix B). The libm product weakens its copy, so a static link takes libc's.
        #[cfg_attr(any(feature = "export", feature = "libm"), unsafe(no_mangle))]
        #[allow(unsafe_code)]
        // The names are the ABI's (glibc's mixed-case symbols), not ours to rename.
        #[allow(non_snake_case)]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $name($($arg: $ty),*) $(-> $ret)? $body
    };
    (asm $version:literal; $(#[$meta:meta])* pub extern "C" fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block) => {
        $(#[$meta])*
        #[doc = concat!("C symbol `", stringify!($name), "@", $version, "`, defined in assembly (csys/src/machine, under `LIELLA_EXPORT`); this Rust twin calls the same code and is what the tests call.")]
        // UNSAFE(export): no unmangled name here; the allow covers the body's `unsafe` blocks (each with
        // its own UNSAFE(slice) comment), as in the other arms.
        #[allow(unsafe_code)]
        // The names are the ABI's (glibc's mixed-case symbols), not ours to rename.
        #[allow(non_snake_case)]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $name($($arg: $ty),*) $(-> $ret)? $body
    };
    (local $cname:literal; $(#[$meta:meta])* pub extern "C" fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block) => {
        $(#[$meta])*
        #[doc = concat!("Local C name `", $cname, "` (every build): reached from the assembly in csys/src/machine, never exported (not in the version script).")]
        // UNSAFE(export): an unmangled name the assembly jumps to; unique (`liella_` prefix).
        #[unsafe(export_name = $cname)]
        #[allow(unsafe_code)]
        // The names are the ABI's (glibc's mixed-case symbols), not ours to rename.
        #[allow(non_snake_case)]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $name($($arg: $ty),*) $(-> $ret)? $body
    };
    (runtime $version:literal; $(#[$meta:meta])* pub extern "C" fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block) => {
        $(#[$meta])*
        #[doc = concat!("C symbol `", stringify!($name), "@", $version, "` (products only: feature `runtime`).")]
        // UNSAFE(export): a C symbol needs an unmangled name; names are unique per version node
        // (spec Appendix B).
        #[cfg_attr(feature = "runtime", unsafe(no_mangle))]
        #[allow(unsafe_code)]
        // The names are the ABI's (glibc's mixed-case symbols), not ours to rename.
        #[allow(non_snake_case)]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $name($($arg: $ty),*) $(-> $ret)? $body
    };
    ($version:literal; $(#[$meta:meta])* pub extern "C" fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block) => {
        $(#[$meta])*
        #[doc = concat!("C symbol `", stringify!($name), "@", $version, "`.")]
        // UNSAFE(export): a C symbol needs an unmangled name; the attribute is unsafe because
        // two symbols of one name would conflict at link time. Names are unique per version
        // node (spec Appendix B), checked by the products' symbol test.
        #[cfg_attr(feature = "export", unsafe(no_mangle))]
        #[allow(unsafe_code)]
        // The names are the ABI's (glibc's mixed-case symbols), not ours to rename.
        #[allow(non_snake_case)]
        // `memcpy`, `memset`, `memcmp`, `bcmp`, `strlen` and `memmove` are the runtime symbols
        // Rust's own code calls. The lint expects `unsafe extern "C" fn`; a safe `extern "C"
        // fn` has the same ABI. Their implementations never call themselves (D-4.11).
        #[allow(suspicious_runtime_symbol_definitions)]
        // The exports are C entry points: their preconditions are the C contracts stated in
        // each body's `UNSAFE(slice) SAFETY` comment. Rust calls them only from tests, as safe
        // functions (D-X.5), so they are not marked `unsafe`.
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $name($($arg: $ty),*) $(-> $ret)? $body
    };
}

/// Define one of the library's internal C entries (D-X.15): `liella_internal_memcpy`,
/// `_memmove` and `_memset` (`conv`). They are C symbols with feature `export` only, in no
/// version node: every `liella_` name is local in `libc.so.6`. The products' own calls to
/// `memcpy`, `memmove` and `memset` reach them through `cargo xtask so`'s archive pass
/// (`xtask/src/copies.rs`); programs never call them.
#[macro_export]
macro_rules! internal_entry {
    ($(#[$meta:meta])* pub extern "C" fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? $body:block) => {
        $(#[$meta])*
        // UNSAFE(export): a C symbol needs an unmangled name; the `liella_internal_` names are
        // the library's own (local in `libc.so.6`).
        #[cfg_attr(feature = "export", unsafe(no_mangle))]
        #[allow(unsafe_code)]
        // The names are the ABI's (glibc's mixed-case symbols), not ours to rename.
        #[allow(non_snake_case)]
        // C entry points like the exports: their preconditions are the C contracts.
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub extern "C" fn $name($($arg: $ty),*) $(-> $ret)? $body
    };
}
