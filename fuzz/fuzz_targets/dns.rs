//! DNS responses (spec §16.4; RFC 1035): what the stub resolver reads from a server. The
//! message parser (`ns_initparse`, `ns_parserr`), name decompression (`dn_expand`,
//! `dn_skipname`), the reply check against the query, the host answer reader of the `dns`
//! source (`A`, `AAAA`, `PTR`, `CNAME` chains), the TTL, and the text form (`ns_sprintrr`,
//! `res_nquery`'s printing). Input: the response message.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella::mem::TryVec;
use liella::names::dns::{client, host, msg, name, text, CLASS_IN, T_A, T_AAAA, T_PTR};

fuzz_target!(|m: &[u8]| {
    // The name asked: the question's name when there is one.
    let mut wire = [0u8; name::MAXDNAME];
    let mut text_buf = [0u8; name::MAXDNAME];
    let asked_text: &[u8] = match name::unpack(m, msg::HFIXEDSZ, &mut wire) {
        Ok((n, _)) => match name::expand_text(wire.get(..n).unwrap_or(&[]), &mut text_buf) {
            Ok(t) => text_buf.get(..t.saturating_sub(1)).unwrap_or(&[]),
            Err(_) => b"example.com",
        },
        Err(_) => b"example.com",
    };

    for want in [T_A, T_AAAA, T_PTR] {
        if let Ok(host::Outcome::Found(f)) = host::read(m, asked_text, want) {
            // A host found by address type has an address of that type.
            if want != T_PTR {
                let len = if want == T_A { 4 } else { 16 };
                assert!(!f.addrs.is_empty() && f.addrs.iter().all(|a| a.bytes().len() == len), "found host has addresses of the type");
            }
        }
        let mut q = [0u8; 512];
        if let Ok(n) = msg::mkquery(0, asked_text, CLASS_IN, want, None, &mut q, 0x1234, true) {
            let _ = client::matches(&q[..n], m);
            let _ = msg::queries_match(&q[..n], m);
            let _ = msg::name_in_query(asked_text, want, CLASS_IN, m);
        }
    }
    let _ = host::ttl(m);
    let _ = client::summary(m);
    let _ = msg::Header::of(m);
    let _ = name::skip(m);

    if let Ok(p) = msg::parse(m) {
        for section in 0..4 {
            for index in 0..64 {
                let Ok(rr) = msg::rr(m, &p, section, index) else { break };
                let mut owner = [0u8; name::MAXDNAME];
                let Ok(n) = name::ntop(rr.owner(), &mut owner) else { continue };
                let rec = text::Record {
                    owner: owner.get(..n.saturating_sub(1)).unwrap_or(&[]),
                    class: rr.class,
                    rtype: rr.rtype,
                    ttl: u64::from(rr.ttl),
                    rdata: rr.data(m),
                    rdata_at: rr.rdata,
                };
                let mut out = TryVec::new();
                let _ = text::sprintrr(m, &rec, None, None, &mut out);
                let mut out = TryVec::new();
                let _ = text::sprintrr(m, &rec, Some(b"example.com"), Some(b"com"), &mut out);
            }
        }
    }
    let mut out = TryVec::new();
    let _ = text::nquery(m, u64::MAX, &mut out);
    // Decompression at every offset (dn_expand on a pointer into the message).
    for start in 0..m.len().min(64) {
        let mut buf = [0u8; name::MAXDNAME];
        let _ = name::unpack(m, start, &mut buf);
    }
});
