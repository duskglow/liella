//! Name services and their databases (spec §16, phase 10).
//!
//! Written under the clean-room rules of implementation-plan.md §1.4 from the spec, the
//! `nsswitch.conf(5)`, `hosts(5)`, `networks(5)`, `services(5)`, `protocols(5)`, `rpc(5)`,
//! `host.conf(5)`, `gai.conf(5)` and `getaddrinfo(3)` man pages, the GNU C Library manual's NSS
//! chapter, RFC 3493 and RFC 6724, and `spec/behaviour/netdb.md` with the black-box probes
//! recorded in D-16.1/D-16.2.
//!
//! * [`nss`]: the Name Service Switch as a value: the databases, the sources, the
//!   `[STATUS=ACTION]` rules, the `nsswitch.conf` grammar and its defaults, and the engine that
//!   runs one lookup through a database's sources ([`nss::run`]). Pure: the process-wide
//!   configuration (loaded from `/etc/nsswitch.conf`, reloaded when it changes) and the sources
//!   themselves live in `liella_legacy::nss`.
//! * [`db`]: the record formats of the `files` source (one line per record, borrowed views).
//! * [`sort`]: RFC 6724 destination address selection and the `gai.conf` policy tables.
//! * [`msg`]: the texts of `gai_strerror` and `hstrerror`.
//! * [`cache`]: the table of the integrated name-service cache (D-16.10), as untrusted atomic
//!   words; the cache's process-wide logic is `liella_legacy::nscache`.

pub mod cache;
pub mod db;
pub mod dns;
pub mod msg;
pub mod nss;
pub mod sort;

/// An Internet address: 4 or 16 bytes, network order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ip {
    /// IPv4.
    V4([u8; 4]),
    /// IPv6.
    V6([u8; 16]),
}

impl Ip {
    /// The address bytes (4 or 16).
    pub fn bytes(&self) -> &[u8] {
        match self {
            Ip::V4(a) => a,
            Ip::V6(a) => a,
        }
    }

    /// The family.
    pub fn family(&self) -> Family {
        match self {
            Ip::V4(_) => Family::V4,
            Ip::V6(_) => Family::V6,
        }
    }

    /// The IPv6 form: IPv4 as its mapped address (`::ffff:a.b.c.d`, RFC 4291 §2.5.5.2).
    pub fn to_v6(&self) -> [u8; 16] {
        match *self {
            Ip::V6(a) => a,
            Ip::V4([a, b, c, d]) => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, a, b, c, d],
        }
    }

    /// The IPv4 address an IPv4-mapped IPv6 address stands for.
    pub fn unmapped(a: &[u8; 16]) -> Option<[u8; 4]> {
        match *a {
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, w, x, y, z] => Some([w, x, y, z]),
            _ => None,
        }
    }
}

/// An address family a lookup asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// `AF_INET`.
    V4,
    /// `AF_INET6`.
    V6,
    /// `AF_UNSPEC`: either.
    Any,
}

impl Family {
    /// Whether `ip` is of this family.
    pub fn admits(self, ip: &Ip) -> bool {
        self == Family::Any || ip.family() == self
    }
}

/// ASCII white space, as the database formats separate fields with it.
pub fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}
