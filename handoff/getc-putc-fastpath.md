# GETC-PUTC-FASTPATH handoff (checkpoint, NOT READY)

State: branch task/getc-putc-fastpath, code commit eeccae4a (on origin/next 9e9f5067; the stacked
STDIO-EOF-PARTIAL-REFERENCE work already landed in next). Role keke:~/w/r3.

Done (eeccae4a): getc/putc byte forms with no lock to take trust the published window
(`ptr < end`, one byte via tb::window_byte / window_set_byte, category slice, then advance);
store publishes the read window bounded by the buffer and closed unless byte-oriented
(`published_read_window`), write window clamped to buf_len; locked forms unchanged (validated at
lock time); crafted-FILE tests parameterised (`trusted` = the _unlocked forms, since a test
process is never `alone()`); wide and unbuffered read-window checks in read_front; D-12.19
amended (GETC-PUTC-FASTPATH paragraph). clippy and static were clean on the role (foreground).

NOT VERIFIED / open:
- `cargo test -p liella-boundary --test s12_stdio_symbols` on r3 (run without `cargo xtask so`
  first) gave 67 FAILED (sym__IO_2_1_std*_, printf/scanf families, fam_*_on_library_threads)
  and ended with "double free or corruption" (SIGABRT). Closing the window only for wide
  streams (mode > 0) instead of mode >= 0 did not change the count, so that is not the cause.
  NO BASELINE WAS RUN: these may be "products missing" failures and not caused by this change.
  Next: run the same command on the before snapshot /dev/shm/claude/getc-putc-fastpath/before
  (origin/next source, needs `cargo xtask so`), or run the direct gate on r3; then bisect
  against this change if the baseline is green.
- No gate, plants or counts ran. The before bench job was queued and then cancelled (it would
  have measured the changed tree). ~/work/getc-putc-fastpath/bench.sh <dir>... runs the six
  target rows, default and hwcaps=-AVX2, in each tree (plain jobrun, not gate: /dev/shm).
- Plants still to write: refill publishing READ_END one past the data; getc_refill skipping
  Glue.record (read_crafted's empty-window EBADF cases must catch it); published_read_window
  ignoring the mode (read_front's wide check must catch it).
- Scope: fputs/fwrite/fgets/fread front paths are left per-call checked (stated in D-12.19).
- Saturation register row (frozen): add after counts exist.

Plan rows: D-12.19 (plan/07-decisions/s12-buffered-streams.md).
