# PERF-FWRITE-4K (branch task/perf-fwrite-4k)

State: no product change. Added validation/percall/fwrite4k.c (per-call cases) and a register section in
docs/saturation/13-next-perf-stories-from-the-10.md with the numbers (mei, Broadwell-EP class, instructions:u).

Findings: fwrite_4k 798.7 vs glibc 645.0 per call. Window path (64 KiB buffer) is at parity (562.0 / 559.3). The copy
is the same AVX2 loop as glibc's (434 / 422) because the stream's 8-byte reserve misaligns it; moving to rep movsb is
memcpy policy (MEMCPY-POLICY-2, D-4.14), not touched. The excess is the straddling call's engine overhead plus the
write wrapper (53 vs 21 instr). No single site; not changed (no profile for a targeted edit; VM perf sampling poor).

Jobs (mei, final head): perf-fwrite-4k-counts-before (percall; "after" is identical, no code change), perf-fwrite-4k-gate
(affected run, rc 0, finished in 20 s), perf-fwrite-4k-static. clippy --workspace --all-targets --all-features -D warnings: rc 0.
Plants: none, no product code changed (the only new code is a measurement spec).
Not done: cargo xtask bench --compare --counters --runs 3 (no change to compare); brief asked for keke/shiki, the
admitted host was mei.

Next step: straddle fast path in write_raw (copy head, drain, copy tail without run set-up) and the write wrapper's 53
instructions; both need a per-site instruction breakdown first.
