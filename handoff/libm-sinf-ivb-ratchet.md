# LIBM-SINF-IVB-RATCHET(-2, -3, -4): handoff (sinf's Ivy Bridge loss from the bit-built trig operand, owner #4424)

State: branch `task/libm-sinf-ivb-ratchet-4` (stacks on -3, -2, -1; origin/next merged in -4), code unchanged since -3.
Role mei:~/w/s1 (-4); kanon:~/w/kn1 (-3).
Decisions: D-10.67, D-10.68, D-10.72 in force; D-10.79 (amends D-10.67; rulings #4434, #4445) with the measurements.
Pending saturation record records/pending/LIBM-SINF-IVB-RATCHET-2-1.toml (SR-NEW, section 1: the NEGATIVE Estrin
trial; the coordinator assigns the SR id). Reports: handoff/reports/LIBM-SINF-IVB-RATCHET-{2,3}.json.

## The change (D-10.79)
* `hot/fpregs.rs::widen`: one `cvtss2sd xmm0, xmm0` (`in("xmm0")`, `lateout("xmm0")`, `nomem`, not `pure`), the
  machine-register category's third template (ruling #4445). `hot/f32_trig.rs` passes it to
  `fast32::{sin,cos,tan}_nearest_widen` -> `trig_core_with`; `s10_math_gen.rs` (math-gen output, committed in -3)
  dispatches sinf/cosf/tanf there. sincosf, cexpf, Bessel keep the bit-built operand (`trig_core`).
* sinf disassembly: `movdqa %xmm1,%xmm0; cvtss2sd %xmm0,%xmm0` (x stays live in xmm1 for the refusal paths; the copy is
  a full-register move).

## Done in -3
* Static red fixed: the audit's named-register test read `asm!(` as an `in(` operand (every template looked like a
  register name: 0 MXCSR stores); now only `in(`/`out(` literals count. The widening's comment inside its `unsafe`
  block pushed `asm!` past the audit's 3-line window; moved above. unsafe-audit ok (machine-register=4).
* `trig_widen_entries_match_the_bit_built_ones`: floor 300,000 was unreachable (about 138k in-range calls sampled,
  128,091 taken); now 120,000.

## Jobs (kanon unless named)
* build1: static green, partial-write 0 findings (791 exports), the widen test's floor failed (fixed above).
* plants (handoff/LIBM-SINF-IVB-RATCHET-3-plants.txt): 7 CAUGHT, 0 SURVIVED, 0 WARN (dst == src flagged; dst != src
  accepted; audit template parse; widen nomem; other cvt template; other register in fpregs -> static; sin entry
  taking cos).
* counts (cbench_libm): sinf 85.1 / glibc 42.0, cosf 85.1 / 42.1 (91.2 bit-built, handoff/libm-f32-onepass-rest.md),
  tanf 94.2 / 106.3, asinhf 100.0 / 88.9, acoshf 93.2 / 73.0.
* Timing (kotori Ivy Bridge #157, nozomi N100 #158; base = same tree with the bit-built dispatch, base.tar; 6 rounds
  branch/base/glibc interleaved; median Liella/glibc, branch vs base):
  | row | IVB branch | IVB base | N100 branch | N100 base |
  |---|---|---|---|---|
  | sinf | 1.704 (1.507-2.359) | 1.825 | 1.855 | 2.330 |
  | cosf | 1.685 | 1.805 | 1.741 | 2.183 |
  | tanf | 0.787 | 0.819 | 0.557 | 0.678 |
  | asinhf | 1.334 | 1.413 | 1.688 | 1.688 |
  | acoshf | 1.425 | 1.429 | 2.033 | 2.033 |
  | expf (control) | 1.391 | 1.394 | 1.661 | 1.660 |
  Gate met: sinf IVB 1.70 <= 1.78; the four gains and the N100 sinf gain kept (N100 improves further). The IVB
  branch rounds spread wide (1.51-2.36 on sinf; base 1.76-1.84): judge by the median (#4435). asinhf's IVB move with
  unchanged code is a placement effect, not this change.
* gate: `cargo xtask affected --run --base origin/next` on the final head (label in the report).

## -4: the -3 verify red attributed (fork child 8 > 7 on kanon, gate2)
-3's verify gate `libm-sinf-ivb-ratchet-3-gate2` (kanon) failed only `hot_text_products
init_fork_child_stays_within_its_page_fault_bound`: fork child 8 (bound 7; warm-up 8, runs all 8). The branch touches
no start-up or fork code. Measured on mei (Skylake-SP, CI host class; load 24-28 on 6 vCPU), fast products of the
merged head and of origin/next (the branch's crates/xtask/records diff reverse-applied in the role, then restored),
through LAYOUT-STABILITY-RETHINK's `measure.py` (imported unchanged; fast profile, `orig` + the 19 `pre` and 19
`cold` pads; one warm-up, five runs, lowest; every run identical). Driver and scripts: mei
`~/work/LIBM-SINF-IVB-RATCHET-4/` (`fork_pads.py`, `attr.sh`, `pads.sh`, `attrib.sh`); jobs attr1, pads1, attrib1.
| | fork test (own layout) | fork over `pre` pads | fork over `cold` pads |
|---|---|---|---|
| origin/next | 7 (bound 7) | 6-8 (8 at 0xE000, 0xF000) | 7-8 (8 at 0xE000, 0xF000) |
| head (merged) | 7 (bound 7) | 6-7 | 7 |
origin/next reaches the kanon count, 8, from padding alone; the head never exceeds 7 on mei: LAYOUT-JITTER (ruling
#4347), not this change. Also seen: `true` 17 vs 16 and `localtime` 20 vs 19 at the own layout (head vs next), +1 at
most pads. perf attribution (`perf record -e page-faults -d`, own layout): text faults equal (true: 5 and 5); the
difference is `.lbss` (head 5 pages over 0x1D000, next 4 over 0x4900) against `.data` (3 vs 4). Both `.lbss` hold
the same objects of the same sizes (total 0x56418) from one rustc codegen object, in another order: placement of
written data by codegen order (the class LAYOUT-DATA-CLUSTERS fixes), not data this branch adds. The
`init_short_programs` bounds passed on kanon and in the -4 gate.

-4 jobs (mei, final head = the merge with origin/next + this note): `libm-sinf-ivb-ratchet-4-lint0` (xtask clippy +
static, rc 0), `-gate1` (`affected --run --base origin/next`: 32 suites ok, 11,486 passed, 0 failed, conformance
127/0, peak 16G, rc 0; the fork test passed), `-plants` (the -3 plants file: 7 CAUGHT, 0 SURVIVED, 0 WARN). Diagnostic
(not on JOBS): `-attr1` (builds, fork test head 7 / next 7; its pad step hit a pickling error, rerun as `-pads1`),
`-pads1`, `-attrib1`.

## Next step
Handed back `INCOMPLETE: LAYOUT-JITTER` as the -4 brief asks for a jitter red; the merged head is green on the CI
host class, so the coordinator may release it without waiting for the layout fix. No code work left. Possible follow-up: sincosf/cexpf could take the same widening through their boundary wrappers.

## Do not redo
* The DSB/MITE and RS counters, the stack-offset sweep, the Estrin trial (NEGATIVE, recorded pending).
