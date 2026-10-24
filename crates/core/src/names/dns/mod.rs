//! The DNS stub resolver (spec §16.4; D-16.3) and IDNA (spec §16.2; D-16.4), as values and
//! functions over the kernel.
//!
//! Written under the clean-room rules of implementation-plan.md §1.4 from the spec, RFC 1034,
//! RFC 1035 (messages, names, compression), RFC 1876 (`LOC`), RFC 2874 (`A6`), RFC 3596
//! (`AAAA`), RFC 3492 (Punycode), RFC 5891–5893 (IDNA2008 and its Bidi rule), RFC 6891 (EDNS),
//! UTS #46 15.1.0, the `resolver(3)`, `resolv.conf(5)`, `host.conf(5)` and `hostname(7)` man
//! pages, `spec/behaviour/dns.md` and the black-box probes recorded in D-16.3 and D-16.4
//! (`tests/oracle/resolv_diff.c`).
//!
//! * [`name`]: domain names in text and wire form: the escapes, compression, comparison and
//!   the host-name checks (`dn_comp`, `dn_expand`, `ns_name_*`, `res_hnok`, ...).
//! * [`msg`]: messages: building queries, parsing answers into sections and records.
//! * [`text`]: the text forms of records, TTLs, dates, Base64, `LOC` data, and the debugging
//!   printers (`ns_sprintrr`, `fp_nquery`, `p_type`, ...).
//! * [`conf`]: the resolver configuration (`resolv.conf`, `RES_OPTIONS`, `LOCALDOMAIN`).
//! * [`client`]: sending queries to the configured servers (UDP, TCP, retries, validation) and
//!   the search rules.
//! * [`host`]: host answers (`A`, `AAAA`, `PTR` with `CNAME` chains) for the `dns` source.
//! * [`idna`]: UTS #46 processing and Punycode.

pub mod client;
pub mod conf;
pub mod host;
pub mod idna;
mod idna_tables;
pub mod msg;
pub mod name;
pub mod text;

/// `C_IN`.
pub const CLASS_IN: u16 = 1;
/// `T_A`.
pub const T_A: u16 = 1;
/// `T_NS`.
pub const T_NS: u16 = 2;
/// `T_CNAME`.
pub const T_CNAME: u16 = 5;
/// `T_SOA`.
pub const T_SOA: u16 = 6;
/// `T_PTR`.
pub const T_PTR: u16 = 12;
/// `T_AAAA`.
pub const T_AAAA: u16 = 28;
/// `T_OPT` (RFC 6891).
pub const T_OPT: u16 = 41;

/// The response codes the resolver acts on (RFC 1035 §4.1.1).
pub mod rcode {
    /// No error.
    pub const NOERROR: u8 = 0;
    /// Format error.
    pub const FORMERR: u8 = 1;
    /// Server failure.
    pub const SERVFAIL: u8 = 2;
    /// Name error (the name does not exist).
    pub const NXDOMAIN: u8 = 3;
    /// Not implemented.
    pub const NOTIMP: u8 = 4;
    /// Refused.
    pub const REFUSED: u8 = 5;
}
