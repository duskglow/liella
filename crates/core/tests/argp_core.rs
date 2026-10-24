//! argp's parse engine and help text through the public interface of `liella::argp` (spec §1.7;
//! D-1.7), written before COLD-REWRITE2-W-far-01's rewrite so that both versions answer to them.
//! The parse edges the spec leaves open (`ARGP_NO_ARGS`, `ARGP_KEY_ARGS` taken, a parser moving
//! `next`) and the help details the probe record does not show (several `args_doc`
//! alternatives, sort ties, shadowed names, help filters) were settled by black-box probes of
//! the reference; where the result differs from the reference by decision, the test says so.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use liella::argp::*;
use liella::getopt::{Event, Span};
use liella::mem::TryVec;

fn opt(name: Option<&'static str>, key: i32, arg: Option<&'static str>, flags: i32, doc: Option<&'static str>, group: i32) -> Opt<'static> {
    Opt { name: name.map(str::as_bytes), key, arg: arg.map(str::as_bytes), flags, doc: doc.map(str::as_bytes), group }
}

fn table(opts: &[Opt<'static>]) -> TryVec<Opt<'static>> {
    let mut t = TryVec::new();
    for o in opts {
        t.push(*o).unwrap();
    }
    t
}

/// A root with `-v`/`--verbose` and `-q`/`--quiet` (which its parser declines), and one child
/// without options.
fn small_tree() -> TryVec<Parser<'static>> {
    let mut t = TryVec::new();
    let root = table(&[opt(Some("verbose"), i32::from(b'v'), None, 0, Some("V"), 0), opt(Some("quiet"), i32::from(b'q'), None, 0, Some("Q"), 0)]);
    t.push(Parser { options: root, args_doc: Some(b"A"), has_parser: true, ..Parser::default() }).unwrap();
    t.push(Parser { parent: Some(0), has_parser: true, ..Parser::default() }).unwrap();
    t
}

/// One parser call: parser, key, argument text, `next`, `arg_num`.
type Call = (usize, &'static str, String, usize, u32);

type Act = Box<dyn FnMut(usize, i32, &str, &mut View) -> i32>;

/// A host over owned texts that records every call; `act` answers the parser calls.
struct Rec {
    argv: Vec<String>,
    calls: Vec<Call>,
    other: Vec<String>,
    act: Act,
}

fn key_name(key: i32) -> &'static str {
    match key {
        KEY_ARG => "ARG",
        KEY_ARGS => "ARGS",
        KEY_END => "END",
        KEY_NO_ARGS => "NO_ARGS",
        KEY_INIT => "INIT",
        KEY_SUCCESS => "SUCCESS",
        KEY_ERROR => "ERROR",
        KEY_FINI => "FINI",
        0x76 => "v",
        0x71 => "q",
        _ => "?",
    }
}

impl Rec {
    fn new(argv: &[&str], act: Act) -> Rec {
        Rec { argv: argv.iter().map(|s| (*s).to_string()).collect(), calls: Vec::new(), other: Vec::new(), act }
    }
    fn trace(&self) -> String {
        self.calls.iter().map(|(p, k, a, n, c)| format!("{p}:{k}({a},n{n},a{c})")).collect::<Vec<_>>().join(" ")
    }
}

impl Host for Rec {
    type Args = [String];
    fn args(&mut self) -> &mut [String] {
        &mut self.argv
    }
    fn call(&mut self, parser: usize, key: i32, arg: Arg, view: &mut View) -> i32 {
        let text = match arg {
            Arg::None => String::new(),
            Arg::Span(s) => String::from_utf8_lossy(s.of(&self.argv[..])).into_owned(),
            Arg::Element(i) => self.argv[i].clone(),
        };
        self.calls.push((parser, key_name(key), text.clone(), view.next, view.arg_num));
        (self.act)(parser, key, &text, view)
    }
    fn child_input(&mut self, parser: usize, slot: usize) -> usize {
        100 * (parser + 1) + slot
    }
    fn builtin(&mut self, which: Builtin, arg: Option<Span>, _: &mut View) {
        let a = arg.map(|s| String::from_utf8_lossy(s.of(&self.argv[..])).into_owned());
        self.other.push(format!("builtin {which:?} {a:?}"));
    }
    fn option_error(&mut self, event: Event, _: &Table<'_, '_>, _: &mut View) {
        self.other.push(format!("option_error {event:?}"));
    }
    fn usage_error(&mut self, problem: Problem<'_>, _: &mut View) {
        let p = match problem {
            Problem::TooManyArguments => "too many".to_string(),
            Problem::Unhandled { letter, name } => format!("unhandled {:?} {}", letter.map(char::from), String::from_utf8_lossy(name)),
        };
        self.other.push(format!("usage_error {p}"));
    }
}

/// The root takes `-v` and operands; everything else is unknown.
fn plain() -> Act {
    Box::new(|p, key, _, _| if p == 0 && (key == KEY_ARG || key == 0x76) { 0 } else { ERR_UNKNOWN })
}

fn run(argv: &[&str], flags: u32, want_index: bool, act: Act) -> (Result<Parsed, Failed>, Rec) {
    let tree = small_tree();
    let mut h = Rec::new(argv, act);
    let r = parse(&tree, Defaults::default(), flags, 7, want_index, false, &mut h);
    (r, h)
}

const FINISH: &str = "1:SUCCESS(,n{n},a0) 0:SUCCESS(,n{n},a{a}) 1:FINI(,n{n},a0) 0:FINI(,n{n},a{a})";

fn finish(n: usize, a: u32) -> String {
    FINISH.replace("{n}", &n.to_string()).replace("{a}", &a.to_string())
}

#[test]
fn args_key_takes_the_rest() {
    // Clauses: s1_7_c_0 (probe: a parser that takes ARGP_KEY_ARGS has consumed every remaining element, whatever it leaves in `next`)
    for moved in [true, false] {
        let act: Act = Box::new(move |p, key, _, view| match (p, key) {
            (0, KEY_ARGS) => {
                if moved {
                    view.next = 4;
                }
                0
            }
            _ => ERR_UNKNOWN,
        });
        let (r, h) = run(&["prog", "a", "b", "c"], 0, true, act);
        assert_eq!(r, Ok(Parsed { index: 4, stopped: false }));
        let want = format!("0:INIT(,n0,a0) 1:INIT(,n0,a0) 0:ARG(a,n2,a0) 0:ARGS(,n1,a0) 1:NO_ARGS(,n4,a0) 1:END(,n4,a0) 0:END(,n4,a3) {}", finish(4, 3));
        assert_eq!(h.trace(), want, "moved {moved}");
    }
}

#[test]
fn parser_moves_next() {
    // Clauses: s1_7_c_0 (probe: a raised `next` counts the skipped elements as taken; a lowered one does not count the operand)
    let act: Act = Box::new(|_, key, arg, view| match key {
        KEY_ARG if arg == "skip" => {
            view.next += 1;
            0
        }
        KEY_ARG => 0,
        _ => ERR_UNKNOWN,
    });
    let (r, h) = run(&["prog", "skip", "y", "z"], 0, true, act);
    assert_eq!(r, Ok(Parsed { index: 4, stopped: false }));
    assert!(h.trace().starts_with("0:INIT(,n0,a0) 1:INIT(,n0,a0) 0:ARG(skip,n2,a0) 0:ARG(z,n4,a2)"), "{}", h.trace());
    assert!(h.trace().contains("0:END(,n4,a3)"), "{}", h.trace());
    let mut once = true;
    let act: Act = Box::new(move |p, key, arg, view| match key {
        KEY_ARG if p == 0 && arg == "back" && once => {
            once = false;
            view.next = 1;
            0
        }
        KEY_ARG if p == 0 => 0,
        _ => ERR_UNKNOWN,
    });
    let (r, h) = run(&["prog", "x", "back", "y"], 0, true, act);
    assert_eq!(r, Ok(Parsed { index: 4, stopped: false }));
    assert!(h.trace().starts_with("0:INIT(,n0,a0) 1:INIT(,n0,a0) 0:ARG(x,n2,a0) 0:ARG(back,n3,a1) 0:ARG(x,n2,a1) 0:ARG(back,n3,a2) 0:ARG(y,n4,a3) 1:NO_ARGS(,n4,a0) 1:END(,n4,a0) 0:END(,n4,a4)"), "{}", h.trace());
}

#[test]
fn no_args_flag_stops_at_the_first_operand() {
    // Clauses: s1_7_c_0, s1_7_i_0 (probe: ARGP_NO_ARGS ends the options at the first operand, which no parser is offered)
    let (r, h) = run(&["prog", "a", "-v", "b"], NO_ARGS, true, plain());
    assert_eq!(r, Ok(Parsed { index: 1, stopped: true }));
    assert_eq!(h.trace(), format!("0:INIT(,n0,a0) 1:INIT(,n0,a0) {}", finish(1, 0)));
    let (r, h) = run(&["prog", "a", "-v", "b"], NO_ARGS, false, plain());
    assert_eq!(r, Err(Failed { code: 22, index: None }));
    assert_eq!(h.other, ["usage_error too many"]);
    assert_eq!(h.trace(), "0:INIT(,n0,a0) 1:INIT(,n0,a0) 0:ERROR(,n1,a0) 1:ERROR(,n1,a0) 1:FINI(,n1,a0) 0:FINI(,n1,a0)");
    let (r, h) = run(&["prog", "-v"], NO_ARGS, true, plain());
    assert_eq!(r, Ok(Parsed { index: 2, stopped: false }));
    assert_eq!(h.trace(), format!("0:INIT(,n0,a0) 1:INIT(,n0,a0) 0:v(,n2,a0) 0:NO_ARGS(,n2,a0) 1:NO_ARGS(,n2,a0) 1:END(,n2,a0) 0:END(,n2,a0) {}", finish(2, 0)));
}

#[test]
fn in_order_double_dash_and_inputs() {
    // Clauses: s1_7_c_0, s1_7_c_1
    let (r, h) = run(&["prog", "a", "-v", "b"], IN_ORDER, true, plain());
    assert_eq!(r, Ok(Parsed { index: 4, stopped: false }));
    assert!(h.trace().starts_with("0:INIT(,n0,a0) 1:INIT(,n0,a0) 0:ARG(a,n2,a0) 0:v(,n3,a1) 0:ARG(b,n4,a1) 1:NO_ARGS(,n4,a0)"), "{}", h.trace());
    // After `--` every element is an operand; `quoted` names the element after it.
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let s2 = seen.clone();
    let act: Act = Box::new(move |p, key, _, view| {
        s2.borrow_mut().push((p, key, view.quoted, view.input));
        if p == 0 && (key == KEY_ARG || key == 0x76) { 0 } else { ERR_UNKNOWN }
    });
    let (r, h) = run(&["prog", "-v", "--", "-x", "a"], 0, true, act);
    assert_eq!(r, Ok(Parsed { index: 5, stopped: false }));
    assert!(h.trace().starts_with("0:INIT(,n0,a0) 1:INIT(,n0,a0) 0:v(,n2,a0) 0:ARG(-x,n4,a0) 0:ARG(a,n5,a1) 1:NO_ARGS(,n5,a0)"), "{}", h.trace());
    let seen = seen.borrow();
    assert!(seen.contains(&(0, KEY_ARG, 3, 7)), "{seen:?}");
    // The child's input is slot 0 of its parent's child inputs.
    assert!(seen.contains(&(1, KEY_INIT, 0, 100)), "{seen:?}");
}

#[test]
fn standard_options_and_errors() {
    // Clauses: s1_7_c_0, s1_7_c_3, s1_7_f_0
    let tree = small_tree();
    for (argv, want) in [
        (&["prog", "--help"][..], "builtin Help None"),
        (&["prog", "-?"][..], "builtin Help None"),
        (&["prog", "--usage"][..], "builtin Usage None"),
        (&["prog", "-V"][..], "builtin Version None"),
        (&["prog", "--version"][..], "builtin Version None"),
        (&["prog", "--program-name=other"][..], "builtin ProgramName Some(\"other\")"),
        (&["prog", "--HANG=5"][..], "builtin Hang Some(\"5\")"),
        (&["prog", "--HANG"][..], "builtin Hang None"),
    ] {
        let mut h = Rec::new(argv, plain());
        let r = parse(&tree, Defaults { help: true, version: true }, 0, 0, false, false, &mut h);
        assert_eq!(r, Ok(Parsed { index: 2, stopped: false }), "{argv:?}");
        assert_eq!(h.other, [want], "{argv:?}");
    }
    // Without the standard options, --help is unknown.
    let mut h = Rec::new(&["prog", "--help"], plain());
    let r = parse(&tree, Defaults::default(), 0, 0, false, false, &mut h);
    assert_eq!(r, Err(Failed { code: 22, index: None }));
    assert_eq!(h.other.len(), 1);
    assert!(h.other[0].starts_with("option_error Unrecognized"), "{:?}", h.other);
    assert!(h.trace().ends_with("0:ERROR(,n2,a0) 1:ERROR(,n2,a0) 1:FINI(,n2,a0) 0:FINI(,n2,a0)"), "{}", h.trace());
    // An option its own parser declines is a usage error.
    for (argv, want) in [(&["prog", "-q"][..], "usage_error unhandled Some('q')"), (&["prog", "--quiet"][..], "usage_error unhandled None quiet")] {
        let (r, h) = run(argv, 0, false, plain());
        assert_eq!(r, Err(Failed { code: 22, index: None }));
        assert!(h.other.len() == 1 && h.other[0].starts_with(want), "{:?}", h.other);
    }
    // A parser's own error: during the operands no index, after them the index.
    let act: Act = Box::new(|_, key, _, _| if key == KEY_ARG { 5 } else { ERR_UNKNOWN });
    let (r, h) = run(&["prog", "a", "b"], 0, true, act);
    assert_eq!(r, Err(Failed { code: 5, index: None }));
    assert!(h.trace().ends_with("0:ARG(a,n2,a0) 0:ERROR(,n2,a0) 1:ERROR(,n2,a0) 1:FINI(,n2,a0) 0:FINI(,n2,a0)"), "{}", h.trace());
    let act: Act = Box::new(|p, key, _, _| if p == 0 && key == KEY_END { 9 } else { ERR_UNKNOWN });
    let (r, _) = run(&["prog"], 0, true, act);
    assert_eq!(r, Err(Failed { code: 9, index: Some(1) }));
}

#[test]
fn events_and_operands() {
    // Clauses: s1_7_c_0, s1_7_i_0 (moved from the unit tests of `argp.rs`)
    let act: Act = Box::new(|p, key, arg, _| match (p, key) {
        (0, KEY_ARG) if arg == "stop" => ERR_UNKNOWN,
        (0, KEY_ARG | 0x76) => 0,
        _ => ERR_UNKNOWN,
    });
    let (r, h) = run(&["prog", "a", "stop", "-v", "b"], 0, true, act);
    assert_eq!(r, Ok(Parsed { index: 3, stopped: true }));
    let want = "0:INIT(,n0,a0) 1:INIT(,n0,a0) 0:v(,n4,a0) 0:ARG(a,n3,a0) 0:ARG(stop,n4,a1) 0:ARGS(,n3,a1) 1:ARG(stop,n4,a0) 1:ARGS(,n3,a0) 1:SUCCESS(,n3,a0) 0:SUCCESS(,n3,a1) 1:FINI(,n3,a0) 0:FINI(,n3,a1)";
    assert_eq!(h.trace(), want);
    assert_eq!(h.argv, ["prog", "-v", "a", "stop", "b"]);
    let (r, h) = run(&["prog", "stop"], 0, false, plain_stop());
    assert_eq!(r, Err(Failed { code: 22, index: None }));
    assert!(h.trace().ends_with("0:ERROR(,n1,a0) 1:ERROR(,n1,a0) 1:FINI(,n1,a0) 0:FINI(,n1,a0)"), "{}", h.trace());
}

fn plain_stop() -> Act {
    Box::new(|p, key, arg, _| if p == 0 && key == KEY_ARG && arg != "stop" { 0 } else { ERR_UNKNOWN })
}

// ------------------------------------------------------------------------------ help

/// The tree of `tests/oracle/argp_diff.c`.
fn probe_tree() -> TryVec<Parser<'static>> {
    let c = |b: u8| i32::from(b);
    let main = table(&[
        opt(Some("verbose"), c(b'v'), None, 0, Some("Produce verbose output"), 0),
        opt(Some("output"), c(b'o'), Some("FILE"), 0, Some("Output to FILE instead of standard output, with a description long enough to wrap"), 0),
        opt(Some("level"), c(b'l'), Some("N"), OPTION_ARG_OPTIONAL, Some("Set the level"), 0),
        opt(Some("quiet"), 0, None, OPTION_ALIAS, None, 0),
        opt(Some("silent"), c(b's'), None, OPTION_ALIAS, None, 0),
        opt(Some("hidden"), c(b'H'), None, OPTION_HIDDEN, Some("Hidden option"), 0),
        opt(None, 0, None, 0, Some("Advanced options:"), 2),
        opt(Some("long-only"), 1000, None, 0, Some("Only a long name"), 2),
        opt(None, c(b'x'), None, 0, Some("Only a short name"), 2),
        opt(Some("FORMAT"), 0, None, OPTION_DOC | OPTION_NO_USAGE, Some("A documentation entry"), 3),
    ]);
    let child = table(&[
        opt(None, 0, None, 0, Some("Child options:"), 5),
        opt(Some("child"), c(b'c'), None, 0, Some("A child option"), 0),
        opt(Some("child-long"), 1001, Some("VAL"), OPTION_ARG_OPTIONAL, Some("A long child option with an optional value"), 0),
    ]);
    let mut t = TryVec::new();
    t.push(Parser { options: main, args_doc: Some(b"ARG1 ARG2\n-x ARG"), doc: Some(b"Program description.\x0bText after the options."), has_parser: true, ..Parser::default() }).unwrap();
    t.push(Parser { options: child, doc: Some(b"Child documentation."), parent: Some(0), has_parser: true, ..Parser::default() }).unwrap();
    t
}

fn render(tree: &[Parser<'_>], fmt: &Format, flags: u32, filter: impl FnMut(usize, i32, Option<&[u8]>) -> Filtered) -> String {
    let mut out = TryVec::new();
    help(tree, Defaults { help: true, version: true }, fmt, flags, b"prog", Some(b"<bugs@example.org>"), filter, &mut out).unwrap();
    String::from_utf8(out.into_inner()).unwrap()
}

#[test]
fn help_layout() {
    // Clauses: s1_7_c_2 (moved from the unit tests of `argp.rs`)
    let tree = probe_tree();
    let same = |_: usize, _: i32, _: Option<&[u8]>| Filtered::Same;
    let want = "Usage: prog [OPTION...] ARG1 ARG2\n  or:  prog [OPTION...] -x ARG\nProgram description.\n\n  -l, -s, --level[=N], --quiet[=N], --silent[=N]\n                             Set the level\n  -o, --output=FILE          Output to FILE instead of standard output, with a\n                             description long enough to wrap\n  -v, --verbose              Produce verbose output\n\n Advanced options:\n      --long-only            Only a long name\n  -x                         Only a short name\n\n  FORMAT                     A documentation entry\n\n Child options:\n      --child-long[=VAL]     A long child option with an optional value\n  -c, --child                A child option\n\n  -?, --help                 Give this help list\n      --usage                Give a short usage message\n  -V, --version              Print program version\n\nMandatory or optional arguments to long options are also mandatory or optional\nfor any corresponding short options.\n\nText after the options.\n\nReport bugs to <bugs@example.org>.\n";
    assert_eq!(render(&tree, &Format::default(), HELP_STD_HELP, same), want);
    let narrow = Format { rmargin: 30, ..Format::default() };
    let want = "Usage: prog [-vxc?V] [-l[N]]\n            [-s[N]] [-o FILE]\n            [--level[=N]]\n            [--quiet[=N]]\n            [--silent[=N]]\n            [--output=FILE]\n            [--verbose]\n            [--long-only]\n            [--child-long[=VAL]]\n            [--child] [--help]\n            [--usage]\n            [--version]\n            ARG1 ARG2\n  or:  prog [OPTION...]\n            -x ARG\n";
    assert_eq!(render(&tree, &narrow, HELP_USAGE, same), want);
    assert_eq!(render(&tree, &Format::default(), HELP_SEE | HELP_BUG_ADDR, same), "Try `prog --help' or `prog --usage' for more information.\n\nReport bugs to <bugs@example.org>.\n");
    // D-1.7: names wider than the right margin start their text on the next line.
    let mid = Format { rmargin: 40, ..Format::default() };
    let got = render(&tree, &mid, HELP_LONG, same);
    assert!(got.starts_with("  -l, -s, --level[=N], --quiet[=N], --silent[=N]\n                             Set the\n                             level\n  -o, --output=FILE          Output to\n                             FILE\n"), "{got}");
    // The columns: names past the text column by at most 3 get 3 spaces, wider ones the next line.
    let cols = Format { short_opt_col: 4, long_opt_col: 10, opt_doc_col: 20, header_col: 2, ..Format::default() };
    let got = render(&tree, &cols, HELP_LONG, same);
    assert!(got.starts_with("    -l, -s, --level[=N], --quiet[=N], --silent[=N]\n                    Set the level\n    -o,   --output=FILE   Output to FILE instead of standard output, with a\n                    description long enough to wrap\n    -v,   --verbose Produce verbose output\n\n  Advanced options:\n          --long-only   Only a long name\n"), "{got}");
    assert!(got.contains("\n  Child options:\n          --child-long[=VAL]\n                    A long child option"), "{got}");
    let dup = Format { dup_args: true, dup_args_note: false, ..Format::default() };
    let got = render(&tree, &dup, HELP_LONG, same);
    assert!(got.starts_with("  -l[N], -s[N], --level[=N], --quiet[=N], --silent[=N]\n"), "{got}");
    assert!(got.contains("\n  -o FILE, --output=FILE "), "{got}");
    assert!(!got.contains("Mandatory"), "{got}");
}

#[test]
fn help_format() {
    // Clauses: s1_7_c_2 (moved from the unit tests of `argp.rs`)
    let mut errors = Vec::new();
    let f = Format::new(Some(b"rmargin=40, dup-args no-dup-args-note,bogus,rmargin=x,no-rmargin,=1"), Some(b"50"), |e| errors.push(format!("{e:?}")));
    assert_eq!((f.rmargin, f.dup_args, f.dup_args_note), (40, true, false));
    assert_eq!(errors.len(), 4, "{errors:?}");
    assert_eq!(Format::new(None, Some(b"50"), |_| {}).rmargin, 49);
    // The differences row: rmargin=0 is reported and ignored.
    let mut n = 0;
    assert_eq!(Format::new(Some(b"rmargin=0"), None, |_| n += 1).rmargin, 79);
    assert_eq!(n, 1);
    let f = Format::new(Some(b"short-opt-col=4,long-opt-col=10,opt-doc-col=20,header-col=2,usage-indent=8,doc-opt-col=3"), None, |_| panic!());
    assert_eq!(f, Format { short_opt_col: 4, long_opt_col: 10, opt_doc_col: 20, header_col: 2, usage_indent: 8, doc_opt_col: 3, ..Format::default() });
    // Probe-settled (COLD-REWRITE2-W-far-01-PROOF-4): a flag's value is a number, 0 turns it off
    // and any other turns it on.
    assert!(!Format::new(Some(b"dup-args,dup-args=0"), None, |_| panic!()).dup_args);
    assert!(Format::new(Some(b"dup-args=5"), None, |_| panic!()).dup_args);
    // D-1.7 differences row: numbers go up to 65535; a larger one is reported and ignored.
    assert_eq!(Format::new(Some(b"rmargin=65535"), None, |_| panic!()).rmargin, 65535);
    let mut bad = Vec::new();
    let f = Format::new(Some(b"rmargin=65536,dup-args=70000"), None, |e| bad.push(format!("{e:?}")));
    assert_eq!((f.rmargin, f.dup_args, bad.len()), (79, false, 2), "{bad:?}");
}

/// The tree of the help probe: sort ties, aliases, shadowed names, a documentation entry
/// starting with `-`, groups 1 and -2, a child that merges and a child with a header, and
/// `args_doc` alternatives in two parsers.
fn detail_tree() -> TryVec<Parser<'static>> {
    let c = |b: u8| i32::from(b);
    let root = table(&[
        opt(Some("zeta"), c(b'z'), None, 0, Some("Z doc"), 0),
        opt(Some("Alpha"), c(b'A'), None, 0, Some("A doc"), 0),
        opt(Some("alpha2"), 2000, None, 0, Some("a2 doc"), 0),
        opt(Some("beta"), c(b'b'), Some("ARG"), 0, Some("B doc"), 0),
        opt(Some("bee"), 0, None, OPTION_ALIAS | OPTION_HIDDEN, None, 0),
        opt(Some("dup"), c(b'z'), None, 0, Some("dup short"), 0),
        opt(Some("zeta"), 2001, None, 0, Some("dup long"), 0),
        opt(Some("-dash"), 0, None, OPTION_DOC, Some("dash doc"), 0),
        opt(Some("zdoc"), 0, None, OPTION_DOC, Some("doc entry"), 0),
        opt(Some("neg"), c(b'n'), None, 0, Some("neg doc"), -2),
        opt(Some("one"), c(b'1'), None, 0, Some("one doc"), 1),
        opt(Some("gone"), c(b'g'), None, 0, Some("drop me"), 0),
    ]);
    let mut t = TryVec::new();
    t.push(Parser { options: root, args_doc: Some(b"R1\nR2"), doc: Some(b"Root doc"), has_filter: true, ..Parser::default() }).unwrap();
    t.push(Parser { options: table(&[opt(Some("cee"), c(b'c'), None, 0, Some("C doc"), 0)]), args_doc: Some(b"C1"), doc: Some(b"C1 pre"), parent: Some(0), ..Parser::default() }).unwrap();
    t.push(Parser { options: table(&[opt(Some("kid"), c(b'k'), Some("V"), OPTION_ARG_OPTIONAL, Some("K doc"), 0)]), args_doc: Some(b"K1\nK2"), doc: Some(b"\x0bkid post"), parent: Some(0), header: Some(b"Kid:"), ..Parser::default() }).unwrap();
    t
}

#[test]
fn help_details_and_filters() {
    // Clauses: s1_7_c_2 (probe-settled layout; D-1.7: shadowed names and documentation entries are left out where the reference shows them)
    let tree = detail_tree();
    let mut seen: Vec<(usize, i32, Option<String>)> = Vec::new();
    let got = render(&tree, &Format::default(), HELP_STD_HELP, |p, key, text| {
        seen.push((p, key, text.map(|t| String::from_utf8_lossy(t).into_owned())));
        match text {
            Some(b"Z doc") => Filtered::Text(table_text(b"ZZ")),
            Some(b"drop me") => Filtered::Omit,
            _ if key == KEY_HELP_EXTRA => Filtered::Text(table_text(b"Extra text.")),
            _ => Filtered::Same,
        }
    });
    let want = "Usage: prog [OPTION...] R1 C1 K1
  or:  prog [OPTION...] R1 C1 K2
  or:  prog [OPTION...] R2 C1 K1
  or:  prog [OPTION...] R2 C1 K2
Root doc

      --alpha2               a2 doc
  -A, --Alpha                A doc
  -b, --beta=ARG             B doc
  -c, --cee                  C doc
  -dash                      dash doc
      --dup                  dup short
  -z, --zeta                 ZZ
  zdoc                       doc entry

 Kid:
  -k, --kid[=V]              K doc

  -1, --one                  one doc
  -g, --gone

  -n, --neg                  neg doc

  -?, --help                 Give this help list
      --usage                Give a short usage message
  -V, --version              Print program version

Mandatory or optional arguments to long options are also mandatory or optional
for any corresponding short options.

Extra text.

kid post

Report bugs to <bugs@example.org>.
";
    assert_eq!(got, want);
    let has = |p: usize, key: i32, text: Option<&str>| seen.iter().any(|s| *s == (p, key, text.map(String::from)));
    assert!(has(0, KEY_HELP_ARGS_DOC, Some("R1\nR2")), "{seen:?}");
    assert!(has(0, KEY_HELP_PRE_DOC, Some("Root doc")), "{seen:?}");
    assert!(has(0, KEY_HELP_HEADER, Some("Kid:")), "{seen:?}");
    assert!(has(0, KEY_HELP_POST_DOC, None), "{seen:?}");
    assert!(has(0, i32::from(b'z'), Some("Z doc")), "{seen:?}");
    assert!(has(0, 0, Some("doc entry")), "{seen:?}");
    assert!(seen.iter().all(|s| s.0 == 0), "only the root has a filter: {seen:?}");
    // Long usage (short usage wins when both are asked for); then short usage alone.
    let got = render(&tree, &Format::default(), HELP_USAGE, |_, _, _| Filtered::Same);
    assert_eq!(got, "Usage: prog [-Acz1gn?V] [-b ARG] [-k[V]] [--alpha2] [--Alpha] [--beta=ARG]\n            [--cee] [--dup] [--zeta] [--kid[=V]] [--one] [--gone] [--neg]\n            [--help] [--usage] [--version] R1 C1 K1\n  or:  prog [OPTION...] R1 C1 K2\n  or:  prog [OPTION...] R2 C1 K1\n  or:  prog [OPTION...] R2 C1 K2\n");
    assert_eq!(render(&tree, &Format::default(), HELP_USAGE | HELP_SHORT_USAGE, |_, _, _| Filtered::Same), render(&tree, &Format::default(), HELP_SHORT_USAGE, |_, _, _| Filtered::Same));
    let got = render(&tree, &Format::default(), HELP_SHORT_USAGE, |_, _, _| Filtered::Same);
    assert_eq!(got, "Usage: prog [OPTION...] R1 C1 K1\n  or:  prog [OPTION...] R1 C1 K2\n  or:  prog [OPTION...] R2 C1 K1\n  or:  prog [OPTION...] R2 C1 K2\n");
    let got = render(&tree, &Format::default(), HELP_POST_DOC, |_, _, _| Filtered::Same);
    assert_eq!(got, "kid post\n");
}

/// Help for one root table, as `prog --help` / `prog --usage` print it with no version and no bug address.
fn render_one(options: &[Opt<'static>], flags: u32) -> String {
    render_fmt(options, &Format::default(), flags)
}

fn render_fmt(options: &[Opt<'static>], fmt: &Format, flags: u32) -> String {
    let mut tree = TryVec::new();
    tree.push(Parser { options: table(options), has_parser: true, ..Parser::default() }).unwrap();
    let mut out = TryVec::new();
    help(&tree, Defaults { help: true, version: false }, fmt, flags, b"prog", None, |_, _, _| Filtered::Same, &mut out).unwrap();
    String::from_utf8(out.into_inner()).unwrap()
}

#[test]
fn help_groups_and_no_usage() {
    // Clauses: s1_7_c_2
    // Settled by black-box probes of the reference (COLD-REWRITE2-W-far-01-PROOF-2): a header with
    // group 0 starts a new group, so an option before it stays above it.
    let c = |b: u8| i32::from(b);
    let tail = "\n  -?, --help                 Give this help list\n      --usage                Give a short usage message\n";
    let implicit = [
        opt(Some("zeta"), c(b'z'), None, 0, Some("zeta option"), 0),
        opt(None, 0, None, 0, Some("Section one:"), 0),
        opt(Some("alpha"), c(b'a'), None, 0, Some("alpha option"), 0),
    ];
    let want = format!("Usage: prog [OPTION...]\n\n  -z, --zeta                 zeta option\n\n Section one:\n  -a, --alpha                alpha option\n{tail}");
    assert_eq!(render_one(&implicit, HELP_STD_HELP), want);
    // Not tested: the reference lists an option with neither a long name nor a printable key
    // (its doc text at the header column, sorted by its empty name); Liella leaves it out
    // (docs/differences-from-glibc.md, argp nameless options).
    // OPTION_NO_USAGE leaves an option out of the usage line, never out of the option list.
    let no_usage = [
        opt(Some("quiet"), c(b'q'), None, OPTION_NO_USAGE, Some("quiet option"), 0),
        opt(Some("level"), c(b'l'), Some("N"), OPTION_NO_USAGE, Some("level option"), 0),
        opt(Some("verbose"), c(b'v'), None, 0, Some("verbose option"), 0),
    ];
    assert_eq!(render_one(&no_usage, HELP_USAGE), "Usage: prog [-v?] [--verbose] [--help] [--usage]\n");
    let want = "Usage: prog [OPTION...]\n\n  -l, --level=N              level option\n  -q, --quiet                quiet option\n  -v, --verbose              verbose option\n  -?, --help                 Give this help list\n      --usage                Give a short usage message\n\nMandatory or optional arguments to long options are also mandatory or optional\nfor any corresponding short options.\n";
    // Probed (ARGP-HELP-FILTER-KEYS-2): option groups are not separated until a header has
    // appeared, so no blank line before `--help` here, as in the reference.
    assert_eq!(render_one(&no_usage, HELP_STD_HELP), want);
    // Probed (ARGP_HELP_FMT=dup-args): with the argument on both names the note is not printed,
    // even with dup-args-note left on.
    let got = render_fmt(&no_usage, &Format { dup_args: true, dup_args_note: true, ..Format::default() }, HELP_STD_HELP);
    assert!(got.contains("\n  -l N, --level=N            level option\n") && !got.contains("Mandatory"), "{got}");
}

fn table_text(t: &[u8]) -> TryVec<u8> {
    let mut v = TryVec::new();
    v.extend_from_slice(t).unwrap();
    v
}

#[test]
fn help_lists_options_without_a_name() {
    // Clauses: s1_7_c_2 (probe-settled: an entry with neither a long name nor a printable key is listed as a header line
    // carrying its text, even when hidden, in its group before the group's options, filtered as a header; one without
    // text prints nothing; the table is the reference's answer for the same table, checked by argp_products)
    let c = i32::from;
    let root = table(&[
        opt(Some("alpha"), c(b'a'), None, 0, Some("alpha doc"), 0),
        opt(None, 0, None, 0, Some("Real header:"), 0),
        opt(None, c(b'z'), None, 0, Some("short z"), 0),
        opt(None, 1000, None, 0, Some("keyless 1000"), 0),
        opt(Some("beta"), 1002, Some("B"), 0, Some("long only"), 0),
        opt(None, 0x80, None, 0, Some("key 0x80"), 0),
        opt(None, -1, None, 0, Some("key -1"), 0),
        opt(Some("gamma"), c(b'g'), None, 0, Some("gamma doc"), 0),
        opt(None, 1005, Some("A"), 0, Some("keyless g7"), 7),
        opt(Some("delta"), c(b'd'), None, 0, Some("delta g7"), 7),
        opt(None, 1006, None, 0, None, 0),
        opt(None, 1007, None, OPTION_DOC, Some("keyless doc"), 0),
        opt(None, 1008, None, OPTION_NO_USAGE, Some("keyless nousage"), 0),
        opt(None, 1003, None, OPTION_HIDDEN, Some("hidden keyless"), 0),
    ]);
    let mut t = TryVec::new();
    t.push(Parser { options: root, has_filter: true, ..Parser::default() }).unwrap();
    let mut keys: Vec<(i32, String)> = Vec::new();
    let got = render(&t, &Format::default(), HELP_STD_HELP, |_, key, text| {
        keys.push((key, text.map(|t| String::from_utf8_lossy(t).into_owned()).unwrap_or_default()));
        Filtered::Same
    });
    let want = "Usage: prog [OPTION...]\n\n  -a, --alpha                alpha doc\n\n Real header:\n\n keyless 1000\n\n key 0x80\n\n key -1\n      --beta=B               long only\n  -g, --gamma                gamma doc\n  -z                         short z\n\n keyless g7\n\n keyless doc\n\n keyless nousage\n\n hidden keyless\n  -d, --delta                delta g7\n\n  -?, --help                 Give this help list\n      --usage                Give a short usage message\n  -V, --version              Print program version\n\nReport bugs to <bugs@example.org>.\n";
    assert_eq!(got, want);
    for text in ["keyless 1000", "key 0x80", "keyless g7", "keyless doc", "hidden keyless"] {
        assert!(keys.contains(&(KEY_HELP_HEADER, text.to_string())), "{text}: {keys:?}");
    }
}

#[test]
fn help_hidden_clusters_and_short_args() {
    // Clauses: s1_7_c_2
    // Settled by black-box probes of the reference (COLD-REWRITE2-W-far-01-PROOF-3).
    let c = |b: u8| i32::from(b);
    let tail = "\n  -?, --help                 Give this help list\n      --usage                Give a short usage message\n";
    // OPTION_HIDDEN hides its own name only: an alias of a hidden option is listed, with the
    // entry's text, in help and in usage.
    let hide = [
        opt(Some("plain"), c(b'p'), None, 0, Some("Plain"), 0),
        opt(Some("secret"), c(b's'), None, OPTION_HIDDEN, Some("Secret"), 0),
        opt(Some("alt"), c(b'a'), None, OPTION_ALIAS, None, 0),
    ];
    // Probed (ARGP-HELP-FILTER-KEYS-2): no header before `--help`, so no blank line, as in the reference.
    let want = format!("Usage: prog [OPTION...]\n\n  -a, --alt                  Secret\n  -p, --plain                Plain\n{}", &tail[1..]);
    assert_eq!(render_one(&hide, HELP_STD_HELP), want);
    assert_eq!(render_one(&hide, HELP_USAGE), "Usage: prog [-ap?] [--alt] [--plain] [--help] [--usage]\n");
    // A short name without a long one shows its argument (`-n NUM`, `-o[X]`) without dup-args.
    let short = [
        opt(None, c(b'n'), Some("NUM"), 0, Some("number"), 0),
        opt(None, c(b'o'), Some("X"), OPTION_ARG_OPTIONAL, Some("optional"), 0),
        opt(Some("long"), c(b'l'), Some("V"), 0, Some("long one"), 0),
    ];
    let note = "\nMandatory or optional arguments to long options are also mandatory or optional\nfor any corresponding short options.\n";
    let want = format!("Usage: prog [OPTION...]\n\n  -l, --long=V               long one\n  -n NUM                     number\n  -o[X]                      optional\n{}{note}", &tail[1..]);
    assert_eq!(render_one(&short, HELP_STD_HELP), want);
    // A header's lines all start at header-col, an empty first line included.
    let head = [
        opt(Some("one"), c(b'1'), None, 0, Some("one"), 0),
        opt(None, 0, None, 0, Some("\nNext:"), 0),
        opt(Some("two"), c(b'2'), None, 0, Some("two"), 0),
    ];
    let want = format!("Usage: prog [OPTION...]\n\n  -1, --one                  one\n\n \n Next:\n  -2, --two                  two\n{tail}");
    assert_eq!(render_one(&head, HELP_STD_HELP), want);
    // A child with a non-zero group and no header is a cluster of its own: after the parent's
    // group-0 entries for group 1 (and -1). With group 0 its entries sort among the parent's.
    for (group, zeta_first) in [(0, false), (1, true), (-1, true)] {
        let mut tree = TryVec::new();
        tree.push(Parser { options: table(&[opt(Some("zeta"), c(b'z'), None, 0, Some("zeta option"), 0)]), has_parser: true, ..Parser::default() }).unwrap();
        tree.push(Parser { options: table(&[opt(Some("alpha"), c(b'a'), None, 0, Some("alpha option"), 0)]), parent: Some(0), group, has_parser: true, ..Parser::default() }).unwrap();
        let got = render(&tree, &Format::default(), HELP_STD_HELP, |_, _, _| Filtered::Same);
        let (z, a) = (got.find("  -z, --zeta ").unwrap(), got.find("  -a, --alpha ").unwrap());
        assert_eq!(z < a, zeta_first, "group {group}: {got}");
    }
}

#[test]
fn plus_key_is_an_option() {
    // Clauses: s1_7_c_0
    // D-1.7 (COLD-REWRITE2-W-far-01-REWRITE decision): a table whose first short option is `-+`
    // keeps it an option and keeps permuting. The reference reads that `+` as getopt's order
    // character instead (probe: `-+` is an invalid option and operands stop the scan);
    // docs/differences-from-glibc.md. The `;` that guards it is never an option itself.
    let mut tree = TryVec::new();
    tree.push(Parser { options: table(&[opt(None, i32::from(b'+'), None, 0, Some("plus"), 0)]), has_parser: true, ..Parser::default() }).unwrap();
    let act: Act = Box::new(|_, key, _, _| if key == KEY_ARG || key == i32::from(b'+') { 0 } else { ERR_UNKNOWN });
    let mut h = Rec::new(&["prog", "a", "-+"], act);
    let r = parse(&tree, Defaults::default(), 0, 0, false, false, &mut h);
    assert_eq!(r, Ok(Parsed { index: 3, stopped: false }));
    assert!(h.trace().starts_with("0:INIT(,n0,a0) 0:?(,n3,a0) 0:ARG(a,n3,a0) 0:END"), "{}", h.trace());
    let mut h = Rec::new(&["prog", "-;"], Box::new(|_, _, _, _| ERR_UNKNOWN));
    let r = parse(&tree, Defaults::default(), 0, 0, false, false, &mut h);
    assert_eq!(r, Err(Failed { code: 22, index: None }));
    assert!(h.other.len() == 1 && h.other[0].starts_with("option_error"), "{:?}", h.other);
}

#[test]
fn next_lowered_to_the_operand_itself() {
    // Clauses: s1_7_c_0
    // Ruling #397 (probe-settled in the decision record): a parser that takes an operand and sets
    // `next` to that operand's own index resumes one element earlier, so the element before it
    // is offered again.
    let mut once = true;
    let act: Act = Box::new(move |p, key, arg, view| match key {
        KEY_ARG if p == 0 && arg == "back" && once => {
            once = false;
            view.next = 2;
            0
        }
        KEY_ARG if p == 0 => 0,
        _ => ERR_UNKNOWN,
    });
    let (r, h) = run(&["prog", "x", "back", "z"], 0, true, act);
    assert_eq!(r, Ok(Parsed { index: 4, stopped: false }));
    let args: Vec<&str> = h.calls.iter().filter(|c| c.0 == 0 && c.1 == "ARG").map(|c| c.2.as_str()).collect();
    assert_eq!(args, ["x", "back", "x", "back", "z"], "{}", h.trace());
}

#[test]
fn child_inputs_and_init_errors() {
    // Clauses: s1_7_c_0
    // Each child reads its own slot of its parent's child inputs, in table order (the host
    // answers 100 * (parent + 1) + slot); the root gets argp_parse's input (7).
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = std::rc::Rc::clone(&seen);
    let act: Act = Box::new(move |p, key, _, view| {
        if key == KEY_INIT {
            log.borrow_mut().push((p, view.input));
        }
        ERR_UNKNOWN
    });
    let mut tree = small_tree();
    tree.push(Parser { parent: Some(0), has_parser: true, ..Parser::default() }).unwrap();
    let mut h = Rec::new(&["prog"], act);
    let r = parse(&tree, Defaults::default(), 0, 7, false, false, &mut h);
    assert_eq!(r, Ok(Parsed { index: 1, stopped: false }));
    assert_eq!(*seen.borrow(), [(0, 7), (1, 100), (2, 101)]);
    // An error from INIT ends the parse with that error before any element is read.
    let act: Act = Box::new(|p, key, _, _| if p == 0 && key == KEY_INIT { 5 } else { ERR_UNKNOWN });
    let (r, h) = run(&["prog", "a"], 0, true, act);
    assert_eq!(r, Err(Failed { code: 5, index: None }));
    assert!(!h.trace().contains("ARG"), "{}", h.trace());
}

#[test]
fn functionless_parsers_and_in_place_moves() {
    // Clauses: s1_7_c_0
    // A parser without a function takes nothing: an operand the root declines is one too many.
    let mut tree = TryVec::new();
    tree.push(Parser { has_parser: true, ..Parser::default() }).unwrap();
    tree.push(Parser { parent: Some(0), ..Parser::default() }).unwrap();
    let mut h = Rec::new(&["prog", "a"], Box::new(|_, _, _, _| ERR_UNKNOWN));
    let r = parse(&tree, Defaults::default(), 0, 0, false, false, &mut h);
    assert!(r.is_err(), "{r:?}");
    assert_eq!(h.other, ["usage_error too many"]);
    // In order (operands returned in place), a parser that raises `next` while taking an
    // operand skips the elements in between.
    let act: Act = Box::new(|_, key, arg, view| match key {
        KEY_ARG if arg == "skip" => {
            view.next += 1;
            0
        }
        KEY_ARG => 0,
        _ => ERR_UNKNOWN,
    });
    let (r, h) = run(&["prog", "skip", "y", "z"], IN_ORDER, true, act);
    assert_eq!(r, Ok(Parsed { index: 4, stopped: false }));
    let args: Vec<&str> = h.calls.iter().filter(|c| c.0 == 0 && c.1 == "ARG").map(|c| c.2.as_str()).collect();
    assert_eq!(args, ["skip", "z"], "{}", h.trace());
}

#[test]
fn help_wraps_past_the_margin_only() {
    // Clauses: s1_7_c_2
    // Probed (ARGP_HELP_FMT=rmargin=40, COLD-REWRITE2-W-far-01-PROOF-3): a word that ends exactly at
    // the right margin stays on the line; one column more wraps it.
    let opts = [opt(Some("xx"), i32::from(b'x'), None, 0, Some("aaaaa bbbbb ccc"), 0), opt(Some("yy"), i32::from(b'y'), None, 0, Some("aaaaa bbbbbb ccc"), 0)];
    let got = render_fmt(&opts, &Format { rmargin: 40, ..Format::default() }, HELP_STD_HELP);
    let want = "  -x, --xx                   aaaaa bbbbb\n                             ccc\n  -y, --yy                   aaaaa\n                             bbbbbb ccc\n";
    assert!(got.contains(want), "{got}");
}

#[test]
fn help_without_options() {
    // Clauses: s1_7_c_2
    // D-1.7 (one blank line between sections): with nothing to list, the option section and the
    // blank line before it are left out.
    let mut tree = TryVec::new();
    tree.push(Parser { has_parser: true, ..Parser::default() }).unwrap();
    let mut out = TryVec::new();
    help(&tree, Defaults::default(), &Format::default(), HELP_USAGE | HELP_LONG, b"prog", None, |_, _, _| Filtered::Same, &mut out).unwrap();
    let got = String::from_utf8(out.into_inner()).unwrap();
    assert!(got.starts_with("Usage: prog") && !got.ends_with("\n\n"), "{got:?}");
}
