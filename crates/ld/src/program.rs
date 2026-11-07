//! `ld.so` run as a program (plan/ph14/design.md §19, ld.so(8)): what its arguments ask for, and the
//! text of `--version` and `--help` (phase 14 DL-5, D-27.13). Pure logic; the boundary writes the text.
//!
//! DL-5 runs `--version` and `--help` only. Loading a program (`ld.so PROGRAM`, or the kernel naming
//! the interpreter in `PT_INTERP`) comes with DL-6 and is refused here with status 127.

use crate::arch::Arch;

/// The exit status of a refused start (design §1 "Failure").
pub const FAILURE: i32 = 127;

/// What the command line asks of `ld.so`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action<'a> {
    /// `--version`: print the version and exit 0.
    Version,
    /// `--help`: print the usage and exit 0.
    Help,
    /// No program named: the usage hint on standard error, status 127.
    Missing,
    /// An option this loader does not know (or does not handle yet).
    Unknown(&'a [u8]),
    /// Run `PROGRAM` (DL-6).
    Program(&'a [u8]),
}

/// The action for the arguments after `argv[0]`. The first of `--version` and `--help` wins, as
/// ld.so(8) lists them as stand-alone options; scanning stops at the first argument that is not an
/// option, the program name.
pub fn action<'a>(args: impl IntoIterator<Item = &'a [u8]>) -> Action<'a> {
    match args.into_iter().next() {
        None => Action::Missing,
        Some(b"--version") => Action::Version,
        Some(b"--help") => Action::Help,
        Some(arg @ [b'-', ..]) => Action::Unknown(arg),
        Some(arg) => Action::Program(arg),
    }
}

/// The `--version` text, in the order it is written: Liella's name and version, the ABI it
/// implements and the interpreter's own name ([`Arch::INTERP`]).
pub fn version<A: Arch>(liella_version: &'static str) -> [&'static str; 5] {
    ["ld.so (Liella) ", liella_version, "\nImplements the GNU C Library 2.39 ABI.\nProgram interpreter: ", A::INTERP, "\n"]
}

/// The `--help` text, in the order it is written.
pub fn help<A: Arch>() -> [&'static str; 3] {
    [
        "Usage: ",
        A::INTERP,
        " [OPTION]... EXECUTABLE-FILE [ARGS-FOR-PROGRAM...]\n\
         Liella's program interpreter. Run as a program, it loads and runs EXECUTABLE-FILE.\n\
         \n\
         \x20 --help     print this help and exit\n\
         \x20 --version  print version information and exit\n\
         \n\
         Running a program through this interpreter is not available in this build.\n",
    ]
}

/// The message for an action that ends with [`FAILURE`], in the order it is written (standard error).
pub fn refusal<'a>(action: Action<'a>) -> [&'a [u8]; 3] {
    match action {
        Action::Unknown(arg) => [b"ld.so: unrecognized option '", arg, b"'\nTry 'ld.so --help' for more information.\n"],
        Action::Program(_) => [b"ld.so: loading a program is not available in this build\n", b"", b""],
        Action::Missing | Action::Version | Action::Help => [b"ld.so: missing program name\nTry 'ld.so --help' for more information.\n", b"", b""],
    }
}

/// The message when the kernel started this interpreter for a program (`PT_INTERP`): DL-6 loads it.
pub const AS_INTERPRETER: &[u8] = b"ld.so: loading a program is not available in this build\n";
