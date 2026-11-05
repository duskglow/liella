# PARITY-LIBM-1105 handoff (planning; no product code changed)

Branch task/parity-libm-1105. Full report and table: handoff/parity-libm-1105/summary.md; raw bench: bench-cbench_libm.out there.
Result: six code briefs in handoff/parity-libm-1105/ (PERF-LIBM-EXPM1-LOG1P, -TRIG, -INVTRIG, -HYPERBOLIC, -SPECIAL, -LONGDOUBLE), each with
rows, a percall-profile hypothesis and a JOBS list. Not briefed (named in the summary): f64/f32 exp/log/pow family and envelope rows
(docs/saturation/16 + math-m6-f64-gap-*, math-f64-fma-path), complex rows (Ziv chain), _Float128 real rows (6-group cap; next round), sqrt wrappers.
Finding: libm is far above parity in instructions (3x to 9000x), not near it; most groups lack any fast tier.
Tooling fix made: validation/wl_percall_run.sh prefers the cached tree's liella-cc (a stale ~/liella-test wrapper drops -lm).
Probe added: validation/percall/parity_libm1105.c.
Do not redo: the bench and percall runs (labels in the summary). Next step: the coordinator submits the six briefs; suggested order
INVTRIG/HYPERBOLIC/EXPM1-LOG1P (shared fast64 pattern), then TRIG, SPECIAL, LONGDOUBLE.
Counter host note: run on mei (Skylake-SP), not Zen 1 / Haswell-EP as the brief named; instruction ratios are architectural.
