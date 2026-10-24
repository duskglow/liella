//! ONC RPC server rules (spec §29.1; RFC 5531): the call header a server decodes from untrusted
//! input, the `AUTH_UNIX` credential `_authenticate` reads, the reply header a server writes, the
//! program/version routing of `svc_getreq_common`, the TCP record-marking admission rule, and the
//! `_seterr_reply` mapping. Every decode is bounds-checked and fails with a typed error; nothing
//! here allocates or touches a pointer, so all of it is tested in-process (R15-2) and fuzzed
//! (`fuzz/fuzz_targets/rpc_call.rs`). The C transports (`svcudp_*`, `svctcp_*`, `svc_run`) that
//! apply these rules are the next step of LG-14 (`handoff/rpc-server.md`).

/// The RPC protocol version (RFC 5531 §8).
pub const RPC_VERS: u32 = 2;
/// `msg_type` CALL.
pub const MSG_CALL: u32 = 0;
/// `msg_type` REPLY.
pub const MSG_REPLY: u32 = 1;
/// `reply_stat` MSG_ACCEPTED.
pub const MSG_ACCEPTED: u32 = 0;
/// `reply_stat` MSG_DENIED.
pub const MSG_DENIED: u32 = 1;
/// The largest opaque auth body (RFC 5531 §8.2).
pub const MAX_AUTH_BYTES: u32 = 400;
/// The longest `AUTH_UNIX` machine name (RFC 5531 appendix A).
pub const MAX_MACHINE_NAME: u32 = 255;
/// The most supplementary groups an `AUTH_UNIX` credential carries.
pub const NGRPS: usize = 16;
/// `AUTH_NONE`.
pub const AUTH_NONE: u32 = 0;
/// `AUTH_UNIX` (`AUTH_SYS`).
pub const AUTH_UNIX: u32 = 1;
/// `AUTH_SHORT`.
pub const AUTH_SHORT: u32 = 2;
/// `auth_stat` AUTH_BADCRED.
pub const AUTH_BADCRED: u32 = 1;
/// `auth_stat` AUTH_REJECTEDCRED.
pub const AUTH_REJECTEDCRED: u32 = 2;
/// The last-fragment bit of a record-marking header (RFC 5531 §11).
pub const LAST_FRAG: u32 = 0x8000_0000;

/// Why a call could not be decoded. A server answers none of these (there is no trustworthy xid
/// or header to answer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The message ends inside an item.
    Truncated,
    /// The message is not a CALL.
    NotCall,
    /// An auth body is longer than `MAX_AUTH_BYTES`.
    AuthTooLong,
}

/// A bounds-checked reader of XDR units over a received message.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `buf`.
    #[must_use]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// The offset of the next unread byte.
    #[must_use]
    pub const fn pos(&self) -> usize {
        self.pos
    }

    /// `n` bytes, or `Truncated` when fewer remain (nothing is consumed then).
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::Truncated)?;
        let got = self.buf.get(self.pos..end).ok_or(DecodeError::Truncated)?;
        self.pos = end;
        Ok(got)
    }

    /// One big-endian unsigned word.
    pub fn word(&mut self) -> Result<u32, DecodeError> {
        let b = self.take(4)?;
        let mut w = [0u8; 4];
        w.copy_from_slice(b);
        Ok(u32::from_be_bytes(w))
    }

    /// `len` opaque bytes and their padding to a whole unit; the padding is not returned.
    pub fn opaque(&mut self, len: u32) -> Result<&'a [u8], DecodeError> {
        let n = usize::try_from(len).map_err(|_| DecodeError::Truncated)?;
        let padded = n.checked_add(usize::try_from(crate::sunrpc::pad(len)).map_err(|_| DecodeError::Truncated)?);
        let all = self.take(padded.ok_or(DecodeError::Truncated)?)?;
        all.get(..n).ok_or(DecodeError::Truncated)
    }
}

/// An `opaque_auth`: a flavour and its body (at most `MAX_AUTH_BYTES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Auth<'a> {
    /// The flavour.
    pub flavor: u32,
    /// The body.
    pub body: &'a [u8],
}

/// Reads an `opaque_auth`; a length above `MAX_AUTH_BYTES` fails before any body is read.
pub fn auth<'a>(r: &mut Reader<'a>) -> Result<Auth<'a>, DecodeError> {
    let flavor = r.word()?;
    let len = r.word()?;
    if len > MAX_AUTH_BYTES {
        return Err(DecodeError::AuthTooLong);
    }
    Ok(Auth { flavor, body: r.opaque(len)? })
}

/// A decoded call header; the procedure's arguments start at `args`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Call<'a> {
    /// The transaction id the reply echoes.
    pub xid: u32,
    /// The RPC version the caller speaks.
    pub rpcvers: u32,
    /// Program, version and procedure.
    pub prog: u32,
    /// The program version.
    pub vers: u32,
    /// The procedure.
    pub proc_: u32,
    /// The credential.
    pub cred: Auth<'a>,
    /// The verifier.
    pub verf: Auth<'a>,
    /// The offset of the arguments in the message.
    pub args: usize,
}

/// Decodes a call header (RFC 5531 §9) from a received message.
pub fn decode_call(m: &[u8]) -> Result<Call<'_>, DecodeError> {
    let mut r = Reader::new(m);
    let xid = r.word()?;
    if r.word()? != MSG_CALL {
        return Err(DecodeError::NotCall);
    }
    let rpcvers = r.word()?;
    let prog = r.word()?;
    let vers = r.word()?;
    let proc_ = r.word()?;
    let cred = auth(&mut r)?;
    let verf = auth(&mut r)?;
    Ok(Call { xid, rpcvers, prog, vers, proc_, cred, verf, args: r.pos() })
}

/// A decoded `AUTH_UNIX` credential (`struct authunix_parms`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnixCred<'a> {
    /// The caller's time stamp.
    pub stamp: u32,
    /// The machine name (at most `MAX_MACHINE_NAME` bytes).
    pub machine: &'a [u8],
    /// The user id.
    pub uid: u32,
    /// The group id.
    pub gid: u32,
    /// The supplementary groups (the first `ngids`).
    pub gids: [u32; NGRPS],
    /// How many of `gids` are set.
    pub ngids: usize,
}

/// Decodes an `AUTH_UNIX` body; a name over 255 bytes, more than 16 groups or a short body fail.
pub fn decode_unix(body: &[u8]) -> Result<UnixCred<'_>, DecodeError> {
    let mut r = Reader::new(body);
    let stamp = r.word()?;
    let len = r.word()?;
    if len > MAX_MACHINE_NAME {
        return Err(DecodeError::AuthTooLong);
    }
    let machine = r.opaque(len)?;
    let uid = r.word()?;
    let gid = r.word()?;
    let n = usize::try_from(r.word()?).map_err(|_| DecodeError::AuthTooLong)?;
    if n > NGRPS {
        return Err(DecodeError::AuthTooLong);
    }
    let mut gids = [0u32; NGRPS];
    for g in gids.iter_mut().take(n) {
        *g = r.word()?;
    }
    Ok(UnixCred { stamp, machine, uid, gid, gids, ngids: n })
}

/// `_authenticate`'s decision on a credential: `AUTH_NONE` passes with no parsed credential,
/// `AUTH_UNIX` passes when its body decodes (else `AUTH_BADCRED`), every other flavour
/// (`AUTH_SHORT` included: this server issues no short handles) is `AUTH_REJECTEDCRED`.
pub fn authenticate<'a>(cred: &Auth<'a>) -> Result<Option<UnixCred<'a>>, u32> {
    match cred.flavor {
        AUTH_NONE => Ok(None),
        AUTH_UNIX => decode_unix(cred.body).map(Some).map_err(|_| AUTH_BADCRED),
        _ => Err(AUTH_REJECTEDCRED),
    }
}

/// The reply a server sends (RFC 5531 §9 `reply_body`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// MSG_ACCEPTED SUCCESS; the results follow the header.
    Success,
    /// MSG_ACCEPTED PROG_UNAVAIL.
    ProgUnavail,
    /// MSG_ACCEPTED PROG_MISMATCH with the lowest and highest versions served.
    ProgMismatch(u32, u32),
    /// MSG_ACCEPTED PROC_UNAVAIL.
    ProcUnavail,
    /// MSG_ACCEPTED GARBAGE_ARGS.
    GarbageArgs,
    /// MSG_ACCEPTED SYSTEM_ERR.
    SystemErr,
    /// MSG_DENIED RPC_MISMATCH with the versions supported.
    RpcMismatch(u32, u32),
    /// MSG_DENIED AUTH_ERROR with the `auth_stat`.
    AuthError(u32),
}

/// Writes the reply header for `xid` into `out` (the verifier only on accepted replies) and
/// answers its length, or `None` when `out` is too small or the verifier too long.
#[must_use]
pub fn reply_header(xid: u32, verf: &Auth<'_>, reply: Reply, out: &mut [u8]) -> Option<usize> {
    let mut w = [0u32; 8];
    let accepted = |stat: u32| (MSG_ACCEPTED, Some(stat));
    let (rstat, astat) = match reply {
        Reply::Success | Reply::ProgMismatch(..) => accepted(if reply == Reply::Success { 0 } else { 2 }),
        Reply::ProgUnavail => accepted(1),
        Reply::ProcUnavail => accepted(3),
        Reply::GarbageArgs => accepted(4),
        Reply::SystemErr => accepted(5),
        Reply::RpcMismatch(..) | Reply::AuthError(_) => (MSG_DENIED, None),
    };
    let mut n = 0usize;
    let mut push = |v: u32, w: &mut [u32; 8]| {
        if let Some(s) = w.get_mut(n) {
            *s = v;
        }
        n = n.saturating_add(1);
    };
    push(xid, &mut w);
    push(MSG_REPLY, &mut w);
    push(rstat, &mut w);
    let mut tail = [0u32; 3];
    let mut tn = 0usize;
    match (reply, astat) {
        (Reply::RpcMismatch(lo, hi), _) => {
            tail = [0, lo, hi];
            tn = 3;
        }
        (Reply::AuthError(why), _) => {
            tail = [1, why, 0];
            tn = 2;
        }
        (Reply::ProgMismatch(lo, hi), Some(s)) => {
            tail = [s, lo, hi];
            tn = 3;
        }
        (_, Some(s)) => {
            tail[0] = s;
            tn = 1;
        }
        (_, None) => {}
    }
    let vlen = u32::try_from(verf.body.len()).ok().filter(|l| *l <= MAX_AUTH_BYTES)?;
    let vbytes = if rstat == MSG_ACCEPTED { verf.body.len().checked_add(usize::try_from(crate::sunrpc::pad(vlen)).ok()?)? } else { 0 };
    let total = n.checked_add(tn)?.checked_mul(4)?.checked_add(if rstat == MSG_ACCEPTED { vbytes.checked_add(8)? } else { 0 })?;
    let dst = out.get_mut(..total)?;
    dst.fill(0);
    let mut at = 0usize;
    for v in w.iter().take(n) {
        put(dst, &mut at, &v.to_be_bytes())?;
    }
    if rstat == MSG_ACCEPTED {
        put(dst, &mut at, &verf.flavor.to_be_bytes())?;
        put(dst, &mut at, &vlen.to_be_bytes())?;
        put(dst, &mut at, verf.body)?;
        at = at.checked_add(vbytes.checked_sub(verf.body.len())?)?;
    }
    for v in tail.iter().take(tn) {
        put(dst, &mut at, &v.to_be_bytes())?;
    }
    Some(at)
}

/// Copies `b` into `dst` at `*at` and advances `*at`; `None` past the end of `dst`.
fn put(dst: &mut [u8], at: &mut usize, b: &[u8]) -> Option<()> {
    let end = at.checked_add(b.len())?;
    dst.get_mut(*at..end)?.copy_from_slice(b);
    *at = end;
    Some(())
}

/// `svc_getreq_common`'s routing of an authenticated call: the answer when the program is not
/// served at the asked version, or `None` to dispatch. `served` lists the registered
/// (program, version) pairs.
#[must_use]
pub fn route(served: &[(u32, u32)], prog: u32, vers: u32) -> Option<Reply> {
    let mut range: Option<(u32, u32)> = None;
    for &(p, v) in served {
        if p != prog {
            continue;
        }
        if v == vers {
            return None;
        }
        range = Some(range.map_or((v, v), |(lo, hi)| (lo.min(v), hi.max(v))));
    }
    Some(range.map_or(Reply::ProgUnavail, |(lo, hi)| Reply::ProgMismatch(lo, hi)))
}

/// A call to dispatch (with its parsed `AUTH_UNIX` credential), or the (xid, reply) to send.
pub type Verdict<'a> = Result<(Call<'a>, Option<UnixCred<'a>>), (u32, Reply)>;

/// The full answer to a received message: `None` means send nothing (undecodable), otherwise the
/// reply to send, or `Ok` with the call to dispatch.
pub fn verdict<'a>(served: &[(u32, u32)], m: &'a [u8]) -> Option<Verdict<'a>> {
    let call = decode_call(m).ok()?;
    if call.rpcvers != RPC_VERS {
        return Some(Err((call.xid, Reply::RpcMismatch(RPC_VERS, RPC_VERS))));
    }
    let cred = match authenticate(&call.cred) {
        Ok(c) => c,
        Err(why) => return Some(Err((call.xid, Reply::AuthError(why)))),
    };
    match route(served, call.prog, call.vers) {
        Some(r) => Some(Err((call.xid, r))),
        None => Some(Ok((call, cred))),
    }
}

/// Why a TCP record was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordError {
    /// The record would exceed the server's record limit.
    TooLong,
}

/// Splits a record-marking header into (last fragment, fragment length).
#[must_use]
pub const fn fragment(header: u32) -> (bool, u32) {
    (header & LAST_FRAG != 0, header & !LAST_FRAG)
}

/// Admits a fragment of `len` bytes after `have` bytes of the record: the new record length, or
/// `TooLong` when it would pass `cap`. A server checks this before reading or allocating for the
/// fragment, so a hostile length costs nothing.
pub fn admit(have: usize, len: u32, cap: usize) -> Result<usize, RecordError> {
    let total = usize::try_from(len).ok().and_then(|l| have.checked_add(l)).ok_or(RecordError::TooLong)?;
    if total > cap { Err(RecordError::TooLong) } else { Ok(total) }
}

/// The header of a record sent as one last fragment, or `None` past the 31-bit length.
#[must_use]
pub fn frame(len: usize) -> Option<u32> {
    u32::try_from(len).ok().filter(|l| *l & LAST_FRAG == 0).map(|l| l | LAST_FRAG)
}

/// The longest call header a stream server reads before the arguments: six words, then two
/// `opaque_auth`s of at most `MAX_AUTH_BYTES` each (840 bytes).
pub const CALL_HEADER_MAX: usize = 24 + 2 * (8 + 400);

/// Reads `n` more bytes of a header into `buf` at `at` through `get`.
fn chunk(get: &mut impl FnMut(&mut [u8]) -> bool, buf: &mut [u8], at: &mut usize, n: usize) -> Option<()> {
    let end = at.checked_add(n)?;
    get(buf.get_mut(*at..end)?).then(|| *at = end)
}

/// Reads a call header off a record stream into `buf` through `get` (which fills its slice from
/// the record, or fails): each auth length is checked against `MAX_AUTH_BYTES` before its body is
/// asked for, so a hostile length costs nothing and no more than `CALL_HEADER_MAX` bytes are read.
/// The header's length (`decode_call` of `buf[..n]` then decodes it), or `None`.
pub fn read_call_header(mut get: impl FnMut(&mut [u8]) -> bool, buf: &mut [u8; CALL_HEADER_MAX]) -> Option<usize> {
    let mut at = 0usize;
    chunk(&mut get, buf, &mut at, 24)?;
    for _ in 0..2 {
        chunk(&mut get, buf, &mut at, 8)?;
        let len = u32::from_be_bytes(buf.get(at.checked_sub(4)?..at)?.try_into().ok()?);
        if len > MAX_AUTH_BYTES {
            return None;
        }
        chunk(&mut get, buf, &mut at, usize::try_from(len.div_ceil(4)).ok()?.checked_mul(4)?)?;
    }
    Some(at)
}

/// `clnt_stat` values `_seterr_reply` produces.
pub mod clnt_stat {
    /// RPC_SUCCESS.
    pub const SUCCESS: i32 = 0;
    /// RPC_VERSMISMATCH.
    pub const VERSMISMATCH: i32 = 6;
    /// RPC_AUTHERROR.
    pub const AUTHERROR: i32 = 7;
    /// RPC_PROGUNAVAIL.
    pub const PROGUNAVAIL: i32 = 8;
    /// RPC_PROGVERSMISMATCH.
    pub const PROGVERSMISMATCH: i32 = 9;
    /// RPC_PROCUNAVAIL.
    pub const PROCUNAVAIL: i32 = 10;
    /// RPC_CANTDECODEARGS.
    pub const CANTDECODEARGS: i32 = 11;
    /// RPC_SYSTEMERROR.
    pub const SYSTEMERROR: i32 = 12;
    /// RPC_FAILED.
    pub const FAILED: i32 = 16;
}

/// What `_seterr_reply` stores in `struct rpc_err`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcErr {
    /// A status alone.
    Status(i32),
    /// RPC_VERSMISMATCH / RPC_PROGVERSMISMATCH with (low, high).
    Versions(i32, u64, u64),
    /// RPC_AUTHERROR with the `auth_stat`.
    Why(u32),
    /// RPC_FAILED with the two raw values (`re_lb`).
    Failed(i64, i64),
}

/// `_seterr_reply`: the client error of a reply. `stat` is `ar_stat` (accepted) or `rj_stat`
/// (denied); `low`/`high` and `why` are read from the reply's union as the stat selects.
#[must_use]
pub fn seterr(rp_stat: u32, stat: u32, low: u64, high: u64, why: u32) -> RpcErr {
    match (rp_stat, stat) {
        (MSG_ACCEPTED, 0) => RpcErr::Status(clnt_stat::SUCCESS),
        (MSG_ACCEPTED, 1) => RpcErr::Status(clnt_stat::PROGUNAVAIL),
        (MSG_ACCEPTED, 2) => RpcErr::Versions(clnt_stat::PROGVERSMISMATCH, low, high),
        (MSG_ACCEPTED, 3) => RpcErr::Status(clnt_stat::PROCUNAVAIL),
        (MSG_ACCEPTED, 4) => RpcErr::Status(clnt_stat::CANTDECODEARGS),
        (MSG_ACCEPTED, 5) => RpcErr::Status(clnt_stat::SYSTEMERROR),
        (MSG_DENIED, 0) => RpcErr::Versions(clnt_stat::VERSMISMATCH, low, high),
        (MSG_DENIED, 1) => RpcErr::Why(why),
        (MSG_ACCEPTED | MSG_DENIED, s) => RpcErr::Failed(i64::from(rp_stat), i64::from(s)),
        (r, _) => RpcErr::Failed(i64::from(r), 0),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn words(ws: &[u32]) -> Vec<u8> {
        ws.iter().flat_map(|w| w.to_be_bytes()).collect()
    }

    /// A call: xid 0x01020304, the given rpcvers/prog/vers/proc, then cred and verf words.
    fn call(rpcvers: u32, prog: u32, vers: u32, proc_: u32, tail: &[u32]) -> Vec<u8> {
        let mut w = vec![0x0102_0304, MSG_CALL, rpcvers, prog, vers, proc_];
        w.extend_from_slice(tail);
        words(&w)
    }

    const NONE2: [u32; 4] = [0, 0, 0, 0];
    const SERVED: [(u32, u32); 3] = [(0x2000_0001, 1), (0x2000_0001, 3), (0x2000_0002, 1)];

    // Malformed-call corpus (no spec clause; RFC 5531 §9/§11 and §29.1's hostile-input rule).
    #[test]
    fn call_decode_corpus() {
        let ok = call(2, 0x2000_0001, 1, 1, &[0, 0, 0, 0, 41]);
        let c = decode_call(&ok).unwrap();
        assert_eq!((c.xid, c.prog, c.vers, c.proc_, c.args), (0x0102_0304, 0x2000_0001, 1, 1, 40));
        // Every truncation of a valid header fails as Truncated, never reads past the end.
        for n in 0..40 {
            assert_eq!(decode_call(&ok[..n]), Err(DecodeError::Truncated), "cut at {n}");
        }
        // A REPLY is not a call.
        let mut rep = ok.clone();
        rep[7] = 1;
        assert_eq!(decode_call(&rep), Err(DecodeError::NotCall));
        // Absurd and over-limit auth lengths fail before the body is read.
        for len in [401, 0x7fff_ffff, 0xffff_ffff] {
            assert_eq!(decode_call(&call(2, 1, 1, 1, &[0, len, 0, 0])), Err(DecodeError::AuthTooLong), "cred {len}");
            assert_eq!(decode_call(&call(2, 1, 1, 1, &[0, 0, 0, len])), Err(DecodeError::AuthTooLong), "verf {len}");
        }
        // 400 is the limit itself; a body claimed but missing is Truncated.
        let mut big = call(2, 1, 1, 1, &[0, 400]);
        big.extend_from_slice(&[0u8; 400]);
        big.extend_from_slice(&words(&[0, 0]));
        assert!(decode_call(&big).is_ok());
        assert_eq!(decode_call(&call(2, 1, 1, 1, &[0, 100, 0, 0])), Err(DecodeError::Truncated));
        // A body whose padding is missing is Truncated (5 bytes need 8).
        let mut odd = call(2, 1, 1, 1, &[0, 5]);
        odd.extend_from_slice(&[1, 2, 3, 4, 5, 0]);
        assert_eq!(decode_call(&odd), Err(DecodeError::Truncated));
    }

    #[test]
    fn auth_unix_corpus() {
        // stamp, name "h" (1 byte + 3 pad), uid 1, gid 2, one group 3.
        let good = words(&[7, 1, 0x6800_0000, 1, 2, 1, 3]);
        let u = decode_unix(&good).unwrap();
        assert_eq!((u.stamp, u.machine, u.uid, u.gid, u.ngids, u.gids[0]), (7, &b"h"[..], 1, 2, 1, 3));
        for n in 0..good.len() {
            assert!(decode_unix(&good[..n]).is_err(), "cut at {n}");
        }
        assert_eq!(decode_unix(&words(&[7, 256])), Err(DecodeError::AuthTooLong));
        assert_eq!(decode_unix(&words(&[7, 0xffff_ffff])), Err(DecodeError::AuthTooLong));
        assert_eq!(decode_unix(&words(&[7, 0, 1, 2, 17])), Err(DecodeError::AuthTooLong));
        let mut sixteen = vec![7, 0, 1, 2, 16];
        sixteen.extend_from_slice(&[9; 16]);
        assert_eq!(decode_unix(&words(&sixteen)).unwrap().ngids, 16);
        let cred = |flavor, body: &'static [u8]| Auth { flavor, body };
        assert_eq!(authenticate(&cred(AUTH_NONE, b"")), Ok(None));
        assert_eq!(authenticate(&cred(AUTH_UNIX, b"\0\0\0\x07")), Err(AUTH_BADCRED));
        assert_eq!(authenticate(&cred(AUTH_SHORT, b"")), Err(AUTH_REJECTEDCRED));
        assert_eq!(authenticate(&cred(9, b"")), Err(AUTH_REJECTEDCRED));
    }

    #[test]
    fn verdicts() {
        let v = |m: &[u8]| verdict(&SERVED, m).map(|r| r.map(|(c, _)| c.proc_).map_err(|e| e.1));
        assert_eq!(v(&call(2, 0x2000_0001, 3, 5, &NONE2)), Some(Ok(5)));
        assert_eq!(v(&call(2, 0x2000_0001, 2, 5, &NONE2)), Some(Err(Reply::ProgMismatch(1, 3))));
        assert_eq!(v(&call(2, 0x2000_0009, 1, 5, &NONE2)), Some(Err(Reply::ProgUnavail)));
        assert_eq!(v(&call(3, 0x2000_0001, 1, 5, &NONE2)), Some(Err(Reply::RpcMismatch(2, 2))));
        assert_eq!(v(&call(2, 0x2000_0001, 1, 5, &[2, 0, 0, 0])), Some(Err(Reply::AuthError(AUTH_REJECTEDCRED))));
        assert_eq!(v(&call(2, 0x2000_0001, 1, 5, &[1, 4, 0, 0, 0])), Some(Err(Reply::AuthError(AUTH_BADCRED))));
        assert_eq!(v(&call(2, 0x2000_0001, 1, 5, &[0, 0])), None);
        assert_eq!(v(b""), None);
    }

    #[test]
    fn reply_headers() {
        let none = Auth { flavor: 0, body: b"" };
        let mut out = [0u8; 64];
        let n = reply_header(9, &none, Reply::Success, &mut out).unwrap();
        assert_eq!(&out[..n], &words(&[9, 1, 0, 0, 0, 0])[..]);
        let n = reply_header(9, &none, Reply::ProgMismatch(1, 3), &mut out).unwrap();
        assert_eq!(&out[..n], &words(&[9, 1, 0, 0, 0, 2, 1, 3])[..]);
        let n = reply_header(9, &none, Reply::RpcMismatch(2, 2), &mut out).unwrap();
        assert_eq!(&out[..n], &words(&[9, 1, 1, 0, 2, 2])[..]);
        let n = reply_header(9, &none, Reply::AuthError(AUTH_BADCRED), &mut out).unwrap();
        assert_eq!(&out[..n], &words(&[9, 1, 1, 1, 1])[..]);
        let v = Auth { flavor: 1, body: b"abcde" };
        let n = reply_header(9, &v, Reply::GarbageArgs, &mut out).unwrap();
        assert_eq!(&out[..n], &[&words(&[9, 1, 0, 1, 5])[..], b"abcde\0\0\0", &words(&[4])[..]].concat()[..]);
        assert_eq!(reply_header(9, &none, Reply::Success, &mut out[..23]), None);
        let long = [0u8; 401];
        assert_eq!(reply_header(9, &Auth { flavor: 1, body: &long }, Reply::Success, &mut [0u8; 512]), None);
    }

    #[test]
    fn record_marking() {
        assert_eq!(fragment(0x8000_0010), (true, 16));
        assert_eq!(fragment(0x7fff_ffff), (false, 0x7fff_ffff));
        assert_eq!(admit(0, 16, 16), Ok(16));
        assert_eq!(admit(1, 16, 16), Err(RecordError::TooLong));
        assert_eq!(admit(usize::MAX, 1, usize::MAX), Err(RecordError::TooLong));
        assert_eq!(admit(0, 0x7fff_ffff, 1 << 22), Err(RecordError::TooLong));
        assert_eq!(frame(8), Some(0x8000_0008));
        assert_eq!(frame(0x8000_0000), None);
    }

    #[test]
    fn stream_call_headers() {
        // Trace: no spec clause (LG-14 stream transport; RFC 5531 §9 call header, MAX_AUTH_BYTES).
        let read = |m: &[u8]| {
            let mut src = m;
            let mut asked = 0usize;
            let mut buf = [0u8; CALL_HEADER_MAX];
            let n = read_call_header(
                |b| {
                    asked += b.len();
                    let Some((h, t)) = src.split_at_checked(b.len()) else { return false };
                    b.copy_from_slice(h);
                    src = t;
                    true
                },
                &mut buf,
            );
            (n.map(|n| decode_call(&buf[..n]).map(|c| (c.proc_, c.cred.body.len(), c.args))), asked)
        };
        let good = [&call(2, 0x2000_0001, 1, 5, &[1, 5, 0x6162_6364, 0x6500_0000, 0, 0])[..], &words(&[41])].concat();
        assert_eq!(read(&good), (Some(Ok((5, 5, 48))), 48), "header read, arguments left in the stream");
        for n in 0..48 {
            assert_eq!(read(&good[..n]).0, None, "cut at {n}");
        }
        // A hostile length fails before its body is asked for.
        assert_eq!(read(&call(2, 1, 1, 5, &[1, 0xffff_fff0, 0, 0])), (None, 32));
        assert_eq!(read(&call(2, 1, 1, 5, &[1, 401])), (None, 32), "credential over 400 bytes");
        let mut max = call(2, 1, 1, 5, &[]);
        max.truncate(24);
        for _ in 0..2 {
            max.extend_from_slice(&words(&[1, 400]));
            max.extend_from_slice(&[0; 400]);
        }
        assert_eq!(read(&max), (Some(Ok((5, 400, CALL_HEADER_MAX))), CALL_HEADER_MAX), "two 400-byte auths fill the header");
    }

    /// Serves records off a record stream the way a stream transport does (`skiprecord`, call
    /// header, `xdrrec_eof` for the rest), the input handed over at most `chunk` bytes per read.
    /// The xids of the calls decoded (`None`: a record dropped). Checks that the reader is never
    /// asked for more than the stream's fixed buffer and that the buffer never grows.
    fn serve(input: &[u8], chunk: usize) -> Vec<Option<u32>> {
        use crate::sunrpc_rec::RecStream;
        let mut s = RecStream::new(0, 0).unwrap();
        let cap = s.in_bytes().len();
        let mut src = input;
        let mut biggest = 0usize;
        let mut served = Vec::new();
        let ended = core::cell::Cell::new(false);
        let mut reader = |b: &mut [u8]| -> i64 {
            biggest = biggest.max(b.len());
            let n = b.len().min(chunk).min(src.len());
            b[..n].copy_from_slice(&src[..n]);
            src = &src[n..];
            ended.set(n == 0);
            i64::try_from(n).unwrap()
        };
        for _ in 0..64 {
            if !s.skip_record(&mut reader) {
                break;
            }
            let mut hdr = [0u8; CALL_HEADER_MAX];
            let n = read_call_header(|b| s.get(b, &mut reader), &mut hdr);
            if n.is_none() && ended.get() {
                break; // end of input: the connection dies, no record is dropped
            }
            served.push(n.and_then(|n| decode_call(&hdr[..n]).ok()).map(|c| c.xid));
            s.eof(&mut reader);
        }
        assert!(biggest <= cap, "the reader is asked for at most the fixed buffer ({biggest} > {cap})");
        assert_eq!(s.in_bytes().len(), cap, "the record buffer never grows");
        served
    }

    #[test]
    fn stream_record_corpus() {
        // Trace: no spec clause (LG-14 stream transport, owner ruling 2026-10-23: records streamed
        // through xdrrec, no record buffer or cap; RFC 5531 §11 record marking).
        use crate::sunrpc_rec::header;
        let frag = |b: &[u8], last: bool| [&header(u32::try_from(b.len()).unwrap(), last).to_be_bytes()[..], b].concat();
        let rpc = |xid: u32, cred: &[u32]| {
            let mut w = vec![xid, MSG_CALL, 2, 0x2000_0001, 1, 5];
            w.extend_from_slice(cred);
            w.extend_from_slice(&[0, 0, 7]);
            words(&w)
        };
        let g = rpc(1, &[0, 0]);
        let next = frag(&rpc(2, &[0, 0]), true);
        let claim = |len: u32, b: &[u8]| [&header(len, false).to_be_bytes()[..], b].concat();
        let tiny: Vec<u8> = g.iter().enumerate().flat_map(|(i, b)| frag(&[*b], i.checked_add(1) == Some(g.len()))).collect();
        type Case<'a> = (&'a str, Vec<u8>, Vec<Option<u32>>);
        let corpus: [Case<'_>; 8] = [
            ("two good records", [frag(&g, true), next.clone()].concat(), vec![Some(1), Some(2)]),
            ("zero-length fragment inside a record", [frag(&g[..12], false), frag(&[], false), frag(&g[12..], true), next.clone()].concat(), vec![None, Some(2)]),
            ("zero-length record", [frag(&[], true), next.clone()].concat(), vec![None, Some(2)]),
            ("last-fragment flag cutting a call short", [frag(&g[..12], true), frag(&g[12..], true), next.clone()].concat(), vec![None, None, Some(2)]),
            ("one-byte fragments", [tiny, next.clone()].concat(), vec![Some(1), Some(2)]),
            ("a 2 GiB fragment claim that never arrives", claim(0x7fff_ffff, &g), vec![Some(1)]),
            ("credential length near 4 GiB", [frag(&rpc(3, &[1, 0xffff_fff0]), true), next.clone()].concat(), vec![None, Some(2)]),
            ("a record cut short", frag(&g, true)[..30].to_vec(), vec![]),
        ];
        for (what, input, want) in &corpus {
            for chunk in [1, 2, 3, 5, 7, 9, 4096] {
                assert_eq!(&serve(input, chunk), want, "{what} (reads of {chunk} bytes)");
            }
        }
    }

    #[test]
    fn seterr_mapping() {
        use clnt_stat::*;
        assert_eq!(seterr(0, 0, 0, 0, 0), RpcErr::Status(SUCCESS));
        assert_eq!(seterr(0, 2, 1, 3, 0), RpcErr::Versions(PROGVERSMISMATCH, 1, 3));
        assert_eq!(seterr(0, 4, 0, 0, 0), RpcErr::Status(CANTDECODEARGS));
        assert_eq!(seterr(1, 0, 2, 2, 0), RpcErr::Versions(VERSMISMATCH, 2, 2));
        assert_eq!(seterr(1, 1, 0, 0, 5), RpcErr::Why(5));
        assert_eq!(seterr(0, 9, 0, 0, 0), RpcErr::Failed(0, 9));
        assert_eq!(seterr(4, 0, 0, 0, 0), RpcErr::Failed(4, 0));
    }
}
