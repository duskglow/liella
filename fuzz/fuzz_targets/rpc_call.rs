//! ONC RPC calls (spec §29.1; RFC 5531): what a server decodes from the network. The call header,
//! the auth bodies, `AUTH_UNIX` credentials, the server's verdict and the reply header it writes,
//! and the TCP record-marking admission. Input: one received message.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella_legacy::sunrpc_svc::{self as svc, Reply};

fuzz_target!(|m: &[u8]| {
    if let Ok(c) = svc::decode_call(m) {
        // Every decoded piece lies inside the message, and the auth bodies respect the limit.
        assert!(c.args <= m.len(), "args offset inside the message");
        assert!(c.cred.body.len() <= 400 && c.verf.body.len() <= 400, "auth bodies within MAX_AUTH_BYTES");
        if let Ok(Some(u)) = svc::authenticate(&c.cred) {
            assert!(u.machine.len() <= 255 && u.ngids <= 16, "AUTH_UNIX limits");
        }
    }
    let served = [(0x2000_0001u32, 1u32), (0x2000_0001, 3)];
    let mut out = [0u8; 512];
    if let Some(v) = svc::verdict(&served, m) {
        let (xid, reply, verf) = match v {
            Ok((c, _)) => (c.xid, Reply::Success, c.verf),
            Err((xid, r)) => (xid, r, svc::Auth { flavor: 0, body: &[] }),
        };
        let n = svc::reply_header(xid, &verf, reply, &mut out);
        assert!(n.is_some_and(|n| n <= out.len() && n % 4 == 0), "a reply header always fits 512 bytes");
    }
    // Record marking: each word of the input as a fragment header against a 4 MiB limit.
    let mut have = 0usize;
    for w in m.chunks_exact(4) {
        let (_, len) = svc::fragment(u32::from_be_bytes([w[0], w[1], w[2], w[3]]));
        match svc::admit(have, len, 1 << 22) {
            Ok(t) => {
                assert!(t <= 1 << 22 && t >= have, "admitted record within the limit");
                have = t;
            }
            Err(_) => break,
        }
    }
});
