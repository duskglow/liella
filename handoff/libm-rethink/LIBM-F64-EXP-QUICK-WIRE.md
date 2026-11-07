# LIBM-F64-EXP-QUICK-WIRE: wire the binary64 exp quick phase and trim the in-place envelope

Id: LIBM-F64-EXP-QUICK-WIRE
Kind: code
Priority: 88
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 80 tool calls / 160k
Stacks-on: LIBM-F64-EXP-QUICK

## The change

Decision row: D-NEW "Two-phase `double` elementary functions". Wire LIBM-F64-EXP-QUICK's kernel into `exp` as tier 1
with today's `exp_near` as tier 2 (`crates/boundary/src/hot/exp_log.rs` `exp_nearest`, both builds through
`dispatch::fma()`), keeping `mret::elem_fast`'s contract (MATH-M6-ENVELOPE-COST, D-10.25, D-10.50). Then exp2 and exp10
on the same table if the budget allows (same kernel shape, own constants).

Profile first (developer-flow "profile before you design"): a one-call split with `validation/wl_trace.sh` on the base
(today, Zen 1, FMA build: entry and envelope 21 of 125 instructions, the kernel about 100) and on the SSE2 build
(`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`: 189 per call in the loop).

The envelope items of DESIGN.md §5.1, each kept only if its count AND the booked timing improve:
- `fpu::peek` calls the assembly `liella_machine_mxcsr_get` out of line on every call: an inline `stmxcsr` in
  liella-boundary needs an `UNSAFE(<category>)`; if no documented category fits, write RULING NEEDED with the count.
- `core::hint::black_box(x)` sends the argument through the stack on the hot path; a register-only barrier keeps the
  ordering guarantee (the arithmetic after the MXCSR read) without the store and load.
- `dispatch::fma()` is a load and a branch per call, and the FMA build is a `#[target_feature]` call.

## Tests

The exp accuracy suites in all four modes (directed modes still go to `elem_switched`), both builds identical
(`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`), `crates/boundary/tests/libm_fma_select.rs`, the hard cases, errno and flags
(inexact raised by the arithmetic; no flag on the exact cases).

## Plants (`cargo xtask plants`, `msg:` per plant)

Tier 1's value returned on a miss; tier 2 skipped on a miss (straight to the accurate path: the count test must catch
it); the MXCSR default check inverted; the FMA build selected without FMA in the tunable.

## Counts and timing

`cargo xtask bench --compare --counters cbench_libm` (exp rows, both builds through the tunable): no row slower.
Timing is required here (the instruction mix changes): book an Ivy Bridge (nico or kotori, the shorter queue) and the
N100 (`ops timing-book`), stage with `cargo xtask bench --stage`, run the cbench_libm exp rows (`CBENCH_ROWS`).
Expected from the prototype (DESIGN.md §4.1): Ivy Bridge exp 2.96x -> about 1.4x glibc's time.

## JOBS

lint, gate, plants, counts, `<ivb-host>:<label>`, `nozomi:<label>`.

Hand-back first line: `COMPLETE: exp quick tier wired: Ivy Bridge <x> ns (glibc <y>), N100 <x'> ns (glibc <y'>)`
