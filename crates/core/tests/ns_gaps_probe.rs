// Specified from spec §16.3 and §16.4 (phase 10's settled gaps); the record is spec/behaviour/ns-gaps.md.

//! Phase 10's name-service gaps, settled by black-box probes of the host C library
//! (`tests/oracle/ns_gaps.c` and `ns_gaps2.c`, the latter run in its own user, mount and network
//! namespaces) and pinned here with the probes' recorded outputs: `fp_nquery` headings under a
//! non-zero `pfcode`, MINFO and RP records with single-label and root names, the `networks`
//! database through `dns`, and the group-list merge when groups share a GID or a name. The DNS
//! fixture is the in-process server on a loopback port; nothing leaves the machine.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]

use liella::names::dns::text;
use liella::names::nss::Config;
use liella_boundary::sys::CSys;
use liella_legacy::netdb_compat as netdb;
use liella_legacy::nss::{Ctx, Paths};
use liella_legacy::resolv_compat::Resolver;

#[path = "support/dns_server.rs"]
mod dns_server;

fn put16(m: &mut Vec<u8>, v: u16) {
    m.extend_from_slice(&v.to_be_bytes());
}

fn name(m: &mut Vec<u8>, s: &str) {
    for l in s.split('.').filter(|l| !l.is_empty()) {
        m.push(u8::try_from(l.len()).unwrap());
        m.extend_from_slice(l.as_bytes());
    }
    m.push(0);
}

fn rr(m: &mut Vec<u8>, t: u16, a: &str, b: &str) {
    name(m, "a.test");
    put16(m, t);
    put16(m, 1);
    m.extend_from_slice(&300u32.to_be_bytes());
    let at = m.len();
    put16(m, 0);
    name(m, a);
    name(m, b);
    let l = u16::try_from(m.len() - at - 2).unwrap();
    m[at..at + 2].copy_from_slice(&l.to_be_bytes());
}

/// The probe's message: one question, four answers (MINFO `r e`, RP `mbox txt`, MINFO of two
/// root names, RP `r.b.test t.test`).
fn message() -> Vec<u8> {
    let mut m = Vec::new();
    for v in [0x1234, 0x8580, 1, 4, 0, 0] {
        put16(&mut m, v);
    }
    name(&mut m, "a.test");
    put16(&mut m, 14);
    put16(&mut m, 1);
    rr(&mut m, 14, "r", "e");
    rr(&mut m, 17, "mbox", "txt");
    rr(&mut m, 14, "", "");
    rr(&mut m, 17, "r.b.test", "t.test");
    m
}

fn dump(pfcode: u64) -> String {
    let mut out = liella::mem::TryVec::new();
    text::nquery(&message(), pfcode, &mut out).unwrap();
    String::from_utf8(out.as_slice().to_vec()).unwrap()
}

const QUESTION: &str = ";;\ta.test, type = MINFO, class = IN\n";
const ANSWERS: &str = "a.test.\t\t\t5M IN MINFO\tr. e.\na.test.\t\t\t5M IN RP\tmbox. txt.\na.test.\t\t\t5M IN MINFO\t. .\na.test.\t\t\t5M IN RP\tr.b.test. t.test.\n";
const COUNTS: &str = "; QUERY: 1, ANSWER: 4, AUTHORITY: 0, ADDITIONAL: 0\n";

/// Spec: s16_4_f_0. MINFO and RP with single-label and root names print as the probe shows: each
/// name with its trailing dot, the root as `.`.
#[test]
fn ns_gaps_minfo_rp_single_label() {
    assert_eq!(dump(0x20), ANSWERS);
    assert_eq!(dump(0x10), QUESTION);
}

/// Spec: s16_4_f_0. A non-zero `pfcode`: the counts line only with HEAD1, the section headings
/// (with a blank line after each section) only with HEAD1 and only for a section that has
/// records, none for an empty authority or additional section.
#[test]
fn ns_gaps_nquery_headings() {
    assert_eq!(dump(0x100), COUNTS);
    assert_eq!(dump(0x110), format!("{COUNTS};; QUERY SECTION:\n{QUESTION}\n"));
    let full = format!("{COUNTS};; QUERY SECTION:\n{QUESTION}\n;; ANSWER SECTION:\n{ANSWERS}\n");
    assert_eq!(dump(0x130), full);
    assert_eq!(dump(0x1f0), full, "empty authority and additional sections have no heading");
    assert_eq!(dump(0x140), COUNTS);
    assert_eq!(dump(0x180), COUNTS);
    assert_eq!(dump(0x40), "");
    assert_eq!(dump(0x80), "");
    assert_eq!(dump(0x300), "; flags: qr aa rd ra; QUERY: 1, ANSWER: 4, AUTHORITY: 0, ADDITIONAL: 0\n");
    assert_eq!(dump(0x800), ";; ->>HEADER<<- opcode: QUERY, status: NOERROR, id: 4660\n;\n");
    assert_eq!(dump(0x8f0), format!(";; ->>HEADER<<- opcode: QUERY, status: NOERROR, id: 4660\n;\n{QUESTION}{ANSWERS}"));
    assert_eq!(dump(0x9f0), format!(";; ->>HEADER<<- opcode: QUERY, status: NOERROR, id: 4660\n;{COUNTS}{}", &full[COUNTS.len()..]));
    assert_eq!(dump(0xb30), format!(";; ->>HEADER<<- opcode: QUERY, status: NOERROR, id: 4660\n;; flags: qr aa rd ra; QUERY: 1, ANSWER: 4, AUTHORITY: 0, ADDITIONAL: 0\n{}", &full[COUNTS.len()..]));
}

/// The fixture files of the lookup contexts: the group file and the switch only.
struct Fx {
    dir: std::path::PathBuf,
    paths: Paths<'static>,
}

fn z(p: &std::path::Path) -> &'static [u8] {
    let mut v = p.as_os_str().as_encoded_bytes().to_vec();
    v.push(0);
    v.leak()
}

impl Fx {
    fn new(tag: &str, group: &str) -> Fx {
        let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("nsgaps-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("group"), group).unwrap();
        let f = |n: &str| z(&dir.join(n));
        let paths = Paths {
            nsswitch: f("nsswitch.conf"),
            host_conf: f("host.conf"),
            gai_conf: f("gai.conf"),
            hosts: f("hosts"),
            networks: f("networks"),
            services: f("services"),
            protocols: f("protocols"),
            rpc: f("rpc"),
            ethers: f("ethers"),
            netgroup: f("netgroup"),
            aliases: f("aliases"),
            passwd: f("passwd"),
            group: f("group"),
            shadow: f("shadow"),
            gshadow: f("gshadow"),
        };
        Fx { dir, paths }
    }

    fn ctx(&self, conf: &str) -> Ctx<'_, CSys> {
        Ctx { sys: &CSys, paths: &self.paths, config: Config::parse(conf.as_bytes()), multi: false, dns: None, locale: None, cache: None }
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn net_resolver(port: u16) -> Resolver {
    let mut conf = liella::names::dns::conf::Config::defaults();
    conf.servers.push(liella::names::dns::conf::Server { ip: liella::names::Ip::V4([127, 0, 0, 1]), port, scope: 0 }).unwrap();
    conf.timeout = 1;
    conf.attempts = 1;
    Resolver { conf, aliases: None }
}

type Net = Result<(String, u32, String), liella::names::nss::LookupError>;

fn net_show(n: &liella::names::db::Numbered<'_, u32>) -> (String, u32, String) {
    let al: Vec<String> = n.aliases.clone().map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    (String::from_utf8_lossy(n.name).into_owned(), n.number, al.join(" "))
}

fn ok(name: &str, number: u32, aliases: &str) -> Net {
    Ok((name.into(), number, aliases.into()))
}

/// Spec: s16_3_c_0. The `networks` database through `dns`, as the reference does (the probe
/// `tests/oracle/net_dns.c`): `getnetbyaddr` asks a PTR question for the number without its
/// trailing zero octets, padded to four and reversed under `in-addr.arpa`; the first target is
/// the name, the others aliases. `getnetbyname` asks a PTR question for the name (search rules);
/// a target under `in-addr.arpa` gives the number. NXDOMAIN: NOT_FOUND; NOERROR without
/// records: NO_DATA; SERVFAIL, REFUSED, or an answer without a usable PTR: TRY_AGAIN.
#[test]
fn ns_networks_dns_case_table() {
    use liella::names::nss::LookupError::{NoData, NotFound, TryAgain};
    let srv = dns_server::start(vec![
        dns_server::ptr("0.2.0.192.in-addr.arpa", "netx.test"),
        dns_server::ptr("0.0.0.10.in-addr.arpa", "ten.test"),
        dns_server::ptr("0.0.16.172.in-addr.arpa", "b1.test"),
        dns_server::ptr("0.0.16.172.in-addr.arpa", "b2.test"),
        dns_server::ptr("1.0.0.224.in-addr.arpa", "d.test"),
        dns_server::ptr("p3.test", "2.0.192.in-addr.arpa"),
        dns_server::ptr("p5.test", "16.172.IN-ADDR.ARPA"),
        dns_server::ptr("p1.test", "netx.test"),
    ]);
    let fx = Fx::new("net", "");
    let r = net_resolver(srv.port);
    let ctx = Ctx { dns: Some(&r), ..fx.ctx("networks: dns\n") };
    let by_addr = |n: u32| -> Net { netdb::net_by_addr(&ctx, n, 2, net_show) };
    let by_name = |n: &str| -> Net { netdb::net_by_name(&ctx, n.as_bytes(), net_show) };
    assert_eq!(by_addr(0xc0_0002), ok("netx.test", 0xc0_0002, ""));
    assert_eq!(by_addr(0xc000_0200), ok("netx.test", 0xc0_0002, ""));
    assert_eq!(by_addr(10), ok("ten.test", 10, ""));
    assert_eq!(by_addr(0x0a00_0000), ok("ten.test", 10, ""));
    assert_eq!(by_addr(0xac10_0000), ok("b1.test", 0xac10, "b2.test"));
    assert_eq!(by_addr(0xe000_0001), ok("d.test", 0xe000_0001, ""));
    assert_eq!(by_addr(0xffff), Err(NoData));
    assert!(netdb::net_by_addr(&ctx, 10, 10, net_show).is_err());
    assert_eq!(by_name("p3.test"), ok("p3.test", 0xc0_0002, "2.0.192.in-addr.arpa"));
    assert_eq!(by_name("P5.TEST"), ok("P5.TEST", 0xac10, "16.172.IN-ADDR.ARPA"));
    assert_eq!(by_name("p1.test"), Err(TryAgain));
    assert_eq!(by_name("nx.test"), Err(NotFound));
    assert_eq!(by_name("nodata.test"), Err(NoData));
    assert_eq!(by_name("servfail.test"), Err(TryAgain));
    assert_eq!(by_name("refused.test"), Err(TryAgain));
    let log = srv.take_log();
    for q in ["0.2.0.192.in-addr.arpa", "0.0.0.10.in-addr.arpa", "0.0.16.172.in-addr.arpa", "1.0.0.224.in-addr.arpa", "0.0.255.255.in-addr.arpa", "p3.test", "nx.test"] {
        assert!(log.iter().any(|l| l.contains(&format!(" {q} ")) && l.contains("12") || l.contains(&format!(" {q} PTR"))), "no PTR question for {q}: {log:?}");
    }
    assert!(netdb::net_by_name(&Ctx { dns: Some(&r), ..fx.ctx("networks: files\n") }, b"p3.test", net_show).is_err());
}

/// Spec: s16_3_c_0. A damaged `networks` answer: an answer count beyond the packet gives the
/// records that fit; a PTR target that cannot be read is NOT_FOUND; nothing crashes.
#[test]
fn ns_networks_dns_damaged_answers() {
    let sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = sock.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut b = [0u8; 512];
        while let Ok((n, from)) = sock.recv_from(&mut b) {
            let mut o = b[..n].to_vec();
            o[2] = 0x85;
            o[3] = 0x80;
            o[7] = 3;
            o.extend_from_slice(&[0xc0, 0x0c, 0, 12, 0, 1, 0, 0, 0, 60]);
            if b[13] == b'1' {
                o.extend_from_slice(&[0, 2, 0xc5, 0xff]);
            } else {
                let mut rd = Vec::new();
                name(&mut rd, "short.test");
                o.extend_from_slice(&[0, rd.len() as u8]);
                o.extend_from_slice(&rd);
            }
            let _ = sock.send_to(&o, from);
        }
    });
    let fx = Fx::new("net", "");
    let r = net_resolver(port);
    let ctx = Ctx { dns: Some(&r), ..fx.ctx("networks: dns\n") };
    assert_eq!(netdb::net_by_addr(&ctx, 0xc0_0002, 2, net_show), ok("short.test", 0xc0_0002, ""));
    assert_eq!(netdb::net_by_addr(&ctx, 0x0100_0001, 2, net_show), Err(liella::names::nss::LookupError::NotFound));
}

const GROUPS: &str = "a:x:50:u\nb:x:50:u\nc:x:60:u\nc:x:61:u\nd:x:60:v\ne:x:70:u,u\nf:x:99:u\n";

fn list(fx: &Fx, user: &str, base: u32) -> Vec<u32> {
    let ctx = fx.ctx("group: files\ninitgroups: files\n");
    liella_legacy::nss::group_list(&ctx, user.as_bytes(), base).unwrap().as_slice().to_vec()
}

/// Spec: s16_3_c_0. The group-list merge (the probe's `getgrouplist` on a group file with two
/// groups of one GID, one name with two GIDs, a repeated member and the base GID in the file):
/// the base GID comes first and is never repeated; the file's other GIDs follow in file order,
/// and a GID shared by two groups is listed once per group (`50 50`), as the reference does.
#[test]
fn ns_gaps_initgroups_merge_disagreeing_groups() {
    let fx = Fx::new("grp", GROUPS);
    assert_eq!(list(&fx, "u", 50), [50, 60, 61, 70, 99]);
    assert_eq!(list(&fx, "u", 99), [99, 50, 50, 60, 61, 70]);
    assert_eq!(list(&fx, "u", 1), [1, 50, 50, 60, 61, 70, 99]);
    assert_eq!(list(&fx, "v", 60), [60]);
    assert_eq!(list(&fx, "v", 5), [5, 60]);
    assert_eq!(list(&fx, "nobody-here", 7), [7]);
}
