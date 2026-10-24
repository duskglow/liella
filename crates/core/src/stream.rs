//! Buffered streams (spec §12.1–12.5a, §12.8; D-12.1, D-12.11): the buffering engine.
//!
//! A stream is a [`Backing`] (where bytes come from and go: a descriptor, memory, callbacks), a
//! buffer, and a [`State`]. The engine is a set of operations on a `State` over a caller-held
//! buffer slice, so the same engine serves the C `FILE` (whose buffer and state live in memory
//! the C interface can see, D-12.11) and a Rust-owned stream.
//!
//! **The buffer.** `buf[..RESERVE]` is kept free for pushback, so that pushed-back bytes stay
//! inside the read window `[rpos, rend)` that header-inline `getc` reads (Appendix C.9); data
//! occupies `buf[RESERVE..]`. One buffer serves reading or writing, and the engine switches its
//! role when the caller switches direction (stricter than ISO C, which requires a flush or a
//! seek in between; D-12.1).
//!
//! **Windows.** While reading, `[rpos, rend)` holds the bytes not yet consumed. While writing,
//! `[RESERVE, wpos)` holds the bytes not yet written and `[wpos, wend)` the room a caller may
//! fill directly; a line-buffered or unbuffered stream keeps `wend == wpos`, so every byte
//! reaches the engine (Appendix C.9). When the stream is idle, all windows are empty.
//!
//! **Errors.** A failed transfer sets the error indicator and reports the error; an interrupted
//! system call (`EINTR`) is reported too, not retried, as the reference library does.

#![allow(clippy::arithmetic_side_effects)] // window offsets stay within the buffer: every update is checked against its length

use crate::error::{Errno, Result};

/// Bytes kept in front of the data for pushback (`ungetc`).
pub const RESERVE: usize = 8;

/// Where a seek counts from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Whence {
    /// From the start.
    Set,
    /// From the current position.
    Cur,
    /// From the end.
    End,
}

/// The source and sink of a stream's bytes.
pub trait Backing {
    /// Read into `buf`; 0 is end of input.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    /// Write from `buf`; may write less than all.
    fn write(&mut self, buf: &[u8]) -> Result<usize>;
    /// Move the position; the new position. `ESPIPE` if the backing cannot.
    fn seek(&mut self, offset: i64, whence: Whence) -> Result<u64>;
    /// The backing's position as last learnt from its own successful seek and advanced by the
    /// reads since (STDIO-SEEKWIN, D-12.22); `None` when not known (no seek yet, a write, an
    /// error) or not tracked. Only a backing that can seek ever answers `Some`.
    fn position(&self) -> Option<u64> {
        None
    }
}

/// The buffering policy (`setvbuf`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Policy {
    /// Written when the buffer fills.
    #[default]
    Full,
    /// Written at each newline, and when the buffer fills.
    Line,
    /// Written at once.
    Unbuffered,
}

/// What the buffer holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Role {
    /// Nothing.
    #[default]
    Idle,
    /// Bytes read ahead.
    Reading,
    /// Bytes not yet written.
    Writing,
}

/// A stream's state: plain data, so that a record in C-visible memory can hold it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct State {
    /// The read window: `[rpos, rend)`.
    pub rpos: usize,
    /// End of the read window.
    pub rend: usize,
    /// Where the next byte written goes; `[RESERVE, wpos)` are pending.
    pub wpos: usize,
    /// End of the room a caller may fill directly.
    pub wend: usize,
    /// What the buffer holds.
    pub role: Role,
    /// The buffering policy.
    pub policy: Policy,
    /// End of input was seen (`feof`).
    pub eof: bool,
    /// An error was seen (`ferror`).
    pub err: bool,
    /// The stream may be read.
    pub readable: bool,
    /// The stream may be written.
    pub writable: bool,
    /// Writes go to the end (`O_APPEND`): positions after writing count from the end.
    pub append: bool,
    /// The read window's bytes `buf[RESERVE..rend]` are the backing's bytes ending at offset
    /// [`State::win_end`] (STDIO-SEEKWIN, D-12.22): set by a fill when the backing knew its
    /// position; cleared when the window goes, the backing moves past it without the buffer
    /// (a direct read), or pushback overwrites one of its bytes.
    pub win_known: bool,
    /// The backing's offset of `buf[rend]` at the fill (meaningful only with `win_known`).
    pub win_end: u64,
}

/// The outcome of a transfer: the bytes moved, and the error that stopped it, if any.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Transfer {
    /// Bytes moved.
    pub done: usize,
    /// What stopped the transfer short (`None`: all moved, or end of input).
    pub error: Option<Errno>,
}

impl State {
    /// An idle stream.
    pub fn new(readable: bool, writable: bool, append: bool, policy: Policy) -> State {
        State { rpos: RESERVE, rend: RESERVE, wpos: RESERVE, wend: RESERVE, role: Role::Idle, policy, eof: false, err: false, readable, writable, append, win_known: false, win_end: 0 }
    }

    /// Bytes read ahead and not yet consumed.
    pub fn unread(&self) -> usize {
        if self.role == Role::Reading { self.rend.saturating_sub(self.rpos) } else { 0 }
    }

    /// Bytes written to the buffer and not yet to the backing (`__fpending`).
    pub fn pending(&self) -> usize {
        if self.role == Role::Writing { self.wpos.saturating_sub(RESERVE) } else { 0 }
    }

    fn idle(&mut self) {
        *self = State { rpos: RESERVE, rend: RESERVE, wpos: RESERVE, wend: RESERVE, role: Role::Idle, win_known: false, ..*self };
    }

    fn fail(&mut self, e: Errno) -> Errno {
        self.err = true;
        e
    }

    /// Write the pending bytes, retrying partial writes. On an error the bytes not written are
    /// dropped, as the reference library does (a later flush or close then succeeds; the error
    /// indicator stays set).
    fn drain(&mut self, buf: &mut [u8], back: &mut dyn Backing) -> Result<()> {
        self.drain_to(buf, back, self.wpos)
    }

    /// Write the pending bytes before `upto`; those after it move to the front of the buffer
    /// and stay pending.
    fn drain_to(&mut self, buf: &mut [u8], back: &mut dyn Backing, upto: usize) -> Result<()> {
        if self.role != Role::Writing {
            return Ok(());
        }
        let mut from = RESERVE;
        while from < upto {
            match back.write(buf.get(from..upto).unwrap_or(&[])) {
                Ok(n) if n > 0 => from += n,
                Ok(_) => {
                    self.idle();
                    return Err(self.fail(Errno::EIO));
                }
                Err(e) => {
                    self.idle();
                    return Err(self.fail(e));
                }
            }
        }
        if upto >= self.wpos {
            self.idle();
        } else {
            let rest = self.wpos - upto;
            buf.copy_within(upto..self.wpos, RESERVE);
            self.wpos = RESERVE + rest;
            self.set_write_window(buf.len());
        }
        Ok(())
    }

    fn set_write_window(&mut self, len: usize) {
        self.wend = if self.policy == Policy::Full { len } else { self.wpos };
    }

    fn start_writing(&mut self, len: usize) {
        self.role = Role::Writing;
        self.wpos = RESERVE;
        self.set_write_window(len);
    }

    /// Make the buffer ready for reading: pending output is written first.
    fn enter_reading(&mut self, buf: &mut [u8], back: &mut dyn Backing) -> Result<()> {
        if !self.readable {
            return Err(self.fail(Errno::EBADF));
        }
        if self.role == Role::Writing {
            self.drain(buf, back)?;
        }
        if self.role == Role::Idle {
            self.role = Role::Reading;
            self.rpos = RESERVE;
            self.rend = RESERVE;
        }
        Ok(())
    }

    /// Make the buffer ready for writing: bytes read ahead are given back to the backing (a
    /// seek back), or dropped if it cannot seek.
    fn enter_writing(&mut self, buf: &mut [u8], back: &mut dyn Backing) -> Result<()> {
        if !self.writable {
            return Err(self.fail(Errno::EBADF));
        }
        if self.role == Role::Reading {
            let unread = self.unread();
            if unread > 0 && !self.append {
                let back_by = i64::try_from(unread).map_err(|_| Errno::EOVERFLOW)?;
                match back.seek(-back_by, Whence::Cur) {
                    Ok(_) | Err(Errno::ESPIPE) => {}
                    Err(e) => return Err(self.fail(e)),
                }
            }
            self.idle();
        }
        if self.role == Role::Idle {
            self.start_writing(buf.len());
        }
        Ok(())
    }

    /// Fill the empty read window from the backing; the bytes read (0 at end of input).
    fn fill(&mut self, buf: &mut [u8], back: &mut dyn Backing) -> Result<usize> {
        self.rpos = RESERVE;
        self.rend = RESERVE;
        let r = back.read(buf.get_mut(RESERVE..).unwrap_or(&mut []));
        self.mark_window(back.position(), &r, buf.len());
        match r {
            Ok(0) => {
                self.eof = true;
                Ok(0)
            }
            Ok(n) => {
                self.rend = RESERVE + n.min(buf.len().saturating_sub(RESERVE));
                Ok(n)
            }
            Err(e) => Err(self.fail(e)),
        }
    }

    /// After a fill of `buf[RESERVE..]` that returned `r`, with the backing now at `at`: the
    /// window `buf[RESERVE..RESERVE + n]` holds the backing's bytes `[at - n, at)` (D-12.22),
    /// unless the position is not known or the read claims more than the buffer holds.
    pub fn mark_window(&mut self, at: Option<u64>, r: &Result<usize>, len: usize) {
        self.win_known = false;
        if let (Some(at), Ok(n)) = (at, r) {
            if *n <= len.saturating_sub(RESERVE) {
                self.win_known = true;
                self.win_end = at;
            }
        }
    }

    /// The next byte (`fgetc`, `__uflow`); `None` at end of input.
    pub fn getc(&mut self, buf: &mut [u8], back: &mut dyn Backing) -> Result<Option<u8>> {
        if self.role != Role::Reading || self.rpos >= self.rend {
            self.enter_reading(buf, back)?;
            if self.rpos >= self.rend && self.fill(buf, back)? == 0 {
                return Ok(None);
            }
        }
        let b = buf.get(self.rpos).copied().ok_or(Errno::EINVAL)?;
        self.rpos += 1;
        Ok(Some(b))
    }

    /// Read up to `out.len()` bytes (`fread`): stops early only at end of input or on an error.
    /// A request at least as large as the buffer bypasses it.
    pub fn read(&mut self, buf: &mut [u8], back: &mut dyn Backing, out: &mut [u8]) -> Transfer {
        let mut done = 0;
        if let Err(e) = self.enter_reading(buf, back) {
            return Transfer { done, error: Some(e) };
        }
        while done < out.len() {
            let avail = self.rend.saturating_sub(self.rpos);
            if avail > 0 {
                let n = avail.min(out.len() - done);
                if let (Some(dst), Some(src)) = (out.get_mut(done..done + n), buf.get(self.rpos..self.rpos + n)) {
                    dst.copy_from_slice(src);
                }
                self.rpos += n;
                done += n;
                continue;
            }
            if out.len() - done >= buf.len().saturating_sub(RESERVE) {
                // A direct read moves the backing past the window (D-12.22).
                self.win_known = false;
                match back.read(out.get_mut(done..).unwrap_or(&mut [])) {
                    Ok(0) => {
                        self.eof = true;
                        break;
                    }
                    Ok(n) => done += n,
                    Err(e) => return Transfer { done, error: Some(self.fail(e)) },
                }
            } else {
                match self.fill(buf, back) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => return Transfer { done, error: Some(e) },
                }
            }
        }
        Transfer { done, error: None }
    }

    /// Push `c` back (`ungetc`): the next read returns it. Clears end of input. Fails only
    /// when the pushback room is full.
    pub fn ungetc(&mut self, buf: &mut [u8], back: &mut dyn Backing, c: u8) -> Result<()> {
        self.enter_reading(buf, back)?;
        if self.rpos == 0 {
            return Err(Errno::EAGAIN);
        }
        self.rpos -= 1;
        let slot = buf.get_mut(self.rpos).ok_or(Errno::EINVAL)?;
        // A different byte over one of the window's own: the window no longer holds the
        // backing's bytes, so a later seek must not reuse it (D-12.22).
        if self.rpos >= RESERVE && *slot != c {
            self.win_known = false;
        }
        *slot = c;
        self.eof = false;
        Ok(())
    }

    /// Write `data` (`fwrite`, `fputs`, `fputc`): accepted into the buffer or written through,
    /// by the policy. A line-buffered stream writes its buffer when `data` holds a newline.
    pub fn write(&mut self, buf: &mut [u8], back: &mut dyn Backing, data: &[u8]) -> Transfer {
        if let Err(e) = self.enter_writing(buf, back) {
            return Transfer { done: 0, error: Some(e) };
        }
        let cap = buf.len().saturating_sub(RESERVE);
        let mut done = 0;
        // Unbuffered streams, and writes at least as large as the buffer, go straight to the
        // backing once the pending bytes are written.
        if self.policy == Policy::Unbuffered || data.len() >= cap {
            if let Err(e) = self.drain(buf, back) {
                return Transfer { done, error: Some(e) };
            }
            while done < data.len() {
                match back.write(data.get(done..).unwrap_or(&[])) {
                    Ok(n) if n > 0 => done += n,
                    Ok(_) => return Transfer { done, error: Some(self.fail(Errno::EIO)) },
                    Err(e) => return Transfer { done, error: Some(self.fail(e)) },
                }
            }
            self.start_writing(buf.len());
            return Transfer { done, error: None };
        }
        while done < data.len() {
            if self.wpos >= buf.len() {
                if let Err(e) = self.drain(buf, back) {
                    return Transfer { done, error: Some(e) };
                }
                self.start_writing(buf.len());
            }
            let n = (buf.len() - self.wpos).min(data.len() - done);
            if let (Some(dst), Some(src)) = (buf.get_mut(self.wpos..self.wpos + n), data.get(done..done + n)) {
                dst.copy_from_slice(src);
            }
            self.wpos += n;
            done += n;
        }
        self.set_write_window(buf.len());
        // A line-buffered stream writes through its last newline; the rest stays pending.
        if self.policy == Policy::Line && data.contains(&b'\n') {
            let last = buf.get(RESERVE..self.wpos).and_then(|p| p.iter().rposition(|&c| c == b'\n'));
            if let Some(i) = last {
                if let Err(e) = self.drain_to(buf, back, RESERVE + i + 1) {
                    return Transfer { done, error: Some(e) };
                }
                if self.role == Role::Idle {
                    self.start_writing(buf.len());
                }
            }
        }
        Transfer { done, error: None }
    }

    /// Write one byte (`fputc`, `__overflow`).
    pub fn putc(&mut self, buf: &mut [u8], back: &mut dyn Backing, c: u8) -> Result<()> {
        match self.write(buf, back, &[c]).error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// `fflush`: write pending output; for a stream reading ahead, give the bytes read ahead
    /// back to the backing (POSIX 2008: the backing's position becomes the stream's), if it can
    /// seek.
    pub fn flush(&mut self, buf: &mut [u8], back: &mut dyn Backing) -> Result<()> {
        match self.role {
            Role::Writing => self.drain(buf, back),
            Role::Reading => {
                let unread = i64::try_from(self.unread()).map_err(|_| Errno::EOVERFLOW)?;
                match back.seek(-unread, Whence::Cur) {
                    Ok(_) => {
                        self.idle();
                        Ok(())
                    }
                    Err(Errno::ESPIPE) => Ok(()),
                    Err(e) => Err(self.fail(e)),
                }
            }
            Role::Idle => Ok(()),
        }
    }

    /// Discard buffered input and output without writing it (`__fpurge`).
    pub fn purge(&mut self) {
        self.idle();
    }

    /// The logical position (`ftell`): the backing's, less the bytes read ahead, plus the bytes
    /// pending. FTELL-CACHE (D-12.22): a backing that knows its position answers without a seek,
    /// as the reference library does (seccomp probe f21612fa: no `lseek` once the offset is known,
    /// whatever the descriptor did since); an append stream that is writing still asks for the end.
    pub fn tell(&mut self, back: &mut dyn Backing) -> Result<u64> {
        let appending = self.role == Role::Writing && self.append;
        let at = match back.position() {
            Some(at) if !appending => at,
            _ if appending => back.seek(0, Whence::End)?,
            _ => back.seek(0, Whence::Cur)?,
        };
        let unread = u64::try_from(self.unread()).map_err(|_| Errno::EOVERFLOW)?;
        let pending = u64::try_from(self.pending()).map_err(|_| Errno::EOVERFLOW)?;
        at.checked_sub(unread).and_then(|p| p.checked_add(pending)).ok_or(Errno::EINVAL)
    }

    /// `fseek`: write pending output, drop pushback and read-ahead, clear end of input, and
    /// move the backing. The new position.
    pub fn seek(&mut self, buf: &mut [u8], back: &mut dyn Backing, offset: i64, whence: Whence) -> Result<u64> {
        if let Some(at) = self.seek_in_window(back, offset, whence) {
            return Ok(at);
        }
        let (target, from) = match whence {
            Whence::Cur => {
                let here = i64::try_from(self.tell(back)?).map_err(|_| Errno::EOVERFLOW)?;
                (here.checked_add(offset).ok_or(Errno::EOVERFLOW)?, Whence::Set)
            }
            w => (offset, w),
        };
        if from == Whence::Set && target < 0 {
            return Err(Errno::EINVAL);
        }
        self.drain(buf, back)?;
        let at = back.seek(target, from)?;
        self.idle();
        self.eof = false;
        Ok(at)
    }

    /// The read-window seek (STDIO-SEEKWIN, D-12.22; owner ruling 2026-10-10 "keep the buffer,
    /// mark for inclusion for a COMPAT flag later", COMPAT-5): a `SEEK_SET` or `SEEK_CUR` target
    /// inside the window's backing range `[win_end - (rend - RESERVE), win_end]` moves the read
    /// pointer instead of discarding the bytes read ahead; pushback is dropped and end of input
    /// cleared, as by any seek. The backing is put back at `win_end` (one seek, its result not
    /// used), so that a caller who moved the descriptor under the stream finds it where the
    /// window ends, as the reference library leaves it. `None`: not in the window, the caller
    /// takes the full seek.
    fn seek_in_window(&mut self, back: &mut dyn Backing, offset: i64, whence: Whence) -> Option<u64> {
        let (target, to) = self.window_target(offset, whence)?;
        let end = i64::try_from(self.win_end).ok()?;
        let _ = back.seek(end, Whence::Set);
        self.land_in_window(to);
        Some(target)
    }

    /// The in-window part of [`State::seek_in_window`] without the backing (SEEKWIN-3: the C
    /// layer's descriptor fast path calls it directly): for a reading stream whose window is
    /// known, the target offset and its buffer index when the target lies in the window, else
    /// `None`. Nothing changes; the caller puts the backing at [`State::win_end`] and then calls
    /// [`State::land_in_window`].
    #[inline(always)]
    pub fn window_target(&self, offset: i64, whence: Whence) -> Option<(u64, usize)> {
        if self.role != Role::Reading || !self.win_known {
            return None;
        }
        let len = u64::try_from(self.rend.checked_sub(RESERVE)?).ok()?;
        let start = self.win_end.checked_sub(len)?;
        let target = match whence {
            Whence::Set => u64::try_from(offset).ok()?,
            Whence::Cur => {
                let here = self.win_end.checked_sub(u64::try_from(self.rend.checked_sub(self.rpos)?).ok()?)?;
                u64::try_from(i64::try_from(here).ok()?.checked_add(offset)?).ok()?
            }
            Whence::End => return None,
        };
        if target < start || target > self.win_end {
            return None;
        }
        let to = RESERVE + usize::try_from(target - start).ok()?;
        // The backing goes back to `win_end` by a signed offset.
        if i64::try_from(self.win_end).is_err() {
            return None;
        }
        Some((target, to))
    }

    /// Finish an in-window seek at buffer index `to` from [`State::window_target`]: the read
    /// pointer moves there and end of input is cleared.
    #[inline(always)]
    pub fn land_in_window(&mut self, to: usize) {
        self.rpos = to;
        self.eof = false;
    }

    /// Read up to and including `delim` (`getdelim`), giving the bytes to `sink` in pieces; the
    /// total. `Ok(0)` is end of input before any byte.
    pub fn read_until(&mut self, buf: &mut [u8], back: &mut dyn Backing, delim: u8, sink: &mut dyn FnMut(&[u8]) -> Result<()>) -> Result<usize> {
        self.read_until_with(buf, back, delim, &liella_mem::find_u8, sink)
    }

    /// [`State::read_until`] with the caller's scanner for the delimiter (`find(window, delim)`:
    /// the index of the first one), so that the C layer's vector `memchr` can serve (GETDELIM).
    pub fn read_until_with(&mut self, buf: &mut [u8], back: &mut dyn Backing, delim: u8, find: &dyn Fn(&[u8], u8) -> Option<usize>, sink: &mut dyn FnMut(&[u8]) -> Result<()>) -> Result<usize> {
        self.enter_reading(buf, back)?;
        let mut total = 0;
        loop {
            if self.rpos >= self.rend && self.fill(buf, back)? == 0 {
                return Ok(total);
            }
            let window = buf.get(self.rpos..self.rend).unwrap_or(&[]);
            let (n, found) = match find(window, delim) {
                Some(i) => (i + 1, true),
                None => (window.len(), false),
            };
            sink(window.get(..n).unwrap_or(&[]))?;
            self.rpos += n;
            total += n;
            if found {
                return Ok(total);
            }
        }
    }

    /// Up to `out.len()` bytes into `out`, stopping after `delim` (`fgets`, STDIO-FGETS: the
    /// buffered window is scanned with `find` and copied in pieces instead of byte by byte).
    /// The bytes stored and whether the line is complete: `delim` stored, `out` full, end of
    /// input, or a read error after some bytes (the error indicator is set). An error before
    /// any byte is the result. With `once`, the backing is read only when the window is empty
    /// on entry; the call then ends incomplete when the window runs dry, so that the caller
    /// can flush the line-buffered streams before the next read, as ISO C asks of input.
    pub fn read_line_into(&mut self, buf: &mut [u8], back: &mut dyn Backing, delim: u8, find: &dyn Fn(&[u8], u8) -> Option<usize>, out: &mut [u8], once: bool) -> Result<(usize, bool)> {
        self.enter_reading(buf, back)?;
        let mut total = 0;
        let mut may_fill = true;
        while total < out.len() {
            if self.rpos >= self.rend {
                if !may_fill {
                    return Ok((total, false));
                }
                match self.fill(buf, back) {
                    Ok(0) => return Ok((total, true)),
                    Ok(_) => {}
                    Err(e) if total == 0 => return Err(e),
                    Err(_) => return Ok((total, true)),
                }
            }
            may_fill = !once;
            let window = buf.get(self.rpos..self.rend).unwrap_or(&[]);
            let window = window.get(..window.len().min(out.len() - total)).unwrap_or(&[]);
            let (n, found) = match find(window, delim) {
                Some(i) => (i + 1, true),
                None => (window.len(), false),
            };
            if let (Some(dst), Some(src)) = (out.get_mut(total..total + n), window.get(..n)) {
                dst.copy_from_slice(src);
            }
            self.rpos += n;
            total += n;
            // An empty window after a fill (no data room in the buffer) must not loop forever.
            if found || n == 0 {
                return Ok((total, true));
            }
        }
        Ok((total, true))
    }
}
