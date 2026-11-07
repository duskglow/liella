//! §29.1 what the ONC RPC stubs (D-29.5) keep: the `clnt_*err` printer texts and the order
//! `bindresvport` tries the reserved ports (D-15.7: unchanged). Moved here from the removed
//! legacy RPC client (LEGACY-RPC-CRATE-REMOVAL); texts byte for byte as black-box probes of the
//! host glibc print them (RPC-CLIENT, 2026-10-23).

/// `RPC_CANTSEND`.
const CANTSEND: i32 = 3;
/// `RPC_CANTRECV`.
const CANTRECV: i32 = 4;
/// `RPC_VERSMISMATCH`.
const VERSMISMATCH: i32 = 6;
/// `RPC_AUTHERROR`.
const AUTHERROR: i32 = 7;
/// `RPC_PROGVERSMISMATCH`.
const PROGVERSMISMATCH: i32 = 9;
/// `RPC_SYSTEMERROR` (a create error carrying an errno).
const SYSTEMERROR: i32 = 12;
/// `RPC_PMAPFAILURE` (a create error carrying the port mapper call's status).
const PMAPFAILURE: i32 = 14;
/// The first reserved port `bindresvport` tries.
const RESV_LOW: u16 = 512;
/// The last reserved port `bindresvport` tries.
const RESV_HIGH: u16 = 1023;

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
const fn auth_why(why: u64) -> Option<&'static str> {
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
    fn num(&mut self, mut v: u64) {
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
        AUTHERROR => {
            t.put(b"; why = ");
            if let Some(w) = auth_why(a) {
                t.put(w.as_bytes());
            } else {
                t.put(b"(unknown authentication error - ");
                t.num(a);
                t.put(b")");
            }
        }
        VERSMISMATCH | PROGVERSMISMATCH => {
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

    fn text(f: impl FnOnce(&mut Text<'_>)) -> String {
        let mut b = [0u8; 256];
        let mut t = Text::new(&mut b);
        f(&mut t);
        let n = t.finish();
        String::from_utf8(b[..n].to_vec()).unwrap()
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
