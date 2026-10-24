//! The XDR record-marking stream (`xdrrec_*`, spec §29.1, RFC 5531 §11) as safe state over two
//! byte buffers; the C layer (`liella_boundary::posix::s29_xdr_streams`) supplies the caller's
//! `readit`/`writeit` callbacks as closures. Settled black-box (`spec/behaviour/xdr_streams.glibc.txt`):
//! buffers below 100 bytes become 4000, others round up to a unit; each flush is one `writeit`
//! call; a decode refill keeps the unread bytes at the front and asks for the rest of the buffer;
//! a read before the first `skiprecord` fails; `eof` consumes the rest of the current record.
//! A read callback that returns 0 (or less) ends the record with an error: the decode, `skiprecord`
//! or `eof` in progress fails (`eof` reports true) and the callback is not called again for it.
//!
//! Both buffers are allocated once at their full size (S8); `out_len`, `head`, `finger` and
//! `fill` index into them with the invariants `head + 4 <= out_len <= out.len()` and
//! `finger <= fill <= inb.len()`, so every slice is taken with `get`/`get_mut` and every
//! offset update saturates (it cannot overflow under the invariants).

use alloc::vec::Vec;

/// The last-fragment bit of a record-marking header.
pub const LAST_FRAG: u32 = 0x8000_0000;

/// The size of a fragment header.
const HDR: usize = 4;

/// The buffer size `xdrrec_create` uses for a requested size.
#[must_use]
pub fn buffer_size(requested: u32) -> usize {
    if requested < 100 {
        4000
    } else {
        // 100..=u32::MAX rounds up to a unit within usize (x86_64).
        usize::try_from(requested).ok().and_then(|n| n.div_ceil(4).checked_mul(4)).unwrap_or(usize::MAX)
    }
}

/// A fragment header word: `len` bytes, `last` sets the top bit.
#[must_use]
pub const fn header(len: u32, last: bool) -> u32 {
    if last { len | LAST_FRAG } else { len & !LAST_FRAG }
}

/// A zero-filled buffer of `n` bytes, or `None` when it cannot be had.
fn zeroed(n: usize) -> Option<Vec<u8>> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).ok()?;
    // Capacity was reserved above, so this cannot allocate (S8).
    #[allow(clippy::disallowed_methods)]
    v.resize(n, 0);
    Some(v)
}

/// The record stream's state (both directions; a stream uses one).
#[derive(Debug)]
pub struct RecStream {
    /// The encode buffer at its full size (headers included).
    out: Vec<u8>,
    /// Bytes of `out` in use.
    out_len: usize,
    /// Start of the current fragment's header in `out`.
    head: usize,
    inb: Vec<u8>,
    /// Unread bytes are `inb[finger..fill]`.
    finger: usize,
    fill: usize,
    /// Bytes left in the current input fragment.
    remaining: u32,
    last: bool,
}

/// `readit`: fills the slice, returns the byte count (`<= 0`: end of input; the record ends with an error).
pub type Reader<'a> = &'a mut dyn FnMut(&mut [u8]) -> i64;
/// `writeit`: writes the slice, returns the byte count.
pub type Writer<'a> = &'a mut dyn FnMut(&[u8]) -> i64;

impl RecStream {
    /// The stream for `xdrrec_create(sendsize, recvsize)`; `None` when the buffers cannot be had.
    #[must_use]
    pub fn new(sendsize: u32, recvsize: u32) -> Option<Self> {
        let out = zeroed(buffer_size(sendsize))?;
        let inb = zeroed(buffer_size(recvsize))?;
        // Encode: 4 bytes held for the first header. Decode: no fragment yet and `last` set, so a
        // read before `skiprecord` fails.
        Some(Self { out, out_len: HDR, head: 0, inb, finger: 0, fill: 0, remaining: 0, last: true })
    }

    fn out_room(&self) -> usize {
        self.out.len().saturating_sub(self.out_len)
    }

    /// Writes the current fragment's header word.
    fn seal(&mut self, last: bool) {
        let len = self.out_len.saturating_sub(self.head).saturating_sub(HDR);
        let word = header(u32::try_from(len).unwrap_or(0), last).to_be_bytes();
        if let Some(h) = self.out.get_mut(self.head..self.head.saturating_add(HDR)) {
            h.copy_from_slice(&word);
        }
    }

    /// Writes the buffer as one `writeit` call, the current fragment marked `last` or not.
    fn flush(&mut self, last: bool, w: Writer<'_>) -> bool {
        self.seal(last);
        let data = self.out.get(..self.out_len).unwrap_or_default();
        let ok = i64::try_from(data.len()).is_ok_and(|n| w(data) == n);
        self.out_len = HDR;
        self.head = 0;
        ok
    }

    /// Encode: appends bytes, flushing full buffers as non-last fragments.
    pub fn put(&mut self, mut data: &[u8], w: Writer<'_>) -> bool {
        while !data.is_empty() {
            let room = self.out_room();
            if room == 0 {
                if !self.flush(false, w) {
                    return false;
                }
                continue;
            }
            let Some((now, rest)) = data.split_at_checked(room.min(data.len())) else { return false };
            let end = self.out_len.saturating_add(now.len());
            let Some(dst) = self.out.get_mut(self.out_len..end) else { return false };
            dst.copy_from_slice(now);
            self.out_len = end;
            data = rest;
        }
        true
    }

    /// Encode `x_inline`: room for `len` bytes in the buffer, claimed; `None` when it does not fit.
    pub fn put_inline(&mut self, len: usize) -> Option<core::ops::Range<usize>> {
        let at = self.out_len;
        let end = at.checked_add(len).filter(|e| *e <= self.out.len())?;
        self.out_len = end;
        Some(at..end)
    }

    /// The encode buffer (for an inline range).
    pub fn out_bytes(&mut self) -> &mut [u8] {
        &mut self.out
    }

    /// `xdrrec_endofrecord`: ends the record; sends it now when asked or when no header fits.
    pub fn end_of_record(&mut self, send_now: bool, w: Writer<'_>) -> bool {
        if send_now || self.out_room() < HDR {
            return self.flush(true, w);
        }
        self.seal(true);
        self.head = self.out_len;
        self.out_len = self.out_len.saturating_add(HDR);
        true
    }

    /// Bytes held for the next write (headers included).
    #[must_use]
    pub const fn out_pending(&self) -> usize {
        self.out_len
    }

    /// Bytes read from the reader and not yet consumed.
    #[must_use]
    pub const fn in_pending(&self) -> usize {
        self.fill.saturating_sub(self.finger)
    }

    /// Consumes `n` buffered bytes of the current fragment.
    fn consume(&mut self, n: usize) {
        self.finger = self.finger.saturating_add(n);
        self.remaining = self.remaining.saturating_sub(u32::try_from(n).unwrap_or(u32::MAX));
    }

    /// One refill: the unread bytes move to the front, the reader gets the rest of the buffer.
    fn refill(&mut self, r: Reader<'_>) -> bool {
        let pending = self.in_pending();
        if self.finger > 0 {
            let Some(end) = self.finger.checked_add(pending).filter(|e| *e <= self.inb.len()) else { return false };
            self.inb.copy_within(self.finger..end, 0);
        }
        self.fill = pending;
        self.finger = 0;
        let Some(dst) = self.inb.get_mut(pending..) else { return false };
        let want = dst.len();
        let got = r(dst);
        // A count of 0 is end of input: the record fails, nothing retries; more than asked is a broken reader.
        match usize::try_from(got) {
            Ok(n) if n > 0 && n <= want => {
                self.fill = pending.saturating_add(n);
                true
            }
            _ => false,
        }
    }

    /// Makes `n` (<= the buffer size) unread bytes contiguous.
    fn ensure(&mut self, n: usize, r: Reader<'_>) -> bool {
        while self.in_pending() < n {
            if !self.refill(r) {
                return false;
            }
        }
        true
    }

    /// Reads the next fragment header.
    fn next_header(&mut self, r: Reader<'_>) -> bool {
        if !self.ensure(HDR, r) {
            return false;
        }
        let end = self.finger.saturating_add(HDR);
        let Some(word) = self.inb.get(self.finger..end).and_then(|s| <[u8; 4]>::try_from(s).ok()) else {
            return false;
        };
        self.finger = end;
        let h = u32::from_be_bytes(word);
        self.remaining = h & !LAST_FRAG;
        self.last = h & LAST_FRAG != 0;
        true
    }

    /// Moves to a fragment with data: fails at the record's end, or at an empty fragment.
    fn want_data(&mut self, r: Reader<'_>) -> bool {
        if self.remaining > 0 {
            return true;
        }
        !self.last && self.next_header(r) && self.remaining > 0
    }

    /// Decode: fills `buf` from the record, across fragments; a word within one fragment is
    /// made contiguous first (the reader sees the rest of the buffer asked for).
    pub fn get(&mut self, buf: &mut [u8], r: Reader<'_>) -> bool {
        let mut done = 0;
        while done < buf.len() {
            if !self.want_data(r) {
                return false;
            }
            let frag = usize::try_from(self.remaining).unwrap_or(usize::MAX);
            let need = buf.len().saturating_sub(done).min(frag);
            let have = if need <= HDR { self.ensure(need, r) } else { self.in_pending() > 0 || self.refill(r) };
            if !have {
                return false;
            }
            let n = need.min(self.in_pending());
            let to = done.saturating_add(n);
            let (Some(dst), Some(src)) =
                (buf.get_mut(done..to), self.inb.get(self.finger..self.finger.saturating_add(n)))
            else {
                return false;
            };
            dst.copy_from_slice(src);
            self.consume(n);
            done = to;
        }
        true
    }

    /// Decode `x_inline`: `len` buffered bytes of the current fragment, consumed; `None` when
    /// they are not both in the fragment and in the buffer.
    pub fn get_inline(&mut self, len: usize) -> Option<core::ops::Range<usize>> {
        let ok = usize::try_from(self.remaining).is_ok_and(|f| f >= len) && self.in_pending() >= len;
        ok.then(|| {
            let at = self.finger;
            self.consume(len);
            at..self.finger
        })
    }

    /// The decode buffer (for an inline range).
    pub fn in_bytes(&mut self) -> &mut [u8] {
        &mut self.inb
    }

    /// Skips the rest of the current record (and of a record begun by `skiprecord`).
    fn skip_record_body(&mut self, r: Reader<'_>) -> bool {
        while self.remaining > 0 || !self.last {
            while self.remaining > 0 {
                if self.in_pending() == 0 && !self.refill(r) {
                    return false;
                }
                let n = self.in_pending().min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
                self.consume(n);
            }
            if !self.last && !self.next_header(r) {
                return false;
            }
        }
        true
    }

    /// `xdrrec_skiprecord`: skips the rest of the record; the next read starts a new one.
    pub fn skip_record(&mut self, r: Reader<'_>) -> bool {
        if !self.skip_record_body(r) {
            return false;
        }
        self.last = false;
        true
    }

    /// `xdrrec_eof`: consumes the rest of the current record; `true` when nothing more is
    /// buffered (or the input failed). It does not read ahead.
    pub fn eof(&mut self, r: Reader<'_>) -> bool {
        !self.skip_record_body(r) || self.in_pending() == 0
    }
}

#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: Vec<u8> = s.bytes().filter(|b| *b != b' ').collect();
        s.chunks(2).map(|p| u8::from_str_radix(core::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
    }

    /// A reader serving `data` in chunks, logging (asked, got).
    fn reader(data: Vec<u8>, chunk: usize, log: &mut Vec<(usize, i64)>) -> impl FnMut(&mut [u8]) -> i64 + '_ {
        let mut off = 0;
        move |buf: &mut [u8]| {
            let n = (data.len() - off).min(buf.len()).min(chunk);
            if n == 0 {
                log.push((buf.len(), -1));
                return -1;
            }
            buf[..n].copy_from_slice(&data[off..off + n]);
            off += n;
            let got = i64::try_from(n).unwrap();
            log.push((buf.len(), got));
            got
        }
    }

    fn int(s: &mut RecStream, r: Reader<'_>) -> Option<i32> {
        let mut b = [0; 4];
        s.get(&mut b, r).then(|| i32::from_be_bytes(b))
    }

    #[test]
    fn buffer_sizes() {
        // enc send=0/1/24/50 -> W4000; 100 -> W100; 101, 104 -> W104; 5000 -> one 4404 write.
        assert_eq!([0, 1, 24, 50, 99, 100, 101, 104, 200, 4000, 5000].map(buffer_size), [4000, 4000, 4000, 4000, 4000, 100, 104, 104, 200, 4000, 5000]);
    }

    #[test]
    fn encode_split_and_records() {
        let writes = core::cell::RefCell::new(Vec::<Vec<u8>>::new());
        let mut w = |b: &[u8]| {
            writes.borrow_mut().push(b.to_vec());
            i64::try_from(b.len()).unwrap()
        };
        let mut s = RecStream::new(0, 0).unwrap();
        for k in 0..1100u32 {
            assert!(s.put(&k.to_be_bytes(), &mut w));
        }
        assert!(s.end_of_record(true, &mut w));
        // [W4000:00000f9c00000000] then [W408:80000194000003e7].
        assert_eq!((writes.borrow()[0].len(), &writes.borrow()[0][..8]), (4000, &hex("00000f9c 00000000")[..]));
        assert_eq!((writes.borrow()[1].len(), &writes.borrow()[1][..8]), (408, &hex("80000194 000003e7")[..]));
        writes.borrow_mut().clear();
        // two records send=8: one write of both; endofrecord(FALSE) writes nothing.
        let mut s = RecStream::new(8, 0).unwrap();
        assert!(s.put(&7u32.to_be_bytes(), &mut w) && s.end_of_record(false, &mut w));
        assert!(s.put(&8u32.to_be_bytes(), &mut w) && s.end_of_record(true, &mut w));
        assert_eq!(*writes.borrow(), [hex("80000004 00000007 80000004 00000008")]);
        writes.borrow_mut().clear();
        let mut s = RecStream::new(100, 0).unwrap();
        assert!(s.end_of_record(true, &mut w));
        assert_eq!(*writes.borrow(), [hex("80000000")]);
        // writefail: the write happens, endofrecord fails.
        let mut s = RecStream::new(0, 0).unwrap();
        let mut bad = |_: &[u8]| -1;
        assert!(s.put(&7u32.to_be_bytes(), &mut bad) && !s.end_of_record(true, &mut bad));
    }

    #[test]
    fn decode_reader_requests_chunk3() {
        // dec one chunk=3: [R4000->3] [R3997->3] [R3998->3] int=1 [R3999->3] int=2 eof=1 skip=1
        // [R4000->3] [R3997->3] [R3998->2] next=1/63 eof=1.
        let mut log = Vec::new();
        let mut r = reader(hex("80000008 00000001 00000002 80000004 00000063"), 3, &mut log);
        let mut s = RecStream::new(0, 0).unwrap();
        assert!(s.skip_record(&mut r));
        assert_eq!((int(&mut s, &mut r), int(&mut s, &mut r)), (Some(1), Some(2)));
        assert!(s.eof(&mut r));
        assert!(s.skip_record(&mut r));
        assert_eq!(int(&mut s, &mut r), Some(0x63));
        assert!(s.eof(&mut r));
        drop(r);
        assert_eq!(log, [(4000, 3), (3997, 3), (3998, 3), (3999, 3), (4000, 3), (3997, 3), (3998, 2)]);
    }

    /// The `rec dec` lines: skip0, the ints, then skiprecord before eof (as the record's argument
    /// order ran them), the next int, eof.
    fn case(data: &str, chunk: usize, nints: usize) -> (bool, Vec<Option<i32>>, bool, bool, Option<i32>, bool) {
        let mut log = Vec::new();
        let mut r = reader(hex(data), chunk, &mut log);
        let mut s = RecStream::new(0, 0).unwrap();
        let skip0 = s.skip_record(&mut r);
        let ints = (0..nints).map(|_| int(&mut s, &mut r)).collect();
        let skip = s.skip_record(&mut r);
        let eof = s.eof(&mut r);
        (skip0, ints, eof, skip, int(&mut s, &mut r), s.eof(&mut r))
    }

    #[test]
    fn decode_record_lines() {
        let one = "80000008 00000001 00000002 80000004 00000063";
        assert_eq!(case(one, 64, 2), (true, vec![Some(1), Some(2)], true, true, None, true));
        assert_eq!(case(one, 3, 2), (true, vec![Some(1), Some(2)], true, true, None, true));
        assert_eq!(case("00000004 00000001 80000004 00000002 80000004 00000063", 64, 2).1, vec![Some(1), Some(2)]);
        assert_eq!(case("80000004 00000001 80000004 00000063", 64, 2).1, vec![Some(1), None]);
        assert_eq!(case("80000000 80000004 00000063", 64, 1), (true, vec![None], true, true, None, true));
        assert_eq!(case("00000000 80000004 00000001 80000004 00000063", 64, 1), (true, vec![None], true, true, None, true));
        assert_eq!(case("80000008 00000001", 64, 2), (true, vec![Some(1), None], true, false, None, true));
        assert_eq!(case("", 64, 1), (true, vec![None], true, false, None, true));
        assert_eq!(case("8000", 64, 1), (true, vec![None], true, false, None, true));
    }

    #[test]
    fn decode_eof_order_and_fresh_stream() {
        // dec one chunk=64: eof=0 while the next record is buffered, then skip, next=1/63, eof=1.
        let mut log = Vec::new();
        let mut r = reader(hex("80000008 00000001 00000002 80000004 00000063"), 64, &mut log);
        let mut s = RecStream::new(0, 8).unwrap();
        assert!(s.skip_record(&mut r));
        assert_eq!((int(&mut s, &mut r), int(&mut s, &mut r)), (Some(1), Some(2)));
        assert!(!s.eof(&mut r));
        assert!(s.skip_record(&mut r));
        assert_eq!(int(&mut s, &mut r), Some(0x63));
        assert!(s.eof(&mut r));
        drop(r);
        assert_eq!(log, [(4000, 20)]);
        // eof-first / int-first: eof=1, a read before skiprecord fails without calling the reader.
        let mut log = Vec::new();
        let mut r = reader(hex("80000004 00000001"), 64, &mut log);
        let mut s = RecStream::new(0, 0).unwrap();
        assert!(s.eof(&mut r));
        assert_eq!(int(&mut s, &mut r), None);
        assert!(s.skip_record(&mut r));
        assert_eq!(int(&mut s, &mut r), Some(1));
        drop(r);
        assert_eq!(log, [(4000, 8)]);
    }

    #[test]
    fn decode_zero_read_is_end_of_input() {
        let mut s = RecStream::new(0, 0).unwrap();
        let mut n = 0;
        let mut zero = |_: &mut [u8]| {
            n += 1;
            assert!(n <= 8, "the reader was retried after returning 0");
            0
        };
        assert!(s.skip_record(&mut zero));
        assert_eq!(int(&mut s, &mut zero), None);
        assert!(!s.skip_record(&mut zero));
    }

    /// A reader that returns 0 once `data` is used up and counts its calls; more than `cap` calls
    /// panic, so a retry loop ends the test instead of hanging it. Spec: no spec clause (XDR-READ-EOF).
    fn bounded(data: Vec<u8>, calls: &core::cell::Cell<usize>) -> impl FnMut(&mut [u8]) -> i64 + '_ {
        let mut off = 0;
        move |buf: &mut [u8]| {
            calls.set(calls.get() + 1);
            assert!(calls.get() <= 8, "the reader was retried after returning 0");
            let n = (data.len() - off).min(buf.len());
            buf[..n].copy_from_slice(&data[off..off + n]);
            off += n;
            i64::try_from(n).unwrap()
        }
    }

    #[test]
    fn zero_read_ends_record_start_header_and_data() {
        // no spec clause (XDR-READ-EOF): at the start of a record.
        let calls = core::cell::Cell::new(0);
        let mut r = bounded(vec![], &calls);
        let mut s = RecStream::new(0, 0).unwrap();
        assert!(s.skip_record(&mut r));
        assert_eq!(int(&mut s, &mut r), None);
        assert_eq!(calls.get(), 1, "the header read asked once");
        assert!(!s.skip_record(&mut r));
        assert_eq!(calls.get(), 2);
        assert!(s.eof(&mut r));
        drop(s);
        // inside a fragment header (2 of 4 bytes).
        let calls = core::cell::Cell::new(0);
        let mut r = bounded(hex("8000"), &calls);
        let mut s = RecStream::new(0, 0).unwrap();
        assert!(s.skip_record(&mut r));
        assert_eq!(int(&mut s, &mut r), None);
        assert_eq!(calls.get(), 2);
        assert!(s.eof(&mut r));
        assert!(!s.skip_record(&mut r));
        drop(s);
        // inside fragment data (header says 8, 4 present), via get, then via skiprecord and eof.
        for how in 0..3 {
            let calls = core::cell::Cell::new(0);
            let mut r = bounded(hex("80000008 00000001"), &calls);
            let mut s = RecStream::new(0, 0).unwrap();
            assert!(s.skip_record(&mut r));
            match how {
                0 => assert_eq!((int(&mut s, &mut r), int(&mut s, &mut r)), (Some(1), None)),
                1 => assert!(!s.skip_record(&mut r)),
                _ => assert!(s.eof(&mut r)),
            }
            assert!(calls.get() <= 2);
            assert_eq!(int(&mut s, &mut r), None, "the failed record stays failed");
            drop(s);
        }
    }

    #[test]
    fn decode_opaque_over_fragments_and_inline() {
        let mut log = Vec::new();
        let mut r = reader(hex("00000003 616263 80000003 646566 80000004 00000063"), 64, &mut log);
        let mut s = RecStream::new(0, 0).unwrap();
        assert!(s.skip_record(&mut r));
        let mut b = [0; 6];
        assert!(s.get(&mut b, &mut r));
        assert_eq!(&b, b"abcdef");
        assert!(!s.get(&mut [0; 2], &mut r), "the padding is past the record");
        drop(r);
        let mut log = Vec::new();
        let mut r = reader(hex("80000010 00000001 00000002 00000003 00000004"), 64, &mut log);
        let mut s = RecStream::new(0, 0).unwrap();
        assert!(s.skip_record(&mut r));
        assert_eq!(int(&mut s, &mut r), Some(1));
        let at = s.get_inline(8).unwrap();
        assert_eq!(s.in_bytes()[at].to_vec(), hex("00000002 00000003"));
        assert_eq!(s.get_inline(8), None);
        assert_eq!(s.put_inline(8).map(|r| r.len()), Some(8));
    }
}
