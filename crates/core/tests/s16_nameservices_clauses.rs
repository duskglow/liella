// Specified from spec §16 (phase 10, part 3: §16.1 the Name Service Switch, §16.2 getaddrinfo and
// getnameinfo without DNS, §16.3, §16.5, §16.6; part 4: §16.4 the DNS resolver, the `dns` source,
// IDNA and getaddrinfo_a; nscd is D-16.5).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §16 (38 clauses). The switch, the `files` source and the lookups run
//! through `liella::names` and `liella_legacy::{nss, nss_files, netdb_compat, gai}` on fixture
//! files with configurations built from text (a `Ctx` of the test's own); the process-wide
//! state (the switch's overrides, the enumeration cursors) only in re-executed children. The
//! system's files are only read; nothing leaves the machine (numeric and loopback addresses).
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]

use liella::error::Errno;
use liella::names::nss::{self, Config, Database, LookupError, Service, Spec};
use liella::names::sort::{self, Candidate, Policy};
use liella::names::{msg, Family, Ip};
use liella_boundary::sys::CSys;
use liella_legacy::gai::{self, GaiError, Hints};
use liella_legacy::netdb_compat as netdb;
use liella_legacy::nss::{Ctx, Paths, Switch};
use liella_legacy::nss_files::Key;
use liella_legacy::resolv_compat::{self as resolv, Resolver, State, STATE_SIZE};
use liella::names::dns::conf::{self, Config as RConf, Server as RServer};
use liella::names::dns::{client, msg as dmsg, name, text};
use spectest::clause;

#[path = "support/dns_server.rs"]
mod dns_server;

// ------------------------------------------------------------------------------ fixtures

const HOSTS: &str = "127.0.0.1\tlocalhost\n::1 localhost ip6-localhost\n# c\n192.0.2.1 alpha alpha.example.org a1 # t\n192.0.2.2 beta\n192.0.2.3 beta b3\n\
                     2001:db8::10 gamma gamma6\n192.0.2.4 gamma\n300.0.0.1 badaddr\n192.0.2.6\n192.0.2.20 host.example.org hostalias\n";
const SERVICES: &str = "echo 7/tcp\necho 7/udp\nhttp 80/tcp www\nhttp 80/udp\nssh 22/tcp\ndgonly 5000/udp\nsctpx 6000/sctp\nbadport 70000/tcp\n";
const NETGROUP: &str = "trusted (alpha,alice,example.org) (beta,-,) nested\nnested (gamma,bob,)\nempty\nloop1 loop2\nloop2 loop1\n";

/// A fixture `/etc` under the temporary directory: its files (NUL-terminated paths).
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
    fn new(tag: &str) -> Fx {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c16-{}-{tag}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let files: [(&str, &str); 13] = [
            ("nsswitch.conf", "hosts: files\nnetworks: files\n"),
            ("hosts", HOSTS),
            ("networks", "loopback 127\nexample 192.0.2 exnet\nbad notanumber\n"),
            ("services", SERVICES),
            ("protocols", "ip 0 IP\ntcp 6 TCP\nudp 17 UDP\nsctp 132 SCTP\nbad x\n"),
            ("rpc", "portmapper 100000 portmap sunrpc\nnfs 100003 nfsprog\n"),
            ("ethers", "00:11:22:33:44:55 alpha\n0:1:2:3:4:5 beta\n"),
            ("netgroup", NETGROUP),
            ("aliases", "postmaster: root\nroot: alice, bob\nlist: :include:/x\nmulti: a,\n  b,\n  c\n"),
            ("passwd", "root:x:0:0::/root:/bin/sh\nalice:x:1000:1000::/home/alice:/bin/sh\n"),
            ("group", "root:x:0:\ng:x:10:a,b\nh:x:11:alice\n"),
            ("shadow", "alice:!:19000:0:99999:7:::\n"),
            ("gshadow", "g:!:a:a,b\n"),
        ];
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
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

    /// A lookup context: the fixture's files and the configuration `conf` (an nsswitch.conf text).
    fn ctx(&self, conf: &str) -> Ctx<'_, CSys> {
        Ctx { sys: &CSys, paths: &self.paths, config: Config::parse(conf.as_bytes()), multi: false, dns: None, locale: None, cache: None }
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const FILES: &str = "hosts: files\nnetworks: files\nservices: files\nprotocols: files\nrpc: files\nethers: files\nnetgroup: files\naliases: files\n";

/// The host's name, aliases and addresses (as text).
fn host(r: Result<liella_legacy::nss_files::Host, LookupError>) -> Result<String, LookupError> {
    r.map(|h| {
        let t = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        let addrs: Vec<String> = h
            .addrs
            .iter()
            .map(|a| match *a {
                Ip::V4(v) => std::net::Ipv4Addr::from(v).to_string(),
                Ip::V6(v) => std::net::Ipv6Addr::from(v).to_string(),
            })
            .collect();
        format!("{} [{}] {}", t(h.name()), h.aliases().map(t).collect::<Vec<_>>().join(","), addrs.join(","))
    })
}

/// Run the test `name` again, alone, in a child (process-wide state): true in the child.
fn in_child(name: &str) -> bool {
    if std::env::var("LIELLA_C16_CHILD").is_ok_and(|v| v == name) {
        // This process runs this one test alone: `forbid_threads` may be called in it.
        liella_boundary::selftest::mark_own_process();
        return true;
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args([name, "--exact", "--test-threads=1"]).env("LIELLA_C16_CHILD", name).output().unwrap();
    assert!(o.status.success(), "{name} in a child:\n{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    false
}

/// `getaddrinfo` over the fixture: the entries as `family/type/proto address port`, or the code.
fn gai(fx: &Fx, node: Option<&str>, service: Option<&str>, h: Hints) -> Result<Vec<String>, i32> {
    let r = gai::getaddrinfo(&fx.ctx(FILES), node.map(str::as_bytes), service.map(str::as_bytes), Some(h));
    match r {
        Ok(f) => Ok(f
            .entries
            .iter()
            .map(|e| {
                let a = match e.ip {
                    Ip::V4(v) => format!("2/{}/{} {}", e.socktype, e.protocol, std::net::Ipv4Addr::from(v)),
                    Ip::V6(v) => format!("10/{}/{} {}", e.socktype, e.protocol, std::net::Ipv6Addr::from(v)),
                };
                format!("{a} {}", e.port)
            })
            .collect()),
        Err(GaiError::Code(c)) => Err(c),
        Err(GaiError::System(_)) => Err(gai::EAI_SYSTEM),
    }
}

fn hints(flags: i32, family: i32, socktype: i32, protocol: i32) -> Hints {
    Hints { flags, family, socktype, protocol }
}

// ------------------------------------------------------------------------------ §16.1

#[test]
fn s16_1_c_0() {
    // Every database has its nsswitch.conf name and is configured per line.
    let names = ["passwd", "group", "shadow", "gshadow", "hosts", "networks", "protocols", "services", "rpc", "ethers", "netgroup", "aliases", "publickey", "initgroups"];
    for n in names {
        let db = Database::from_name(n.as_bytes()).unwrap();
        assert_eq!(db.name(), n.as_bytes());
        let c = Config::parse(format!("{n}: nis [NOTFOUND=return] files\n").as_bytes());
        assert_eq!(c.spec(db).steps().len(), 2, "{n}: its own line");
        assert_eq!(c.spec(db).steps()[0].service, Service::Module);
    }
    assert_eq!(Database::from_name(b"sudoers"), None, "unknown databases are ignored");
    // Each database the library reads is routed through its line: with `files` the fixture
    // answers, with an unavailable source alone nothing does.
    let fx = Fx::new("dbs");
    let on = fx.ctx(FILES);
    let off = fx.ctx("hosts: x\nnetworks: x\nservices: x\nprotocols: x\nrpc: x\nethers: x\nnetgroup: x\naliases: x\npasswd: x\ngroup: x\nshadow: x\ngshadow: x\n");
    for (c, ok) in [(&on, true), (&off, false)] {
        assert_eq!(netdb::host_by_name(c, b"alpha", Family::V4).is_ok(), ok, "hosts");
        assert_eq!(netdb::net_by_name(c, b"loopback", |n| n.number).is_ok(), ok, "networks");
        assert_eq!(netdb::serv_by_name(c, b"ssh", None, |s| s.port).is_ok(), ok, "services");
        assert_eq!(netdb::proto_by_number(c, 6, |p| p.number).is_ok(), ok, "protocols");
        assert_eq!(netdb::rpc_by_number(c, 100003, |p| p.number).is_ok(), ok, "rpc");
        assert_eq!(liella_legacy::nss::ether_by_name(c, b"alpha").is_ok(), ok, "ethers");
        assert_eq!(liella_legacy::nss::netgroup(c, b"trusted").is_ok(), ok, "netgroup");
        assert_eq!(liella_legacy::nss::alias(c, b"root", |_| ()).is_ok(), ok, "aliases");
        assert_eq!(liella_legacy::nss::passwd(c, Key::Name(b"alice"), |p| p.uid).is_ok(), ok, "passwd");
        assert_eq!(liella_legacy::nss::group(c, Key::Id(10), |g| g.gid).is_ok(), ok, "group");
        assert_eq!(liella_legacy::nss::shadow(c, b"alice", |_| ()).is_ok(), ok, "shadow");
        assert_eq!(liella_legacy::nss::gshadow(c, b"g", |_| ()).is_ok(), ok, "gshadow");
        assert_eq!(liella_legacy::nss::group_list(c, b"alice", 1000).unwrap().len(), if ok { 2 } else { 1 }, "initgroups (group's sources)");
    }
}

#[test]
fn s16_1_c_1() {
    for (name, s) in [("files", Service::Files), ("dns", Service::Dns), ("compat", Service::Compat), ("db", Service::Module), ("systemd", Service::Module), ("sss", Service::Module), ("ldap", Service::Module), ("winbind", Service::Module)] {
        assert_eq!(Service::from_name(name.as_bytes()), s, "{name}");
    }
    let fx = Fx::new("sources");
    // A module (until the NSS module ABI of phase 14) is unavailable: the lookup goes on.
    let c = fx.ctx("passwd: sss files\nhosts: systemd files\n");
    assert_eq!(liella_legacy::nss::passwd(&c, Key::Name(b"alice"), |p| p.uid), Ok(1000));
    assert!(netdb::host_by_name(&c, b"alpha", Family::V4).is_ok());
    let c = fx.ctx("passwd: sss [UNAVAIL=return] files\n");
    assert_eq!(liella_legacy::nss::passwd(&c, Key::Name(b"alice"), |p| p.uid), Err(LookupError::Unavailable));
    // `compat` reads the files for passwd, group and shadow, and is a module elsewhere.
    let c = fx.ctx("passwd: compat\ngroup: compat\nshadow: compat\nhosts: compat\n");
    assert_eq!(liella_legacy::nss::passwd(&c, Key::Id(1000), |p| p.uid), Ok(1000));
    assert_eq!(liella_legacy::nss::group(&c, Key::Name(b"g"), |g| g.gid), Ok(10));
    assert!(liella_legacy::nss::shadow(&c, b"alice", |_| ()).is_ok());
    assert_eq!(netdb::host_by_name(&c, b"alpha", Family::V4).map(|_| ()), Err(LookupError::Unavailable));
    // Names are case-sensitive (they name modules).
    assert_eq!(Service::from_name(b"Files"), Service::Module);
}

#[test]
fn s16_1_c_2() {
    use nss::{Action, Status};
    let s = Spec::parse(b"dns [!UNAVAIL=return] files [NOTFOUND=RETURN tryagain = continue] x [SUCCESS=merge]").unwrap();
    let st = s.steps();
    assert_eq!(st.len(), 3);
    assert_eq!([Status::Success, Status::NotFound, Status::Unavail, Status::TryAgain].map(|x| st[0].action(x)), [Action::Return, Action::Return, Action::Continue, Action::Return]);
    assert_eq!(st[1].action(Status::NotFound), Action::Return, "keywords in any case");
    assert_eq!(st[1].action(Status::Unavail), Action::Continue, "the defaults stay");
    assert_eq!(st[2].action(Status::Success), Action::Merge);
    for bad in ["[NOTFOUND=return] files", "files [FOO=return]", "files [NOTFOUND=stop]", "files [NOTFOUND=return"] {
        assert_eq!(Spec::parse(bad.as_bytes()), None, "{bad}");
    }
    // A line that does not parse leaves the database without sources.
    assert_eq!(Config::parse(b"hosts: [NOTFOUND=return] files\n").spec(Database::Hosts).steps().len(), 0);
    // The rules decide which source answers.
    let ask = |r: [Result<i32, LookupError>; 2]| move |svc: Service| if svc == Service::Files { r[0] } else { r[1] };
    let spec = |t: &str| Spec::parse(t.as_bytes()).unwrap();
    let keep = |_: &mut i32, _: i32| Ok(());
    assert_eq!(nss::run(&spec("files dns"), ask([Err(LookupError::NotFound), Ok(2)]), keep), Ok(2), "notfound continues");
    assert_eq!(nss::run(&spec("files [NOTFOUND=return] dns"), ask([Err(LookupError::NotFound), Ok(2)]), keep), Err(LookupError::NotFound));
    assert_eq!(nss::run(&spec("files [SUCCESS=continue] dns"), ask([Ok(1), Err(LookupError::NotFound)]), keep), Err(LookupError::NotFound), "continue discards a success");
    assert_eq!(nss::run(&spec("files [TRYAGAIN=return] dns"), ask([Err(LookupError::TryAgain), Ok(2)]), keep), Err(LookupError::TryAgain));
    // __nss_configure_lookup: process-wide, so in a child.
    if !in_child("s16_1_c_2") {
        return;
    }
    let sw = Switch::new();
    let fx = Fx::new("configure");
    assert_eq!(sw.configure(&CSys, b"hosts", b"x [UNAVAIL=return] files"), Ok(()));
    let mut c = sw.config(&CSys, fx.paths.nsswitch);
    assert_eq!(c.spec(Database::Hosts).steps().len(), 2, "the override replaces the file's line");
    assert_eq!(c.spec(Database::Networks).steps().len(), 1, "other databases keep the file's");
    assert_eq!(sw.configure(&CSys, b"nosuchdb", b"files"), Err(Errno::EINVAL));
    assert_eq!(sw.configure(&CSys, b"hosts", b"files [NOTFOUND=return"), Err(Errno::EINVAL));
    c = sw.config(&CSys, fx.paths.nsswitch);
    assert_eq!(c.spec(Database::Hosts).steps().len(), 2, "a refused override changes nothing");
    // Through the export: the process's switch.
    use liella_boundary::posix::s16_nameservices as x;
    assert_eq!(x::__nss_configure_lookup(c"hosts".as_ptr(), c"nosuch".as_ptr()), 0);
    assert!(x::gethostbyname(c"alpha.invalid".as_ptr()).is_null());
    assert_eq!(liella_boundary::netdb::h_errno(), netdb::NO_RECOVERY, "no source is available");
    liella_boundary::errno::set_raw(0);
    assert_eq!(x::__nss_configure_lookup(c"nosuchdb".as_ptr(), c"files".as_ptr()), -1);
    assert_eq!(liella_boundary::errno::get(), Errno::EINVAL.get());
}

#[test]
fn s16_1_g_1() {
    let fx = Fx::new("merge");
    // The first source that succeeds answers.
    let c = fx.ctx("passwd: x files files\n");
    assert_eq!(liella_legacy::nss::passwd(&c, Key::Name(b"alice"), |p| p.dir.to_vec()), Ok(b"/home/alice".to_vec()));
    // `merge` concatenates group members across sources (repeats kept, nsswitch.conf(5)).
    let c = fx.ctx("group: files [SUCCESS=merge] files\n");
    let members = |c: &Ctx<'_, CSys>| liella_legacy::nss::group(c, Key::Name(b"g"), |g| g.members.iter().map(<[u8]>::to_vec).collect::<Vec<_>>());
    assert_eq!(members(&c), Ok(vec![b"a".to_vec(), b"b".to_vec(), b"a".to_vec(), b"b".to_vec()]));
    assert_eq!(members(&fx.ctx("group: files\n")), Ok(vec![b"a".to_vec(), b"b".to_vec()]));
    // After a merge, a later failure is ignored and the kept result returned.
    let c = fx.ctx("group: files [SUCCESS=merge] x\n");
    assert_eq!(members(&c), Ok(vec![b"a".to_vec(), b"b".to_vec()]));
}

#[test]
fn s16_1_g_2() {
    let fx = Fx::new("reload");
    let sw = Switch::new();
    let path = fx.paths.nsswitch;
    assert_eq!(sw.config(&CSys, path).spec(Database::Hosts).steps().len(), 1);
    std::fs::write(fx.dir.join("nsswitch.conf"), "hosts: x files [NOTFOUND=return] y\n").unwrap();
    assert_eq!(sw.config(&CSys, path).spec(Database::Hosts).steps().len(), 3, "a changed file is read again");
    std::fs::remove_file(fx.dir.join("nsswitch.conf")).unwrap();
    assert_eq!(sw.config(&CSys, path).spec(Database::Hosts), nss::default_spec(Database::Hosts), "without the file, the defaults (files dns)");
    assert_eq!(sw.config(&CSys, path).spec(Database::Passwd).steps()[0].service, Service::Files);
}

// ------------------------------------------------------------------------------ __nss_hostname_digits_dots

/// One call through the export: (return value, entry name, family, address bytes, status,
/// h_errno, result written, errno). `alloc`: the allocating form, else a caller buffer of
/// `buflen` bytes.
fn digits_dots(name: &str, af: i32, alloc: bool, buflen: usize) -> (i32, String, i32, Vec<u8>, i32, i32, bool, i32) {
    use liella_abi::layout::struct_hostent;
    use liella_boundary::posix::s04_strings as st;
    use liella_boundary::posix::s16_nameservices as ns;
    let c = std::ffi::CString::new(name).unwrap();
    let mut h = struct_hostent { h_name: 0, h_aliases: 0, h_addrtype: -7, h_length: -7, h_addr_list: 0 };
    let mut store = vec![0u64; 64];
    let mut buffer = if alloc { 0 } else { store.as_mut_ptr() as usize };
    let mut size = 0usize;
    let mut result = 1usize;
    let (mut status, mut herr) = (12345, 777);
    liella_boundary::errno::set_raw(0);
    let r = ns::__nss_hostname_digits_dots(c.as_ptr(), Some(&mut h), Some(&mut buffer), if alloc { Some(&mut size) } else { None }, buflen, Some(&mut result), Some(&mut status), af, Some(&mut herr));
    let e = liella_boundary::errno::get();
    let text = |a: usize| {
        let p = a as *const core::ffi::c_char;
        let mut v = vec![0u8; st::strlen(p)];
        st::memcpy(v.as_mut_ptr().cast(), p.cast(), v.len());
        String::from_utf8(v).unwrap()
    };
    let (n, bytes) = if h.h_name != 0 {
        let list = h.h_addr_list as *const usize;
        let mut first = [0usize; 2];
        st::memcpy(first.as_mut_ptr().cast(), list.cast(), 16);
        let mut b = vec![0u8; usize::try_from(h.h_length).unwrap()];
        st::memcpy(b.as_mut_ptr().cast(), (first[0] as *const u8).cast(), b.len());
        assert_eq!(first[1], 0, "one address");
        let mut alias = 0usize;
        st::memcpy((&raw mut alias).cast(), (h.h_aliases as *const usize).cast(), 8);
        assert_eq!(alias, 0, "no aliases");
        (text(h.h_name), b)
    } else {
        (String::new(), Vec::new())
    };
    // An allocated `*buffer` comes from the caller's allocator (the host's in a test build): left
    // to the test process (a few small blocks).
    (r, n, h.h_addrtype, bytes, status, herr, result != 1, e)
}

const AF_INET: i32 = 2;
const AF_INET6: i32 = 10;

#[test]
fn s16_1_c_3() {
    // 1 for a numeric name (dotted IPv4 as inet_aton reads it, or an IPv6 literal), found or
    // not; 0 for any other, with nothing written.
    for n in ["1.2.3.4", "1.2.3", "123", "1.2.3.256", "1.2.3.4.5", "::1", "::ffff:1.2.3.4"] {
        assert_eq!(digits_dots(n, AF_INET, true, 0).0, 1, "{n}");
    }
    for n in ["example.com", "", "1.2.3.4.", "0x7f.1", "1.2.3.a", "fe80::1%1"] {
        let (r, name, ty, _, status, herr, result, e) = digits_dots(n, AF_INET, false, 512);
        assert_eq!((r, name.as_str(), ty, status, herr, result, e), (0, "", -7, 12345, 777, false, 0), "{n}");
    }
    assert_eq!(digits_dots("1.2.3", AF_INET, true, 0).3, [1, 2, 0, 3], "inet_aton's reading");
    assert_eq!(digits_dots("123", AF_INET, true, 0).3, [0, 0, 0, 123]);
}

#[test]
fn s16_1_c_4() {
    // The entry: the name as given, no aliases, one address of af's family; any af but AF_INET6
    // is AF_INET; the other family is not found.
    let (r, name, ty, bytes, ..) = digits_dots("10.20.30.40", AF_INET, true, 0);
    assert_eq!((r, name.as_str(), ty, bytes), (1, "10.20.30.40", AF_INET, vec![10, 20, 30, 40]));
    assert_eq!(digits_dots("1.2.3.4", 99, true, 0).2, AF_INET, "af 99 acts as AF_INET");
    assert_eq!(digits_dots("1.2.3.4", 0, true, 0).2, AF_INET);
    let (_, name, ty, bytes, ..) = digits_dots("::1", AF_INET6, true, 0);
    assert_eq!((name.as_str(), ty, bytes.len(), bytes[15]), ("::1", AF_INET6, 16, 1));
    for (n, af) in [("1.2.3.4", AF_INET6), ("::1", AF_INET), ("1.2.3.256", AF_INET)] {
        let (r, name, _, _, _, herr, ..) = digits_dots(n, af, true, 0);
        assert_eq!((r, name.as_str(), herr), (1, "", 1), "{n} {af}: HOST_NOT_FOUND");
    }
}

#[test]
fn s16_1_c_5() {
    // The allocating form writes *result (resbuf or NULL) and leaves *status; the caller-buffer
    // form writes *status (1, 0) and leaves *result.
    let (_, _, _, _, status, herr, result, _) = digits_dots("1.2.3.4", AF_INET, true, 0);
    assert_eq!((status, herr, result), (12345, 0, true));
    let (_, _, _, _, status, herr, result, _) = digits_dots("1.2.3.256", AF_INET, true, 0);
    assert_eq!((status, herr, result), (12345, 1, true), "NULL written");
    let (_, name, _, _, status, herr, result, _) = digits_dots("1.2.3.4", AF_INET, false, 512);
    assert_eq!((name.as_str(), status, herr, result), ("1.2.3.4", 1, 0, false));
    let (_, _, _, _, status, herr, result, _) = digits_dots("::1", AF_INET, false, 512);
    assert_eq!((status, herr, result), (0, 1, false));
}

#[test]
fn s16_1_c_6() {
    // A caller buffer too small: 1, NSS_STATUS_TRYAGAIN, NETDB_INTERNAL, ERANGE (the reference:
    // 48 bytes fail and 56 suffice for "10.20.30.40", with an aligned buffer); errno untouched
    // otherwise. NULL status and h_errnop are not written (S6).
    for len in [0, 8, 40, 48] {
        let (r, name, _, _, status, herr, result, e) = digits_dots("10.20.30.40", AF_INET, false, len);
        assert_eq!((r, name.as_str(), status, herr, result, e), (1, "", -2, -1, false, Errno::ERANGE.get()), "{len}");
    }
    assert_eq!(digits_dots("10.20.30.40", AF_INET, false, 56).4, 1);
    assert_eq!(digits_dots("10.20.30.40", AF_INET, false, 56).7, 0, "errno untouched");
    use liella_abi::layout::struct_hostent;
    let mut h = struct_hostent { h_name: 0, h_aliases: 0, h_addrtype: 0, h_length: 0, h_addr_list: 0 };
    let mut store = [0u64; 64];
    let mut buffer = store.as_mut_ptr() as usize;
    let c = std::ffi::CString::new("1.2.3.4").unwrap();
    let r = liella_boundary::posix::s16_nameservices::__nss_hostname_digits_dots(c.as_ptr(), Some(&mut h), Some(&mut buffer), None, 512, None, None, AF_INET, None);
    assert_eq!((r, h.h_addrtype), (1, AF_INET), "NULL result, status and h_errnop");
    // Compared with the reference line by line: digits_dots_products.rs.
}

clause!(s16_1_g_3, "16.1", "Guarantees", r##"`nscd`, if running and enabled, answers from its cache over a Unix socket first."##);

// ------------------------------------------------------------------------------ §16.2

#[test]
fn s16_2_c_1() {
    let fx = Fx::new("gai");
    assert_eq!(gai(&fx, Some("192.0.2.50"), Some("80"), hints(0, 0, 1, 0)), Ok(vec!["2/1/6 192.0.2.50 80".into()]));
    assert_eq!(gai(&fx, Some("alpha"), Some("http"), hints(0, 2, 1, 0)), Ok(vec!["2/1/6 192.0.2.1 80".into()]), "a name and a service name");
    assert_eq!(gai(&fx, Some("1.2"), Some("7"), hints(0, 2, 2, 0)), Ok(vec!["2/2/17 1.0.0.2 7".into()]), "inet_aton's forms");
    assert_eq!(gai(&fx, Some("nonesuch"), Some("80"), hints(0, 0, 1, 0)), Err(gai::EAI_NONAME));
    assert_eq!(gai(&fx, None, None, hints(0, 0, 0, 0)), Err(gai::EAI_NONAME));
}

#[test]
fn s16_2_c_2() {
    let fx = Fx::new("hints");
    assert_eq!(gai(&fx, Some("beta"), Some("7"), hints(0, 0, 0, 0)), Ok(vec!["2/1/6 192.0.2.2 7".into(), "2/2/17 192.0.2.2 7".into(), "2/3/0 192.0.2.2 7".into()]), "no type: stream, datagram, raw");
    assert_eq!(gai(&fx, Some("localhost"), Some("http"), hints(0, 0, 0, 0)), Ok(vec!["2/1/6 127.0.0.1 80".into(), "2/2/17 127.0.0.1 80".into()]), "a named service per protocol");
    assert_eq!(gai(&fx, Some("localhost"), Some("sctpx"), hints(0, 2, 0, 0)), Ok(vec!["2/1/132 127.0.0.1 6000".into(), "2/5/132 127.0.0.1 6000".into()]));
    assert_eq!(gai(&fx, Some("localhost"), Some("80"), hints(0, 0, 0, 17)), Ok(vec!["2/2/17 127.0.0.1 80".into()]), "the protocol selects");
    assert_eq!(gai(&fx, Some("localhost"), None, hints(0, 0, 0, 99)), Ok(vec!["2/3/99 127.0.0.1 0".into()]), "an unknown protocol is a raw socket");
    assert_eq!(gai(&fx, Some("localhost"), Some("80"), hints(0, 99, 1, 0)), Err(gai::EAI_FAMILY));
    assert_eq!(gai(&fx, Some("localhost"), Some("80"), hints(0, 0, 99, 0)), Err(gai::EAI_SOCKTYPE));
    assert_eq!(gai(&fx, Some("localhost"), Some("80"), hints(0, 0, 1, 17)), Err(gai::EAI_SOCKTYPE));
    assert_eq!(gai(&fx, Some("localhost"), Some("80"), hints(0, 0, 3, 0)), Err(gai::EAI_SERVICE), "a raw socket named with a service");
    assert_eq!(gai(&fx, Some("localhost"), Some("nonesuch"), hints(0, 0, 1, 0)), Err(gai::EAI_SERVICE));
    assert_eq!(gai(&fx, Some("localhost"), Some("65536"), hints(0, 0, 1, 0)), Err(gai::EAI_SERVICE), "a port is 0-65535");
    assert_eq!(gai(&fx, Some("localhost"), Some(" 80"), hints(0, 0, 1, 0)), Err(gai::EAI_SERVICE), "decimal digits only");
    assert_eq!(gai(&fx, Some("localhost"), Some("80"), hints(0x800, 0, 1, 0)), Err(gai::EAI_BADFLAGS));
    let all = gai::AI_MASK & !gai::AI_CANONNAME & !gai::AI_NUMERICHOST & !gai::AI_ADDRCONFIG;
    assert_eq!(gai(&fx, Some("localhost"), Some("80"), hints(all, 0, 1, 0)).map(|v| v.len()), Ok(1), "every known flag is accepted");
}

#[test]
fn s16_2_c_2_1() {
    let fx = Fx::new("passive");
    assert_eq!(gai(&fx, None, Some("80"), hints(gai::AI_PASSIVE, 0, 1, 0)), Ok(vec!["2/1/6 0.0.0.0 80".into(), "10/1/6 :: 80".into()]), "wildcards for bind, IPv4 first");
    assert_eq!(gai(&fx, None, Some("80"), hints(gai::AI_PASSIVE, 10, 1, 0)), Ok(vec!["10/1/6 :: 80".into()]));
    assert_eq!(gai(&fx, None, Some("80"), hints(0, 2, 1, 0)), Ok(vec!["2/1/6 127.0.0.1 80".into()]), "without AI_PASSIVE, the loopback address");
}

#[test]
fn s16_2_c_2_2() {
    let fx = Fx::new("canon");
    let c = fx.ctx(FILES);
    let f = gai::getaddrinfo(&c, Some(b"a1"), None, Some(hints(gai::AI_CANONNAME, 0, 0, 0))).unwrap();
    assert_eq!(f.canon.as_deref(), Some(&b"alpha"[..]), "the official name");
    assert_eq!(f.entries.len(), 3, "one name for the list (the boundary puts it on the first entry)");
    let f = gai::getaddrinfo(&c, Some(b"192.0.2.9"), None, Some(hints(gai::AI_CANONNAME, 0, 1, 0))).unwrap();
    assert_eq!(f.canon.as_deref(), Some(&b"192.0.2.9"[..]), "a numeric node is its own name");
    assert!(gai::getaddrinfo(&c, Some(b"alpha"), None, Some(hints(0, 0, 1, 0))).unwrap().canon.is_none());
    assert_eq!(gai(&fx, None, Some("80"), hints(gai::AI_CANONNAME, 0, 1, 0)), Err(gai::EAI_BADFLAGS));
}

#[test]
fn s16_2_c_2_3() {
    let fx = Fx::new("numeric");
    assert_eq!(gai(&fx, Some("alpha"), Some("80"), hints(gai::AI_NUMERICHOST, 0, 1, 0)), Err(gai::EAI_NONAME));
    assert_eq!(gai(&fx, Some("127.1"), Some("80"), hints(gai::AI_NUMERICHOST, 0, 1, 0)), Ok(vec!["2/1/6 127.0.0.1 80".into()]));
    assert_eq!(gai(&fx, Some("fe80::1%1"), Some("80"), hints(gai::AI_NUMERICHOST, 10, 1, 0)), Ok(vec!["10/1/6 fe80::1 80".into()]));
    assert_eq!(gai(&fx, Some("2001:db8::1%1"), Some("80"), hints(gai::AI_NUMERICHOST, 10, 1, 0)), Err(gai::EAI_NONAME), "a zone on a global address");
    assert_eq!(gai(&fx, Some("localhost"), Some("http"), hints(gai::AI_NUMERICSERV, 0, 1, 0)), Err(gai::EAI_NONAME));
    assert_eq!(gai(&fx, Some("localhost"), Some("22"), hints(gai::AI_NUMERICSERV, 0, 1, 0)), Ok(vec!["2/1/6 127.0.0.1 22".into()]));
}

#[test]
fn s16_2_c_2_4() {
    let fx = Fx::new("mapped");
    assert_eq!(gai(&fx, Some("alpha"), None, hints(gai::AI_V4MAPPED, 10, 2, 0)), Ok(vec!["10/2/17 ::ffff:192.0.2.1 0".into()]), "no IPv6 address: the mapped IPv4 ones");
    assert_eq!(gai(&fx, Some("gamma"), None, hints(gai::AI_V4MAPPED, 10, 2, 0)), Ok(vec!["10/2/17 2001:db8::10 0".into()]), "IPv6 addresses first and alone");
    assert_eq!(gai(&fx, Some("gamma"), None, hints(gai::AI_V4MAPPED | gai::AI_ALL, 10, 2, 0)), Ok(vec!["10/2/17 2001:db8::10 0".into(), "10/2/17 ::ffff:192.0.2.4 0".into()]), "AI_ALL: both");
    assert_eq!(gai(&fx, Some("alpha"), None, hints(gai::AI_ALL, 10, 2, 0)), Err(gai::EAI_NONAME), "AI_ALL without AI_V4MAPPED");
    assert_eq!(gai(&fx, Some("192.0.2.1"), None, hints(0, 10, 2, 0)), Err(gai::EAI_ADDRFAMILY));
    assert_eq!(gai(&fx, Some("192.0.2.1"), None, hints(gai::AI_V4MAPPED, 10, 2, 0)), Ok(vec!["10/2/17 ::ffff:192.0.2.1 0".into()]));
    assert_eq!(gai(&fx, Some("alpha"), None, hints(gai::AI_V4MAPPED, 2, 2, 0)), Ok(vec!["2/2/17 192.0.2.1 0".into()]), "ignored for AF_INET");
}

#[test]
fn s16_2_c_2_5() {
    let fx = Fx::new("addrconfig");
    // The families configured, as the test reads the interfaces itself: non-loopback, and for
    // IPv6 non-link-local addresses.
    let text = std::fs::read_to_string("/proc/net/if_inet6").unwrap_or_default();
    let v6 = text.lines().any(|l| {
        let a = l.split_whitespace().next().unwrap_or("");
        a != "00000000000000000000000000000001" && !["fe8", "fe9", "fea", "feb"].iter().any(|p| a.starts_with(p))
    });
    let addrs = liella::net::iface::addrs(&CSys).unwrap();
    let v4 = addrs.iter().any(|a| a.family == 2 && a.own().is_some_and(|x| x[0] != 127));
    let (keep4, keep6) = (v4 || !v6, v6 || !v4);
    let n = |node: &str| gai(&fx, Some(node), Some("80"), hints(gai::AI_ADDRCONFIG, 0, 1, 0)).map(|v| v.len());
    assert_eq!(n("127.0.0.1"), if keep4 { Ok(1) } else { Err(gai::EAI_NONAME) }, "IPv4 configured: {v4}, IPv6: {v6}");
    assert_eq!(n("2001:db8::1"), if keep6 { Ok(1) } else { Err(gai::EAI_NONAME) });
}

#[test]
fn s16_2_c_2_6() {
    // AI_IDN: the node, in the locale's character set, through UTS #46 ToASCII (built in,
    // D-16.4); AI_CANONIDN: the canonical name back in Unicode; NI_IDN likewise.
    use liella::names::dns::idna;
    let ascii = |s: &str| {
        let cps: Vec<u32> = s.chars().map(u32::from).collect();
        let mut out = liella::mem::TryVec::new();
        idna::to_ascii(&cps, &mut out).map(|()| String::from_utf8(out.to_vec()).unwrap())
    };
    assert_eq!(ascii("Bücher.test").as_deref(), Ok("xn--bcher-kva.test"), "mapped, then Punycode");
    assert_eq!(ascii("faß.test").as_deref(), Ok("xn--fa-hia.test"), "nontransitional");
    assert_eq!(ascii("a_bé.test").as_deref(), Ok("xn--a_b-dma.test"), "no STD3 rules");
    assert!(ascii("-bücher.test").is_err(), "CheckHyphens");
    assert!(ascii("a\u{200d}b.test").is_err(), "CheckJoiners");
    assert!(ascii("aש.test").is_err(), "CheckBidi");
    assert!(ascii("😀.test").is_err(), "excluded by IDNA2008 (NV8)");
    let srv = dns_server::start(zone());
    let fx = Fx::new("idn");
    let r = Resolver { conf: rconf(srv.port), aliases: None };
    let utf8 = liella::locale::Locale::c_utf8(b"C.UTF-8");
    let ctx = Ctx { dns: Some(&r), locale: Some(&utf8), ..fx.ctx("hosts: dns\nservices: files\n") };
    let h = hints(gai::AI_IDN | gai::AI_CANONNAME | gai::AI_CANONIDN, 2, 1, 0);
    let f = gai::getaddrinfo(&ctx, Some("bücher.test".as_bytes()), None, Some(h)).unwrap();
    assert_eq!(f.entries[0].ip, Ip::V4([192, 0, 2, 50]));
    assert_eq!(f.canon.as_deref(), Some("bücher.test".as_bytes()));
    let f = gai::getaddrinfo(&ctx, Some("bücher.test".as_bytes()), None, Some(hints(gai::AI_IDN | gai::AI_CANONNAME, 2, 1, 0))).unwrap();
    assert_eq!(f.canon.as_deref(), Some(&b"xn--bcher-kva.test"[..]), "without AI_CANONIDN: the ASCII name");
    assert_eq!(gai::getaddrinfo(&ctx, Some("bücher.test".as_bytes()), None, Some(hints(0, 2, 1, 0))).err(), Some(GaiError::Code(gai::EAI_NONAME)), "without AI_IDN the bytes are no host name");
    let ascii_only = Ctx { locale: None, ..ctx };
    assert_eq!(gai::getaddrinfo(&ascii_only, Some("bücher.test".as_bytes()), None, Some(hints(gai::AI_IDN, 2, 1, 0))).err(), Some(GaiError::Code(gai::EAI_IDN_ENCODE)), "not in the locale's character set");
    let mut sa = [0u8; 16];
    sa[..2].copy_from_slice(&2u16.to_ne_bytes());
    sa[4..8].copy_from_slice(&[192, 0, 2, 50]);
    let n = gai::getnameinfo(&ctx, &sa, gai::NI_IDN | gai::NI_NAMEREQD, true, false).unwrap();
    assert_eq!(n.host.as_deref(), Some("bücher.test".as_bytes()));
    let n = gai::getnameinfo(&ascii_only, &sa, gai::NI_IDN | gai::NI_NAMEREQD, true, false).unwrap();
    assert_eq!(n.host.as_deref(), Some(&b"xn--bcher-kva.test"[..]), "kept in ASCII when the locale cannot show it");
}

#[test]
fn s16_2_c_3() {
    // RFC 6724 with the default policy: usable destinations first, then precedence (::1 50,
    // IPv4 35, others 40), stable otherwise.
    let pol = Policy::DEFAULT;
    let a = |t: &str| liella::net::addr::parse_v6(t.as_bytes()).unwrap();
    let c = |d: &str, s: Option<&str>| Candidate { dest: a(d), source: s.map(|s| (a(s), 64)) };
    let mut v = [c("::ffff:127.0.0.1", Some("::ffff:127.0.0.1")), c("::1", Some("::1"))];
    sort::sort(&pol, &mut v, |x| *x);
    assert_eq!(v[0].dest, a("::1"));
    let mut v = [c("2001:db8::1", None), c("::ffff:192.0.2.1", Some("::ffff:192.0.2.9"))];
    sort::sort(&pol, &mut v, |x| *x);
    assert_eq!(v[0].dest, a("::ffff:192.0.2.1"), "rule 1: avoid unusable destinations");
    let mut v = [c("::ffff:192.0.2.12", None), c("::ffff:192.0.2.13", None)];
    sort::sort(&pol, &mut v, |x| *x);
    assert_eq!(v[0].dest, a("::ffff:192.0.2.12"), "rule 10: order kept");
    // gai.conf replaces a table (`gai.conf(5)`).
    let p = Policy::parse(b"precedence ::ffff:0:0/96 100\nprecedence ::/0 40\n");
    let mut v = [c("2001:db8::1", None), c("::ffff:192.0.2.1", None)];
    sort::sort(&p, &mut v, |x| *x);
    assert_eq!(v[0].dest, a("::ffff:192.0.2.1"), "IPv4 preferred by the configured table");
    assert_eq!(Policy::decode(&p.encode()), Some(p));
    // getaddrinfo sorts: the loopback addresses (both routed through `lo`) put ::1 first.
    let fx = Fx::new("sort");
    let r = gai(&fx, None, Some("80"), hints(0, 0, 1, 0)).unwrap();
    assert_eq!(r.len(), 2);
    let routed = |d: &[u8]| liella::net::iface::route(&CSys, d).ok().flatten().is_some();
    if routed(&a("::1")) && routed(&[127, 0, 0, 1]) {
        assert_eq!(r[0], "10/1/6 ::1 80");
    }
}

/// `getnameinfo` over the fixture for an IPv4 or IPv6 socket address.
fn gni(fx: &Fx, addr: &str, port: u16, flags: i32) -> Result<(String, String), i32> {
    let mut sa = vec![0u8; 28];
    match addr.parse::<std::net::IpAddr>().unwrap() {
        std::net::IpAddr::V4(v) => {
            sa.truncate(16);
            sa[..2].copy_from_slice(&2u16.to_ne_bytes());
            sa[4..8].copy_from_slice(&v.octets());
        }
        std::net::IpAddr::V6(v) => {
            sa[..2].copy_from_slice(&10u16.to_ne_bytes());
            sa[8..24].copy_from_slice(&v.octets());
        }
    }
    sa[2..4].copy_from_slice(&port.to_be_bytes());
    match gai::getnameinfo(&fx.ctx(FILES), &sa, flags, true, true) {
        Ok(n) => Ok((String::from_utf8(n.host.unwrap().to_vec()).unwrap(), String::from_utf8(n.serv.unwrap().to_vec()).unwrap())),
        Err(GaiError::Code(c)) => Err(c),
        Err(GaiError::System(_)) => Err(gai::EAI_SYSTEM),
    }
}

#[test]
fn s16_2_c_4() {
    let fx = Fx::new("gni");
    let s = |a: &str, b: &str| Ok((a.to_string(), b.to_string()));
    assert_eq!(gni(&fx, "192.0.2.1", 80, 0), s("alpha", "http"));
    assert_eq!(gni(&fx, "192.0.2.1", 80, gai::NI_NUMERICHOST), s("192.0.2.1", "http"));
    assert_eq!(gni(&fx, "192.0.2.1", 80, gai::NI_NUMERICSERV), s("alpha", "80"));
    assert_eq!(gni(&fx, "192.0.2.99", 22, 0), s("192.0.2.99", "ssh"), "no name: the numeric host");
    assert_eq!(gni(&fx, "192.0.2.99", 22, gai::NI_NAMEREQD), Err(gai::EAI_NONAME));
    assert_eq!(gni(&fx, "192.0.2.1", 5000, 0), s("alpha", "5000"), "a TCP service by default");
    assert_eq!(gni(&fx, "192.0.2.1", 5000, gai::NI_DGRAM), s("alpha", "dgonly"));
    assert_eq!(gni(&fx, "::ffff:192.0.2.1", 80, 0), s("alpha", "http"), "a mapped address is its IPv4 address");
    assert_eq!(gni(&fx, "2001:db8::10", 7, 0), s("gamma", "echo"));
    assert_eq!(gni(&fx, "2001:db8::99", 7, 0), s("2001:db8::99", "echo"));
    assert_eq!(gni(&fx, "192.0.2.1", 80, 0x100), Err(gai::EAI_BADFLAGS));
    // NI_NOFQDN: the host's own domain is cut (the fixture's names are in no local domain
    // unless the host's own name says so).
    let own = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    let want = if own.trim().ends_with(".example.org") { "host" } else { "host.example.org" };
    assert_eq!(gni(&fx, "192.0.2.20", 80, gai::NI_NOFQDN), s(want, "http"));
    let c = fx.ctx(FILES);
    assert_eq!(gai::getnameinfo(&c, &[2, 0, 0, 80], 0, true, true).err(), Some(GaiError::Code(gai::EAI_FAMILY)), "a short address");
    assert!(gai::getnameinfo(&c, &[0u8; 16], 0, false, false).is_err(), "AF_UNSPEC is no Internet address");
}

#[test]
fn s16_2_c_5() {
    // Thread creation is forbidden process-wide: in a child of its own, not in the shared test process.
    if !in_child("s16_2_c_5") {
        return;
    }
    liella_boundary::selftest::forbid_threads();
    // getaddrinfo_a: each request is getaddrinfo's; GAI_WAIT returns when all are done; the
    // status is `__return` (gai_error). The asynchronous paths (workers, notification, waits that
    // time out, cancelling a queued request) need a process Liella started: dns_products,
    // scenario gaia. In this test process no thread can be made (D-22.10): GAI_WAIT then runs
    // the requests in the caller, GAI_NOWAIT is EAI_AGAIN.
    use core::ffi::c_void;
    use liella_abi::layout::{struct_addrinfo, struct_gaicb};
    use liella_boundary::posix::s16_nameservices as x;
    const EAI_ALLDONE: i32 = -103;
    let h = struct_addrinfo { ai_flags: gai::AI_NUMERICSERV, ai_family: 2, ai_socktype: 1, ai_protocol: 0, ai_addrlen: 0, ai_addr: 0, ai_canonname: 0, ai_next: 0 };
    let cb = |name: &'static core::ffi::CStr| struct_gaicb { ar_name: name.as_ptr() as usize, ar_service: c"80".as_ptr() as usize, ar_request: core::ptr::from_ref(&h) as usize, ar_result: 0, __return: 7, __glibc_reserved: [0; 5] };
    let (mut g1, mut g2) = (cb(c"127.0.0.1"), cb(c"not:numeric"));
    let p1: *mut c_void = core::ptr::from_mut(&mut g1).cast();
    let p2: *mut c_void = core::ptr::from_mut(&mut g2).cast();
    let list = [p1, core::ptr::null_mut(), p2];
    assert_eq!(x::getaddrinfo_a(0, list.as_ptr(), 3, None), 0);
    assert_eq!(x::gai_error(p1), 0);
    assert_ne!(g1.ar_result, 0);
    let ai: struct_addrinfo = unsafe_free_copy(g1.ar_result);
    assert_eq!((ai.ai_family, ai.ai_socktype), (2, 1));
    x::freeaddrinfo(g1.ar_result as *mut struct_addrinfo);
    assert_eq!(x::gai_error(p2), gai::EAI_NONAME, "each request has its own result");
    // gai_suspend and gai_cancel on completed requests; lists without requests.
    let nothing = [core::ptr::null::<c_void>(); 2];
    assert_eq!(x::gai_suspend(nothing.as_ptr(), 2, None), EAI_ALLDONE);
    let done = [p1.cast_const()];
    assert_eq!(x::gai_suspend(done.as_ptr(), 1, None), 0, "a completed request ends the wait (getaddrinfo_a(3))");
    assert_eq!(x::gai_cancel(p1), EAI_ALLDONE);
    assert_eq!(x::gai_cancel(core::ptr::null_mut()), EAI_ALLDONE);
    assert_eq!(x::getaddrinfo_a(0, list.as_ptr(), 0, None), 0, "no entries");
    liella_boundary::errno::set_raw(0);
    assert_eq!(x::getaddrinfo_a(7, list.as_ptr(), 3, None), gai::EAI_SYSTEM);
    assert_eq!(liella_boundary::errno::get(), Errno::EINVAL.get());
    // GAI_NOWAIT: asynchronous where threads can be made, else EAI_AGAIN for the list.
    let mut g3 = cb(c"127.0.0.2");
    let p3: *mut c_void = core::ptr::from_mut(&mut g3).cast();
    let one = [p3];
    match x::getaddrinfo_a(1, one.as_ptr(), 1, None) {
        0 => {
            let w = [p3.cast_const()];
            while x::gai_error(p3) == -100 {
                let t = liella_kernel::uapi::Timespec { sec: 1, nsec: 0 };
                x::gai_suspend(w.as_ptr(), 1, Some(&t));
            }
            assert_eq!(x::gai_error(p3), 0);
            x::freeaddrinfo(g3.ar_result as *mut struct_addrinfo);
        }
        r => {
            assert_eq!(r, gai::EAI_AGAIN);
            assert_eq!(x::gai_error(p3), gai::EAI_AGAIN);
        }
    }
}

/// A copy of the `struct addrinfo` at `addr` (a block getaddrinfo made).
fn unsafe_free_copy(addr: usize) -> liella_abi::layout::struct_addrinfo {
    let mut v = liella_abi::layout::struct_addrinfo { ai_flags: 0, ai_family: 0, ai_socktype: 0, ai_protocol: 0, ai_addrlen: 0, ai_addr: 0, ai_canonname: 0, ai_next: 0 };
    liella_boundary::posix::s04_strings::memcpy(core::ptr::from_mut(&mut v).cast(), addr as *const core::ffi::c_void, size_of::<liella_abi::layout::struct_addrinfo>());
    v
}

#[test]
fn s16_2_f_0() {
    let fx = Fx::new("errors");
    let c = fx.ctx(FILES);
    // The codes are returned, not stored in errno.
    liella_boundary::errno::set_raw(0);
    assert_eq!(gai(&fx, Some("nonesuch"), Some("80"), hints(0, 0, 1, 0)), Err(gai::EAI_NONAME));
    assert_eq!(liella_boundary::errno::get(), 0);
    let unavailable = Ctx { config: Config::parse(b"hosts: x\n"), ..c };
    assert_eq!(gai::getaddrinfo(&unavailable, Some(b"alpha"), None, None).err(), Some(GaiError::Code(gai::EAI_FAIL)), "no source available");
    let missing = Paths { hosts: b"/nonexistent/hosts\0", ..fx.paths };
    let gone = Ctx { paths: &missing, ..c };
    assert_eq!(gai::getaddrinfo(&gone, Some(b"alpha"), None, None).err(), Some(GaiError::System(Errno::ENOENT)), "EAI_SYSTEM: the error in errno");
    assert_eq!(gai::of_lookup(LookupError::TryAgain), GaiError::Code(gai::EAI_AGAIN));
    assert_eq!(gai::of_lookup(LookupError::NoData), GaiError::Code(gai::EAI_NODATA));
    assert_eq!(gai::of_lookup(LookupError::Os(Errno::ENOMEM)), GaiError::Code(gai::EAI_MEMORY));
    // gai_strerror: a text per code.
    let codes = [
        (-1, "Bad value for ai_flags"),
        (-2, "Name or service not known"),
        (-3, "Temporary failure in name resolution"),
        (-4, "Non-recoverable failure in name resolution"),
        (-5, "No address associated with hostname"),
        (-6, "ai_family not supported"),
        (-7, "ai_socktype not supported"),
        (-8, "Servname not supported for ai_socktype"),
        (-9, "Address family for hostname not supported"),
        (-10, "Memory allocation failure"),
        (-11, "System error"),
        (-12, "Result too large for supplied buffer"),
        (-100, "Processing request in progress"),
        (-101, "Request canceled"),
        (-102, "Request not canceled"),
        (-103, "All requests done"),
        (-104, "Interrupted by a signal"),
        (-105, "Parameter string not correctly encoded"),
        (0, "Success"),
        (7, "Unknown error"),
    ];
    for (code, text) in codes {
        assert_eq!(msg::gai(code), format!("{text}\0").as_bytes(), "{code}");
    }
}

/// The `struct addrinfo` at `a` (a copy, through the exported `memcpy`).
fn addrinfo_at(a: usize) -> liella_abi::layout::struct_addrinfo {
    let mut v = liella_abi::layout::struct_addrinfo { ai_flags: 0, ai_family: 0, ai_socktype: 0, ai_protocol: 0, ai_addrlen: 0, ai_addr: 0, ai_canonname: 0, ai_next: 0 };
    liella_boundary::posix::s04_strings::memcpy(core::ptr::from_mut(&mut v).cast(), core::ptr::with_exposed_provenance(a), size_of::<liella_abi::layout::struct_addrinfo>());
    v
}

#[test]
fn s16_2_g_0() {
    use liella_boundary::posix::s15_sockets as s;
    use liella_boundary::posix::s16_nameservices as x;
    // A loopback TCP listener the entries connect to.
    let l = s::socket(2, 1, 0);
    let mut sa = [0u8; 16];
    sa[..2].copy_from_slice(&2u16.to_ne_bytes());
    sa[4..8].copy_from_slice(&[127, 0, 0, 1]);
    assert_eq!(s::bind(l, sa.as_ptr().cast(), 16), 0);
    assert_eq!(s::listen(l, 4), 0);
    let mut len = 16u32;
    assert_eq!(s::getsockname(l, sa.as_mut_ptr().cast(), Some(&mut len)), 0);
    let port = std::ffi::CString::new(u16::from_be_bytes([sa[2], sa[3]]).to_string()).unwrap();
    let mut h = liella_abi::layout::struct_addrinfo { ai_flags: gai::AI_NUMERICHOST | gai::AI_NUMERICSERV, ai_family: 2, ai_socktype: 0, ai_protocol: 0, ai_addrlen: 0, ai_addr: 0, ai_canonname: 0, ai_next: 0 };
    let mut res = 0usize;
    assert_eq!(x::getaddrinfo(c"127.0.0.1".as_ptr(), port.as_ptr(), Some(&h), Some(&mut res)), 0);
    // One list: stream, datagram and raw entries, linked; each ready for socket and connect.
    let mut at = res;
    let mut kinds = Vec::new();
    while at != 0 {
        let e = addrinfo_at(at);
        kinds.push(e.ai_socktype);
        if e.ai_socktype == 1 {
            let fd = s::socket(e.ai_family, e.ai_socktype, e.ai_protocol);
            assert!(fd >= 0);
            assert_eq!(s::connect(fd, core::ptr::with_exposed_provenance(e.ai_addr), e.ai_addrlen), 0, "the entry's address connects");
            liella_boundary::posix::s11_lowio::close(fd);
        }
        at = e.ai_next;
    }
    assert_eq!(kinds, [1, 2, 3]);
    x::freeaddrinfo(core::ptr::with_exposed_provenance_mut(res));
    x::freeaddrinfo(core::ptr::null_mut());
    h.ai_flags = 0x800;
    assert_eq!(x::getaddrinfo(c"127.0.0.1".as_ptr(), port.as_ptr(), Some(&h), Some(&mut res)), gai::EAI_BADFLAGS);
    liella_boundary::posix::s11_lowio::close(l);
}

// ------------------------------------------------------------------------------ §16.3

#[test]
fn s16_3_c_0() {
    let fx = Fx::new("hosts");
    let c = fx.ctx(FILES);
    let ok = |s: &str| Ok(s.to_string());
    assert_eq!(host(netdb::host_by_name(&c, b"ALPHA", Family::V4)), ok("alpha [alpha.example.org,a1] 192.0.2.1"), "names match without regard to case");
    assert_eq!(host(netdb::host_by_name(&c, b"beta", Family::V4)), ok("beta [] 192.0.2.2"), "the first line wins");
    assert_eq!(host(netdb::host_by_name(&c, b"gamma", Family::V6)), ok("gamma [gamma6] 2001:db8::10"));
    assert_eq!(host(netdb::host_by_name(&c, b"gamma6", Family::V4)), Err(LookupError::NotFound));
    assert_eq!(host(netdb::host_by_name(&c, b"badaddr", Family::V4)), Err(LookupError::NotFound), "a line whose address does not parse");
    assert_eq!(host(netdb::host_by_name(&c, b"", Family::V4)), Err(LookupError::NotFound), "an empty name");
    assert_eq!(host(netdb::host_by_name(&c, b"0x7f.1", Family::V4)), ok("0x7f.1 [] 127.0.0.1"), "inet_aton's forms, answered without the file");
    assert_eq!(host(netdb::host_by_name(&c, b"::1", Family::V4)), Err(LookupError::NotFound));
    let multi = Ctx { multi: true, ..c };
    assert_eq!(host(netdb::host_by_name(&multi, b"beta", Family::V4)), ok("beta [b3] 192.0.2.2,192.0.2.3"), "multi on: every line");
    assert_eq!(host(netdb::host_by_name(&multi, b"gamma", Family::Any)), ok("gamma [gamma6] 2001:db8::10,192.0.2.4"));
    assert!(liella_legacy::nss::multi_of(b"order hosts,bind\nmulti on\n") && !liella_legacy::nss::multi_of(b"multi off\n"));
    assert_eq!(host(netdb::host_by_addr(&c, Ip::V4([192, 0, 2, 3]))), ok("beta [b3] 192.0.2.3"));
    assert_eq!(host(netdb::host_by_addr(&c, Ip::V6(Ip::V4([192, 0, 2, 1]).to_v6()))), ok("alpha [alpha.example.org,a1] ::ffff:192.0.2.1"), "a mapped address");
    // Networks: right-aligned numbers, found both ways.
    assert_eq!(netdb::net_by_name(&c, b"EXNET", |n| (n.name.to_vec(), n.number)), Ok((b"example".to_vec(), 0xc00002)));
    assert_eq!(netdb::net_by_addr(&c, 0x7f, 2, |n| n.name.to_vec()), Ok(b"loopback".to_vec()));
    assert_eq!(netdb::net_by_addr(&c, 0x7f, 10, |n| n.name.to_vec()), Err(LookupError::NotFound), "AF_INET only");
    assert_eq!(netdb::net_by_name(&c, b"bad", |_| ()), Err(LookupError::NotFound));
    // The enumerations are process-wide: in a child.
    if !in_child("s16_3_c_0") {
        return;
    }
    let mut seen = Vec::new();
    while let Some(l) = netdb::next_host(&c, |l| liella_legacy::cursors::Step::Take(format!("{} {:?}", String::from_utf8_lossy(l.name), l.addr.family()))).unwrap() {
        seen.push(l);
    }
    assert_eq!(seen, ["localhost V4", "localhost V6", "alpha V4", "beta V4", "beta V4", "gamma V6", "gamma V4", " V4", "host.example.org V4"], "every line in its family");
    let mut nets = Vec::new();
    while let Some(n) = netdb::next_net(&c, |n| liella_legacy::cursors::Step::Take(n.number)).unwrap() {
        nets.push(n);
    }
    assert_eq!(nets, [0x7f, 0xc00002]);
}

#[test]
fn s16_3_f_0() {
    use liella_boundary::netdb as b;
    use liella_boundary::posix::s16_nameservices as x;
    assert_eq!(netdb::h_errno_of(LookupError::NotFound), (netdb::HOST_NOT_FOUND, None));
    assert_eq!(netdb::h_errno_of(LookupError::TryAgain), (netdb::TRY_AGAIN, None));
    assert_eq!(netdb::h_errno_of(LookupError::Unavailable), (netdb::NO_RECOVERY, None));
    assert_eq!(netdb::h_errno_of(LookupError::NoData), (netdb::NO_DATA, None));
    assert_eq!(netdb::h_errno_of(LookupError::Os(Errno::ENOENT)), (netdb::NO_RECOVERY, Some(Errno::ENOENT)));
    // h_errno is per thread.
    let here = x::__h_errno_location() as usize;
    b::set_h_errno(netdb::TRY_AGAIN);
    let there = std::thread::spawn(|| {
        b::set_h_errno(netdb::NO_DATA);
        (x::__h_errno_location() as usize, b::h_errno())
    })
    .join()
    .unwrap();
    assert_ne!(there.0, here);
    assert_eq!((there.1, b::h_errno()), (netdb::NO_DATA, netdb::TRY_AGAIN));
    // An argument the call cannot take: NETDB_INTERNAL with errno.
    assert!(x::gethostbyname2(c"localhost".as_ptr(), 99).is_null());
    assert_eq!((b::h_errno(), liella_boundary::errno::get()), (netdb::NETDB_INTERNAL, Errno::EAFNOSUPPORT.get()));
    // The messages.
    let text = |c: i32| String::from_utf8(msg::h_error(c).strip_suffix(b"\0").unwrap().to_vec()).unwrap();
    assert_eq!([1, 2, 3, 4].map(text), ["Unknown host", "Host name lookup failure", "Unknown server error", "No address associated with name"]);
    assert_eq!((text(-1), text(0), text(9)), ("Resolver internal error".into(), "Resolver Error 0 (no error)".into(), "Unknown resolver error".into()));
}

// ------------------------------------------------------------------------------ §16.4 (part 4)
//
// The resolver over a DNS server in the test process (`support/dns_server.rs`, 127.0.0.1 and a
// port of its own); configurations are built from text or by hand, so nothing reads or reaches
// the machine's resolver.

/// A configuration asking the test server at `port` (timeout 1 s, one attempt).
fn rconf(port: u16) -> RConf {
    let mut c = RConf::defaults();
    c.servers.push(RServer { ip: Ip::V4([127, 0, 0, 1]), port, scope: 0 }).unwrap();
    c.timeout = 1;
    c.attempts = 1;
    c
}

fn zone() -> Vec<dns_server::Record> {
    use dns_server::{a, aaaa, cname, ptr};
    vec![
        a("a.test", [192, 0, 2, 10]),
        aaaa("aaaa.test", "2001:db8::20".parse().unwrap()),
        a("both.test", [192, 0, 2, 11]),
        aaaa("both.test", "2001:db8::21".parse().unwrap()),
        cname("cname.test", "a.test"),
        a("multi.test", [192, 0, 2, 12]),
        a("multi.test", [10, 1, 1, 1]),
        a("multi.test", [130, 155, 161, 1]),
        a("x.s2.test", [192, 0, 2, 60]),
        a("ad.test", [192, 0, 2, 33]),
        a("badid.test", [192, 0, 2, 32]),
        a("trunc.test", [192, 0, 2, 30]),
        a("xn--bcher-kva.test", [192, 0, 2, 50]),
        ptr("10.2.0.192.in-addr.arpa", "a.test"),
        ptr("50.2.0.192.in-addr.arpa", "xn--bcher-kva.test"),
    ]
}

/// The A addresses of a response.
fn addrs_of(m: &[u8]) -> Vec<[u8; 4]> {
    let p = dmsg::parse(m).unwrap();
    (0..usize::from(p.header.counts[1])).filter_map(|i| dmsg::rr(m, &p, 1, i).ok()).filter(|r| r.rtype == 1).map(|r| <[u8; 4]>::try_from(r.data(m)).unwrap()).collect()
}

fn query(c: &RConf, name: &str, rtype: i32) -> Result<Vec<u8>, resolv::Fail> {
    let mut ans = [0u8; 4096];
    resolv::nquery(&CSys, c, name.as_bytes(), 1, rtype, &mut ans).map(|n| ans[..n].to_vec())
}

#[test]
fn s16_4_c_0() {
    // A client of the configured servers that returns the raw message.
    let srv = dns_server::start(zone());
    let c = rconf(srv.port);
    let m = query(&c, "a.test", 1).unwrap();
    assert_eq!(addrs_of(&m), [[192, 0, 2, 10]]);
    let h = dmsg::Header::of(&m).unwrap();
    assert!(h.has(dmsg::flag::QR) && h.has(dmsg::flag::AA), "the server's message, flags and all");
    assert_eq!(srv.take_log(), ["udp a.test A ad=0 edns=0"]);
    // The `dns` source answers host lookups from the same client (D-16.1).
    let fx = Fx::new("dnssrc");
    let r = Resolver { conf: rconf(srv.port), aliases: None };
    let ctx = Ctx { dns: Some(&r), ..fx.ctx("hosts: dns\n") };
    assert_eq!(host(netdb::host_lookup(&ctx, b"cname.test", Family::V4)), Ok("a.test [cname.test] 192.0.2.10".into()));
    assert_eq!(host(netdb::host_lookup(&ctx, b"both.test", Family::Any)), Ok("both.test [] 2001:db8::21,192.0.2.11".into()), "AAAA and A, IPv6 first");
    assert_eq!(host(netdb::host_lookup(&ctx, b"aaaa.test", Family::V4)), Err(LookupError::NoData));
    assert_eq!(host(netdb::host_lookup(&ctx, b"nx.test", Family::V4)), Err(LookupError::NotFound));
    assert_eq!(host(netdb::host_lookup(&ctx, b"servfail.test", Family::V4)), Err(LookupError::TryAgain));
    assert_eq!(host(netdb::host_lookup(&ctx, b"b\xc3\xbccher.test", Family::V4)), Err(LookupError::NotFound), "not a host name: not asked");
    assert_eq!(host(netdb::host_lookup_addr(&ctx, Ip::V4([192, 0, 2, 10]))), Ok("a.test [] 192.0.2.10".into()));
    let unreachable = Resolver { conf: rconf(dns_server::closed_port()), aliases: None };
    let ctx = Ctx { dns: Some(&unreachable), ..fx.ctx("hosts: dns\n") };
    assert_eq!(host(netdb::host_lookup(&ctx, b"a.test", Family::V4)), Err(LookupError::Unavailable), "no server reachable: UNAVAIL");
}

#[test]
fn s16_4_c_1() {
    // The configuration: the file, the environment and the defaults.
    let t = b"nameserver 127.0.0.1\nnameserver ::1\ndomain d.test\nsearch a.test b.test\noptions ndots:2 timeout:3 attempts:4 rotate\n";
    let c = RConf::parse(Some(t), None, None, b"host", |_| None).unwrap();
    assert_eq!(c.servers.len(), 2);
    assert_eq!(c.search.iter().map(|d| d.as_slice()).collect::<Vec<_>>(), [b"a.test", b"b.test"], "the last of domain and search wins");
    assert_eq!((c.ndots, c.timeout, c.attempts), (2, 3, 4));
    assert_eq!(c.options, conf::RES_INIT | conf::RES_DEFAULT | conf::RES_ROTATE);
    let c = RConf::parse(Some(t), Some(b"l1.test l2.test"), Some(b"ndots:5 no-aaaa"), b"host", |_| None).unwrap();
    assert_eq!(c.search.len(), 2);
    assert_eq!(c.search[0].as_slice(), b"l1.test", "LOCALDOMAIN replaces the search list");
    assert_eq!(c.ndots, 5, "RES_OPTIONS after the file's options");
    assert_ne!(c.options & conf::RES_NOAAAA, 0);
    let d = RConf::parse(None, None, None, b"box.example.org", |_| None).unwrap();
    assert_eq!(d.servers[0], RServer { ip: Ip::V4([127, 0, 0, 1]), port: 53, scope: 0 }, "no file: the local server");
    assert_eq!((d.ndots, d.timeout, d.attempts, d.options), (1, 5, 2, 0x2c1));
    assert_eq!(d.search[0].as_slice(), b"example.org", "the domain of the host name");
    // The state shows the configuration (`res_ninit`).
    let mut b = [0u8; STATE_SIZE];
    let mut st = State(&mut b);
    let v6 = st.fill(&c, 0x10000, 1, 7);
    assert!(v6[0].is_none() && v6[1].is_some(), "the IPv6 server goes to _u._ext");
    assert_eq!(st.options() & conf::RES_INIT, conf::RES_INIT);
    let ext = [None, v6[1].map(|s| resolv::sockaddr_in6(&s)), None];
    let back = st.config(st.servers(&ext).unwrap(), &[b"l1.test", b"l2.test"]).unwrap();
    assert_eq!((back.servers.len(), back.ndots, back.timeout, back.attempts, back.options), (2, 5, 3, 4, c.options));
}

#[test]
fn s16_4_c_1_1() {
    // At most 3 servers show in the state; the configuration keeps them all (P-lift, D-16.3).
    let t = b"nameserver 127.0.0.1\nnameserver 127.0.0.2\nnameserver 127.0.0.3\nnameserver 127.0.0.4\n";
    let full = RConf::parse(Some(t), None, None, b"h", |_| None).unwrap();
    assert_eq!(full.servers.len(), 4);
    let mut b = [0u8; STATE_SIZE];
    let mut st = State(&mut b);
    st.fill(&full, 0x10000, 1, 0);
    let mut shown = st.config(st.servers(&[None; 3]).unwrap(), &[]).unwrap();
    assert_eq!(shown.servers.len(), 3, "MAXNS");
    resolv::extend(&mut shown, &full).unwrap();
    assert_eq!(shown.servers.len(), 4, "the file's fourth server is used behind the first three");
    let mut other = RConf::parse(Some(b"nameserver 10.0.0.1\nnameserver 10.0.0.2\nnameserver 10.0.0.3\n"), None, None, b"h", |_| None).unwrap();
    resolv::extend(&mut other, &full).unwrap();
    assert_eq!(other.servers.len(), 3, "a state with other servers is not extended");
}

#[test]
fn s16_4_c_1_2() {
    // At most 6 search domains show in the state; the configuration keeps them all.
    let t = b"search s1.test s2.test s3.test s4.test s5.test s6.test s7.test\n";
    let full = RConf::parse(Some(t), None, None, b"h", |_| None).unwrap();
    assert_eq!(full.search.len(), 7);
    let mut b = [0u8; STATE_SIZE];
    let mut st = State(&mut b);
    st.fill(&full, 0x10000, 1, 0);
    assert_eq!(st.search_addrs().iter().filter(|&&a| a != 0).count(), 6, "MAXDNSRCH");
    let names: Vec<&[u8]> = full.search.iter().take(6).map(|d| d.as_slice()).collect();
    let mut shown = st.config(st.servers(&[None; 3]).unwrap(), &names).unwrap();
    resolv::extend(&mut shown, &full).unwrap();
    assert_eq!(shown.search.len(), 7);
}

#[test]
fn s16_4_c_1_3() {
    let c = RConf::parse(Some(b"options ndots:20 timeout:99 attempts:9 edns0 single-request single-request-reopen use-vc no-aaaa trust-ad no-reload inet6 debug bogus\n"), None, None, b"h", |_| None).unwrap();
    assert_eq!((c.ndots, c.timeout, c.attempts), (15, 30, 5), "capped as resolv.conf(5) says");
    let want = conf::RES_INIT | conf::RES_DEFAULT | conf::RES_USE_EDNS0 | conf::RES_SNGLKUP | conf::RES_SNGLKUPREOP | conf::RES_USEVC | conf::RES_NOAAAA | conf::RES_TRUSTAD | conf::RES_NORELOAD;
    assert_eq!(c.options, want, "inet6 (obsolete), debug and unknown options set nothing");
    let c = RConf::parse(Some(b"options rotate\noptions no-tld-query timeout:0\n"), None, None, b"h", |_| None).unwrap();
    assert_eq!(c.options & (conf::RES_ROTATE | conf::RES_NOTLDQUERY), conf::RES_ROTATE | conf::RES_NOTLDQUERY, "options lines add up");
    assert_eq!(c.timeout, 0);
    // EDNS: an OPT record with the 1200-byte payload; use-vc: TCP; no-aaaa: AAAA asked as A.
    let srv = dns_server::start(zone());
    let mut c = rconf(srv.port);
    c.options |= conf::RES_USE_EDNS0;
    assert!(query(&c, "a.test", 1).is_ok());
    c.options = conf::RES_INIT | conf::RES_DEFAULT | conf::RES_USEVC;
    assert!(query(&c, "a.test", 1).is_ok());
    c.options = conf::RES_INIT | conf::RES_DEFAULT | conf::RES_NOAAAA;
    assert_eq!(query(&c, "aaaa.test", 28).err().map(|f| f.h), Some(netdb::NO_DATA));
    assert_eq!(srv.take_log(), ["udp a.test A ad=0 edns=1", "tcp a.test A ad=0 edns=0", "udp aaaa.test A ad=0 edns=0"]);
}

#[test]
fn s16_4_c_1_4() {
    // The sort list orders the IPv4 addresses of host lookups (resolv.conf(5)): the first
    // matching pair's position, the unmatched last in their order.
    let c = RConf::parse(Some(b"sortlist 130.155.160.0/255.255.240.0 10.0.0.0 192.168.0.0/8\n"), None, None, b"h", |_| None).unwrap();
    assert_eq!(c.sort[0], ([130, 155, 160, 0], [255, 255, 240, 0]));
    assert_eq!(c.sort[1], ([10, 0, 0, 0], [255, 0, 0, 0]), "the class mask by default");
    assert_eq!(c.sort[2].1, [0, 0, 0, 8], "the mask as inet_aton reads it (observed)");
    let srv = dns_server::start(zone());
    let mut conf = rconf(srv.port);
    conf.sort = c.sort;
    let fx = Fx::new("sortlist");
    let r = Resolver { conf, aliases: None };
    let ctx = Ctx { dns: Some(&r), ..fx.ctx("hosts: dns\n") };
    assert_eq!(host(netdb::host_lookup(&ctx, b"multi.test", Family::V4)), Ok("multi.test [] 130.155.161.1,10.1.1.1,192.0.2.12".into()));
}

#[test]
fn s16_4_c_2() {
    let srv = dns_server::start(zone());
    let mut c = rconf(srv.port);
    // res_nsearch: names with fewer than ndots dots go through the search list first.
    for d in ["s1.test", "s2.test"] {
        c.search.push(liella::mem::TryVec::from_slice(d.as_bytes()).unwrap()).unwrap();
    }
    let mut ans = [0u8; 512];
    let n = resolv::nsearch(&CSys, &c, b"x", 1, 1, &mut ans, None).unwrap();
    assert_eq!(addrs_of(&ans[..n]), [[192, 0, 2, 60]]);
    assert_eq!(srv.take_log(), ["udp x.s1.test A ad=0 edns=0", "udp x.s2.test A ad=0 edns=0"]);
    assert_eq!(resolv::nsearch(&CSys, &c, b"a.test.", 1, 1, &mut ans, None).map(|_| ()), Ok(()));
    assert_eq!(srv.take_log(), ["udp a.test A ad=0 edns=0"], "a final dot: the name alone");
    assert!(resolv::nsearch(&CSys, &c, b"y.z", 1, 1, &mut ans, None).is_err());
    assert_eq!(srv.take_log(), ["udp y.z A ad=0 edns=0", "udp y.z.s1.test A ad=0 edns=0", "udp y.z.s2.test A ad=0 edns=0"], "ndots reached: the name first");
    // HOSTALIASES: a name without a dot is replaced by its alias, without the search list.
    assert!(resolv::nsearch(&CSys, &c, b"Alias", 1, 1, &mut ans, Some(b"alias a.test\n")).is_ok());
    assert_eq!(srv.take_log(), ["udp a.test A ad=0 edns=0"]);
    // res_nquerydomain joins the names.
    assert!(resolv::nquerydomain(&CSys, &c, b"x", Some(b"s2.test"), 1, 1, &mut ans).is_ok());
    assert!(resolv::nquerydomain(&CSys, &c, b"x.", Some(b"s2.test"), 1, 1, &mut ans).is_err(), "x..s2.test is no name");
    // res_nsend sends a prebuilt message and returns the response.
    let mut q = [0u8; 512];
    let ql = resolv::mkquery(&CSys, c.options, 0, b"a.test", 1, 1, None, &mut q).unwrap();
    let n = resolv::nsend(&CSys, &c, &q[..ql], &mut ans).unwrap();
    assert_eq!(ans[..2], q[..2], "the query's ID");
    assert_eq!(addrs_of(&ans[..n]), [[192, 0, 2, 10]]);
    // A response larger than the buffer is cut to it (observed).
    assert_eq!(resolv::nsend(&CSys, &c, &q[..ql], &mut ans[..20]), Ok(20));
}

#[test]
fn s16_4_c_3() {
    // A query: RD and AD, one question (observed).
    let mut q = [0u8; 64];
    let n = dmsg::mkquery(0, b"www.example.com", 1, 1, None, &mut q, 0x1234, true).unwrap();
    assert_eq!(&q[..n], b"\x12\x34\x01\x20\x00\x01\x00\x00\x00\x00\x00\x00\x03www\x07example\x03com\x00\x00\x01\x00\x01");
    assert!(dmsg::mkquery(1, b"a", 1, 1, None, &mut q, 0, true).is_err(), "IQUERY is no longer built");
    // Compression and expansion.
    let mut m = [0u8; 128];
    let w = |t: &[u8]| {
        let mut b = [0u8; 256];
        let (n, _) = name::pton(t, &mut b).unwrap();
        b[..n].to_vec()
    };
    let mut names = [0usize; 8];
    let mut cp = name::Compress { names: &mut names, len: 0, record: true };
    let a = name::pack(&w(b"www.example.com"), &mut m, 12, 100, Some(&mut cp)).unwrap();
    let b = name::pack(&w(b"mail.example.com"), &mut m, 12 + a, 100, Some(&mut cp)).unwrap();
    assert_eq!(&m[12 + a..12 + a + b], b"\x04mail\xc0\x10");
    let mut out = [0u8; 256];
    let (len, used) = name::unpack(&m, 12 + a, &mut out).unwrap();
    assert_eq!(used, 7);
    let mut text = [0u8; 256];
    let t = name::ntop(&out[..len], &mut text).unwrap();
    assert_eq!(&text[..t], b"mail.example.com\0");
    // Records, parsed and printed.
    let srv = dns_server::start(zone());
    let resp = query(&rconf(srv.port), "cname.test", 1).unwrap();
    let p = dmsg::parse(&resp).unwrap();
    let r = dmsg::rr(&resp, &p, 1, 0).unwrap();
    let mut owner = [0u8; 1025];
    let k = name::expand_text(r.owner(), &mut owner).unwrap();
    let rec = text::Record { owner: &owner[..k - 1], class: r.class, rtype: r.rtype, ttl: u64::from(r.ttl), rdata: r.data(&resp), rdata_at: r.rdata };
    let mut out = liella::mem::TryVec::new();
    text::sprintrr(&resp, &rec, None, None, &mut out).unwrap();
    assert_eq!(out.as_slice(), b"cname.test.\t\t5M IN CNAME\ta.test.");
    assert_eq!(dmsg::rr(&resp, &p, 1, 9).err(), Some(Errno::ENODEV), "an index out of range");
    assert_eq!(dmsg::parse(&resp[..resp.len() - 1]).err(), Some(Errno::EMSGSIZE), "a message cut short");
}

#[test]
fn s16_4_c_4() {
    use name::Check::{Domain, Host, Mail, Owner};
    let chk = |t: &str| [Host, Owner, Mail, Domain].map(|c| u8::from(name::check(t.as_bytes(), c)));
    assert_eq!(chk("ex-ample.com"), [1, 1, 1, 1]);
    assert_eq!(chk("-example.com"), [0, 0, 1, 1], "a leading hyphen");
    assert_eq!(chk("ex_ample.com"), [1, 1, 1, 1], "underscores are accepted (observed)");
    assert_eq!(chk("*.example.com"), [0, 1, 1, 1], "a wildcard owner");
    assert_eq!(chk("ex ample.com"), [0, 0, 0, 0]);
    assert_eq!(chk("a..b"), [0, 0, 0, 0], "an empty label");
    assert_eq!(chk("foo@bar.example"), [0, 0, 1, 1], "a mail name's local part");
    assert_eq!(chk("a"), [1, 1, 0, 1], "a mail name needs a host part");
    assert_eq!(chk("a/b"), [0, 0, 0, 1]);
    assert_eq!(chk(""), [1, 1, 1, 1]);
}

#[test]
fn s16_4_g_1() {
    // Servers in order: one that cannot be reached is passed over at once, one that does not
    // answer after its timeout, until one answers.
    let srv = dns_server::start(zone());
    let mut c = rconf(srv.port);
    c.servers.clear();
    for port in [dns_server::closed_port(), dns_server::silent_port(), srv.port] {
        c.servers.push(RServer { ip: Ip::V4([127, 0, 0, 1]), port, scope: 0 }).unwrap();
    }
    let t0 = std::time::Instant::now();
    assert_eq!(query(&c, "a.test", 1).map(|m| addrs_of(&m)), Ok(vec![[192, 0, 2, 10]]));
    let dt = t0.elapsed().as_secs_f64();
    assert!((0.9..2.5).contains(&dt), "one timeout: {dt}");
    // No answer at all: every attempt waits its timeout, then TRY_AGAIN with ETIMEDOUT.
    let mut c = rconf(dns_server::silent_port());
    c.attempts = 2;
    let t0 = std::time::Instant::now();
    assert_eq!(query(&c, "a.test", 1).err(), Some(resolv::Fail { h: netdb::TRY_AGAIN, errno: Some(Errno::ETIMEDOUT) }));
    let dt = t0.elapsed().as_secs_f64();
    assert!((1.9..3.5).contains(&dt), "two attempts: {dt}");
    // Rotation spreads the queries over the servers.
    let s2 = dns_server::start(zone());
    let mut c = rconf(srv.port);
    c.servers.push(RServer { ip: Ip::V4([127, 0, 0, 1]), port: s2.port, scope: 0 }).unwrap();
    c.options |= conf::RES_ROTATE;
    srv.take_log();
    for _ in 0..4 {
        query(&c, "a.test", 1).unwrap();
    }
    assert_eq!((srv.take_log().len(), s2.take_log().len()), (2, 2));
}

#[test]
fn s16_4_g_2() {
    let srv = dns_server::start(zone());
    let c = rconf(srv.port);
    // A response with another ID is ignored; the right one is used.
    assert_eq!(query(&c, "badid.test", 1).map(|m| addrs_of(&m)), Ok(vec![[192, 0, 2, 32]]));
    // A response to another question is no answer, over UDP and TCP (D-16.3).
    assert_eq!(query(&c, "wrongq.test", 1).err().map(|f| f.h), Some(netdb::TRY_AGAIN));
    let mut vc = rconf(srv.port);
    vc.options |= conf::RES_USEVC;
    assert_eq!(query(&vc, "wrongq.test", 1).err().map(|f| f.h), Some(netdb::TRY_AGAIN));
    // The rules themselves.
    let mut q = [0u8; 64];
    let n = dmsg::mkquery(0, b"a.test", 1, 1, None, &mut q, 7, true).unwrap();
    let mut r = q[..n].to_vec();
    r[2] |= 0x80;
    assert!(client::matches(&q[..n], &r));
    r[1] ^= 1;
    assert!(!client::matches(&q[..n], &r), "another ID");
    r[1] ^= 1;
    r[2] &= 0x7f;
    assert!(!client::matches(&q[..n], &r), "not a response");
    r[2] |= 0x80;
    r[13] = b'b';
    assert!(!client::matches(&q[..n], &r), "another question");
    // A truncated response is asked again over TCP.
    assert_eq!(query(&c, "trunc.test", 1).map(|m| addrs_of(&m)), Ok(vec![[192, 0, 2, 30]]));
    assert!(srv.take_log().ends_with(&["udp trunc.test A ad=0 edns=0".to_string(), "tcp trunc.test A ad=0 edns=0".to_string()]));
}

#[test]
fn s16_4_g_3() {
    let srv = dns_server::start(zone());
    let mut c = rconf(srv.port);
    let ad = |m: &[u8]| m[3] & 0x20 != 0;
    assert!(!ad(&query(&c, "ad.test", 1).unwrap()), "cleared without trust-ad");
    c.options |= conf::RES_TRUSTAD;
    assert!(ad(&query(&c, "ad.test", 1).unwrap()), "kept with trust-ad");
    assert!(!ad(&query(&c, "a.test", 1).unwrap()), "kept, not set");
    assert_eq!(srv.take_log(), ["udp ad.test A ad=0 edns=0", "udp ad.test A ad=1 edns=0", "udp a.test A ad=1 edns=0"], "trust-ad asks for it in the query");
}

#[test]
fn s16_4_g_4() {
    // The file's generation changes when the file does (identity: device, inode, size, times).
    let fx = Fx::new("resolvconf");
    let path = fx.dir.join("resolv.conf");
    std::fs::write(&path, "nameserver 127.0.0.1\n").unwrap();
    let p = z(&path);
    let f = resolv::ConfFile::new();
    let g1 = f.generation(&CSys, p);
    assert_eq!(f.generation(&CSys, p), g1, "unchanged");
    std::fs::write(fx.dir.join("new"), "nameserver 127.0.0.3\noptions no-reload\n").unwrap();
    std::fs::rename(fx.dir.join("new"), &path).unwrap();
    let g2 = f.generation(&CSys, p);
    assert_ne!(g2, g1, "a new file");
    std::fs::remove_file(&path).unwrap();
    assert_ne!(f.generation(&CSys, p), g2, "a removed file");
    // A state records its generation; `no-reload` is an option it carries (the thread's `_res`
    // is read again when its generation is old, unless it has the option: liella_boundary::resolv;
    // end to end in dns_products, scenarios reload and noreload).
    let c = RConf::parse(Some(b"options no-reload\n"), None, None, b"h", |_| None).unwrap();
    let mut b = [0u8; STATE_SIZE];
    let mut st = State(&mut b);
    st.fill(&c, 0x1000, g2, 0);
    assert_eq!(st.generation(), Some(g2));
    assert_ne!(st.options() & conf::RES_NORELOAD, 0);
}

#[test]
fn s16_4_f_0() {
    let srv = dns_server::start(zone());
    let c = rconf(srv.port);
    let h = |name: &str| query(&c, name, 1).err().map(|f| f.h);
    assert_eq!(h("nx.test"), Some(netdb::HOST_NOT_FOUND));
    assert_eq!(h("servfail.test"), Some(netdb::TRY_AGAIN));
    assert_eq!(h("refused.test"), Some(netdb::TRY_AGAIN));
    assert_eq!(h("formerr.test"), Some(netdb::NO_RECOVERY));
    assert_eq!(h("nodata.test"), Some(netdb::NO_DATA));
    assert_eq!(h("a.test"), None);
    // The failure's response is still copied for the caller.
    let mut ans = [0u8; 512];
    assert!(resolv::nquery(&CSys, &c, b"nx.test", 1, 1, &mut ans).is_err());
    assert_eq!(ans[3] & 0x0f, 3);
}

// ------------------------------------------------------------------------------ §16.5, §16.6

#[test]
fn s16_5_c_0() {
    let fx = Fx::new("svc");
    let c = fx.ctx(FILES);
    assert_eq!(netdb::serv_by_name(&c, b"www", Some(b"tcp"), |s| (s.name.to_vec(), s.port)), Ok((b"http".to_vec(), 80)), "by alias");
    assert_eq!(netdb::serv_by_name(&c, b"echo", None, |s| s.proto.to_vec()), Ok(b"tcp".to_vec()), "the first entry of the name");
    assert_eq!(netdb::serv_by_name(&c, b"ECHO", Some(b"tcp"), |_| ()), Err(LookupError::NotFound), "service names are case-sensitive");
    assert_eq!(netdb::serv_by_name(&c, b"badport", None, |_| ()), Err(LookupError::NotFound), "a port beyond 65535 is no entry");
    assert_eq!(netdb::serv_by_port(&c, 5000, None, |s| s.proto.to_vec()), Ok(b"udp".to_vec()));
    assert_eq!(liella_boundary::netdb::port_of(i32::from(80u16.to_be())), 80, "getservbyport's network-order port");
    assert_eq!(netdb::proto_by_name(&c, b"Udp", |p| p.number), Ok(17), "protocol names ignore case");
    assert_eq!(netdb::proto_by_number(&c, 132, |p| p.name.to_vec()), Ok(b"sctp".to_vec()));
    assert_eq!(netdb::rpc_by_name(&c, b"sunrpc", |p| p.number), Ok(100000));
    assert_eq!(netdb::rpc_by_name(&c, b"PORTMAP", |p| p.number), Err(LookupError::NotFound));
    assert_eq!(netdb::rpc_by_number(&c, 100003, |p| p.name.to_vec()), Ok(b"nfs".to_vec()));
    // Netgroups: triples, nested groups, loops.
    use liella_legacy::nss::{in_netgroup as inn, netgroup};
    assert!(inn(&c, b"trusted", Some(b"alpha"), Some(b"alice"), Some(b"EXAMPLE.org")));
    assert!(inn(&c, b"trusted", Some(b"BETA"), None, None), "hosts ignore case");
    assert!(!inn(&c, b"trusted", Some(b"beta"), Some(b"x"), None), "`-` matches no value");
    assert!(inn(&c, b"trusted", Some(b"gamma"), Some(b"bob"), None), "nested groups");
    assert!(!inn(&c, b"trusted", None, Some(b"BOB"), None), "users are case-sensitive");
    assert!(!inn(&c, b"empty", None, None, None));
    assert!(!inn(&c, b"loop1", Some(b"x"), None, None), "a loop ends");
    let t = netgroup(&c, b"trusted").unwrap();
    assert_eq!(t.len(), 3);
    assert_eq!(t.get(1).unwrap(), [Some(&b"beta"[..]), Some(&b"-"[..]), None]);
    assert_eq!(netgroup(&c, b"nonesuch").err(), Some(LookupError::NotFound));
    // The enumeration (a cursor of its own).
    let cur = liella_legacy::nss::NetgroupCursor::new();
    assert!(cur.set(&c, b"trusted") && !cur.set(&c, b"nonesuch") && cur.set(&c, b"trusted"));
    let mut seen = Vec::new();
    while let Some(Some(h)) = cur.next(&c, |t| Some(t[0].map(<[u8]>::to_vec))).unwrap() {
        seen.push(h);
    }
    assert_eq!(seen, [Some(b"alpha".to_vec()), Some(b"beta".to_vec()), Some(b"gamma".to_vec())]);
    assert!(cur.set(&c, b"empty") && cur.next(&c, |_| Some(())).unwrap().is_none(), "a known empty group");
}

/// The `/etc/rpc` malformed-line corpus; every expectation below was probed on the reference
/// (glibc 2.39, the corpus bind-mounted over /etc/rpc). The last line has no newline.
fn rpc_corpus() -> String {
    let long: String = (0..1500).map(|i| format!(" a{i}")).collect();
    [
        "# full-line comment\n",
        "portmapper\t100000\tportmap sunrpc rpcbind\n",
        "nfs 100003 nfsprog # trailing comment\n",
        "ypserv 100004 ypprog#glued\n",
        "mid#word 100005\n",
        "nonum\n",
        "nonum2 # 100006\n",
        "neg -5\nhex 0x67\nbig 99999999999\n",
        "max 2147483647\nover 2147483648\nlead 0100007\n",
        "   indented 100008 ind\n",
        "\t\ttabbed\t100009\t\talias1\t alias2  \n",
        "dup 100010 a1\ndup 100011 a2\nother 100010 a3\n",
        "\n   \n",
        "plus +100012\ntrail 100013x\n",
        "cr 100014 crali\r\nvt 100015\x0bvtali\nff\x0c100019 ffali\n",
        &format!("long 100016{long}\n"),
        "alias 100017 portmap\nzero 0\n#100020 commented 100020\n",
        "last 100018 lastali",
    ]
    .concat()
}

/// §16.5: `getrpcent`, `getrpcbyname`, `getrpcbynumber` over the malformed-line corpus (no spec
/// clause: the line grammar is settled by the probe; the lookups themselves are s16_5_c_0's).
#[test]
fn rpc_malformed_line_corpus() {
    // Trace: no spec clause (the /etc/rpc line grammar is probe-settled; lookups are s16_5_c_0); LG-16.
    use liella_legacy::cursors::{Step, RPC_CURSOR};
    let fx = Fx::new("rpc");
    std::fs::write(fx.dir.join("rpc"), rpc_corpus()).unwrap();
    let c = fx.ctx(FILES);
    type Ent = (String, i32, Vec<String>);
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    let ent = |p: &liella::names::db::Numbered<'_, i32>| -> Ent { (s(p.name), p.number, p.aliases.clone().map(s).collect()) };
    RPC_CURSOR.restart(&CSys, fx.paths.rpc).unwrap();
    let mut all: Vec<Ent> = Vec::new();
    while let Some(e) = netdb::next_rpc(&c, |p| Step::Take(ent(p))).unwrap() {
        all.push(e);
    }
    RPC_CURSOR.end(&CSys);
    let want: [(&str, i32, usize); 19] = [
        ("portmapper", 100000, 3), ("nfs", 100003, 1), ("ypserv", 100004, 1), ("max", i32::MAX, 0), ("over", i32::MIN, 0),
        ("lead", 100007, 0), ("indented", 100008, 1), ("tabbed", 100009, 2), ("dup", 100010, 1), ("dup", 100011, 1),
        ("other", 100010, 1), ("plus", 100012, 0), ("cr", 100014, 1), ("vt", 100015, 1), ("ff", 100019, 1),
        ("long", 100016, 1500), ("alias", 100017, 1), ("zero", 0, 0), ("last", 100018, 1),
    ];
    let got: Vec<(&str, i32, usize)> = all.iter().map(|e| (e.0.as_str(), e.1, e.2.len())).collect();
    assert_eq!(got, want, "getrpcent order, names, numbers and alias counts");
    let aliases = |n: &str| all.iter().find(|e| e.0 == n).map(|e| e.2.clone()).unwrap();
    assert_eq!(aliases("portmapper"), ["portmap", "sunrpc", "rpcbind"], "aliases are split");
    assert_eq!(aliases("nfs"), ["nfsprog"], "a trailing comment is stripped");
    assert_eq!(aliases("ypserv"), ["ypprog"], "a comment glued to a word is stripped");
    assert_eq!(aliases("tabbed"), ["alias1", "alias2"], "tabs and spaces both separate");
    assert_eq!((aliases("cr"), aliases("vt"), aliases("ff")), (vec!["crali".to_string()], vec!["vtali".to_string()], vec!["ffali".to_string()]));
    let l = aliases("long");
    assert_eq!((l.first().map(String::as_str), l.last().map(String::as_str)), (Some("a0"), Some("a1499")), "the over-long line");
    assert_eq!(aliases("last"), ["lastali"], "the last line without a newline");
    // By name: official names and aliases, case-sensitive, first match wins.
    let by_name = |n: &[u8]| netdb::rpc_by_name(&c, n, |p| (s(p.name), p.number)).ok();
    for (n, want) in [
        (&b"rpcbind"[..], Some(("portmapper", 100000))), (b"PORTMAPPER", None), (b"ypprog", Some(("ypserv", 100004))),
        (b"ypprog#glued", None), (b"mid", None), (b"nonum", None), (b"nonum2", None), (b"neg", None), (b"hex", None),
        (b"big", None), (b"trail", None), (b"over", Some(("over", i32::MIN))), (b"ind", Some(("indented", 100008))),
        (b"alias2", Some(("tabbed", 100009))), (b"dup", Some(("dup", 100010))), (b"a2", Some(("dup", 100011))),
        (b"crali", Some(("cr", 100014))), (b"crali\r", None), (b"vtali", Some(("vt", 100015))), (b"a1499", Some(("long", 100016))),
        (b"portmap", Some(("portmapper", 100000))), (b"lastali", Some(("last", 100018))), (b"", None), (b"commented", None),
    ] {
        assert_eq!(by_name(n), want.map(|(a, b)| (a.to_string(), b)), "getrpcbyname({})", String::from_utf8_lossy(n));
    }
    let by_number = |k: i32| netdb::rpc_by_number(&c, k, |p| s(p.name)).ok();
    for (k, want) in [
        (100005, None), (100006, None), (100007, Some("lead")), (100010, Some("dup")), (100011, Some("dup")), (100012, Some("plus")),
        (100013, None), (100016, Some("long")), (100018, Some("last")), (100020, None), (0, Some("zero")), (-5, None), (103, None),
        (i32::MAX, Some("max")), (i32::MIN, Some("over")),
    ] {
        assert_eq!(by_number(k), want.map(str::to_string), "getrpcbynumber({k})");
    }
    // Run here, not as its own test: RPC_CURSOR is process-wide, and two tests iterating it at
    // once would interleave.
    rpc_last_line_without_newline();
}

/// Files whose last entry has no trailing newline (no spec clause: probe-settled on glibc 2.39,
/// shiki, a bind mount over /etc/rpc in an unprivileged namespace). Each case: the file, the
/// getrpcent sequence, then (name or alias, number) lookups that must find the last entry, and
/// names/numbers that must find nothing.
fn rpc_last_line_without_newline() {
    // Trace: no spec clause (the /etc/rpc line grammar is probe-settled; lookups are s16_5_c_0); LG-16.
    use liella_legacy::cursors::{Step, RPC_CURSOR};
    type Case<'a> = (&'a str, &'a [(&'a str, i32, &'a [&'a str])], &'a [&'a str], &'a [i32]);
    let cases: [Case<'_>; 8] = [
        ("first 100100 f1\nlast 100101 l1 l2", &[("first", 100100, &["f1"]), ("last", 100101, &["l1", "l2"])], &[], &[]),
        ("only 100102 o1", &[("only", 100102, &["o1"])], &[], &[]),
        ("x 100103 xa\nlastc 100104 lc # tail", &[("x", 100103, &["xa"]), ("lastc", 100104, &["lc"])], &["tail", "#"], &[]),
        ("y 100105 ya\nlastw 100106 lw \t", &[("y", 100105, &["ya"]), ("lastw", 100106, &["lw"])], &[""], &[]),
        ("z 100107 za\nlastn 100108", &[("z", 100107, &["za"]), ("lastn", 100108, &[])], &[], &[]),
        ("w 100109 wa\nlastbad 100110x lb", &[("w", 100109, &["wa"])], &["lastbad", "lb"], &[100110]),
        ("v 100111 va\n# trailing comment", &[("v", 100111, &["va"])], &["trailing", "comment"], &[]),
        ("u 100112 ua\nlastcr 100113 lcr\r", &[("u", 100112, &["ua"]), ("lastcr", 100113, &["lcr"])], &["lcr\r"], &[]),
    ];
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    for (text, want, absent_names, absent_numbers) in cases {
        let fx = Fx::new("rpc-nonl");
        std::fs::write(fx.dir.join("rpc"), text).unwrap();
        let c = fx.ctx(FILES);
        RPC_CURSOR.restart(&CSys, fx.paths.rpc).unwrap();
        let mut got: Vec<(String, i32, Vec<String>)> = Vec::new();
        while let Some(e) = netdb::next_rpc(&c, |p| Step::Take((s(p.name), p.number, p.aliases.clone().map(s).collect()))).unwrap() {
            got.push(e);
        }
        RPC_CURSOR.end(&CSys);
        let want_ents: Vec<(String, i32, Vec<String>)> =
            want.iter().map(|(n, k, a)| (n.to_string(), *k, a.iter().map(|x| x.to_string()).collect())).collect();
        assert_eq!(got, want_ents, "getrpcent over {text:?}: the last line without a newline is an entry like any other");
        let by_name = |n: &str| netdb::rpc_by_name(&c, n.as_bytes(), |p| (s(p.name), p.number)).ok();
        let by_number = |k: i32| netdb::rpc_by_number(&c, k, |p| s(p.name)).ok();
        for (name, number, aliases) in want {
            for n in std::iter::once(name).chain(aliases.iter()) {
                assert_eq!(by_name(n), Some((name.to_string(), *number)), "getrpcbyname({n:?}) over {text:?}: last line without a newline");
            }
            assert_eq!(by_number(*number), Some(name.to_string()), "getrpcbynumber({number}) over {text:?}: last line without a newline");
        }
        for n in absent_names {
            assert_eq!(by_name(n), None, "getrpcbyname({n:?}) over {text:?} finds nothing");
        }
        for k in absent_numbers {
            assert_eq!(by_number(*k), None, "getrpcbynumber({k}) over {text:?} finds nothing");
        }
    }
}

#[test]
fn s16_5_f_0() {
    use liella_boundary::posix::s16_nameservices as x;
    // Not found: NULL (errno unchanged), `_r`: 0 with a NULL result.
    liella_boundary::errno::set_raw(0);
    assert!(x::getservbyname(c"liella-no-such-service".as_ptr(), c"tcp".as_ptr()).is_null());
    assert!(x::getprotobyname(c"liella-no-such-protocol".as_ptr()).is_null());
    assert!(x::getrpcbyname(c"liella-no-such-program".as_ptr()).is_null());
    assert_eq!(liella_boundary::errno::get(), 0);
    let mut buf = [0u8; 256];
    let mut out = liella_abi::layout::struct_protoent { p_name: 0, p_aliases: 0, p_proto: 0 };
    let mut res = 1usize;
    assert_eq!(x::getprotobyname_r(c"liella-no-such-protocol".as_ptr(), Some(&mut out), buf.as_mut_ptr().cast(), buf.len(), Some(&mut res)), 0);
    assert_eq!(res, 0);
    // ERANGE while the buffer is too small, for an entry of the system's protocols file (read
    // by the test itself; nothing to check without one, or when the switch does not read it).
    let text = std::fs::read_to_string("/etc/protocols").unwrap_or_default();
    let Some(num) = text.lines().filter_map(|l| l.split('#').next()).find_map(|l| {
        let w: Vec<&str> = l.split_whitespace().collect();
        (w.len() >= 2).then(|| w[1].parse::<i32>().ok()).flatten()
    }) else {
        return;
    };
    let conf = std::fs::read_to_string("/etc/nsswitch.conf").ok();
    if !conf.is_none_or(|c| c.lines().rfind(|l| l.trim_start().starts_with("protocols:")).is_none_or(|l| l.contains("files"))) {
        return;
    }
    assert_eq!(
        x::getprotobynumber_r(num, Some(&mut out), buf.as_mut_ptr().cast(), 1, Some(&mut res)),
        Errno::ERANGE.get(),
        "getprotobynumber_r({num}) with a 1-byte buffer: ERANGE (the entry is found: aliases split, comment stripped)"
    );
    assert_eq!(res, 0);
    assert_eq!(x::getprotobynumber_r(num, Some(&mut out), buf.as_mut_ptr().cast(), buf.len(), Some(&mut res)), 0);
    assert_eq!(out.p_proto, num);
}

#[test]
fn s16_6_c_0() {
    let fx = Fx::new("aliases");
    let c = fx.ctx(FILES);
    let members = |n: &[u8]| liella_legacy::nss::alias(&c, n, |a| (a.name.to_vec(), a.members.iter().map(<[u8]>::to_vec).collect::<Vec<_>>()));
    assert_eq!(members(b"ROOT"), Ok((b"root".to_vec(), vec![b"alice".to_vec(), b"bob".to_vec()])), "names ignore case; members trimmed");
    assert_eq!(members(b"multi"), Ok((b"multi".to_vec(), vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()])), "continuation lines");
    assert_eq!(members(b"list"), Err(LookupError::NotFound), ":include: members are skipped; an entry without members is none");
    // The enumeration is process-wide: in a child.
    if !in_child("s16_6_c_0") {
        return;
    }
    let mut seen = Vec::new();
    while let Some(n) = netdb::next_alias(&c, |a| liella_legacy::cursors::Step::Take(a.name.to_vec())).unwrap() {
        seen.push(n);
    }
    assert_eq!(seen, [b"postmaster".to_vec(), b"root".to_vec(), b"multi".to_vec()]);
}
