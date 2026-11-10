# LAYOUT-PLACEMENT-SWEEP: operand placement as a contract (bench offsets, kernel cost over page offsets)

State: branch `task/layout-placement-sweep` (stacks on LAYOUT-MANIFEST-2, ddc2dbdaf), role mei:~/w/s1 (Xeon 8151,
Skylake-SP). Code commits: bench asserts; 244e5b7f2 sweep test + D-X.52 + plants file; 4a813ea96 host name -> processor class. Plan: handoff/layout-contracts-plan.md L15, L16.
Decision in force: D-X.52 (plan/07-decisions/x-implementation-decisions.md row, as D-X.48 on this branch; the landing
moves it to records/decisions/D-X/D-X.52.toml with the rest).

## What was done
- L15: `bench/c/cbench_hot.c`: the operand block is `struct hot_bufs`; `_Static_assert`s on `__alignof__(bufs) == 4096`
  and, per buffer, `offsetof % 4096 != 0 && <= 4032`. A reorder or a new buffer at a bad offset fails the bench build.
  No manifest row BENCH-OPERAND-OFFSETS: LAYOUT-MANIFEST is not on origin/next (03c9a4972), and the manifest's kinds
  are checks on the products, so a bench row also needs a new kind (see informational).
- L16: `crates/boundary/tests/placement_sweep.rs` (no spec clause): a static program built with liella-cc counts user
  instructions per call (perf_event_open, 64 calls per placement, rounded) for strcasecmp/strncasecmp (L 256 / n 64),
  strcmp/strncmp (256 / 64), memcmp 256, strlen 256, strnlen 256 (max 320), memchr 256 (miss), rawmemchr 256, with
  each operand at page offsets {0,1,15,16,31,32,4032,4064,4065,4080,4095} and every pair (two-operand kernels: 121
  placements). Two bounds per kernel, worst/median: all placements (K_all) and head placements, every offset <= 32
  (K_head). SKIP (printed) without AVX2 or when perf_event_open is refused.

## Calibration (origin/next code, mei, job lps-cal1; worst/median in percent, all / head)
| kernel | median | worst (at a/b) | all % | head median | head worst | head % | K_all / K_head |
|---|---|---|---|---|---|---|---|
| strcasecmp | 302 | 848 (4095/4065) | 280 | 267 | 279 (32/0) | 104 | 310 / 115 |
| strncasecmp | 133 | 701 (4095/4065) | 527 | 88 | 100 (32/0) | 113 | 580 / 125 |
| strcmp | 211 | 900 (32/4065) | 426 | 150 | 196 (0/32) | 130 | 470 / 140 |
| strncmp | 123 | 579 (4095/4032) | 470 | 53 | 68 (32/0) | 128 | 520 / 140 |
| memcmp | 71 | 71 | 100 | 71 | 71 | 100 | 110 / 110 |
| strlen | 87 | 109 (4064) | 125 | 87 | 87 | 100 | 140 / 110 |
| strnlen | 110 | 139 (4064) | 126 | 110 | 110 | 100 | 140 / 110 |
| memchr | 103 | 106 (32) | 102 | 106 | 106 | 100 | 115 / 110 |
| rawmemchr | 93 | 97 (4095) | 104 | 93 | 93 | 100 | 115 / 110 |
One K for all kernels would have to be about 530% (strncasecmp) and could not see a casecmp regression, so K is per
kernel: the measured spread plus about 10%. Bounds only tighten (owner 11-07).

## Results
- Plants (mei job lps-plants, tree 244e5b7f2; the final head 4a813ea96 changes comments only): 2 CAUGHT, 0 SURVIVED,
  no WARN. lps-casecmp-head-generic (scan.S, exact head test -> .Lcord_generic): "strncasecmp: 132 instructions at
  head placement 32/0 against a head median of 88 (bound 125%)" (strcasecmp at 256 bytes moves only +12, under its
  bound; the n64 row is the one that sees it). lps-bench-reorder (a64 before the lead): bench build fails with
  "a64 at page offset 0". Both baselines passed (the sweep is green on origin/next code with these bounds).
- Gate: lps-gate (244e5b7f2) red at static lab-names (host name in a comment; fixed in 4a813ea96). lps-gate2
  (--base origin/next) escalated to FULL (the stacked LAYOUT-MANIFEST xtask commits): killed (developers run the
  direct tier). lps-gate3 (--base ddc2dbdaf, the parent story's head) ALSO escalated to FULL; it is left running
  for the dispatcher (mei:~/logs/lps-gate3.log; re-attach `~/bin/jobwait lps-gate3`). Why the direct tier escalates
  for this change is not yet known (the selection output lists placement_sweep.rs just before "FULL (escalated").

## Open / next step
- Not verified on the Zen 1 hosts (keke): counts are of the same AVX2 kernels, but a different CPU class could pick
  another kernel level. If a Zen 1 gate trips a bound, measure there and take the larger spread (owner ruling if it loosens).
- Placement spreads worth a story (measured above): strcmp 900 at 32/4065 (426%), strncmp 579 (470%), strncasecmp 701
  (527%), strcasecmp 848 (280%); also strcmp's head spread (130%: an operand at page offset 0). Each fix tightens its K.
- Manifest: a `build-assert` kind (the compiler is the check; the row names the file and the assert count) would let
  L15 be a manifest row once LAYOUT-MANIFEST lands.
