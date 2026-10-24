//! `host.conf` keywords and `RESOLV_*` overrides against the black-box record of the reference
//! (HOSTCONF-OPTIONS-PROBE, glibc 2.39 under `unshare -r -m` with private `/etc/host.conf`,
//! `/etc/hosts` and `hosts: files`; D-16.13). No spec clause: §16 is silent on `host.conf`'s
//! keywords beyond `multi`; the record below is the source.

use liella::names::hostconf::{on_off, parse, reorder_index, trim, Domains, Env, Settings};

fn conf(text: &str) -> Settings {
    parse(Some(text.as_bytes()), &Env::default())
}

fn env(vars: &[(&str, &str)]) -> Env {
    Env::read(|n| vars.iter().find(|(k, _)| k.as_bytes() == n).map(|(_, v)| v.as_bytes()))
}

fn doms(s: &Settings) -> Vec<String> {
    s.trim.iter().map(|d| String::from_utf8_lossy(d).into_owned()).collect()
}

/// No spec clause. Record: `multi on|off`, case, last line, a bad value keeps the setting.
#[test]
fn multi_keyword() {
    assert!(!conf("").multi);
    assert!(conf("multi on\n").multi);
    assert!(!conf("multi off\n").multi);
    assert!(conf("MULTI ON\n").multi);
    assert!(!conf("multi on\nmulti off\n").multi);
    assert!(conf("multi   on  # c\n").multi);
    assert!(!conf("multi yes\n").multi);
    assert!(parse(None, &Env::default()) == Settings::default());
}

/// No spec clause. Record: `RESOLV_MULTI` overrides the file; a bad value is ignored.
#[test]
fn multi_env() {
    assert!(!parse(Some(b"multi on\n"), &env(&[("RESOLV_MULTI", "off")])).multi);
    assert!(parse(Some(b""), &env(&[("RESOLV_MULTI", "on")])).multi);
    assert!(parse(Some(b"multi on\n"), &env(&[("RESOLV_MULTI", "maybe")])).multi);
    assert_eq!(on_off(b"On"), Some(true));
    assert_eq!(on_off(b"yes"), None);
}

/// No spec clause. Record: `reorder on|off` and `RESOLV_REORDER` both ways.
#[test]
fn reorder_keyword_and_env() {
    assert!(conf("multi on\nreorder on\n").reorder);
    assert!(!conf("multi on\nreorder off\n").reorder);
    assert!(parse(Some(b"multi on\n"), &env(&[("RESOLV_REORDER", "on")])).reorder);
    assert!(!parse(Some(b"reorder on\n"), &env(&[("RESOLV_REORDER", "off")])).reorder);
}

const NETS: [([u8; 4], u8); 2] = [([127, 0, 0, 1], 8), ([10, 0, 100, 53], 24)];

/// No spec clause. Record (interfaces lo 127.0.0.1/8, 10.0.100.53/24): the first address on
/// a local network swaps with the first; netmask honoured; loopback counts.
#[test]
fn reorder_order() {
    let swap = |v: &[[u8; 4]]| {
        let mut v = v.to_vec();
        if let Some(i) = reorder_index(&v, &NETS) {
            v.swap(0, i);
        }
        v
    };
    let (a, b) = ([203, 0, 113, 9], [198, 51, 100, 1]);
    assert_eq!(swap(&[a, [10, 0, 100, 200], [127, 0, 0, 5]]), [[10, 0, 100, 200], a, [127, 0, 0, 5]]);
    assert_eq!(swap(&[a, b, [10, 0, 100, 203]]), [[10, 0, 100, 203], b, a]);
    assert_eq!(swap(&[a, b, [127, 0, 0, 7]]), [[127, 0, 0, 7], b, a]);
    assert_eq!(swap(&[a, [10, 0, 101, 6], [10, 0, 100, 204]]), [[10, 0, 100, 204], [10, 0, 101, 6], a]);
    assert_eq!(swap(&[[10, 0, 100, 1], a]), [[10, 0, 100, 1], a]);
    assert_eq!(swap(&[a, b]), [a, b]);
}

/// No spec clause. Record: `trim` lists, separators, the limit of four, and the environment.
#[test]
fn trim_lists() {
    assert_eq!(doms(&conf("trim example.com,sub.example.org\n")), ["example.com", "sub.example.org"]);
    assert_eq!(doms(&conf("trim   .example.com ; example.org\n")), [".example.com", "example.org"]);
    assert_eq!(doms(&conf("trim a.x b.x c.x d.x e.x\n")), ["a.x", "b.x", "c.x", "d.x"]);
    assert_eq!(doms(&parse(Some(b""), &env(&[("RESOLV_ADD_TRIM_DOMAINS", "example.com")]))), ["example.com"]);
    assert_eq!(doms(&parse(Some(b"trim example.org\n"), &env(&[("RESOLV_OVERRIDE_TRIM_DOMAINS", "example.com")]))), ["example.com"]);
    assert_eq!(doms(&parse(Some(b"trim example.org\n"), &env(&[("RESOLV_ADD_TRIM_DOMAINS", "example.com")]))), ["example.org", "example.com"]);
    assert_eq!(doms(&parse(Some(b""), &env(&[("RESOLV_ADD_TRIM_DOMAINS", ".example.org:.example.com")]))), [".example.org:.example.com"]);
}

/// No spec clause. Record: names trimmed by suffix, ASCII case ignored, never to nothing.
#[test]
fn trim_names() {
    let d = |t: &str| {
        let mut d = Domains::EMPTY;
        d.add_list(t.as_bytes());
        d
    };
    assert_eq!(trim(b"trim1.example.com", &d("example.com")), b"trim1.");
    assert_eq!(trim(b"trim1.example.com", &d(".example.com")), b"trim1");
    assert_eq!(trim(b"t1", &d("example.com")), b"t1");
    assert_eq!(trim(b"trim2.sub.example.org", &d("example.com sub.example.org")), b"trim2.");
    assert_eq!(trim(b"up.Example.Com", &d("example.com")), b"up.");
    assert_eq!(trim(b"EXAMPLE.COM", &d("example.com")), b"EXAMPLE.COM");
}

/// No spec clause. Record: the keywords the reference ignores change nothing.
#[test]
fn ignored_keywords() {
    for t in ["nospoof on\n", "spoof warn\nspoofalert on\n", "order bind,hosts\n"] {
        assert_eq!(conf(t), Settings::default(), "{t}");
    }
    let e = env(&[("RESOLV_SPOOF_CHECK", "warn"), ("RESOLV_SERV_ORDER", "bind")]);
    assert_eq!(e, Env::default());
}
