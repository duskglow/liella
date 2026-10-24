//! argp glue (spec §1.7; D-1.7): the C side of `argp_parse`, `argp_help`, `argp_state_help`,
//! `argp_usage`, `argp_error` and `argp_failure`. The parsing and the help text are
//! `liella::argp`; when messages are printed and when the process ends is
//! `liella_legacy::argp`. This module reads the caller's `struct argp` tree, keeps the
//! `struct argp_state` the parser functions see, calls those functions and the help filters,
//! writes to the streams and exits.
//!
//! Written under the clean-room rules (implementation plan §1.4) from the spec, the public
//! header, `spec/behaviour/argp.md` and black-box probes.
//!
//! **The state.** `argp_parse` keeps the `struct argp_state` on its own stack and gives its
//! address to the parser functions: `root_argp` is the caller's tree, `argc`/`argv` its
//! vector (a parser may replace them, and `next`, `flags`, `input` and `hook`, which are read
//! back after each call), `name` the base name of `argv[0]` (`program_invocation_short_name`
//! without one, or when `ARGP_PARSE_ARGV0` with `ARGP_NO_ERRS` makes element 0 an argument),
//! set before `ARGP_KEY_INIT`; `err_stream`/`out_stream` are `stderr`/`stdout`; `child_inputs`
//! is an array with a slot per child, `pstate` a table of each parser's input for the help
//! filters.
//!
//! **Messages.** Option errors are `getopt`'s texts with `argv[0]` (as `getopt` prints them),
//! then `Try `NAME --help' or `NAME --usage' for more information.`; `argp_error` prints
//! `NAME: message` and that line; `argp_failure` prints `NAME[: message][: error text]`.
//! `NAME` is the state's `name`, or `program_invocation_short_name` without a state.
//! Problems in `ARGP_HELP_FMT` are reported on `stderr` as `NAME: ...` each time help is
//! printed. Help goes to the stream given, as one piece of text.
//!
//! **Program-wide variables.** `argp_program_version`, `argp_program_version_hook`,
//! `argp_program_bug_address` and `argp_err_exit_status` are C objects
//! (`csys/src/adapter/runtime.c`), global in `libc.so.6` as in glibc and weak in `libc.a` (D-X.14),
//! so that a program's own definitions replace them; they are read when used.

use core::mem::{offset_of, size_of};
use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use liella::argp::{self as ap, Arg, Builtin, Defaults, Filtered, Format, FormatError, Host, Opt, Parser, Problem, Table, View};
use liella::diag;
use liella::error::{Errno, KResult};
use liella::getopt::{Args, Event, Span};
use liella::mem::TryVec;
use liella::text::Encoding;
use liella_abi::layout::{struct_argp, struct_argp_child, struct_argp_option, struct_argp_state};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::Timespec;
use liella_legacy::argp as legacy;
use liella_legacy::stdio_compat::{self as sc, Env as _};

use crate::stdio::{self, Locking};
use crate::sys::CSys;
use crate::{conv, errno, ffi, locale, options, printf, process, report, strz, tb};

// ------------------------------------------------------------------------------ C structures

const A_OPTIONS: usize = offset_of!(struct_argp, options);
const A_PARSER: usize = offset_of!(struct_argp, parser);
const A_ARGS_DOC: usize = offset_of!(struct_argp, args_doc);
const A_DOC: usize = offset_of!(struct_argp, doc);
const A_CHILDREN: usize = offset_of!(struct_argp, children);
const A_FILTER: usize = offset_of!(struct_argp, help_filter);
const O_NAME: usize = offset_of!(struct_argp_option, name);
const O_KEY: usize = offset_of!(struct_argp_option, key);
const O_ARG: usize = offset_of!(struct_argp_option, arg);
const O_FLAGS: usize = offset_of!(struct_argp_option, flags);
const O_DOC: usize = offset_of!(struct_argp_option, doc);
const O_GROUP: usize = offset_of!(struct_argp_option, group);
const C_ARGP: usize = offset_of!(struct_argp_child, argp);
const C_HEADER: usize = offset_of!(struct_argp_child, header);
const C_GROUP: usize = offset_of!(struct_argp_child, group);
const S_ROOT: usize = offset_of!(struct_argp_state, root_argp);
const S_ARGC: usize = offset_of!(struct_argp_state, argc);
const S_ARGV: usize = offset_of!(struct_argp_state, argv);
const S_NEXT: usize = offset_of!(struct_argp_state, next);
const S_FLAGS: usize = offset_of!(struct_argp_state, flags);
const S_ARG_NUM: usize = offset_of!(struct_argp_state, arg_num);
const S_QUOTED: usize = offset_of!(struct_argp_state, quoted);
const S_INPUT: usize = offset_of!(struct_argp_state, input);
const S_CHILD_INPUTS: usize = offset_of!(struct_argp_state, child_inputs);
const S_HOOK: usize = offset_of!(struct_argp_state, hook);
const S_NAME: usize = offset_of!(struct_argp_state, name);
const S_ERR: usize = offset_of!(struct_argp_state, err_stream);
const S_OUT: usize = offset_of!(struct_argp_state, out_stream);
const S_PSTATE: usize = offset_of!(struct_argp_state, pstate);
/// `struct argp_state` in words (Appendix C: 96 bytes).
const STATE_WORDS: usize = 12;
const _: () = assert!(size_of::<struct_argp_state>() == 96);

/// The most parsers a tree may have, and entries a table may have (a cycle in a malformed tree
/// ends there).
const MAX_PARSERS: usize = 4096;
const MAX_ENTRIES: usize = 1 << 20;

/// Field `off` of the C object at `base` (0 when there is none).
fn field(base: usize, off: usize) -> usize {
    if base == 0 { 0 } else { base.checked_add(off).unwrap_or(0) }
}

/// Element `i` of the array of `size`-byte objects at `base`.
fn element(base: usize, i: usize, size: usize) -> usize {
    i.checked_mul(size).and_then(|d| base.checked_add(d)).filter(|_| base != 0).unwrap_or(0)
}

/// The pointer-sized C field at `addr` (0 when unreadable).
fn word(addr: usize) -> usize {
    tb::c_word(addr).map_or(0, |w| w.load(Relaxed))
}

fn set_word(addr: usize, v: usize) {
    if let Some(w) = tb::c_word(addr) {
        w.store(v, Relaxed);
    }
}

/// The C `int` at `addr` (0 when unreadable).
fn int(addr: usize) -> i32 {
    tb::c_int(addr).map_or(0, |w| i32::from_ne_bytes(w.load(Relaxed).to_ne_bytes()))
}

fn set_int(addr: usize, v: i32) {
    if let Some(w) = tb::c_int(addr) {
        w.store(u32::from_ne_bytes(v.to_ne_bytes()), Relaxed);
    }
}

/// The C text at `addr`, if any.
fn text<'a>(addr: usize) -> Option<&'a [u8]> {
    (addr != 0).then(|| conv::caller_text(addr))
}

/// A program-wide text (`argp_program_version`, `argp_program_bug_address`), if set.
fn program_text(addr: usize) -> Option<&'static [u8]> {
    (addr != 0).then(|| conv::process_text(addr))
}

/// An address as a C call's integer argument.
fn w(v: usize) -> i64 {
    i64::from_ne_bytes(v.to_ne_bytes())
}

/// The `int` a C function returned (the low half of the register).
fn c_int(r: i64) -> i32 {
    i32::from_ne_bytes(u32::try_from(r & 0xFFFF_FFFF).unwrap_or(0).to_ne_bytes())
}

/// The address of `r`.
fn addr_of<T: ?Sized>(r: &T) -> usize {
    core::ptr::from_ref(r).cast::<u8>().expose_provenance()
}

/// A tree read from C: the parsers in preorder, with each one's `struct argp`, parser
/// function, help filter and number of children.
struct CTree<'a> {
    parsers: TryVec<Parser<'a>>,
    argps: TryVec<usize>,
    fns: TryVec<usize>,
    filters: TryVec<usize>,
    children: TryVec<usize>,
}

/// The option table at `at`, up to its terminator.
fn read_options<'a>(at: usize) -> KResult<TryVec<Opt<'a>>> {
    let mut v = TryVec::new();
    if at == 0 {
        return Ok(v);
    }
    for i in 0..MAX_ENTRIES {
        let o = element(at, i, size_of::<struct_argp_option>());
        let opt = Opt {
            name: text(word(field(o, O_NAME))),
            key: int(field(o, O_KEY)),
            arg: text(word(field(o, O_ARG))),
            flags: int(field(o, O_FLAGS)),
            doc: text(word(field(o, O_DOC))),
            group: int(field(o, O_GROUP)),
        };
        if o == 0 || opt.is_end() {
            break;
        }
        v.push(opt)?;
    }
    Ok(v)
}

/// A `struct argp` still to read: its address, its parent, and its child entry's header and group.
type Pending<'a> = (usize, Option<usize>, Option<&'a [u8]>, i32);

/// The tree whose root `struct argp` is at `root` (none for 0).
fn read_tree<'a>(root: usize) -> KResult<CTree<'a>> {
    let mut t = CTree { parsers: TryVec::new(), argps: TryVec::new(), fns: TryVec::new(), filters: TryVec::new(), children: TryVec::new() };
    let mut stack: TryVec<Pending<'a>> = TryVec::new();
    if root != 0 {
        stack.push((root, None, None, 0))?;
    }
    while let Some((a, parent, header, group)) = stack.pop() {
        if t.parsers.len() >= MAX_PARSERS {
            break;
        }
        let me = t.parsers.len();
        let kids = word(field(a, A_CHILDREN));
        let child = |k: usize| element(kids, k, size_of::<struct_argp_child>());
        let n = (0..MAX_PARSERS).find(|&k| word(field(child(k), C_ARGP)) == 0).unwrap_or(MAX_PARSERS);
        // Children are pushed last first, so that they come out in order, each after the
        // descendants of the one before.
        for k in (0..n).rev() {
            let c = child(k);
            stack.push((word(field(c, C_ARGP)), Some(me), text(word(field(c, C_HEADER))), int(field(c, C_GROUP))))?;
        }
        let parser = word(field(a, A_PARSER));
        let filter = word(field(a, A_FILTER));
        t.parsers.push(Parser {
            options: read_options(word(field(a, A_OPTIONS)))?,
            args_doc: text(word(field(a, A_ARGS_DOC))),
            doc: text(word(field(a, A_DOC))),
            parent,
            header,
            group,
            has_parser: parser != 0,
            has_filter: filter != 0,
        })?;
        t.argps.push(a)?;
        t.fns.push(parser)?;
        t.filters.push(filter)?;
        t.children.push(n)?;
    }
    Ok(t)
}

/// The state's `flags`.
fn flags(state: usize) -> u32 {
    u32::from_ne_bytes(int(field(state, S_FLAGS)).to_ne_bytes())
}

/// The program name messages use: the state's `name`, else `program_invocation_short_name`.
fn state_name<'a>(state: usize) -> &'a [u8] {
    match word(field(state, S_NAME)) {
        0 => conv::process_text(ffi::program_invocation_short_name.load(Relaxed)),
        n => conv::caller_text(n),
    }
}

/// The standard options a parse with `flags` has.
fn defaults(flags: u32) -> Defaults {
    let help = flags & ap::NO_HELP == 0;
    let version = ffi::argp_program_version.load(Relaxed) != 0 || ffi::argp_program_version_hook.load(Relaxed) != 0;
    Defaults { help, version: help && version }
}

// ------------------------------------------------------------------------------ output

/// Write `parts` to the stream `fp` as one text. `errno` is left as it was unless the write
/// fails.
fn put(fp: usize, parts: &[&[u8]]) {
    if fp == 0 {
        return;
    }
    let saved = errno::get();
    let enc = Encoding::of(locale::current());
    let r = stdio::with(fp, Locking::Locked, |g, file, ext| parts.iter().try_for_each(|p| sc::write_text(g, file, ext, p, true, enc)));
    match r.and_then(core::convert::identity) {
        Err(e) => errno::set(e),
        Ok(()) if errno::get() != saved => errno::set_raw(saved),
        Ok(()) => {}
    }
}

/// The help layout, with `ARGP_HELP_FMT`'s problems reported as `name: ...` on `stderr`.
fn format(name: &[u8]) -> Format {
    let env = |n: &[u8]| process::getenv(n).map(strz::env_value);
    Format::new(env(b"ARGP_HELP_FMT"), env(b"COLUMNS"), |e| {
        let saved = errno::get();
        match e {
            FormatError::Unknown(n) => report::emit(&[name, b": ", n, b": Unknown ARGP_HELP_FMT parameter\n"], true, saved),
            FormatError::NeedsValue(n) => report::emit(&[name, b": ", n, b": ARGP_HELP_FMT parameter requires a value\n"], true, saved),
            FormatError::BadValue(n, v) => report::emit(&[name, b": ", v, b": Invalid value for ARGP_HELP_FMT parameter ", n, b"\n"], true, saved),
            FormatError::Garbage(t) => report::emit(&[name, b": Garbage in ARGP_HELP_FMT: ", t, b"\n"], true, saved),
        }
    })
}

/// The input of the parser whose `struct argp` is `argp`, from the table at the state's
/// `pstate` (0 without a state, or for a parser it does not list).
fn input_of(state: usize, argp: usize) -> usize {
    let table = word(field(state, S_PSTATE));
    let n = word(table);
    (0..n.min(MAX_PARSERS))
        .find(|&i| word(element(table, i.saturating_mul(2).saturating_add(1), 8)) == argp)
        .map_or(0, |i| word(element(table, i.saturating_mul(2).saturating_add(2), 8)))
}

/// Parser `p`'s help filter applied to `t` (with `key`). The filter receives a NUL-terminated
/// copy of the text; a different text it returns is copied and freed.
fn filter(tree: &CTree<'_>, state: usize, p: usize, key: i32, t: Option<&[u8]>) -> Filtered {
    let f = tree.filters.get(p).copied().unwrap_or(0);
    if f == 0 {
        return Filtered::Same;
    }
    let mut copy: TryVec<u8> = TryVec::new();
    let given = match t {
        Some(t) => {
            if copy.extend_from_slice(t).and_then(|()| copy.push(0)).is_err() {
                return Filtered::Same;
            }
            addr_of(copy.as_slice())
        }
        None => 0,
    };
    let input = input_of(state, tree.argps.get(p).copied().unwrap_or(0));
    let r = usize::from_ne_bytes(ffi::liella_machine_call3(f, i64::from(key), w(given), w(input)).to_ne_bytes());
    if r == 0 {
        return Filtered::Omit;
    }
    if r == given {
        return Filtered::Same;
    }
    let replaced = TryVec::from_slice(conv::caller_text(r));
    stdio::Glue.free(r);
    replaced.map_or(Filtered::Omit, Filtered::Text)
}

/// The help for the tree at `root`, as `flags` ask, to `stream`; `state` (or 0) gives the help
/// filters their inputs.
fn write_help(root: usize, stream: usize, flags: u32, name: &[u8], d: Defaults, state: usize) {
    let Ok(tree) = read_tree(root) else { return };
    let fmt = format(name);
    let bug = program_text(ffi::argp_program_bug_address.load(Relaxed));
    let mut out = TryVec::new();
    // Without memory for all of it, what was laid out is still printed.
    let _ = ap::help(&tree.parsers, d, &fmt, flags, name, bug, |p, key, t| filter(&tree, state, p, key, t), &mut out);
    put(stream, &[out.as_slice()]);
}

// ------------------------------------------------------------------------------ the C functions

/// `argp_help(argp, stream, flags, name)`: no standard options, no exits (a NULL name is
/// `program_invocation_short_name`).
pub fn help(argp: usize, stream: usize, flags: u32, name: usize) {
    let name = match name {
        0 => conv::process_text(ffi::program_invocation_short_name.load(Relaxed)),
        n => conv::caller_text(n),
    };
    if stream != 0 {
        write_help(argp, stream, flags, name, Defaults::default(), 0);
    }
}

/// `argp_state_help(state, stream, flags)`.
pub fn state_help(state: usize, stream: usize, help: u32) {
    let fl = (state != 0).then(|| flags(state));
    if !legacy::reports(fl) {
        return;
    }
    if stream != 0 {
        let d = fl.map_or(Defaults::default(), defaults);
        write_help(word(field(state, S_ROOT)), stream, help, state_name(state), d, state);
    }
    if let Some(code) = legacy::help_exit(fl, help, ffi::argp_err_exit_status.load(Relaxed)) {
        process::exit(code);
    }
}

/// `argp_usage(state)`.
pub fn usage(state: usize) {
    state_help(state, stdio::stderr(), ap::HELP_STD_USAGE);
}

/// `argp_error`'s text `msg` for `state`.
fn error_text(state: usize, msg: &[u8]) {
    let fl = (state != 0).then(|| flags(state));
    if !legacy::reports(fl) {
        return;
    }
    let stream = if state != 0 { word(field(state, S_ERR)) } else { stdio::stderr() };
    put(stream, &[state_name(state), b": ", msg, b"\n"]);
    state_help(state, stream, ap::HELP_STD_ERR);
}

/// `argp_error(state, fmt, ...)` (`ap`: the `va_list`).
pub fn error(state: usize, fmt: KResult<Option<&[u8]>>, ap: usize) {
    let collected = fmt.ok().flatten().map(|f| printf::collect(f, ap));
    let msg = collected.as_ref().and_then(|m| m.as_ref().ok()).map_or(&[][..], |m| m.as_slice());
    error_text(state, msg);
}

/// `argp_failure(state, status, errnum, fmt, ...)` (`ap`: the `va_list`).
pub fn failure(state: usize, status: i32, errnum: i32, fmt: KResult<Option<&[u8]>>, ap: usize) {
    let fl = (state != 0).then(|| flags(state));
    if legacy::reports(fl) {
        let fmt = fmt.ok().flatten();
        // Formatted first, so that `%m` sees the caller's `errno`.
        let collected = fmt.map(|f| printf::collect(f, ap));
        let msg = collected.as_ref().and_then(|m| m.as_ref().ok()).map_or(&[][..], |m| m.as_slice());
        let mut b = [0u8; 32];
        let error: &[u8] = match (errnum, diag::description(errnum)) {
            (0, _) => &[],
            (_, Some(d)) => d.as_bytes(),
            (e, None) => {
                let n = diag::unknown(e, &mut b);
                b.get(..n).unwrap_or(&[])
            }
        };
        let stream = if state != 0 { word(field(state, S_ERR)) } else { stdio::stderr() };
        let colon = |on: bool| if on { b": ".as_slice() } else { &[][..] };
        put(stream, &[state_name(state), colon(fmt.is_some()), msg, colon(errnum != 0), error, b"\n"]);
    }
    if let Some(code) = legacy::failure_exit(fl, status) {
        process::exit(code);
    }
}

/// `--version`: the hook, else the version text, on the state's `out_stream`; then exit 0.
fn version(state: usize) {
    let out = word(field(state, S_OUT));
    let hook = ffi::argp_program_version_hook.load(Relaxed);
    if hook != 0 {
        let _ = ffi::liella_machine_call3(hook, w(out), w(state), 0);
    } else if let Some(v) = program_text(ffi::argp_program_version.load(Relaxed)) {
        put(out, &[v, b"\n"]);
    }
    if let Some(code) = legacy::version_exit(flags(state)) {
        process::exit(code);
    }
}

/// `--HANG[=SECS]`: sleep SECS seconds (3600 without a value; text that is not a number, 0).
fn hang(arg: Option<&[u8]>) {
    let secs = arg.map_or(3600, |a| a.iter().take_while(|b| b.is_ascii_digit()).fold(0i64, |n, &d| n.saturating_mul(10).saturating_add(i64::from(d.saturating_sub(b'0')))));
    let _ = CSys.clock_nanosleep(0, 0, &Timespec { sec: secs, nsec: 0 }, None);
}

// ------------------------------------------------------------------------------ argp_parse

/// The argument vector the state names now.
struct StateArgs(usize);

impl StateArgs {
    /// The word holding element `i`'s address.
    fn slot(&self, i: usize) -> Option<&'static AtomicUsize> {
        if i >= self.count() {
            return None;
        }
        tb::c_word(element(word(field(self.0, S_ARGV)), i, 8))
    }
}

impl Args for StateArgs {
    fn count(&self) -> usize {
        if word(field(self.0, S_ARGV)) == 0 {
            return 0;
        }
        usize::try_from(int(field(self.0, S_ARGC))).unwrap_or(0)
    }
    fn text(&self, i: usize) -> &[u8] {
        self.slot(i).map_or(&[], |w| conv::caller_text(w.load(Relaxed)))
    }
    fn swap(&mut self, i: usize, j: usize) {
        if let (Some(a), Some(b)) = (self.slot(i), self.slot(j)) {
            let t = a.load(Relaxed);
            a.store(b.load(Relaxed), Relaxed);
            b.store(t, Relaxed);
        }
    }
}

/// One `argp_parse` call: the tree, the state (at `state`) and the arrays it points to.
struct Run<'t, 'a> {
    tree: &'t CTree<'a>,
    state: usize,
    args: StateArgs,
    /// Each parser's `child_inputs`.
    child_inputs: TryVec<TryVec<AtomicUsize>>,
    /// `pstate`: the number of parsers, then each one's `struct argp` and input.
    inputs: TryVec<AtomicUsize>,
}

impl Run<'_, '_> {
    /// The address of the argument `a`.
    fn arg_addr(&self, a: Arg) -> usize {
        match a {
            Arg::None => 0,
            Arg::Span(s) => self.args.slot(s.index).map_or(0, |w| w.load(Relaxed).wrapping_add(s.start)),
            Arg::Element(i) => self.args.slot(i).map_or(0, |w| w.load(Relaxed)),
        }
    }

    /// Write `v` into the state (for parser `p`, its `child_inputs` too).
    fn put_view(&self, p: Option<usize>, v: &View) {
        let s = self.state;
        set_int(field(s, S_NEXT), i32::try_from(v.next).unwrap_or(i32::MAX));
        set_int(field(s, S_FLAGS), i32::from_ne_bytes(v.flags.to_ne_bytes()));
        set_int(field(s, S_ARG_NUM), i32::from_ne_bytes(v.arg_num.to_ne_bytes()));
        set_int(field(s, S_QUOTED), i32::try_from(v.quoted).unwrap_or(i32::MAX));
        set_word(field(s, S_INPUT), v.input);
        set_word(field(s, S_HOOK), v.hook);
        if let Some(p) = p {
            let ci = self.child_inputs.get(p).filter(|c| !c.is_empty()).map_or(0, |c| addr_of(c.as_slice()));
            set_word(field(s, S_CHILD_INPUTS), ci);
            self.note_input(p, v.input);
        }
    }

    /// Read back what a parser may have changed.
    fn get_view(&self, p: Option<usize>, v: &mut View) {
        let s = self.state;
        v.next = usize::try_from(int(field(s, S_NEXT))).unwrap_or(0).min(self.args.count());
        v.flags = flags(s);
        v.input = word(field(s, S_INPUT));
        v.hook = word(field(s, S_HOOK));
        if let Some(p) = p {
            self.note_input(p, v.input);
        }
    }

    /// Record parser `p`'s input in the `pstate` table.
    fn note_input(&self, p: usize, input: usize) {
        if let Some(slot) = self.inputs.get(p.saturating_mul(2).saturating_add(2)) {
            slot.store(input, Relaxed);
        }
    }
}

impl Host for Run<'_, '_> {
    type Args = StateArgs;

    fn args(&mut self) -> &mut StateArgs {
        &mut self.args
    }

    fn call(&mut self, parser: usize, key: i32, arg: Arg, view: &mut View) -> i32 {
        let f = self.tree.fns.get(parser).copied().unwrap_or(0);
        if f == 0 {
            return ap::ERR_UNKNOWN;
        }
        self.put_view(Some(parser), view);
        let a = self.arg_addr(arg);
        // The program's own `argp_parser_t`, given for exactly this use (§1.7).
        let r = c_int(ffi::liella_machine_call3(f, i64::from(key), w(a), w(self.state)));
        self.get_view(Some(parser), view);
        r
    }

    fn child_input(&mut self, parser: usize, slot: usize) -> usize {
        self.child_inputs.get(parser).and_then(|c| c.get(slot)).map_or(0, |w| w.load(Relaxed))
    }

    fn builtin(&mut self, which: Builtin, arg: Option<Span>, view: &mut View) {
        self.put_view(None, view);
        let s = self.state;
        match which {
            Builtin::Help => state_help(s, word(field(s, S_OUT)), ap::HELP_STD_HELP),
            Builtin::Usage => state_help(s, word(field(s, S_OUT)), ap::HELP_USAGE | ap::HELP_EXIT_OK),
            Builtin::Version => version(s),
            Builtin::ProgramName => set_word(field(s, S_NAME), self.arg_addr(arg.map_or(Arg::None, Arg::Span))),
            Builtin::Hang => hang(arg.map(|a| a.of(&self.args))),
        }
        self.get_view(None, view);
    }

    fn option_error(&mut self, event: Event, table: &Table<'_, '_>, view: &mut View) {
        self.put_view(None, view);
        if legacy::reports(Some(flags(self.state))) {
            options::diagnose(self.args.text(0), &self.args, Some(table), event);
        }
        state_help(self.state, word(field(self.state, S_ERR)), ap::HELP_STD_ERR);
        self.get_view(None, view);
    }

    fn usage_error(&mut self, problem: Problem<'_>, view: &mut View) {
        self.put_view(None, view);
        match problem {
            Problem::TooManyArguments => error_text(self.state, b"Too many arguments"),
            Problem::Unhandled { letter, name } => {
                let what = b": (PROGRAM ERROR) Option should have been recognized!?";
                let mut msg: TryVec<u8> = TryVec::new();
                let built = match letter {
                    Some(c) => msg.extend_from_slice(&[b'-', c]),
                    None => msg.extend_from_slice(b"--").and_then(|()| msg.extend_from_slice(name)),
                }
                .and_then(|()| msg.extend_from_slice(what));
                error_text(self.state, if built.is_ok() { msg.as_slice() } else { what.get(2..).unwrap_or(&[]) });
            }
        }
        self.get_view(None, view);
    }
}

/// `argp_parse(argp, argc, argv, flags, arg_index, input)`: 0 or the error; `arg_index` (an
/// address, 0 for none) receives the first element not parsed.
pub fn parse(argp: usize, argc: i32, argv: usize, flags: u32, index: usize, input: usize) -> i32 {
    let tree = match read_tree(argp) {
        Ok(t) => t,
        Err(e) => return e.get(),
    };
    let words = [const { AtomicUsize::new(0) }; STATE_WORDS];
    let s = addr_of(&words);
    let argc = if argv == 0 { 0 } else { argc.max(0) };
    set_word(field(s, S_ROOT), argp);
    set_int(field(s, S_ARGC), argc);
    set_word(field(s, S_ARGV), argv);
    set_int(field(s, S_FLAGS), i32::from_ne_bytes(flags.to_ne_bytes()));
    set_word(field(s, S_ERR), stdio::stderr());
    set_word(field(s, S_OUT), stdio::stdout());
    let argv0 = if argc > 0 { word(argv) } else { 0 };
    let argv0_parsed = flags & ap::PARSE_ARGV0 != 0 && flags & ap::NO_ERRS != 0;
    let name = if argv0 != 0 && !argv0_parsed {
        argv0.wrapping_add(legacy::base_name(conv::caller_text(argv0)))
    } else {
        ffi::program_invocation_short_name.load(Relaxed)
    };
    set_word(field(s, S_NAME), name);
    let mut child_inputs: TryVec<TryVec<AtomicUsize>> = TryVec::new();
    let mut inputs: TryVec<AtomicUsize> = TryVec::new();
    let built: KResult<()> = (|| {
        inputs.push(AtomicUsize::new(tree.parsers.len()))?;
        for (p, &n) in tree.children.iter().enumerate() {
            let mut slots = TryVec::with_capacity(n)?;
            for _ in 0..n {
                slots.push(AtomicUsize::new(0))?;
            }
            child_inputs.push(slots)?;
            inputs.push(AtomicUsize::new(tree.argps.get(p).copied().unwrap_or(0)))?;
            inputs.push(AtomicUsize::new(0))?;
        }
        Ok(())
    })();
    if built.is_err() {
        return Errno::ENOMEM.get();
    }
    set_word(field(s, S_PSTATE), addr_of(inputs.as_slice()));
    let posix = process::getenv(crate::startup_names::POSIXLY_CORRECT).is_some();
    let mut run = Run { tree: &tree, state: s, args: StateArgs(s), child_inputs, inputs };
    let to_int = |i: usize| i32::try_from(i).unwrap_or(i32::MAX);
    match ap::parse(&tree.parsers, defaults(flags), flags, input, index != 0, posix, &mut run) {
        Ok(p) => {
            set_int(index, to_int(p.index));
            0
        }
        Err(f) => {
            if let Some(i) = f.index {
                set_int(index, to_int(i));
            }
            f.code
        }
    }
}
