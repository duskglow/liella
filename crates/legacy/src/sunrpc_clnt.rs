//! ONC RPC client rules (LG-15, D-29.1): the call header, the reply decode and the client
//! error texts. Replies are untrusted input: every read is bounds-checked through
//! [`Reader`](crate::sunrpc_svc::Reader), a reply for another call (wrong xid, or too short to
//! carry one) is skipped, and a reply that cannot be decoded is `RPC_CANTDECODERES`, never a
//! read past the datagram. Behaviour settled by black-box probes of the host glibc (RPC-CLIENT,
//! 2026-10-23); RFC 5531 wire format.

use crate::sunrpc_svc::{self as svc, Auth, MSG_ACCEPTED, MSG_CALL, MSG_DENIED, MSG_REPLY, RPC_VERS, Reader, RpcErr, clnt_stat};

/// `RPC_CANTENCODEARGS`.
pub const CANTENCODEARGS: i32 = 1;
/// `RPC_CANTDECODERES`.
pub const CANTDECODERES: i32 = 2;
/// `RPC_CANTSEND`.
pub const CANTSEND: i32 = 3;
/// `RPC_CANTRECV`.
pub const CANTRECV: i32 = 4;
/// `RPC_TIMEDOUT`.
pub const TIMEDOUT: i32 = 5;
/// `RPC_SYSTEMERROR` (a create error carrying an errno).
pub const SYSTEMERROR: i32 = clnt_stat::SYSTEMERROR;
/// `RPC_PMAPFAILURE` (a create error carrying the port mapper call's status).
pub const PMAPFAILURE: i32 = 14;
/// The bytes of a call header before the credential: xid, CALL, rpcvers, prog, vers, proc.
pub const CALL_HEADER: usize = 24;
/// The reserved ports `bindresvport` tries.
pub const RESV_LOW: u16 = 512;
/// The last reserved port `bindresvport` tries.
pub const RESV_HIGH: u16 = 1023;

/// What one received datagram means to the call waiting for `xid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome<'a> {
    /// Not a reply to this call (shorter than an xid, or another xid): keep waiting.
    Ignore,
    /// The reply to this call, but malformed: `RPC_CANTDECODERES`.
    CantDecode,
    /// An accepted or denied reply that is not a success.
    Error(RpcErr),
    /// Success: the server's verifier, and the offset where the results start.
    Success {
        /// The reply verifier (at most `MAX_AUTH_BYTES`).
        verf: Auth<'a>,
        /// The offset of the results in the datagram.
        results: usize,
    },
}

/// Whether the datagram `from` (a `sockaddr_in`) came from `called`, the address a UDP call
/// was sent to: same port and IPv4 address (D-29.2, owner ruling 10-23).
#[must_use]
pub fn same_source(called: &[u8; 16], from: &[u8; 16]) -> bool {
    called[2..8] == from[2..8]
}

/// Whether `m` has the shape of an RPC reply: an xid of any value, message type REPLY and a
/// reply status of accepted or denied. The xid is not looked at, so a caller that counts
/// such datagrams learns nothing about which transaction IDs are valid (D-29.2).
#[must_use]
pub fn looks_like_reply(m: &[u8]) -> bool {
    let mut r = Reader::new(m);
    r.word().is_ok() && r.word().ok() == Some(MSG_REPLY) && matches!(r.word(), Ok(MSG_ACCEPTED | MSG_DENIED))
}

/// Decodes the reply `m` for the call `xid`.
#[must_use]
pub fn decode_reply(m: &[u8], xid: u32) -> Outcome<'_> {
    let mut r = Reader::new(m);
    match r.word() {
        Ok(x) if x == xid => {}
        _ => return Outcome::Ignore,
    }
    body(&mut r).unwrap_or(Outcome::CantDecode)
}

fn body<'a>(r: &mut Reader<'a>) -> Option<Outcome<'a>> {
    if r.word().ok()? != MSG_REPLY {
        return None;
    }
    let rp = r.word().ok()?;
    if rp == MSG_ACCEPTED {
        let verf = svc::auth(r).ok()?;
        let stat = r.word().ok()?;
        return Some(match stat {
            0 => Outcome::Success { verf, results: r.pos() },
            2 => {
                let (low, high) = (r.word().ok()?, r.word().ok()?);
                Outcome::Error(svc::seterr(rp, stat, u64::from(low), u64::from(high), 0))
            }
            s => Outcome::Error(svc::seterr(rp, s, 0, 0, 0)),
        });
    }
    if rp != MSG_DENIED {
        return None;
    }
    let stat = r.word().ok()?;
    Some(Outcome::Error(match stat {
        0 => {
            let (low, high) = (r.word().ok()?, r.word().ok()?);
            svc::seterr(rp, stat, u64::from(low), u64::from(high), 0)
        }
        1 => svc::seterr(rp, stat, 0, 0, r.word().ok()?),
        s => svc::seterr(rp, s, 0, 0, 0),
    }))
}

/// Writes the call header words (xid, CALL, rpcvers 2, prog, vers, proc) into `out`; the
/// credential and verifier follow (the AUTH's marshal), then the arguments.
#[must_use]
pub fn call_header(xid: u32, prog: u32, vers: u32, proc_: u32, out: &mut [u8]) -> Option<usize> {
    let dst = out.get_mut(..CALL_HEADER)?;
    for (d, w) in dst.chunks_exact_mut(4).zip([xid, MSG_CALL, RPC_VERS, prog, vers, proc_]) {
        d.copy_from_slice(&w.to_be_bytes());
    }
    Some(CALL_HEADER)
}

/// `clnt_sperrno`: the text of a `clnt_stat` (without a newline).
#[must_use]
pub const fn sperrno(stat: i32) -> &'static str {
    match stat {
        0 => "RPC: Success",
        1 => "RPC: Can't encode arguments",
        2 => "RPC: Can't decode result",
        3 => "RPC: Unable to send",
        4 => "RPC: Unable to receive",
        5 => "RPC: Timed out",
        6 => "RPC: Incompatible versions of RPC",
        7 => "RPC: Authentication error",
        8 => "RPC: Program unavailable",
        9 => "RPC: Program/version mismatch",
        10 => "RPC: Procedure unavailable",
        11 => "RPC: Server can't decode arguments",
        12 => "RPC: Remote system error",
        13 => "RPC: Unknown host",
        14 => "RPC: Port mapper failure",
        15 => "RPC: Program not registered",
        16 => "RPC: Failed (unspecified error)",
        17 => "RPC: Unknown protocol",
        _ => "RPC: (unknown error code)",
    }
}

/// The text of an `auth_stat` in `clnt_sperror`; `None` for an unknown value.
#[must_use]
pub const fn auth_why(why: u64) -> Option<&'static str> {
    Some(match why {
        0 => "Authentication OK",
        1 => "Invalid client credential",
        2 => "Server rejected credential",
        3 => "Invalid client verifier",
        4 => "Server rejected verifier",
        5 => "Client credential too weak",
        6 => "Invalid server verifier",
        7 => "Failed (unspecified error)",
        _ => return None,
    })
}

/// A bounded text buffer: output past its end is dropped, one byte is kept for the NUL.
#[derive(Debug)]
pub struct Text<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> Text<'a> {
    /// An empty text over `buf`.
    pub const fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    /// Appends `s`, truncated to the room left.
    pub fn put(&mut self, s: &[u8]) {
        let room = self.buf.len().saturating_sub(1).saturating_sub(self.len);
        let n = s.len().min(room);
        if let (Some(d), Some(src)) = (self.buf.get_mut(self.len..self.len.saturating_add(n)), s.get(..n)) {
            d.copy_from_slice(src);
            self.len = self.len.saturating_add(n);
        }
    }

    /// Appends `v` in decimal.
    pub fn num(&mut self, mut v: u64) {
        let mut d = [0u8; 20];
        let mut i = d.len();
        loop {
            i = i.saturating_sub(1);
            if let Some(c) = d.get_mut(i) {
                *c = b'0'.saturating_add(u8::try_from(v % 10).unwrap_or(0));
            }
            v /= 10;
            if v == 0 || i == 0 {
                break;
            }
        }
        self.put(d.get(i..).unwrap_or_default());
    }

    /// The text so far, NUL-terminated in the buffer (when the buffer is not empty).
    pub fn finish(self) -> usize {
        if let Some(z) = self.buf.get_mut(self.len) {
            *z = 0;
        }
        self.len
    }
}

/// `clnt_sperror`: `"<prefix>: <sperrno>"` and the detail of the error. `errstr` is the
/// `strerror` text of `a` (used for `RPC_CANTSEND`/`RPC_CANTRECV`). As probed, a version
/// mismatch line has no trailing newline; every other line ends in one.
pub fn sperror(t: &mut Text<'_>, prefix: &[u8], status: i32, a: u64, b: u64, errstr: &[u8]) {
    t.put(prefix);
    t.put(b": ");
    t.put(sperrno(status).as_bytes());
    match status {
        CANTSEND | CANTRECV => {
            t.put(b"; errno = ");
            t.put(errstr);
        }
        clnt_stat::AUTHERROR => {
            t.put(b"; why = ");
            if let Some(w) = auth_why(a) {
                t.put(w.as_bytes());
            } else {
                t.put(b"(unknown authentication error - ");
                t.num(a);
                t.put(b")");
            }
        }
        clnt_stat::VERSMISMATCH | clnt_stat::PROGVERSMISMATCH => {
            t.put(b"; low version = ");
            t.num(a);
            t.put(b", high version = ");
            t.num(b);
            return;
        }
        _ => {}
    }
    t.put(b"\n");
}

/// `clnt_spcreateerror`: `"<prefix>: <sperrno(cf_stat)>"`, then `" - "` and the errno text
/// (`RPC_SYSTEMERROR`) or the port mapper call's status text (`RPC_PMAPFAILURE`), then a newline.
pub fn spcreateerror(t: &mut Text<'_>, prefix: &[u8], cf_stat: i32, re_status: i32, errstr: &[u8]) {
    t.put(prefix);
    t.put(b": ");
    t.put(sperrno(cf_stat).as_bytes());
    match cf_stat {
        SYSTEMERROR => {
            t.put(b" - ");
            t.put(errstr);
        }
        PMAPFAILURE => {
            t.put(b" - ");
            t.put(sperrno(re_status).as_bytes());
        }
        _ => {}
    }
    t.put(b"\n");
}

/// The order `bindresvport` tries the reserved ports: every port of 512..=1023 once, starting
/// at `512 + seed % 512`.
pub fn resv_ports(seed: u32) -> impl Iterator<Item = u16> {
    let span = u32::from(RESV_HIGH.wrapping_sub(RESV_LOW)).wrapping_add(1);
    (0..span).map(move |i| {
        let off = seed.checked_rem(span).unwrap_or(0).wrapping_add(i).checked_rem(span).unwrap_or(0);
        RESV_LOW.saturating_add(u16::try_from(off).unwrap_or(0))
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn words(ws: &[u32]) -> Vec<u8> {
        ws.iter().flat_map(|w| w.to_be_bytes()).collect()
    }

    const X: u32 = 0x0a0b_0c0d;

    fn text(f: impl FnOnce(&mut Text<'_>)) -> String {
        let mut b = [0u8; 256];
        let mut t = Text::new(&mut b);
        f(&mut t);
        let n = t.finish();
        String::from_utf8(b[..n].to_vec()).unwrap()
    }

    // Hostile-reply corpus (no spec clause; LG-15, RFC 5531 §9, the probe modes of RPC-CLIENT).
    #[test]
    fn reply_decode_corpus() {
        let ok = words(&[X, 1, 0, 0, 0, 0, 77]);
        assert_eq!(decode_reply(&ok, X), Outcome::Success { verf: Auth { flavor: 0, body: &[] }, results: 24 });
        // Another call's reply, or a datagram too short to carry an xid, is skipped.
        assert_eq!(decode_reply(&ok, X ^ 1), Outcome::Ignore, "wrong xid");
        for n in 0..4 {
            assert_eq!(decode_reply(&ok[..n], X), Outcome::Ignore, "short datagram {n}");
        }
        // Every truncation after the xid is undecodable, never a read past the end.
        for n in 4..24 {
            assert_eq!(decode_reply(&ok[..n], X), Outcome::CantDecode, "cut at {n}");
        }
        // A CALL with this xid, an unknown reply_stat: undecodable.
        assert_eq!(decode_reply(&words(&[X, 0, 0, 0, 0, 0, 77]), X), Outcome::CantDecode, "msg type CALL");
        assert_eq!(decode_reply(&words(&[X, 1, 7, 1, 1]), X), Outcome::CantDecode, "reply_stat 7");
        // A verifier longer than MAX_AUTH_BYTES fails before its body is read.
        let mut long = words(&[X, 1, 0, 0, 401]);
        long.resize(long.len() + 404 + 4, 0);
        assert_eq!(decode_reply(&long, X), Outcome::CantDecode, "verifier 401");
        // A verifier that claims more than the datagram holds.
        assert_eq!(decode_reply(&words(&[X, 1, 0, 0, 400, 0]), X), Outcome::CantDecode, "verifier past end");
        let v = words(&[X, 1, 0, 1, 4, 0xdead_beef, 0, 5]);
        assert_eq!(decode_reply(&v, X), Outcome::Success { verf: Auth { flavor: 1, body: &[0xde, 0xad, 0xbe, 0xef] }, results: 28 });
    }

    // Accepted and denied errors as the host client reports them (no spec clause; LG-15 probe).
    #[test]
    fn reply_errors() {
        let e = |w: &[u32], want: Outcome<'static>, why: &str| assert_eq!(decode_reply(&words(w), X), want, "{why}");
        e(&[X, 1, 0, 0, 0, 1], Outcome::Error(RpcErr::Status(clnt_stat::PROGUNAVAIL)), "&[X, 1, 0, 0, 0, 1]");
        e(&[X, 1, 0, 0, 0, 2, 1, 3], Outcome::Error(RpcErr::Versions(clnt_stat::PROGVERSMISMATCH, 1, 3)), "&[X, 1, 0, 0, 0, 2, 1, 3]");
        e(&[X, 1, 0, 0, 0, 2, 1], Outcome::CantDecode, "mismatch cut short");
        e(&[X, 1, 0, 0, 0, 3], Outcome::Error(RpcErr::Status(clnt_stat::PROCUNAVAIL)), "&[X, 1, 0, 0, 0, 3]");
        e(&[X, 1, 0, 0, 0, 9], Outcome::Error(RpcErr::Failed(0, 9)), "&[X, 1, 0, 0, 0, 9]");
        e(&[X, 1, 1, 0, 2, 2], Outcome::Error(RpcErr::Versions(clnt_stat::VERSMISMATCH, 2, 2)), "&[X, 1, 1, 0, 2, 2]");
        e(&[X, 1, 1, 0, 2], Outcome::CantDecode, "rpc mismatch cut short");
        e(&[X, 1, 1, 1, 1], Outcome::Error(RpcErr::Why(1)), "&[X, 1, 1, 1, 1]");
        e(&[X, 1, 1, 1], Outcome::CantDecode, "auth error without why");
        e(&[X, 1, 1, 5], Outcome::Error(RpcErr::Failed(1, 5)), "&[X, 1, 1, 5]");
    }

    #[test]
    fn call_header_words() {
        // Trace: no spec clause (LG-15 call header, RFC 5531 §9).
        let mut b = [0u8; 24];
        assert_eq!(call_header(X, 0x2000_1234, 1, 7, &mut b), Some(24));
        assert_eq!(b.to_vec(), words(&[X, 0, 2, 0x2000_1234, 1, 7]));
        assert_eq!(call_header(X, 1, 1, 1, &mut [0u8; 23]), None);
    }

    // Texts byte for byte as the host glibc prints them (no spec clause; LG-15 probe).
    #[test]
    fn error_texts() {
        assert_eq!(sperrno(9), "RPC: Program/version mismatch");
        assert_eq!(sperrno(-1), "RPC: (unknown error code)");
        assert_eq!(sperrno(18), "RPC: (unknown error code)");
        assert_eq!(text(|t| sperror(t, b"pfx", 0, 0, 0, b"")), "pfx: RPC: Success\n");
        assert_eq!(text(|t| sperror(t, b"pfx", 4, 111, 0, b"Connection refused")), "pfx: RPC: Unable to receive; errno = Connection refused\n");
        assert_eq!(text(|t| sperror(t, b"pfx", 7, 5, 0, b"")), "pfx: RPC: Authentication error; why = Client credential too weak\n");
        assert_eq!(text(|t| sperror(t, b"pfx", 7, 8, 0, b"")), "pfx: RPC: Authentication error; why = (unknown authentication error - 8)\n");
        assert_eq!(text(|t| sperror(t, b"pfx", 9, 1, 3, b"")), "pfx: RPC: Program/version mismatch; low version = 1, high version = 3");
        assert_eq!(text(|t| sperror(t, b"pfx", 6, 2, 2, b"")), "pfx: RPC: Incompatible versions of RPC; low version = 2, high version = 2");
        assert_eq!(text(|t| sperror(t, b"pfx", 16, 0, 9, b"")), "pfx: RPC: Failed (unspecified error)\n");
        assert_eq!(text(|t| spcreateerror(t, b"pfx", 15, 0, b"")), "pfx: RPC: Program not registered\n");
        assert_eq!(text(|t| spcreateerror(t, b"pfx", 12, 111, b"Connection refused")), "pfx: RPC: Remote system error - Connection refused\n");
        assert_eq!(text(|t| spcreateerror(t, b"pfx", 14, 5, b"")), "pfx: RPC: Port mapper failure - RPC: Timed out\n");
        // A long prefix is cut at the buffer, which stays NUL-terminated.
        let mut b = [0xffu8; 8];
        let mut t = Text::new(&mut b);
        sperror(&mut t, b"a-long-prefix", 0, 0, 0, b"");
        assert_eq!(t.finish(), 7);
        assert_eq!(&b, b"a-long-\0");
        assert_eq!(text(|t| t.num(u64::MAX)), "18446744073709551615");
    }

    #[test]
    fn reserved_port_order() {
        // Trace: no spec clause (LG-15 bindresvport: every port of 512..1023 once).
        let mut p: Vec<u16> = resv_ports(700).collect();
        assert_eq!(p.first(), Some(&700));
        assert_eq!(p.len(), 512);
        p.sort_unstable();
        assert_eq!(p, (512..=1023).collect::<Vec<u16>>());
    }
}
