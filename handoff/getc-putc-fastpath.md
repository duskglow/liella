# GETC-PUTC-FASTPATH handoff (GETC-PUTC-FASTPATH-2)

State: branch task/getc-putc-fastpath-2, code head d33eeac0 (on origin/next 9e9f5067). Role shiki:~/w/sh1.
Commits: eeccae4a (the change: the getc/putc byte forms with no lock to take trust the published window;
refill/store publish it bounded by the buffer and closed unless byte-oriented; D-12.19 amended),
d33eeac0 (test fix, below).

Decisions in force: D-12.19 (plan/07-decisions/s12-buffered-streams.md, GETC-PUTC-FASTPATH paragraph).

What the previous run's "67 FAILED + double free" was: the products were missing (no `cargo xtask so`),
plus a real test bug. getc_forms / putc_forms still expected the `_unlocked` forms to refuse a window the
program widened. The amended D-12.19 no longer checks that per call: putc_unlocked stored past the buffer
and corrupted the test process's heap (the "double free"). Fixed in d33eeac0. The locked forms keep the
refusal cases. getc `_unlocked` now proves that the window is taken as the program left it (a byte placed
just past the data is read; once the window is restored, the next read gives EOF). The past-the-buffer putc
case runs for the locked forms only.

Jobs on d33eeac0 (shiki ~/logs/jobs.tsv):
- getc-putc-fastpath-2-gate2: direct tier, --base origin/next, rc 0. 9 suites, 4918 passed, 0 failed;
  peak 9.6G.
- getc-putc-fastpath-2-plants2: rc 0. Baseline 355 passed; all 4 plants CAUGHT with the expected
  message, no WARN lines: refill window end one past the data, getc_refill skipping Glue.record, read
  window open on a wide stream, write window open on a wide stream.
  Script: shiki ~/work/getc-putc-fastpath-2/plants.sh.
- getc-putc-fastpath-2-counts2: cbench_stdio, before (origin/next source) and after, default and
  hwcaps=-AVX2. putc 47.2 -> 29.2 instructions:u (reference 32.1; 1.47x -> 0.91x), the same with AVX2 off.
  Other rows: fwrite_4k +0.7 (default) / +1.2 (-AVX2) of 1189 / 1480 (+0.06%); snprintf_mixed -AVX2 +0.1.
  Likely cause: the write-window clamp `min(buf_len)` in store_words, once per flush (not bisected).
- getc-putc-fastpath-2-counts3: cbench_hot (getc, getc_unlocked, putc_unlocked, fgetc, fputc rows),
  before/after. It was still running at hand-back. Read it with:
  `grep -E "^== |getc|putc" ~/logs/getc-putc-fastpath-2-counts3.log` on shiki.
- clippy (--workspace --all-targets --all-features -D warnings) and `cargo xtask static`: clean on sh1,
  in the foreground.

NOT DONE / open:
- wl_count (make-n, cut-tr-uniq, gawk-printf) cannot run on shiki: "wl_prep: no link command for cut"
  (and gawk). The base 9e9f5067 is primed there now. Run it on a host with the wl kit (keke).
- fwrite_4k +0.06%: either accept it (the clamp sets up the invariant at the boundary), or drop the write
  clamp if the engine's wpos/wend <= buf_len can be shown by other means. RULING NEEDED in the report.
- Saturation register row (frozen; for the coordinator to land): "§12 getc/putc fast path
  (GETC-PUTC-FASTPATH, D-12.19): per-call seal/bounds/mode checks moved to the refill/flush boundary and to
  lock time; putc 47.2 -> 29.2 instructions:u (reference 32.1); getc rows: see counts3."
- Scope: the fputs/fwrite/fgets/fread front paths are still checked per call (stated in D-12.19).

Next step: read counts3, run wl_count on keke, and get a ruling on the fwrite_4k +0.06%.
