# LIBM-SPECIAL-FAST-TIER handoff (run 4: LIBM-SPECIAL-FAST-TIER-4, verify run 3's in-place entries; profile after)

State: branch `task/libm-special-fast-tier-4` (stacked on -3 -> -2; merge base with origin/next 3856eefe), code
head 34ef3835 (run 3's two commits, unchanged this run), role keke:~/w/r5 (Zen 1 host). Report:
handoff/reports/LIBM-SPECIAL-FAST-TIER-4.json. Accuracy contract unchanged (correct rounding, ruling #2760 option A);
bound 2^-71 (ruling #2916); -2 landed through #2970.

## Verified this run (head 34ef3835)
* build1: math-gen (s10_math_gen.rs current), clippy of math+boundary, `fast_special` unit + integration tests,
  `math_fma_paths` (both builds bit-identical, flags): all green. lint1: `cargo xtask clippy` + `cargo xtask static` clean.
* Plants: `cargo xtask plants handoff/libm-special-fast-tier-3-plants.txt --dir /dev/shm/claude/libm-special-fast-tier-4/plants-1`
  (label libm-special-fast-tier-4-plants): result in the report json.
* Gate (label libm-special-fast-tier-4-gate): `cargo xtask affected --run` escalates to FULL by itself because the
  branch changes `s10_math.tsv` (a generator input); left running at hand-back if not finished.

## Counts after (instr/call, validation/percall/libm_erf.c, base none; label libm-special-fast-tier-4-counts-after)
| row | before (run 3) | after | reference FMA / -FMA | after/ref(-FMA) |
|---|---|---|---|---|
| erf [-5,5] | 376.0 | 238.0 | 157.9 / 172.6 | 1.38 |
| erf table | 368.0 | 238.0 | 159.3 / 174.0 | 1.37 |
| erf series | 294.0 | 156.0 | 63.0 / 63.0 | 2.48 |
| erfc [-5,5] | 452.9 | 310.0 | 162.4 / 177.1 | 1.75 |
| erfc below 1 | 396.8 | 252.1 | 162.4 / 177.1 | 1.42 |
| erfc above 1 | 565.0 | 425.0 | 194.3 / 214.1 | 1.99 |
Every row faster (-138 to -140 instr). Target <= 1.05x NOT reached. Liella's numbers are the BASELINE (Dekker) build
in both columns: see the next section.

## FMA build never runs from libm on this branch (cause found)
The FMA and -FMA Liella counts are identical. Disassembly of the static probe: `erf` -> the hot entry tests
`liella_boundary::dispatch::FMA` and it is 0. The static binary has TWO copies of that byte (libc.a's and libm.a's
boundary crate); startup (`cpu::init_with`) sets only libc's. LIBM-F64-FAST-KERNELS-2/-3 found and fix exactly this
(libm decides at load, .init_array); not on this branch and not merged here (no merge allowed). Once it lands, the
erf/erfc FMA build runs with no change here; count it then.

## Profile after (one erf call, erf_table call 11, 228 instr, baseline build; label libm-special-fast-tier-4-trace-list)
| phase | instr |
|---|---|
| export entry + MXCSR read + return (erff64, mxcsr_get) | 21 |
| FMA dispatch test | 4 |
| range tests (LINEAR, ERF_ONE, SMALL) | 8 |
| slot (index, centre, t = x - c) | 14 |
| f64 Horner, degree 14..4 (10 steps) | 21 |
| 4 double-double steps (Dekker products, two_sum) | ~139 (~35 each) |
| near test + sign select | ~21 |
The envelope (run 3's 169) is now ~33 (entry 21 + dispatch 4 + sign 8). The surplus is the kernel's dd steps.

## Exact next steps (what each should save, before building)
1. After LIBM-F64-FAST-KERNELS-3 lands: rebase/stack, recount on both builds. Expected FMA build: each Dekker
   product (~15 instr) becomes 2 -> about -50 on erf (238 -> ~185, ref 159: ~1.16x), about -100 on erfc above 1.
   NOT VERIFIED (estimate from the listing).
2. Kernel, baseline build: the innermost dd step(s) need far less than 2^-71 (their error is scaled by t^k); a
   per-slot bound from the generator could drop the exact product in the innermost step (~-15 to -20 on erf).
   Needs the generator's eps per slot redone (x near 5 has |c_{k+1} t / c_k| near 1, so not uniform).
3. Envelope (small): the out-of-line kernel call plus the black_box spill (~10 instr) only.
4. erff/erfcf (float, still mp) are not in this story; gamma/Bessel are their own stories.

## Things not to redo
* Degree/slot layout (run 1); the 2^-71 bound; the erfc row's percall MISMATCH is the reference's last bit:
  erfc(0x1.b12e88p+0) is 0x1.11c9655380463p-6 (true value 0.305 ulp below it, Decimal oracle at 200 digits);
  the reference returns ...462. Not a Liella error.
* Do not re-diagnose identical FMA/-FMA counts: it is the libm dispatch byte above.
