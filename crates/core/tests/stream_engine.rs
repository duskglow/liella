//! The buffering engine (`liella::stream`, D-12.1) over an in-memory backing that records every
//! call, so that the policy (when bytes reach the backing) is checked exactly.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

use liella::error::Errno;
use liella::stream::{Backing, Policy, Role, State, Whence, RESERVE};

/// A file in memory; records the size of every write and read, and can fail on demand.
#[derive(Default)]
struct Mem {
    data: Vec<u8>,
    pos: usize,
    writes: Vec<usize>,
    reads: usize,
    fail_write: Option<Errno>,
    seekable: bool,
    max_write: Option<usize>,
    fail_read: Option<Errno>,
}

impl Mem {
    fn file(data: &[u8]) -> Mem {
        Mem { data: data.to_vec(), seekable: true, ..Mem::default() }
    }
}

impl Backing for Mem {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Errno> {
        self.reads += 1;
        if let (Some(e), true) = (self.fail_read, self.pos >= self.data.len()) {
            return Err(e);
        }
        let n = buf.len().min(self.data.len().saturating_sub(self.pos));
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
    fn write(&mut self, buf: &[u8]) -> Result<usize, Errno> {
        if let Some(e) = self.fail_write {
            return Err(e);
        }
        let n = self.max_write.map_or(buf.len(), |m| m.min(buf.len()));
        if self.pos + n > self.data.len() {
            self.data.resize(self.pos + n, 0);
        }
        self.data[self.pos..self.pos + n].copy_from_slice(&buf[..n]);
        self.pos += n;
        self.writes.push(n);
        Ok(n)
    }
    fn seek(&mut self, offset: i64, whence: Whence) -> Result<u64, Errno> {
        if !self.seekable {
            return Err(Errno::ESPIPE);
        }
        let base = match whence {
            Whence::Set => 0,
            Whence::Cur => self.pos as i64,
            Whence::End => self.data.len() as i64,
        };
        let p = base + offset;
        if p < 0 {
            return Err(Errno::EINVAL);
        }
        self.pos = p as usize;
        Ok(p as u64)
    }
}

fn buffer(cap: usize) -> Vec<u8> {
    vec![0; RESERVE + cap]
}

#[test]
fn full_buffering_writes_when_full_or_flushed() {
    // Clauses: s12_1_g_1, s12_1_c_2, s12_4_c_3_1 (D-12.1).
    let (mut b, mut m) = (buffer(4), Mem::file(b""));
    let mut s = State::new(false, true, false, Policy::Full);
    assert_eq!(s.write(&mut b, &mut m, b"ab").done, 2);
    assert!(m.writes.is_empty(), "held");
    assert_eq!(s.pending(), 2);
    assert_eq!(s.wend, b.len(), "the caller may fill the rest directly");
    s.write(&mut b, &mut m, b"cdef");
    assert_eq!(m.writes, [2, 4], "the pending bytes, then a write as large as the buffer goes through");
    s.write(&mut b, &mut m, b"g");
    s.flush(&mut b, &mut m).unwrap();
    assert_eq!(m.data, b"abcdefg");
}

#[test]
fn line_buffering_writes_at_newline_and_shows_no_room() {
    // Clauses: s12_1_g_1, s12_1_c_2 (D-12.1, D-12.11).
    let (mut b, mut m) = (buffer(16), Mem::file(b""));
    let mut s = State::new(false, true, false, Policy::Line);
    s.write(&mut b, &mut m, b"one");
    assert_eq!(s.wend, s.wpos, "every byte reaches the engine (C.9)");
    assert!(m.writes.is_empty());
    s.write(&mut b, &mut m, b" two\nthree");
    assert_eq!(m.data, b"one two\n", "through the last newline");
    assert_eq!(s.pending(), 5, "the rest stays pending");
}

#[test]
fn unbuffered_writes_at_once() {
    // Clauses: s12_1_c_2, s12_5_c_2 (D-12.1).
    let (mut b, mut m) = (buffer(16), Mem::file(b""));
    let mut s = State::new(false, true, false, Policy::Unbuffered);
    s.putc(&mut b, &mut m, b'x').unwrap();
    s.write(&mut b, &mut m, b"yz");
    assert_eq!((m.writes.as_slice(), m.data.as_slice()), (&[1, 2][..], &b"xyz"[..]));
}

#[test]
fn partial_and_failed_writes() {
    // Clauses: s12_4_f_0, s12_4_c_3_1, s12_1_c_3 (D-12.1).
    let (mut b, mut m) = (buffer(8), Mem::file(b""));
    m.max_write = Some(1);
    let mut s = State::new(false, true, false, Policy::Full);
    s.write(&mut b, &mut m, b"abc");
    s.flush(&mut b, &mut m).unwrap();
    assert_eq!(m.data, b"abc", "partial writes are retried");
    m.max_write = None;
    s.write(&mut b, &mut m, b"de");
    m.fail_write = Some(Errno::ENOSPC);
    assert_eq!(s.flush(&mut b, &mut m), Err(Errno::ENOSPC));
    assert!(s.err, "the error indicator");
    assert_eq!(s.pending(), 0, "unwritten bytes are dropped, as the reference library does");
    m.fail_write = None;
    s.flush(&mut b, &mut m).unwrap();
    assert_eq!(m.data, b"abc");
}

#[test]
fn reading_getc_ungetc_eof() {
    // Clauses: s12_5_c_1, s12_5_c_3, s12_1_c_3 (D-12.1).
    let (mut b, mut m) = (buffer(4), Mem::file(b"hello"));
    let mut s = State::new(true, false, false, Policy::Full);
    assert_eq!(s.getc(&mut b, &mut m), Ok(Some(b'h')));
    assert_eq!(s.unread(), 3, "one buffer's worth read ahead");
    s.ungetc(&mut b, &mut m, b'H').unwrap();
    assert_eq!(s.getc(&mut b, &mut m), Ok(Some(b'H')), "pushback, even a different byte");
    let mut out = [0u8; 10];
    let t = s.read(&mut b, &mut m, &mut out);
    assert_eq!((t.done, &out[..4]), (4, &b"ello"[..]));
    assert!(s.eof);
    assert_eq!(s.getc(&mut b, &mut m), Ok(None));
    s.ungetc(&mut b, &mut m, b'!').unwrap();
    assert!(!s.eof, "ungetc clears end of file");
    assert_eq!(s.getc(&mut b, &mut m), Ok(Some(b'!')));
}

#[test]
fn pushback_at_the_start_of_the_buffer_uses_the_reserve() {
    // Clauses: s12_5_c_3 (D-12.1).
    let (mut b, mut m) = (buffer(4), Mem::file(b"ab"));
    let mut s = State::new(true, false, false, Policy::Full);
    for c in b"87654321" {
        s.ungetc(&mut b, &mut m, *c).unwrap();
    }
    assert_eq!(s.ungetc(&mut b, &mut m, b'0'), Err(Errno::EAGAIN), "RESERVE bytes of room");
    let mut out = [0u8; 10];
    assert_eq!(s.read(&mut b, &mut m, &mut out).done, 10);
    assert_eq!(&out, b"12345678ab");
}

#[test]
fn positions_count_read_ahead_and_pending() {
    // Clauses: s12_8_c_2, s12_8_c_1, s12_8_f_0, s12_1_g_2 (D-12.1).
    let (mut b, mut m) = (buffer(4), Mem::file(b"0123456789"));
    let mut s = State::new(true, true, false, Policy::Full);
    s.getc(&mut b, &mut m).unwrap();
    assert_eq!((m.pos, s.tell(&mut m)), (4, Ok(1)));
    s.write(&mut b, &mut m, b"X");
    assert_eq!(s.role, Role::Writing, "switched without a flush");
    assert_eq!(s.tell(&mut m), Ok(2));
    assert_eq!(s.seek(&mut b, &mut m, -1, Whence::End), Ok(9));
    assert_eq!(m.data, b"0X23456789", "the write landed at the logical position");
    assert_eq!(s.getc(&mut b, &mut m), Ok(Some(b'9')));
    assert_eq!(s.seek(&mut b, &mut m, -20, Whence::Cur), Err(Errno::EINVAL));
}

#[test]
fn flush_of_input_gives_read_ahead_back() {
    // Clauses: s12_4_c_3_2 (D-12.1).
    let (mut b, mut m) = (buffer(8), Mem::file(b"abcdef"));
    let mut s = State::new(true, false, false, Policy::Full);
    s.getc(&mut b, &mut m).unwrap();
    s.flush(&mut b, &mut m).unwrap();
    assert_eq!(m.pos, 1, "POSIX 2008: the backing's position becomes the stream's");
    let mut pipe = Mem::file(b"abc");
    pipe.seekable = false;
    let mut p = State::new(true, false, false, Policy::Full);
    p.getc(&mut b, &mut pipe).unwrap();
    p.flush(&mut b, &mut pipe).unwrap();
    assert_eq!(p.getc(&mut b, &mut pipe), Ok(Some(b'b')), "a pipe keeps its read-ahead");
}

#[test]
fn read_until_delimiter() {
    // Clauses: s12_5_c_5 (D-12.1).
    let (mut b, mut m) = (buffer(3), Mem::file(b"line one\nline two"));
    let mut s = State::new(true, false, false, Policy::Full);
    let mut got = Vec::new();
    let n = s.read_until(&mut b, &mut m, b'\n', &mut |p| {
        got.extend_from_slice(p);
        Ok(())
    });
    assert_eq!((n, got.as_slice()), (Ok(9), &b"line one\n"[..]));
    got.clear();
    let n = s.read_until(&mut b, &mut m, b'\n', &mut |p| {
        got.extend_from_slice(p);
        Ok(())
    });
    assert_eq!((n, got.as_slice()), (Ok(8), &b"line two"[..]), "no delimiter at the end");
    assert_eq!(s.read_until(&mut b, &mut m, b'\n', &mut |_| Ok(())), Ok(0));
}

#[test]
fn direction_rules() {
    // Clauses: s12_2_c_1_1, s12_1_c_3, s12_5_c_1, s12_5_c_2 (D-12.1).
    let (mut b, mut m) = (buffer(4), Mem::file(b"x"));
    let mut r = State::new(true, false, false, Policy::Full);
    assert_eq!(r.write(&mut b, &mut m, b"a").error, Some(Errno::EBADF));
    assert!(r.err);
    let mut w = State::new(false, true, false, Policy::Full);
    assert_eq!(w.getc(&mut b, &mut m), Err(Errno::EBADF));
}

/// [`Mem`] that reports its position, as a descriptor backing does once it has seeked (D-12.22),
/// and counts its seeks.
struct Tracked(Mem, usize);

impl Backing for Tracked {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Errno> {
        self.0.read(buf)
    }
    fn write(&mut self, buf: &[u8]) -> Result<usize, Errno> {
        self.0.write(buf)
    }
    fn seek(&mut self, offset: i64, whence: Whence) -> Result<u64, Errno> {
        self.1 += 1;
        self.0.seek(offset, whence)
    }
    fn position(&self) -> Option<u64> {
        Some(self.0.pos as u64)
    }
}

fn tracked(data: &[u8]) -> Tracked {
    Tracked(Mem::file(data), 0)
}

fn byte(s: &mut State, b: &mut [u8], m: &mut Tracked) -> Option<u8> {
    s.getc(b, m).unwrap()
}

#[test]
fn seekwin_seek_inside_the_window_keeps_the_buffer() {
    // Clauses: s12_8_c_1, s12_8_c_4 (D-12.22, owner ruling 2026-10-10, COMPAT-5).
    let (mut b, mut m) = (buffer(4), tracked(b"0123456789"));
    let mut s = State::new(true, false, false, Policy::Full);
    s.seek(&mut b, &mut m, 4, Whence::Set).unwrap();
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'4'));
    assert_eq!(m.0.reads, 1);
    // The window holds offsets [4, 8): its first byte, a middle one, its end.
    for (to, want) in [(4, b'4'), (6, b'6'), (5, b'5')] {
        assert_eq!(s.seek(&mut b, &mut m, to, Whence::Set), Ok(to as u64));
        assert_eq!(byte(&mut s, &mut b, &mut m), Some(want), "seek {to}");
    }
    assert_eq!(m.0.reads, 1, "no read for a target inside the window");
    assert_eq!(s.seek(&mut b, &mut m, 1, Whence::Cur), Ok(7));
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'7'));
    assert_eq!(s.seek(&mut b, &mut m, -2, Whence::Cur), Ok(6));
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'6'));
    assert_eq!(m.0.reads, 1);
    assert_eq!(s.tell(&mut m), Ok(7), "the logical position after an in-window seek");
    // Exactly the window's end: no read at the seek; the next byte comes from the backing.
    assert_eq!(s.seek(&mut b, &mut m, 8, Whence::Set), Ok(8));
    assert_eq!(m.0.reads, 1);
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'8'));
    assert_eq!(m.0.reads, 2);
    // The window is now [8, 10): one before its start is outside.
    assert_eq!(s.seek(&mut b, &mut m, 7, Whence::Set), Ok(7));
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'7'));
    assert_eq!(m.0.reads, 3, "start - 1 takes the full seek");
}

#[test]
fn seekwin_past_the_window_end_takes_the_full_seek() {
    // Clauses: s12_8_c_1 (D-12.22).
    let (mut b, mut m) = (buffer(4), tracked(b"0123456789"));
    let mut s = State::new(true, false, false, Policy::Full);
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'0'));
    assert_eq!(s.seek(&mut b, &mut m, 5, Whence::Set), Ok(5));
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'5'));
    assert_eq!(m.0.reads, 2);
}

#[test]
fn seekwin_clears_eof_and_drops_pushback() {
    // Clauses: s12_8_c_1, s12_5_c_5 (D-12.22).
    let (mut b, mut m) = (buffer(8), tracked(b"abc"));
    let mut s = State::new(true, false, false, Policy::Full);
    while byte(&mut s, &mut b, &mut m).is_some() {}
    assert!(s.eof);
    // The window is empty at the end of input; its end is still in it.
    assert_eq!(s.seek(&mut b, &mut m, 3, Whence::Set), Ok(3));
    assert!(!s.eof, "a seek clears end of input");
    assert_eq!(m.0.reads, 2, "the fill and the read that saw the end");
    // Back to the start, then push back the same byte (no change) and another before it.
    s.seek(&mut b, &mut m, 0, Whence::Set).unwrap();
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'a'));
    s.ungetc(&mut b, &mut m, b'a').unwrap();
    s.ungetc(&mut b, &mut m, b'z').unwrap();
    assert_eq!(s.seek(&mut b, &mut m, 1, Whence::Set), Ok(1));
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'b'), "pushback dropped");
    assert_eq!(m.0.reads, 3, "pushback in front of the window keeps it");
    s.ungetc(&mut b, &mut m, b'q').unwrap();
    assert_eq!(s.seek(&mut b, &mut m, 1, Whence::Set), Ok(1));
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'b'), "the window byte under the pushback");
}

#[test]
fn seekwin_puts_a_moved_backing_back_at_the_window_end() {
    // Clauses: s12_8_c_1 (D-12.22: the descriptor moved under the stream).
    let (mut b, mut m) = (buffer(4), tracked(b"0123456789"));
    let mut s = State::new(true, false, false, Policy::Full);
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'0'));
    m.0.pos = 1;
    assert_eq!(s.seek(&mut b, &mut m, 2, Whence::Set), Ok(2));
    assert_eq!(m.0.pos, 4, "back at the window end");
    let mut rest = Vec::new();
    while let Some(c) = byte(&mut s, &mut b, &mut m) {
        rest.push(c);
    }
    assert_eq!(rest, b"23456789");
}

#[test]
fn seekwin_write_after_an_in_window_seek_lands_at_the_target() {
    // Clauses: s12_8_c_1, s12_1_c_3 (D-12.22, D-12.1).
    let (mut b, mut m) = (buffer(8), tracked(b"0123456789"));
    let mut s = State::new(true, true, false, Policy::Full);
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'0'));
    assert_eq!(s.seek(&mut b, &mut m, 3, Whence::Set), Ok(3));
    assert_eq!(s.write(&mut b, &mut m, b"X").done, 1);
    s.flush(&mut b, &mut m).unwrap();
    assert_eq!(m.0.data, b"012X456789");
    assert_eq!(s.tell(&mut m), Ok(4));
}

#[test]
fn seekwin_backing_without_a_position_keeps_the_full_seek() {
    // Clauses: s12_8_c_1 (D-12.22: only a backing that knows its position).
    let (mut b, mut m) = (buffer(8), Mem::file(b"0123456789"));
    let mut s = State::new(true, false, false, Policy::Full);
    assert_eq!(s.getc(&mut b, &mut m), Ok(Some(b'0')));
    s.seek(&mut b, &mut m, 2, Whence::Set).unwrap();
    assert_eq!(s.getc(&mut b, &mut m), Ok(Some(b'2')));
    assert_eq!(m.reads, 2);
}

#[test]
fn ftell_cache_answers_from_the_known_position() {
    // Clauses: s12_8_c_1 (D-12.22 FTELL-CACHE: no seek once the backing knows its position).
    let (mut b, mut m) = (buffer(4), tracked(b"0123456789"));
    let mut s = State::new(true, true, false, Policy::Full);
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'0'));
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'1'));
    assert_eq!(m.1, 0);
    assert_eq!(s.tell(&mut m), Ok(2), "the known position less the bytes read ahead");
    assert_eq!(m.1, 0, "no seek for a reading stream");
    s.seek(&mut b, &mut m, 5, Whence::Set).unwrap();
    let seeks = m.1;
    assert_eq!(s.tell(&mut m), Ok(5), "idle after a seek");
    assert_eq!(s.write(&mut b, &mut m, b"XY").done, 2);
    assert_eq!(s.tell(&mut m), Ok(7), "the known position plus the bytes pending");
    assert_eq!(m.1, seeks, "no seek for an idle or writing stream either");
}

#[test]
fn ftell_cache_append_writing_still_asks_for_the_end() {
    // Clauses: s12_8_c_1 (D-12.22 FTELL-CACHE: an append stream writes at the end).
    let (mut b, mut m) = (buffer(8), tracked(b"0123456789"));
    let mut s = State::new(false, true, true, Policy::Full);
    assert_eq!(s.write(&mut b, &mut m, b"ab").done, 2);
    let seeks = m.1;
    assert_eq!(s.tell(&mut m), Ok(12));
    assert_eq!(m.1, seeks + 1, "the end comes from a seek");
}

fn first(w: &[u8], c: u8) -> Option<usize> {
    w.iter().position(|&x| x == c)
}

/// `read_line_into` (the `fgets` path): delimiter, full output, end of input, a window cut to
/// the room left, `once`, and read errors before and after some bytes (MUTATION-CAMPAIGN-2B
/// survivors; no spec clause: engine internals behind fgets).
#[test]
fn read_line_into_stops_at_delimiter_full_output_end_and_errors() {
    // no spec clause: engine internals behind fgets (MUTATION-CAMPAIGN-2B).
    let (mut b, mut m) = (buffer(4), Mem::file(b"abcdefg\nxy"));
    let mut s = State::new(true, false, false, Policy::Full);
    let mut out = [0u8; 16];
    assert_eq!(s.read_line_into(&mut b, &mut m, b'\n', &first, &mut out, false), Ok((8, true)));
    assert_eq!(&out[..8], b"abcdefg\n");
    let mut one = [0u8; 1];
    assert_eq!(s.read_line_into(&mut b, &mut m, b'\n', &first, &mut one, false), Ok((1, true)), "output full");
    assert_eq!((one[0], s.unread()), (b'x', 1));
    assert_eq!(s.read_line_into(&mut b, &mut m, b'\n', &first, &mut out, false), Ok((1, true)), "end of input");
    assert_eq!(out[0], b'y');
    assert_eq!(s.read_line_into(&mut b, &mut m, b'\n', &first, &mut out, false), Ok((0, true)));

    // The window is cut to the room left in the output after an earlier piece.
    let (mut b, mut m) = (buffer(2), Mem::file(b"abcdef\n"));
    let mut s = State::new(true, false, false, Policy::Full);
    let mut three = [0u8; 3];
    assert_eq!(s.read_line_into(&mut b, &mut m, b'\n', &first, &mut three, false), Ok((3, true)));
    assert_eq!(&three, b"abc");
    assert_eq!(s.read_line_into(&mut b, &mut m, b'\n', &first, &mut out, false), Ok((4, true)));
    assert_eq!(&out[..4], b"def\n");

    // `once`: the backing is read only when the window is empty on entry.
    let (mut b, mut m) = (buffer(4), Mem::file(b"ab\ncdefgh"));
    let mut s = State::new(true, false, false, Policy::Full);
    assert_eq!(s.read_line_into(&mut b, &mut m, b'\n', &first, &mut out, true), Ok((3, true)));
    assert_eq!(s.read_line_into(&mut b, &mut m, b'\n', &first, &mut out, true), Ok((1, false)), "window ran dry");
    assert_eq!((out[0], m.reads), (b'c', 1));
    assert_eq!(s.read_line_into(&mut b, &mut m, b'\n', &first, &mut out, true), Ok((4, false)));
    assert_eq!(&out[..4], b"defg");

    // A read error after some bytes ends the line; before any byte it is the result.
    let (mut b, mut m) = (buffer(2), Mem { data: b"abc".to_vec(), fail_read: Some(Errno::EIO), ..Mem::default() });
    let mut s = State::new(true, false, false, Policy::Full);
    assert_eq!(s.read_line_into(&mut b, &mut m, b'\n', &first, &mut out, false), Ok((3, true)));
    assert_eq!(&out[..3], b"abc");
    let (mut b, mut m) = (buffer(2), Mem { fail_read: Some(Errno::EIO), ..Mem::default() });
    let mut s = State::new(true, false, false, Policy::Full);
    assert_eq!(s.read_line_into(&mut b, &mut m, b'\n', &first, &mut out, false), Err(Errno::EIO));
}

/// `read_until_with` counts every piece of a line longer than the buffer (no spec clause:
/// engine internals behind getdelim).
#[test]
fn read_until_with_counts_every_piece() {
    // no spec clause: engine internals behind getdelim (MUTATION-CAMPAIGN-2B).
    let (mut b, mut m) = (buffer(3), Mem::file(b"line one\nrest"));
    let mut s = State::new(true, false, false, Policy::Full);
    let mut got = Vec::new();
    let n = s.read_until_with(&mut b, &mut m, b'\n', &first, &mut |p| {
        got.extend_from_slice(p);
        Ok(())
    });
    assert_eq!((n, got.as_slice()), (Ok(9), &b"line one\n"[..]));
}

/// `purge` drops pending output and read-ahead (no spec clause: `__fpurge` engine internals).
#[test]
fn purge_drops_pending_output_and_read_ahead() {
    // no spec clause: `__fpurge` engine internals (MUTATION-CAMPAIGN-2B).
    let (mut b, mut m) = (buffer(8), Mem::file(b""));
    let mut s = State::new(false, true, false, Policy::Full);
    s.write(&mut b, &mut m, b"abc");
    s.purge();
    assert_eq!(s.pending(), 0);
    s.flush(&mut b, &mut m).unwrap();
    assert!(m.writes.is_empty(), "nothing written");
    let (mut b, mut m) = (buffer(4), Mem::file(b"abcdef"));
    let mut s = State::new(true, false, false, Policy::Full);
    assert_eq!(s.getc(&mut b, &mut m), Ok(Some(b'a')));
    s.purge();
    assert_eq!(s.unread(), 0);
    assert_eq!(s.getc(&mut b, &mut m), Ok(Some(b'e')), "the read-ahead is gone");
}

/// Writes keep every byte across buffer refills; a fully buffered stream holds a newline; a
/// backing that takes nothing is an error, not a loop (no spec clause: engine internals).
#[test]
fn writes_keep_bytes_hold_newlines_when_full_and_fail_on_no_progress() {
    // no spec clause: engine internals behind fwrite (MUTATION-CAMPAIGN-2B).
    let (mut b, mut m) = (buffer(4), Mem::file(b""));
    let mut s = State::new(false, true, false, Policy::Full);
    s.write(&mut b, &mut m, b"a\n");
    assert!(m.writes.is_empty(), "full buffering holds a newline");
    s.write(&mut b, &mut m, b"bcd");
    s.write(&mut b, &mut m, b"efg");
    s.flush(&mut b, &mut m).unwrap();
    assert_eq!(m.data, b"a\nbcdefg");
    let (mut b, mut m) = (buffer(8), Mem { max_write: Some(0), ..Mem::file(b"") });
    let mut s = State::new(false, true, false, Policy::Line);
    assert_eq!(s.write(&mut b, &mut m, b"a\n").error, Some(Errno::EIO));
    let (mut b, mut m) = (buffer(4), Mem { max_write: Some(0), ..Mem::file(b"") });
    let mut s = State::new(false, true, false, Policy::Full);
    assert_eq!(s.write(&mut b, &mut m, b"abcdefgh").error, Some(Errno::EIO), "a direct write");
}

/// `read` copies small requests through the buffer and keeps the rest read ahead (no spec
/// clause: engine internals behind fread).
#[test]
fn read_buffers_small_requests_and_counts_pieces() {
    // no spec clause: engine internals behind fread (MUTATION-CAMPAIGN-2B).
    let (mut b, mut m) = (buffer(4), Mem::file(b"abcdefghij"));
    let mut s = State::new(true, false, false, Policy::Full);
    let mut out = [0u8; 5];
    let t = s.read(&mut b, &mut m, &mut out[..2]);
    assert_eq!((t.done, &out[..2], s.unread(), m.reads), (2, &b"ab"[..], 2, 1));
    let t = s.read(&mut b, &mut m, &mut out);
    assert_eq!((t.done, &out[..], s.unread()), (5, &b"cdefg"[..], 1), "the rest is read through the buffer");
}

#[test]
fn reading_after_writing_writes_the_pending_bytes_first() {
    // Clauses: s12_2_c_1_1, s12_1_c_3 (D-12.1; MUTATION-CAMPAIGN-2C: enter_reading drains).
    let (mut b, mut m) = (buffer(8), Mem::file(b""));
    let mut s = State::new(true, true, false, Policy::Full);
    assert_eq!(s.write(&mut b, &mut m, b"ab").done, 2);
    assert!(m.writes.is_empty());
    assert_eq!(s.getc(&mut b, &mut m), Ok(None));
    assert_eq!((&m.data[..], &m.writes[..]), (&b"ab"[..], &[2][..]), "the pending bytes reach the backing before the read");
}

#[test]
fn writing_after_reading_seeks_back_only_over_unread_bytes() {
    // Clauses: s12_2_c_1_1, s12_8_c_1 (D-12.1; MUTATION-CAMPAIGN-2C: enter_writing).
    // The window is used up: nothing to give back, so no seek.
    let (mut b, mut m) = (buffer(4), tracked(b"abcdefgh"));
    let mut s = State::new(true, true, false, Policy::Full);
    for want in *b"abcd" {
        assert_eq!(byte(&mut s, &mut b, &mut m), Some(want));
    }
    assert_eq!(s.unread(), 0);
    let seeks = m.1;
    assert_eq!(s.write(&mut b, &mut m, b"X").done, 1);
    assert_eq!(m.1, seeks, "no seek when no byte was read ahead");
    // An append stream writes at the end: its read-ahead is not given back by a seek.
    let (mut b, mut m) = (buffer(4), tracked(b"abcdefgh"));
    let mut s = State::new(true, true, true, Policy::Full);
    assert_eq!(byte(&mut s, &mut b, &mut m), Some(b'a'));
    assert_eq!(s.unread(), 3);
    let seeks = m.1;
    assert_eq!(s.write(&mut b, &mut m, b"X").done, 1);
    assert_eq!(m.1, seeks, "an append stream does not seek back");
}

#[test]
fn full_buffering_writes_nothing_until_the_buffer_is_full() {
    // Clauses: s12_1_g_1, s12_1_c_2 (D-12.1; MUTATION-CAMPAIGN-2C: write drains only when full).
    let (mut b, mut m) = (buffer(4), Mem::file(b""));
    let mut s = State::new(false, true, false, Policy::Full);
    s.write(&mut b, &mut m, b"ab");
    s.write(&mut b, &mut m, b"c");
    s.write(&mut b, &mut m, b"d");
    assert!(m.writes.is_empty(), "small writes stay in the buffer until it is full");
    s.write(&mut b, &mut m, b"e");
    assert_eq!((&m.data[..], &m.writes[..]), (&b"abcd"[..], &[4][..]));
}

#[test]
fn read_until_keeps_the_rest_of_the_window_for_the_next_line() {
    // Clauses: s12_5_c_5 (D-12.1; MUTATION-CAMPAIGN-2C: read_until_with advances by the line).
    let (mut b, mut m) = (buffer(16), Mem::file(b"ab\ncd\n"));
    let mut s = State::new(true, false, false, Policy::Full);
    let mut got = Vec::new();
    assert_eq!(s.read_until(&mut b, &mut m, b'\n', &mut |p| { got.extend_from_slice(p); Ok(()) }), Ok(3));
    assert_eq!(s.read_until(&mut b, &mut m, b'\n', &mut |p| { got.extend_from_slice(p); Ok(()) }), Ok(3));
    assert_eq!((&got[..], m.reads), (&b"ab\ncd\n"[..], 1));
}

#[test]
fn read_line_into_with_a_full_output_does_not_read_again() {
    // no spec clause: engine internals behind fgets (MUTATION-CAMPAIGN-2C: the loop ends when `out` is full).
    let (mut b, mut m) = (buffer(4), Mem::file(b"abcdefgh"));
    let mut s = State::new(true, false, false, Policy::Full);
    let mut out = [0u8; 4];
    let (n, _) = s.read_line_into(&mut b, &mut m, b'\n', &first, &mut out, false).unwrap();
    assert_eq!((n, &out[..], m.reads), (4, &b"abcd"[..], 1), "no read once the output is full");
}
