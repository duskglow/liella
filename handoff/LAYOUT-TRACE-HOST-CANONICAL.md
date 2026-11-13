# LAYOUT-TRACE-HOST-CANONICAL (LT-4): handoff (run 2, LAYOUT-TRACE-HOST-CANONICAL-2, context checkpoint)

State: branch `task/layout-trace-host-canonical-2` (on top of run 1's `task/layout-trace-host-canonical`), role mei:~/w/s3
(Skylake-SP). Spec: `~/state/briefs/LAYOUT-TRACE-HOST-CANONICAL.md` "The change"; plan `plan/layout-rethink.md` (branch
task/layout-rethink) 2(e), 3, 4; rulings #5191, #5222 (cost: owner 11-13 says correctness first, cost does not gate),
#5223. Decision: pending record `records/pending/LAYOUT-TRACE-HOST-CANONICAL-2-1.toml` (D-NEW, series D-X; the whole
design, one place). Code: tools/hottext only (trace_gdb.py, canon_host.sh, hostdata/, runner.sh, regen.sh,
probe_text.sh); xtask/src/so.rs untouched.

## Run 2 (this run)

1. **p_cold cut short (lthc-canon3-mei, #5226) - fixed.** With the syscall catchpoint on, the first cancellable
   `openat` stops at the instruction after `syscall`, which is the label `liella_machine_cancel_window_end` carrying a
   one-shot breakpoint not yet hit; gdb resuming that stop over the breakpoint ended the program (seen: "No frame",
   trace ends at cancel_window_end). Now: a syscall stop is told by `%orig_rax` (call number; -1 at a breakpoint trap),
   entry/return toggled per thread; the breakpoint at the stop pc is deleted at entry and its function counted at the
   return; redirection also applies when the stop pc is a label. p_cold 407 -> 482 names, regen's call check passes.
   (gdb's `to_string` output does not carry the catchpoint report: do not detect stops from it.)
2. **#5223 nscache region:** `/dev/shm/liella-nscache-<uid>` -> `@run/nscache`, a fresh file per program run (tracer
   empties `<stage>/run` before each class run); unlink/unlinkat are redirected too.
3. **Two self-checks (the plants' catchers):** (a) any path the program opens/stats/unlinks that is neither redirected
   nor a residual input (HOST_OK: /usr/share/locale, locale-langpack, zoneinfo, /proc, /sys, /dev/null, /dev/urandom,
   /tmp, the run dir) fails the trace; it found p_nss reading **/etc/resolv.conf, /etc/host.conf, /etc/gai.conf**: the
   tree now supplies stock copies (tools/hottext/hostdata). (b) an AVX2 class and a non-AVX2 class with identical runs
   fail (every probe differs at least by `liella_machine_xgetbv0`; measured per class on 6 probes).
4. **Proof on the intermediate head e30dd53** (before 3; jobs lthc2-proof-<host>): fast `regen.sh . --replace --build`
   on Skylake-SP and Haswell-EP: **all four lists byte-identical and libc.a 412c17ffe4abfda5..., libc.so.6
   f55cc779eb7a8c84... identical**; Zen 1 not finished (stopped for the final-head proof). The release step
   (`LIELLA_FULL_PROFILE=1 regen.sh . --build` after a fast `--replace`) failed in its own product build: release
   products from fast-only lists break STARTUP-TEXT-WINDOWS and STARTUP-DATA-WINDOWS before any trace. Not the tracer:
   the one-profile path cannot start release from a fast-only list. The final proof uses `regen.sh . --both` instead.
   Trace time (fast regen incl. two builds): 492 s Skylake-SP, 895 s Haswell-EP (loaded).

## Jobs started on the final head (read these)

- mei: `lthc2-plants` (handoff/LAYOUT-TRACE-HOST-CANONICAL-2-plants.txt, 5 plants), `lthc2-gate` (affected --run --base
  origin/next).
- keke, mei, shiki: `lthc2-proof2-<host>` (`~/work/LAYOUT-TRACE-HOST-CANONICAL-2/proof2.sh`, output out2/, log
  `~/logs/lthc2-proof2-<host>.log`): `proof:` lines carry the list and product digests.

## Next step (exact)

1. Collect the jobs. Compare the `proof:` digest lines of the three hosts (fast lists + fast libc.a/libc.so.6; --both
   lists + target/product/regen/{fast,release}/libc.a). Identical = the brief's proof; any difference: diff the lists
   (`out2/<step>/`), then the per-trace names files, and find the input (canon_host digests line).
2. Bounds and contracts with the canonical list on one host, both profiles: in tree2 after proof2, `cargo xtask so`
   and `LIELLA_FULL_PROFILE=1 cargo xtask so` (layout contracts) plus `cargo test -p liella --test hot_text_products`.
   If the lists should land, commit the regenerated lists from tree2 (they are generated: copy, never edit).
3. Plants: all five must be CAUGHT; list any WARN line.
4. Open: the release one-profile regen path from a fast-only list (item 4 above): regen.sh `--build` with release
   could pass `--hottext-regen` contract relief, or the docs should say "use --both"; RULING NEEDED if it matters.

Things not to redo: namespaces for data (nscache private table); `files`-only nsswitch or empty /usr/share/locale (other
paths); detecting syscall stops from gdb's printed output or from the pc.
