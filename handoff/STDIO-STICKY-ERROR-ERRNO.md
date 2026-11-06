# STDIO-STICKY-ERROR-ERRNO: read entries started with an indicator already set leave errno alone

State: branch task/stdio-sticky-error-errno, code commit 120d409c (the handoff commit follows it). Role keke:~/w/n2.
Status: PENDING JOBS (the gate and the plants were running/queued at hand-back).

## Change
- `stdio_compat::getdelim` (so `getline`, `getdelim`, `__getdelim`): with the error indicator already set it now answers
  -1 without reading and leaves `errno` alone (it used to return `Err(EIO)`, which set errno).
- No other entry changed: the probe showed they already matched.
- Rule recorded in D-12.1 (amended, quoting the coordinator ruling): no read entry sets errno on account of an
  indicator; errno changes only when the call's own backing read fails. EOF indicator set: every entry answers end of
  file without reading. Error indicator set: getline/getdelim answer -1 without reading; the other entries read on, and
  the indicator stays set. ungetc works with either indicator.

## Probe-settled behaviour (black box, glibc 2.39 on keke; source in the scratch dir, sticky_all.c)
Entries: fgetc, getc, getc_unlocked, fgetc_unlocked, getchar, fgets, fread, fread_unlocked, getdelim, getline, fscanf,
ungetc, fgetwc, getwc, fgetws, fwscanf. errno preset to 4242. Backings: a cookie that fails (EIO) or ends once and
then has "xy\n"; the same with data already buffered after the error; a directory descriptor (every read is EISDIR).
- Error set, backing would succeed: the char/line/block/scanf entries return the data (errno kept, ferror stays 1);
  getdelim/getline return -1, errno kept, 0 reads. Same when data is buffered.
- Error set, backing fails again (directory): the entries that read on return failure with errno = EISDIR (the new
  failure); getdelim/getline return -1, errno kept.
- EOF set: every entry returns EOF/0/NULL/-1, errno kept, 0 reads; ungetc clears EOF and its byte is read back.
- Before the change Liella differed only in getdelim/getline (errno = EIO). After it, the in-process test passes.
  The reference's wide entries were measured on the directory backing only (see the hand-back for the cookie case).
- glibc is not uniform across entries (getdelim/getline stop, the rest read on). The single rule that holds for every
  entry is the errno rule above. Liella keeps glibc's per-entry stop/read-on behaviour, so no entry differs from glibc.

## Tests
- `crates/core/tests/s12_stdio_clauses.rs::s12_5_c_5_sticky_indicators_leave_errno`: a table over 14 non-varargs
  entries x both indicators (cookie backing; sentinel errno; result; reads asked; indicator kept). Passed on keke
  (job sticky-direct-2: 12 passed of the s12_5_c filter). The scanf family takes varargs and cannot be called there.
- Oracle: `crates/core/tests/oracle/stdio_diff.c` section `sticky_indicators` (all 16 entries, cookie, directory and
  regular-file EOF backings), compared line by line by `stdio_products.rs`. Its result is inside the gate run.
- clippy: clean. static: clean (keke ~/w/n2, after the sync of 120d409c).

## Jobs (keke ~/w/n2)
- gate: `sticky-gate-1` = `cargo xtask affected --base origin/next --run`; log ~/logs/sticky-gate-1.log; re-attach
  `~/bin/jobwait sticky-gate-1`.
- plants: `sticky-plants-1` = `cargo xtask plants handoff/STDIO-STICKY-ERROR-ERRNO-plants.txt --dir
  /dev/shm/claude/STDIO-STICKY-ERROR-ERRNO/plants-1`; log ~/logs/sticky-plants-1.log; re-attach `~/bin/jobwait sticky-plants-1`.
  Three plants, all on the getdelim check: return EIO again, set errno then return -1, read on past the indicator.
  The other entries' code is unchanged, so none of it is planted. NOT VERIFIED at hand-back: gate and plants results.

## Open / next step
- Collect sticky-gate-1 and sticky-plants-1. If the oracle section fails, it is an oracle mismatch the in-process test
  does not see (e.g. the regular-file EOF backing), so read the `sticky indicators` lines of the diff.
- INFORMATIONAL: a plant that sets errno in a read entry other than getdelim was not written (that code is unchanged and
  not reached by a cookie backing in the clause test); a follow-up could plant the fd fast path's sticky-EOF return
  through the products-rebuilding oracle.
- Process note: the code was appended to the clause test with a quoted bash heredoc (content verified by clippy and the
  test run); the rules ask for the Write tool for files with backslashes.
