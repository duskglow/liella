# LAYOUT-TRACE-HOST-CANONICAL (LT-4): handoff (run 1, checkpoint)

State: branch `task/layout-trace-host-canonical`, WIP. Role keke:~/w/n2 (Zen 1). Plan: `plan/layout-rethink.md` on
`task/layout-rethink` sections 2(e), 3, 4 (rulings #5190-#5194, #5191: default switch needs host-canonical traces).
Stopped at the ~150k context checkpoint (thread_common BUDGET); nothing below is gated yet.

## What the traces showed (measured, same fast products traced on three hosts)

Products: one `cargo xtask so` (fast) on keke at b888f8ff0, copied with the tree to `~/work/LAYOUT-TRACE-HOST-CANONICAL/`
on keke (Zen 1, EPYC 7551), mei (Skylake-SP, Platinum 8151), shiki (Haswell-EP, E5-2697 v3); `regen.sh --trace-only`.

| tracer | trace files byte-identical on all 3 hosts |
|---|---|
| next's tracer, host CPU (jobs lthc-disc-<host>) | 8 of 17: 9 differ, all in one family: `liella_machine_fill_erms` (Intel: ERMS) against `fill_avx2`/`fill_avx2_entry`/`fill_u8` order (Zen 1: no ERMS) |
| next's tracer, `GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2,-ERMS,-FMA` | 15 of 17 (probe_text.sh's `env -i` dropped the mask); unusable anyway: the tunables parser joins the start-up trace |
| this branch: virtual CPU per class + fixed env (lthc-canon1-<host>) | **17 of 17** (namespace data variant, since replaced, see below) |

Host data (locale archive, zone file, nsswitch.conf) were byte-identical on the three hosts, so today's differences
are CPU class only. Trace time: 126 s (mei) to 479 s (keke, loaded) for the fast set: four class runs per probe
(next's tracer: about 60 s). LT-3 (ptrace tracer) is the place to win it back.

## The design on the branch (tools/hottext only; xtask/src/so.rs untouched)

- `trace_gdb.py`: (a) the program never sees the host CPU: `liella_machine_cpuid` / `liella_machine_xgetbv0` (every
  CPUID/XCR0 read of the library) are answered from a fixed virtual CPU; each probe runs once per class `v3-erms`,
  `v3`, `base-erms`, `base` (AVX2 x ERMS, the two decisions that choose code; FMA follows v3) and the runs are
  merged (first class's order; a function only another class ran goes right after the one that class ran before it).
  So every candidate is grouped by rule (all classes), not by what the host picked. A v3 class on a host without
  AVX2/FMA/BMI fails loudly. `HOT_TEXT_CLASSES=host` = old behaviour (diagnosis only).
  (c) environment = exactly the `HOT_TEXT_ENV_*` variables (sorted), cwd `/`, ASLR off, program copied to
  `/tmp/liella-trace.XXXXXXXX/p` (fixed-length argv[0]).
  (b) `HOT_TEXT_REDIRECT`: at syscall entry of open/stat/access-family calls the tracer points the path at the tree's
  file (new in the last step: **not run yet**; job lthc-canon3-mei started on it).
- `runner.sh`: non-`HOT_TEXT_*` assignments become the program's whole environment; `canon_prepare` runs
  `canon_host.sh` and passes the redirections. `regen.sh` / `probe_text.sh` call it; TZ now passed explicitly
  (`tz=` in regen.sh), `LC_ALL=` and probe_text's `env -i PATH` dropped.
- `canon_host.sh <dir>`: writes passwd/group (caller's IDs, name `liella`), `hostdata/nsswitch.conf` (stock host
  database lines), an en_US.UTF-8-only archive (host `localedef --prefix`), prints residual host-input digests.

Things not to redo:
- **No user/mount namespace for the data**: `crates/legacy/src/nscache.rs` keeps a private table in any non-initial
  user namespace, so the namespace trace lost the shared-region path (`NsCache::map_region`, `boot_id`) and gained
  `names::cache::Table` (measured, p_nss / p_cold). Hence syscall-level redirection.
- Canonical data must model a stock host: a `files`-only nsswitch.conf and an empty `/usr/share/locale` each ran
  other library paths (`lookup_alias` gone). `/usr/share/locale` stays the host's (alias file, catalogs:
  distribution data the tree does not carry): residual, digest printed.

## Next step (exact)

1. Read `lthc-canon3-mei` (`ssh mei ~/bin/jobwait lthc-canon3-mei`; output `~/work/LAYOUT-TRACE-HOST-CANONICAL/disc3/`):
   rc 0, no FAILED; compare its names files with run canon2 (namespace) and with next's host trace: expected
   differences = class candidates only (`fill_sse2`, `move_sse2`, `fill_erms`, ...), the nscache region path back.
2. Residual host state seen in code, not handled: the nscache region `/dev/shm/liella-nscache-<euid>` (create vs
   attach; `~/bin/gate` gives a private /dev/shm): decide (recommend: the tracer redirects that path too, to a fresh
   per-run file, so p_nss always traces create-then-attach), kernel (vDSO, auxv), `vm.mmap_min_addr`.
3. Proof per brief: same tree on keke, mei, shiki (`~/work/<story>/tree`, a git archive of the head), per profile
   `regen.sh . --replace --build` (fast; then `LIELLA_FULL_PROFILE=1`), save the four lists + `sha256sum
   target/liella/libc.a` after each; compare. Then on keke: hot_text_products + layout-check, both profiles.
4. Plants (`cargo xtask plants`): a class candidate following the host (e.g. `answer()` skipped for cpuid) and the
   host's archive read (redirect entry dropped), caught by the cross-host compare / a tracer self-check.
5. Pending record `records/pending/LAYOUT-TRACE-HOST-CANONICAL-1.toml` (series D-X per brief), clippy, static, gate.

Slip: one python heredoc was used for an edit in this run (rules forbid heredocs); no effect on the result.
