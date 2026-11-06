# TIMING-REOPEN-N100 handoff (planning; no code)

State: branch task/timing-reopen-n100 from origin/next ff125cce; docs only (handoff/timing-reopen-n100/).
Done: N100 timing host booked (booking 43), profile job timing-reopen-n100-prof run, booking cancelled; nothing left running.
Briefs: TIMING-N100-MUTEX-MT, -COPY-MID, -MEMCMP, -SCAN-64K, -STRSTR-64 (summary table in README.md).
Decisions in force: ruling #3046 (vector-vs-vector rows gated on instructions, verdict from timing).
Not verified: instruction-level stall sites (no objdump on the timing host, so perf annotate wrote nothing); the memcpy rows
on next HEAD (PARITY-COPY-AVX2-3 landed after the sweep base).
Not to redo: the per-call event table (profile-n100-percall.txt); Liella perf.data per row stays in
~/work/TIMING-REOPEN-N100/out on the N100 timing host for annotation on a build host.
Next step: submit the five briefs; RULING NEEDED on a planning story for the uncovered cold-start (first_/second_) and wordexp rows.
Process note: perf annotate needs objdump on the timing hosts (or annotate by copying perf.data to a build host).
