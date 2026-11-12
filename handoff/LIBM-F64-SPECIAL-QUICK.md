# LIBM-F64-SPECIAL-QUICK(-2, -3, -4): erf/erfc quick phase (part 1 of the special-function quick phases)

**Superseded by LIBM-F64-SPECIAL-QUICK-REST (2026-11-11): see handoff/LIBM-F64-SPECIAL-QUICK-REST.md.** The SSE2
erf_series +1 (item 1 of "Not done" below) is fixed there (154 -> 151, D-10.90); lgamma/tgamma/Bessel are profiled
there and continue in LIBM-F64-SPECIAL-QUICK-REST-2.

State (-4, 2026-11-11): branch `task/libm-f64-special-quick-4` = -3's head d4376b95b merged with origin/next
ca8aa835a (merge 5350fa888, no conflicts). No code change in -3 or -4. Role keke:~/w/r1 (Zen 1). Decision D-10.84
(`records/decisions/D-10/D-10.84.toml`); also D-10.13, D-10.51. Plants: handoff/LIBM-F64-SPECIAL-QUICK-2-plants.txt.

## -4: the start-up fault bounds hold on the merged head (the layout data-cluster work is in next)

`cargo xtask so && cargo test -p liella --test hot_text_products -- --nocapture` on the merged head (keke, fast
profile, job `libm-f64-special-quick-4-hottext2`): 21 passed. true **12** (bound 17), setlocale + printf **52** (55),
coreutils-shaped tool **55** (58), fork child 6 (7), localtime 19, float 3; every run equal to its warm-up. The -3
reds (18/57/60) are gone, as -3 predicted once the start-up data objects are keyed. (First attempt
`-hottext` failed only because the fresh role had no products: `ld: cannot find target/liella/libc.a`; superseded.)
Then the -2 gate (records check, workspace clippy -D warnings, xtask clippy, static, `affected --run --base
origin/next`) and the -2 plants file on this head: jobs `libm-f64-special-quick-4-gate`, `-plants` (results in the
report, handoff/reports/LIBM-F64-SPECIAL-QUICK-4.json).

## -3: the start-up page-fault reds (CI trains 582/583) are a codegen/phase effect, not erf placement

Measured on keke, fast profile (the test profile), next aad3df74d built outside the role at a path of the same length and the merged head in the role. Scripts and outputs: keke `~/work/LIBM-F64-SPECIAL-QUICK-3/`
(`measure.sh`: products, `hot_text_products --nocapture`, the probe linked as the test links it, `perf record -e
page-faults -d`; `attr.py`: fault address -> section, symbol; `members.py`; `pads.sh`: ld -r pad in crt1, as
layout-stability-rethink's tool).

* `hot_text_products` (true / locale / tool): next 17 / 55 / 58 (exactly the bounds), merged head **18 / 57 / 60**
  (bounds 17 / 55 / 58). Same as CI on mei. Every other test in the file passes on head (fork 7, localtime 19, float 2).
* **No fault lands on erf code or data.** libc.a is one merged object, so all of libm is linked into the probe; the
  change adds 9466 B of text (.text 0x3519e7 -> 0x353da7) after the hot group, which moves .rodata and the data
  segment up 0x2000. `.text.unlikely` is unchanged (154 inputs, 47007 B): the `#[cold]` fallbacks do not sit before
  the group, and the hot group starts at the same address (0x40dbc0). No erf function has a hot key.
* The extra faults (perf, addresses after exec): (1) true/locale/tool: `.data` objects reordered, `__libc_single_threaded`
  and `_IO_2_1_stdout_` moved from ERMS_LARGE_FROM's page (next 0x84d3e8, before it) to their own page (head
  0x84f45c, after it, with 20 other objects between); (2) locale/tool: `liella::locale::find_cached<global_locale::Files<
  Glue>>` is now an out-of-line function in the cold text (0x722240; in next it is inlined, no symbol), one more 64 KiB
  text window. Both come from `liella_boundary`'s fast-profile codegen (thin LTO, 16 codegen units): the generic erf
  kernels are `#[inline(always)]` and the FMA ones `#[inline]`, so the quick phase is compiled inside boundary's
  codegen units. Their larger size reorders those units (data order) and changes inlining elsewhere (symbol diff: 18
  named functions differ, e.g. spawn::execve, fork::clone, find_cached).
* **Pad sweep (ruling #4347's test; `-pads`):** next with a cold-text pad 0..0xC000 reads true 17-18, locale 53-56,
  tool 56-59, and a pre pad (`.text.unlikely`) of 0x2000 gives true **18** (over 17). Next itself crosses the bounds
  with layout alone. Head: pad 0 reads 18/57/60, pads 0x800-0x3000 cold read **17/55/58** (inside every bound),
  0x8000-0xC000 read 57-58/60-61. At equal pads head reads about +2 in locale/tool against next (the reordering above).
* Verdict: LAYOUT-JITTER under ruling #4347 (the change touches no start-up code; the faults come from codegen-unit
  order and the text phase). Not chased by moving sections, padding or a list regen (ruling #4347), and no bound
  raised. The fix that removes both faults is structural: `__libc_single_threaded`/`_IO_2_1_stdout_` in the start-up
  data cluster (LAYOUT-DATA-CLUSTERS; the regen's perf trace sees only the first touch of a page, so objects sharing an
  already-faulted page never get a key) and `find_cached`'s instantiation keyed when it is out of line (hot-text regen),
  plus LAYOUT-TEXT-PIN for the phase.
* Option not taken (RULING NEEDED): build the erf entries so their bodies stay in `liella_math`'s codegen units (not
  `#[inline]`/`#[inline(always)]` at the boundary-facing entry). Boundary's partition would stop depending on libm
  kernel size, but it would first change once (no guarantee the counts land inside the bounds), and in the release
  build (fat LTO) a non-inlined entry costs a call per erf (an estimated +2 to +5 instructions, so SSE2 erf_series
  goes further above next's 153). Not measured.

## What is in

* `crates/math/src/fast_special.rs`: the quick phase (`erf_quick_dd`, degree 12, Estrin, bound 2^-62 / erfc on (0,1)
  2^-61, proved per slot by `special_tables::quick_tests`) now runs INSIDE `erf_nearest`/`erfc_nearest`'s own
  `|x| >= 2^-5` branch (helpers `erf_quick_near`, `erfc_quick_near`); `erf_dd` split into `erf_table_dd` /
  `erf_small_dd`; the table fallback (`erf_table_near`, `erfc_table_near`) is `#[cold] #[inline(never)]` because
  sharing one body made LLVM keep the ERF_T row loads live across the quick phase (SSE2: ~10 stack spills, one-call
  trace, 172 vs 147). `erf_tiered`/`erfc_tiered` are gone; `hot/erf.rs` calls `*_nearest` again.
* Test `quick_phase_rounds_correctly` runs 10 hard cases (quick `(zh, zl)` on or within 2^-12 ulp of a midpoint, exact
  value on the other side; a 2^28-operand search found 3867, ~1 in 69000, worst 4.9e-4 ulp, inside the bound). The
  rounding-test plant is now CAUGHT (#4533).
* erfc percall MISMATCH (#4534) explained: `erfc(0x1.b12e88p+0)` (the erfc row's ex[17]): Liella 0x1.11c9655380463p-6,
  reference ...462p-6; libquadmath erfcq and erfcl both put the exact value 0.6948 ulp above ...462, so Liella's is the
  correctly rounded one (not a defect; on next too). `validation/percall/libm_erf.c` self-check now prints `%.15g`
  (as the long double probes); `crates/boundary/tests/erf_quick.rs::erfc_probe_operands_round_correctly` pins both
  checked operands in both builds. All check columns now `ok`.

## Counts (mei, Skylake-SP; instructions per call; glibc / next 7d31b0621 / head 130f3f328; job -counts3)

| case | FMA | SSE2 (`hwcaps=-FMA`) |
|---|---|---|
| erf_table | 159.3 / 162.0 / 120.0 | 174.0 / 235.0 / 148.0 |
| erf (bench) | 157.9 / 162.0 / 120.0 | 172.6 / 235.0 / 148.0 |
| erfc_below1 | 162.4 / 173.4 / 128.9 | 177.1 / 249.1 / 159.4 |
| erfc (bench) | 162.4 / 197.4 / 167.2 | 177.1 / 307.0 / 245.8 |
| erfc_above1 | 194.3 / 244.0 / 244.0 | 214.1 / 422.0 / 422.0 |
| erf_series | 63.0 / 109.0 / 97.0 | 63.0 / 153.0 / **154.0 (+1)** |

**Open (why INCOMPLETE): SSE2 erf_series is +1 against next** (FMA is -12). Ratchet: it must return to 153. The
tiered version (b3dfc3948's first form with shared finish) measured 153 but spilled on the table path; moving the
series branch first (130f3f328) did not change it. Next step: trace one SSE2 erf_series call on head and on next and
diff the streams. The head trace works (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA bash validation/wl_trace.sh --list
--spec validation/percall/libm_erf.c . erf_series erf` in the role); next's needs a git tree (wl_trace refuses the primed
`~/wl/src/<sha>` dir) or a disassembly of `~/wl/products/7d31b0621.../` (`erf_nearest`'s series block). Candidates: the
`?` early return of the cold call adding a block merge, or `erfc_low_near`/sign selection order.

## Jobs (mei)

`-lint0` (inherited head: clippy + static, rc 0), `-search1` (hard-case search, evidence), `-counts3`, `-gate3`
(records check, the changed tests, clippy, static, `affected --run --base origin/next`), `-plants3` (2 plants).
Superseded: `-gate`, `-plants`, `-counts` (b3dfc3948: SSE2 spills), `-gate2`, `-plants2`, `-counts2` (9f90d407a), `-trace`,
`-trace2` (diagnostics). Scratch probe for the next phase (not committed, untested): a per-call spec for lgamma/tgamma/
j0/j1/y0/y1 at the bench's ranges was drafted (validation/percall/libm_special.c shape as libm_erf.c, cases
lgamma/_pos/_neg, tgamma/_pos/_neg, j0, j1, y0, y1); add it and profile both builds first.

-3 jobs (keke): `libm-f64-special-quick-3-lint1` (merged head d8099c0f2: workspace clippy -D warnings, xtask clippy,
static; rc 0), `-faults-next` and `-faults-head` (evidence above; rc 0 is the script, the head's test reads FAILED
18/57/60 inside its log), `-pads` (pad sweep). Superseded: `-lint0` (the inherited head 9bf232d73 before the merge, rc 0).

-4 jobs (keke): `libm-f64-special-quick-4-lint0` (inherited head d4376b95b: xtask clippy + static, rc 0),
`-hottext2` (fault bounds above), `-gate`, `-plants` (merged head). Superseded: `-hottext` (no products built).

## Not done (next: LIBM-F64-SPECIAL-QUICK-REST, which builds on this branch)

0. Exact next step: this branch lands; then LIBM-F64-SPECIAL-QUICK-REST starts with item 1.
1. SSE2 erf_series +1 (above; owner precedent #4424 accepted it for landing, fixed first in -REST). 2. Booked timing Ivy Bridge / N100 for erf/erfc (target <= 1.3x IVB). 3. erfc above 1
(`e^(−x²)·Y`; plan in D-10.84's "Not in this decision"; quick Y from ERFCX_T restricted to x < 8, `e^(−x²)` from the
exp quick kernel with `zl` as `y·(1 + zl)`). 4. erf series below 2^-5 (SSE2 153 vs 63). 5. lgamma, tgamma, Bessel four.

## Not to redo

Degree 11 does not meet 2^-60 on ERF_T (2^-57.9); Horner-to-P2-sum-last stalls at 2^-61.03. Uniform operands do not
catch a skipped rounding test (about 1 hit in 69000): keep the hard cases. Quick phase and table path in one body
spills on SSE2: keep the fallback cold and out of line. Series-first block order alone does not fix the +1.
