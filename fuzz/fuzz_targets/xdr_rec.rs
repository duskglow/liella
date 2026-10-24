//! XDR record marking (`xdrrec_*`, spec §29.1, RFC 5531 §11) through the safe stream state
//! `liella_legacy::sunrpc_rec::RecStream`, with the `readit`/`writeit` callbacks as closures over
//! memory that hand over data in chunks of a fuzzed size.
//!
//! Invariants, not only "no crash":
//! * round trip: records written with `put` (in fuzzed pieces) and `end_of_record` read back
//!   byte for byte after `skip_record`, whatever the buffer sizes and the reader's chunking,
//!   and the stream is at its end afterwards;
//! * untrusted input: every `get` that succeeds returns exactly the next bytes of the current
//!   record as a slow reference model reads the fragment headers (length, last bit), and never
//!   reads past the record.
//!
//! Input: a mode byte (bit 0: round trip), the send and receive sizes (`u16` each), the
//! reader's chunk size byte, then for the round trip records as length-prefixed parts; for
//! untrusted input, an op list up to a NUL (0: skip_record, n: get of n bytes) and the wire.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use libfuzzer_sys::fuzz_target;
use liella_fuzz::Input;
use liella_legacy::sunrpc_rec::RecStream;

/// The records of `wire` as the reference model reads them: each the payload of its fragments
/// up to one with the last bit set (a final unfinished record is kept as far as it goes).
fn model(wire: &[u8]) -> Vec<Vec<u8>> {
    let mut records = Vec::new();
    let mut cur = Vec::new();
    let mut at = 0;
    while at + 4 <= wire.len() {
        let h = u32::from_be_bytes([wire[at], wire[at + 1], wire[at + 2], wire[at + 3]]);
        at += 4;
        let n = (h & 0x7fff_ffff) as usize;
        let end = at.saturating_add(n).min(wire.len());
        cur.extend_from_slice(&wire[at..end]);
        at = end;
        if h & 0x8000_0000 != 0 {
            records.push(core::mem::take(&mut cur));
        }
    }
    records.push(cur);
    records
}

fn reader<'a>(wire: &'a [u8], chunk: usize, at: &'a mut usize) -> impl FnMut(&mut [u8]) -> i64 + 'a {
    move |buf: &mut [u8]| {
        let n = buf.len().min(chunk).min(wire.len() - *at);
        buf[..n].copy_from_slice(&wire[*at..*at + n]);
        *at += n;
        n as i64
    }
}

fuzz_target!(|data: &[u8]| {
    let mut input = Input::new(data);
    let mode = input.byte();
    let send = u32::from(input.u16());
    let recv = u32::from(input.u16());
    let chunk = usize::from(input.byte()).max(1);

    if mode & 1 != 0 {
        // Round trip.
        let Some(mut out) = RecStream::new(send, recv) else { return };
        let mut wire: Vec<u8> = Vec::new();
        let mut records: Vec<&[u8]> = Vec::new();
        let mut total = 0;
        while !input.is_empty() && records.len() < 32 {
            let piece = usize::from(input.byte()).max(1);
            let r = input.sized();
            total += r.len();
            if total > 1 << 16 {
                return;
            }
            let mut w = |b: &[u8]| {
                wire.extend_from_slice(b);
                b.len() as i64
            };
            for p in r.chunks(piece) {
                assert!(out.put(p, &mut w), "put failed");
            }
            assert!(out.end_of_record(true, &mut w), "end_of_record failed");
            records.push(r);
        }
        let Some(mut inp) = RecStream::new(send, recv) else { return };
        let mut at = 0;
        let mut r = reader(&wire, chunk, &mut at);
        for (k, rec) in records.iter().enumerate() {
            assert!(inp.skip_record(&mut r), "skip_record before record {k} failed");
            let mut buf = vec![0u8; rec.len()];
            assert!(inp.get(&mut buf, &mut r), "record {k} ({} bytes) does not read back", rec.len());
            assert_eq!(&buf[..], *rec, "record {k} reads back different bytes");
            let mut one = [0u8; 1];
            assert!(!inp.get(&mut one, &mut r), "record {k} reads past its end");
        }
        assert!(inp.eof(&mut r), "not at the end after the last record");
        return;
    }

    // Untrusted input against the model.
    let ops = input.part();
    let wire = input.rest();
    if ops.len() > 64 || wire.len() > 1 << 16 {
        return;
    }
    let records = model(wire);
    let Some(mut inp) = RecStream::new(send, recv) else { return };
    let mut at = 0;
    let mut r = reader(wire, chunk, &mut at);
    let mut rec: Option<usize> = None;
    let mut off = 0;
    for &op in ops {
        if op == 0 {
            if !inp.skip_record(&mut r) {
                return;
            }
            rec = Some(rec.map_or(0, |k| k + 1));
            off = 0;
            continue;
        }
        let mut buf = vec![0u8; usize::from(op)];
        if !inp.get(&mut buf, &mut r) {
            return;
        }
        let k = rec.expect("a get succeeded before the first skip_record");
        let want = records.get(k).expect("a get succeeded past the last record");
        assert!(off + buf.len() <= want.len(), "get read past record {k}: {} bytes at {off} of {}", buf.len(), want.len());
        assert_eq!(&buf[..], &want[off..off + buf.len()], "get returned other bytes than record {k} holds at {off}");
        off += buf.len();
    }
});
