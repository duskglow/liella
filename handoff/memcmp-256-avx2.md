# PERF-MEMCMP-256-AVX2 (handoff)

State: branch task/perf-memcmp-256-avx2, change in csys/src/machine/fold.S (`.Lcmp_160`, `.Lcmp_256`), test
`memcmp_129_to_256_one_difference_among_equal_bytes_and_two_differences` in crates/boundary/tests/machine_scanners.rs.

Result (keke, instructions per call, before -> after, glibc): memcmp_256 70.5 -> 58.4 (57.4); memcmp_144 53.5 -> 47.5 (57.4);
other memcmp rows unchanged. Register text: docs/saturation/15-... "Row 1 follow-up".

Decisions: 129-160 B four blocks, 161-256 B seven blocks, no loop; first difference taken from the first differing block
(head blocks first, then tail blocks). Not done: -AVX2 (SSE2) recount (path unreachable there by construction); a per-ISA entry
to drop the NULL tests and cmp_mid word (the remaining 1 instruction at 256).
First plants run: bad-bsf in a middle block survived because the old sweep makes all later bytes differ; the new test
(one difference among equal bytes) was added to catch it.
Continuation (PERF-MEMCMP-256-AVX2-2): the plants2 PLANT-NOOP was a bad regex (`lastblock-offset` looked for a label and
instruction sequence that is not in `.Lcmp_256_g`). The plant now rewrites `leaq -32(%rdx,%rcx)` to `-31` in `.Lcmp_256_g`.
Plants (fresh RAM copy plants-4, 9 plants): all 9 CAUGHT, rc 0 (keke job perf-memcmp-256-avx2-2-plants). Direct-tier gate
perf-memcmp-256-avx2-2-gate rc 0. No code change in this run; clippy and static run on the final head.
Next step: none; remaining idea is the per-ISA entry (see Not done).
