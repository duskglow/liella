# DL-BOOT-RELOC (phase 14 DL-4): self-relocator, first used by libc.a's static-PIE start

**State:** branch `task/dl-boot-reloc-7` (continues dl-boot-reloc-6; origin/next 66b8d3ba merged in at 4d5c2738),
code head `4c1313d4`. Role kanon:~/w/kn1. Verdict: ready to land once the FULL gate and plants named in
handoff/reports/DL-BOOT-RELOC-7.json are green.

## DL-BOOT-RELOC-7: CI conflict with origin/next
* Conflict (CI merge_next rc 3): next added `#[cfg_attr(not(feature = "runtime"), allow(dead_code))]` to
  classed::start and nscache::start (RPC-STUB-HEAP-CHECK-ISOLATE) where -6 added `#[inline(always)]`. Resolved by
  keeping both attributes (merge commit 4d5c2738). Next's other edits in runtime.rs (compat_from_start call removed),
  sys.rs and ffi.rs merged cleanly; thread::init_initial and heap::perturb_from_start keep `#[inline(always)]`.
* Counters, one Broadwell host (kanon, E5-2699 v4), `cargo xtask bench --counters --only cbench_startup`, user-mode
  instructions per process (startup_instr_c / printf / setloc / wctype):
  - origin/next 66b8d3ba: **2152** / 71909 / 68115 / 69165 (job `dl-boot-reloc-7-bench-next`)
  - merged branch 4d5c2738: **2148** / 71897 / 68103 / 69153 (job `dl-boot-reloc-7-bench-branch`): every row below
    origin/next, startup_instr_c at the 2148 target. No row moved against expectations, so no trace was needed.
  - Note: origin/next itself measures 2152 here (above CI's 2150 Zen 1 limit if Zen 1 tracks it); this branch is 4 under.
* Plants: handoff/DL-BOOT-RELOC-7-plants.txt = -6's six `equivalent:` plants, with the classed::start and
  nscache::start patterns updated to skip next's cfg_attr line (the old patterns no longer matched). Job
  `dl-boot-reloc-7-plants` rc 0: 6 EQUIVALENT, 0 SURVIVED, no WARN lines.
* clippy (`--all-features`, and `cargo xtask clippy`) and `cargo xtask static` clean on the inherited head (job
  `-lint0`) and on the merged head (job `-lint1`).

## DL-BOOT-RELOC-6: the instruction regression (CI train 510: startup_instr_c 2200 > 2150)
* Measured on one Haswell host (shiki, E5-2697 v3), `cargo xtask bench --counters --only cbench_startup`,
  user-mode instructions per process (startup_instr_c / printf / setloc / wctype):
  - merge-base 71ffff12: **2148** / 71898 / 68112 / 69151 (job `dl-boot-reloc-6-bench-base`)
  - branch 9c73a0ad: **2200** / 71941 / 68156 / 69196 (jobs `-bench-branch`, `-bench-branch2`; same as CI's 2200)
  - branch cc686672 (fix): **2143** / 71895 / 68089 / 69137 (job `-bench-fix2`): every row at or below the merge-base.
* Attribution: a ptrace single-step trace of the bench child (`cbench_startup true`, exec to exit, one RIP per
  instruction; deterministic, 2667 steps on the merge-base) mapped to symbols. Branch: runtime::start::<false> 1306
  (-69), and five new out-of-line functions: thread::init_initial 62, nscache::start 31, heap::perturb_from_start 22,
  classed::start 14 (+ classed::install_with 12 once classed::start was inlined). Cause: making `runtime::start`
  generic over PIE gave each one-call-site helper two callers (start::<false> and start::<true>), and LLVM stopped
  inlining them into the non-PIE start. Nothing of the relocation walk runs on the non-PIE path.
* Fix: `#[inline(always)]` on those helpers and on classed::decide (one caller each, comment cites D-27.10). After
  it the trace has exactly the merge-base's functions; only runtime::start's own count differs (1375 -> 1369).
* Rejected: relocating in `liella_start_main_static_pie` before a non-generic `start` (adds a test before the
  IRELATIVE pass on the non-PIE path, and a static PIE without PT_PHDR would lose its TLS/RELRO bias fix).
* Tools (scratch, not in the repo): shiki ~/work/DL-BOOT-RELOC-6/stepcount.c, attrib.py, *.attr. perf cannot sample
  `instructions:u` on that host ("Invalid event in per-thread mode"); the single-stepper is exact and cheap.
* Plants: handoff/DL-BOOT-RELOC-6-plants.txt, six `equivalent:` plants (removing each attribute changes no behaviour;
  its cost is guarded by CI's Zen 1 instruction limit, which xtask checks on that CPU only).

## Earlier runs (reports in handoff/reports/)
* -5: CI red was s29_rpc_clnt_svc_stubs' own race (reproduced on origin/next alone); ruling #3693 opened
  RPC-STUB-HEAP-CHECK-ISOLATE. -4: origin/next merged, liella-rcrt1.o documented, FULL gate rc 0. -3: link-time
  selection (`liella-rcrt1.o` -> start::<true>, `__libc_start_main` -> start::<false>); plants 24/24 CAUGHT
  (handoff/DL-BOOT-RELOC-plants.txt). -2/-1: the relocator (crates/ld/src/boot.rs), PC-relative anchor.

## Open / next
1. Land after the FULL gate (`kanon:dl-boot-reloc-7-gate`) and plants are green (merge origin/next again only if
   CI reports another conflict; keep both sides' attributes on the start-up helpers).
2. startup_instr_c headroom is thin: origin/next measures 2152 on Broadwell (Haswell merge-base of -6: 2148) against
   the 2150 Zen 1 limit; this branch runs 4 fewer. Any branch adding a few start-up instructions will trip it.
3. (Done on next by RPC-STUB-HEAP-CHECK-ISOLATE: the cfg_attr dead_code tidy.)
4. Optional: a conformance check that RELRO is read-only in a static PIE. PT_PHDR load offset: STATIC-PIE-LOAD-OFFSET.

Decisions: D-27.10, ruling #3670 (zero added instructions on the non-PIE start). Plan rows: design §0a (item 9), §1;
phase-14 item 8; R14-1.
