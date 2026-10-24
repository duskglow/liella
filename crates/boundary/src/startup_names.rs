//! STARTUP-READ-CONSTANTS: the names that start-up and the first `setlocale`/`getopt` read
//! (environment variables, vDSO symbols and their version), kept together in one static.
//!
//! The kernel maps a file page with its neighbours in the same 64 KiB window, so every
//! read-only window that start-up reads costs one page fault. As separate literals, these names
//! sat wherever code generation put them in `.rodata`. Unrelated code growth then moved one of
//! them into another window (`env_find` read two names on either side of a window boundary,
//! TIMER-SIGEV-THREAD-NOSPAWN-4). Here they share [`STARTUP_NAMES`]. Its alignment is at least
//! its size, so it never crosses a 64 KiB boundary. `cargo xtask so` renames its section to
//! `.rodata1` in `libc.a` (`hottext::group`). GNU ld's default script places that section right
//! after `.rodata`, away from code-dependent neighbours; other linkers keep it with the
//! read-only data. Only placement changes: the bytes, and every comparison made with them, are
//! those of the literals they replace.

/// Generates [`STARTUP_NAMES`] from `NAME = b"text"` pairs and one `&'static [u8]` constant per
/// name. Each constant is a slice of the static, so its uses read the static's bytes.
macro_rules! startup_names {
    ($($name:ident = $text:literal),* $(,)?) => {
        const LIST: &[&[u8]] = &[$($text),*];

        /// The position of each name in `LIST`.
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[repr(usize)]
        enum Index { $($name),* }

        $(
            #[doc = concat!("`", stringify!($name), "`, in [`STARTUP_NAMES`].")]
            pub const $name: &[u8] = name(&STARTUP_NAMES.0, Index::$name as usize);
        )*
    };
}

startup_names! {
    GLIBC_TUNABLES = b"GLIBC_TUNABLES",
    MALLOC_PERTURB = b"MALLOC_PERTURB_",
    LIELLA_NSCACHE = b"LIELLA_NSCACHE",
    VDSO_CLOCK_GETTIME = b"__vdso_clock_gettime",
    VDSO_CLOCK_GETRES = b"__vdso_clock_getres",
    VDSO_GETTIMEOFDAY = b"__vdso_gettimeofday",
    VDSO_TIME = b"__vdso_time",
    VDSO_GETCPU = b"__vdso_getcpu",
    VDSO_VERSION = b"LINUX_2.6",
    LC_ALL = b"LC_ALL",
    LC_CTYPE = b"LC_CTYPE",
    LC_NUMERIC = b"LC_NUMERIC",
    LC_TIME = b"LC_TIME",
    LC_COLLATE = b"LC_COLLATE",
    LC_MONETARY = b"LC_MONETARY",
    LC_MESSAGES = b"LC_MESSAGES",
    LC_PAPER = b"LC_PAPER",
    LC_NAME = b"LC_NAME",
    LC_ADDRESS = b"LC_ADDRESS",
    LC_TELEPHONE = b"LC_TELEPHONE",
    LC_MEASUREMENT = b"LC_MEASUREMENT",
    LC_IDENTIFICATION = b"LC_IDENTIFICATION",
    LANG = b"LANG",
    LOCPATH = b"LOCPATH",
    POSIXLY_CORRECT = b"POSIXLY_CORRECT",
}

/// The names' total length.
const LEN: usize = start(LIST.len());

/// The alignment (= the size) of [`Names`]: a power of two, at least [`LEN`], dividing 64 KiB.
pub const ALIGN: usize = 512;

const _: () = assert!(LEN <= ALIGN && ALIGN.is_power_of_two() && 65536 % ALIGN == 0);

/// The names, back to back, without terminators.
#[repr(C, align(512))]
pub struct Names([u8; LEN]);

const _: () = assert!(core::mem::align_of::<Names>() == ALIGN && core::mem::size_of::<Names>() == ALIGN);

/// Every name start-up reads (see the module documentation). Its section is renamed by
/// `xtask/src/hottext.rs`, which finds it by this name.
pub static STARTUP_NAMES: Names = Names(bytes());

/// The offset of name `i` in [`STARTUP_NAMES`].
// Evaluated only at compile time (`LEN`, `STARTUP_NAMES`, the name constants): an index or
// overflow error there is a build error, never a run-time panic.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn start(i: usize) -> usize {
    let mut at = 0;
    let mut k = 0;
    while k < i {
        at += LIST[k].len();
        k += 1;
    }
    at
}

// Evaluated only at compile time (`LEN`, `STARTUP_NAMES`, the name constants): an index or
// overflow error there is a build error, never a run-time panic.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn bytes() -> [u8; LEN] {
    let mut out = [0u8; LEN];
    let mut at = 0;
    let mut k = 0;
    while k < LIST.len() {
        let mut j = 0;
        while j < LIST[k].len() {
            out[at] = LIST[k][j];
            at += 1;
            j += 1;
        }
        k += 1;
    }
    out
}

/// Name `i` as a slice of `all` (the static's bytes).
// Evaluated only at compile time (`LEN`, `STARTUP_NAMES`, the name constants): an index or
// overflow error there is a build error, never a run-time panic.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn name(all: &'static [u8; LEN], i: usize) -> &'static [u8] {
    all.split_at(start(i)).1.split_at(LIST[i].len()).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_is_its_literal_and_lies_in_the_static() {
        let all = STARTUP_NAMES.0.as_ptr_range();
        for (i, (got, want)) in [MALLOC_PERTURB, VDSO_CLOCK_GETTIME, VDSO_VERSION, LC_ALL, LC_IDENTIFICATION, LANG, LOCPATH, POSIXLY_CORRECT]
            .into_iter()
            .zip([&b"MALLOC_PERTURB_"[..], b"__vdso_clock_gettime", b"LINUX_2.6", b"LC_ALL", b"LC_IDENTIFICATION", b"LANG", b"LOCPATH", b"POSIXLY_CORRECT"])
            .enumerate()
        {
            assert_eq!(got, want, "name {i}");
            assert!(all.contains(&got.as_ptr()), "name {i} is not a slice of STARTUP_NAMES");
        }
        assert_eq!(start(LIST.len()), LEN);
    }
}
