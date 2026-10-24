//! ONC RPC over a stream (spec §29.1; RFC 5531 §11 record marking): what a TCP server decodes
//! from a connection. Records are streamed through the xdrrec record stream (owner ruling
//! 2026-10-23: no whole-record buffer, no cap). Input: the first byte picks the read size (so
//! reads interleave with fragment and word edges), the rest is the byte stream from the peer.
//! Checks: no panic, no hang, the reader is never asked for more than the fixed buffer, the
//! buffer never grows, and every decoded call header lies inside `CALL_HEADER_MAX`.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella_legacy::sunrpc_rec::RecStream;
use liella_legacy::sunrpc_svc as svc;

fuzz_target!(|data: &[u8]| {
    let Some((&first, mut src)) = data.split_first() else { return };
    let chunk = if first == 0 { 1 << 16 } else { usize::from(first % 16) + 1 };
    let Some(mut s) = RecStream::new(0, 0) else { return };
    let cap = s.in_bytes().len();
    let mut biggest = 0usize;
    let mut reader = |b: &mut [u8]| -> i64 {
        biggest = biggest.max(b.len());
        let n = b.len().min(chunk).min(src.len());
        b[..n].copy_from_slice(&src[..n]);
        src = &src[n..];
        i64::try_from(n).unwrap_or(-1)
    };
    // Each record costs at least its 4-byte header, so the input bounds the loop.
    for _ in 0..=data.len() {
        if !s.skip_record(&mut reader) {
            break;
        }
        let mut hdr = [0u8; svc::CALL_HEADER_MAX];
        if let Some(n) = svc::read_call_header(|b| s.get(b, &mut reader), &mut hdr) {
            assert!(n <= svc::CALL_HEADER_MAX && n % 4 == 0, "call header within its bound");
            if let Ok(c) = svc::decode_call(&hdr[..n]) {
                assert!(c.args == n, "the arguments start where the header ends");
                // A few argument words, as a dispatch routine's xdr_int would read them.
                let mut w = [0u8; 4];
                for _ in 0..8 {
                    if !s.get(&mut w, &mut reader) {
                        break;
                    }
                }
            }
        }
        // xdr_stat: the rest of the record is consumed, nothing is read ahead.
        s.eof(&mut reader);
    }
    assert!(biggest <= cap, "the reader is asked for at most the fixed buffer");
    assert_eq!(s.in_bytes().len(), cap, "the record buffer never grows");
});
