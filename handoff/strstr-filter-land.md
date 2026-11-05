# STRSTR-FILTER-LAND handoff

State: branch task/strstr-filter-land = origin/next 6acde4ea + merge of exp/strstr-adaptive-filter (no conflicts) + one docs commit
(e6f82c5a, saturation register row). Verified head for all jobs: e6f82c5a (later commits are handoff files only).

Decisions in force: owner ruling #2686 (filter lands; strstr_hit_64 and strstr_dense_4k are not final, STRSTR-SHORT-DENSE-PARITY works on them).

## wl_count (job strstr-filter-land-count2, mei, rc 0, base 6acde4ea = origin/next, instructions:u, perf stat -r 3)
| workload | new | base | new/base |
|---|---|---|---|
| grep-F | 55830264 | 55835632 | 0.9999 |
| make-n | 753086224 | 753088228 | 1.0000 |
| gawk-regex | 3324869033 | 3324870679 | 1.0000 |
| gawk-sum | 1826915293 | 1826917520 | 1.0000 |
| bash-loop | 19045926584 | 19045929078 | 1.0000 |
| sed-s | 9415917493 | 9415918397 | 1.0000 |
| diff | 784915928 | 784919132 | 1.0000 |
| tar-tv | 123834756 | 123837201 | 1.0000 |
| pipe-mix | 438923076 | 438941678 | 1.0000 |
No workload is worse; outputs SAME as glibc and base. (Workload picks: the programs that call strstr/strcasestr/memmem
in the set; the workload set has no workload dominated by strstr, so the differences are noise-level.)
An earlier count job strstr-filter-land-count rc 1 (cargo not on PATH in the job shell) is superseded.

## Plants (job strstr-filter-land-plants, mei, rc 0, fresh RAM copy /mnt/zram/claude/STRSTR-FILTER-LAND/plants-1)
baseline test rc 0; p1_switchdrop CAUGHT; p2_norewind CAUGHT; p3_direction CAUGHT by count (strstr_dense_4k 997 -> 2675).

## Gate (job strstr-filter-land-gate, rc 0): 10 suites ok, 10199 passed, 0 failed; conformance 117/0; peak 16G.
## Lint (job strstr-filter-land-lint, rc 0): clippy --workspace --all-targets --all-features -D warnings and cargo xtask static clean.

Open: strstr_hit_64 (1.09x) and strstr_dense_4k (1.27x) -> STRSTR-SHORT-DENSE-PARITY.
Next step: CI and landing.
