//! getopt's parser (`liella::getopt`, spec §1.6; D-1.6) against an independent model: random
//! argument vectors in the three orders, long vectors, and the parser state between calls
//! (the end, a moved index, `--`). The model reads the whole vector at once; the parser works
//! one call at a time and permutes lazily, so agreement checks the permutation and the
//! returned indices without fixing how the parser moves elements.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use liella::getopt::{self, Event, HasArg, LongOpt, Order, Spec, State};

/// A small deterministic generator (xorshift64).
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % u64::try_from(n).unwrap()).unwrap()
    }
}

/// Elements that hold options for `abc:d::`, plus unknown letters and the operand `-`.
const OPTION_ELEMENTS: [&str; 12] = ["-a", "-b", "-ab", "-c", "-cX", "-d", "-dY", "-ba", "-q", "-", "-ac", "-adZ"];

fn random_vector(rng: &mut Rng, len: usize) -> Vec<String> {
    let mut v = vec!["prog".to_owned()];
    for k in 0..len {
        let r = rng.below(100);
        v.push(if r < 45 {
            format!("o{k}")
        } else if r < 48 {
            "--".to_owned()
        } else {
            OPTION_ELEMENTS[rng.below(OPTION_ELEMENTS.len())].to_owned()
        });
    }
    v
}

/// What parsing `v` with `abc:d::` in `order` should give: the events (as [`show`] writes them),
/// the vector afterwards and the final index.
fn model(v: &[String], order: Order) -> (Vec<String>, Vec<String>, usize) {
    let n = v.len();
    let (mut events, mut opts, mut ops) = (Vec::new(), Vec::new(), Vec::new());
    let mut i = 1;
    let mut stop = None;
    while i < n {
        let t = v[i].as_str();
        if t == "--" {
            stop = Some(i);
            break;
        }
        if !t.starts_with('-') || t == "-" {
            match order {
                Order::Permute => ops.push(t.to_owned()),
                Order::RequireOrder => break,
                Order::ReturnInOrder => events.push(format!("op@{i}")),
            }
            i += 1;
            continue;
        }
        opts.push(t.to_owned());
        i += 1;
        let b = t.as_bytes();
        let mut j = 1;
        while j < b.len() {
            let c = b[j] as char;
            let rest = &t[j + 1..];
            match c {
                'a' | 'b' => events.push(c.to_string()),
                'c' => {
                    if !rest.is_empty() {
                        events.push(format!("c={rest}"));
                    } else if i < n {
                        events.push(format!("c={}", v[i]));
                        opts.push(v[i].clone());
                        i += 1;
                    } else {
                        events.push(":c".to_owned());
                    }
                    break;
                }
                'd' => {
                    events.push(if rest.is_empty() { "d".to_owned() } else { format!("d={rest}") });
                    break;
                }
                _ => events.push(format!("?{c}")),
            }
            j += 1;
        }
    }
    if order != Order::Permute {
        let next = stop.map_or(i, |s| s + 1);
        return (events, v.to_vec(), next);
    }
    let mut out = vec![v[0].clone()];
    out.extend(opts);
    let tail: &[String] = match stop {
        Some(s) => {
            out.push("--".to_owned());
            &v[s + 1..]
        }
        None => &[],
    };
    let next = out.len();
    out.extend(ops);
    out.extend(tail.iter().cloned());
    (events, out, next)
}

fn show(e: Event, argv: &[String]) -> String {
    match e {
        Event::Short { letter, arg: None } => (letter as char).to_string(),
        Event::Short { letter, arg: Some(a) } => format!("{}={}", letter as char, String::from_utf8_lossy(a.of(argv))),
        Event::Operand(i) => format!("op@{i}"),
        Event::Invalid(c) => format!("?{}", c as char),
        Event::MissingShort(c) => format!(":{}", c as char),
        other => panic!("unexpected event {other:?}"),
    }
}

/// Parse `v` to the end with `optstring`; then, unless a `--` ended it, check that two more calls
/// end at the same element.
fn parse(v: &mut [String], optstring: &str) -> (Vec<String>, usize, State) {
    let spec = Spec::new(optstring.as_bytes(), false);
    let mut st = State::new(0);
    let mut events = Vec::new();
    for _ in 0..(4 * v.len() + 8) {
        let e = getopt::next(&mut st, v, &spec, None::<&[LongOpt<'_>]>, false);
        assert!(st.next <= v.len(), "index {} beyond the vector", st.next);
        if e == Event::End {
            let end = st.next;
            // After a `--` the next call reads on (the options behind it); otherwise it ends again.
            for _ in 0..if st.dashes { 0 } else { 2 } {
                assert_eq!(getopt::next(&mut st, v, &spec, None::<&[LongOpt<'_>]>, false), Event::End, "a call after the end");
                assert_eq!(st.next, end, "a call after the end moved the index");
            }
            return (events, end, st);
        }
        events.push(show(e, v));
    }
    panic!("no end after {} calls", 4 * v.len() + 8);
}

fn check(v: &[String], optstring: &str, order: Order) {
    let (want_events, want_vec, want_next) = model(v, order);
    let mut got = v.to_vec();
    let (events, next, _) = parse(&mut got, optstring);
    assert_eq!(events, want_events, "events for {v:?} ({optstring})");
    assert_eq!(got, want_vec, "vector after parsing {v:?} ({optstring})");
    assert_eq!(next, want_next, "final index for {v:?} ({optstring})");
}

/// §1.6 classification and GNU permutation, `+` (stop at the first operand) and `-` (operands
/// in place), over random vectors.
#[test]
fn random_vectors_match_the_model() {
    // Clauses: s1_6_c_0, s1_6_i_2, s1_6_g_1, s1_6_g_3, s1_6_g_4, s1_6_g_5 (D-1.6).
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..3000 {
        let v = random_vector(&mut rng, case % 23);
        check(&v, "abc:d::", Order::Permute);
        check(&v, "+abc:d::", Order::RequireOrder);
        check(&v, "-abc:d::", Order::ReturnInOrder);
    }
}

/// §1.6 permutation over long vectors: many operand blocks, every block size.
#[test]
fn long_vectors_permute() {
    // Clauses: s1_6_g_4 (D-1.6).
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for len in [200, 1000, 4000] {
        check(&random_vector(&mut rng, len), "abc:d::", Order::Permute);
    }
    // Operands first, then options: one large rotation at the first boundary after them.
    let mut v = vec!["prog".to_owned()];
    v.extend((0..3000).map(|k| format!("o{k}")));
    v.extend((0..3000).map(|_| "-a".to_owned()));
    check(&v, "abc:d::", Order::Permute);
    // Alternating operands and options with separate arguments.
    let mut v = vec!["prog".to_owned()];
    for k in 0..2000 {
        v.extend([format!("o{k}"), "-c".to_owned(), format!("arg{k}")]);
    }
    check(&v, "abc:d::", Order::Permute);
}

/// §1.6 `optind`: a caller that moves the index continues from that element, forgetting the
/// position inside a cluster.
#[test]
fn moved_index_continues_there() {
    // Clauses: s1_6_g_3 (D-1.6).
    let mut v: Vec<String> = ["prog", "-ab", "-c", "x", "y"].iter().map(|s| (*s).to_owned()).collect();
    let spec = Spec::new(b"abc:", false);
    let mut st = State::new(0);
    let none = None::<&[LongOpt<'_>]>;
    assert_eq!(show(getopt::next(&mut st, v.as_mut_slice(), &spec, none, false), &v), "a");
    st.moved(2);
    assert_eq!(show(getopt::next(&mut st, v.as_mut_slice(), &spec, none, false), &v), "c=x");
    assert_eq!(st.next, 4);
    assert_eq!(getopt::next(&mut st, v.as_mut_slice(), &spec, none, false), Event::End);
    assert_eq!(st.next, 4, "the operand y");
}

/// §1.6 `--`: the end of the options is recorded (`dashes`), and the elements after it stay
/// where they are behind the permuted operands.
#[test]
fn double_dash_ends_options() {
    // Clauses: s1_6_g_5 (D-1.6).
    let mut v: Vec<String> = ["prog", "o1", "-a", "--", "-b"].iter().map(|s| (*s).to_owned()).collect();
    let (events, next, st) = parse(&mut v, "ab");
    assert_eq!(events, ["a"]);
    assert_eq!(v, ["prog", "-a", "--", "o1", "-b"]);
    assert_eq!(next, 3);
    assert!(st.dashes && st.ended);
    let mut v: Vec<String> = ["prog", "o1", "-a"].iter().map(|s| (*s).to_owned()).collect();
    let (_, next, st) = parse(&mut v, "ab");
    assert_eq!((next, st.dashes, st.ended), (2, false, true));
}

/// §1.6 long options: an exact name wins over an earlier entry it is a prefix of; a unique
/// prefix matches; `optopt` changes only on errors.
#[test]
fn exact_long_names_win() {
    // Clauses: s1_6_i_3, s1_6_f_2 (D-1.6).
    let t = [
        LongOpt { name: b"fill", has_arg: HasArg::Required, flag: 0, val: 1 },
        LongOpt { name: b"fi", has_arg: HasArg::No, flag: 0, val: 2 },
        LongOpt { name: b"verbose", has_arg: HasArg::No, flag: 0, val: 3 },
    ];
    let mut v: Vec<String> = ["prog", "--fi", "--fil=z", "--v", "--verbose=1", "-fi"].iter().map(|s| (*s).to_owned()).collect();
    let spec = Spec::new(b"f", false);
    let mut st = State::new(5);
    let mut call = |v: &mut Vec<String>, long_only| getopt::next(&mut st, v.as_mut_slice(), &spec, Some(&t[..]), long_only);
    assert_eq!(call(&mut v, false), Event::Long { entry: 1, arg: None });
    let e = call(&mut v, false);
    assert!(matches!(e, Event::Long { entry: 0, arg: Some(a) } if a.of(v.as_slice()) == b"z"), "{e:?}");
    assert_eq!(call(&mut v, false), Event::Long { entry: 2, arg: None });
    assert!(matches!(call(&mut v, false), Event::NoArgument { entry: 2, .. }));
    assert_eq!(call(&mut v, true), Event::Long { entry: 1, arg: None }, "long_only: -fi names the entry fi");
    assert_eq!(st.optopt, 3, "the entry's value after its argument error");
}
