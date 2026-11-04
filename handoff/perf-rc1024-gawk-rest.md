# PERF-RC1024-GAWK-REST handoff (no product code; resolved by PERF-RC1024-HEAP-FREE)

State: branch task/perf-rc1024-gawk-rest, base e79f441a (STRLEN-3 head, which does NOT contain HEAP-FREE).

## Result
Counts on keke, job perf-rc1024-gawk-rest-counts3 (rc 0), wl_count, WL_BASE_EXACT=1, --base fa678f74, instructions:u
(perf stat -r 3). Ratio = tree / fa678f74.

| row | e79f441a (this base) | origin/next bad2e7ae (has HEAP-FREE) | fa678f74 | glibc |
|---|---|---|---|---|
| gawk-regex | 3361296598 (1.0110) | 3324860238 (1.0000) | 3324860493 | 3421689972 |
| gawk-utf8 | 3474613317 (1.0073) | 3449603542 (1.0000) | 3449602115 | 3596735800 |

The +1.10% / +0.72% reported by SED-UTF8-2 reproduces exactly on this base (a stack on STRLEN-3 only, which changed no code)
and is gone on origin/next: 255 and 1427 instructions apart from fa678f74 (<0.00005%), 2.8% / 4.1% below glibc.

## Attribution
The whole delta is crates/rt/src/heap.rs (perf-heap-pair-check, merge 4b31a189), the same cause as the bash-loop/make-n
growth. On next heap.rs is byte-identical to fa678f74 (`git diff --stat fa678f74 bad2e7ae -- crates/rt/src/heap.rs
crates/boundary/src/heap.rs` empty), and the rows return to fa678f74's counts. Nothing else on the path moved. A per-symbol
perf diff was not run: with the rows at parity on next there is no remaining delta to attribute, and the only changed
source file between this base and next on the allocator path is heap.rs.

## Open / next step
Nothing. This story is satisfied by HEAP-FREE (in next). If the malloc+free pair gain is retried, count gawk-regex,
gawk-utf8, bash-loop and make-n against fa678f74 first.

## Jobs (keke)
perf-rc1024-gawk-rest-counts3 (rc 0). Script ~/work/perf-rc1024-gawk-rest/count.sh. Superseded: -counts, -counts2
(rc 0 but every row failed: ~/wl/src/<sha> not primed / empty dir).
Note: running wl_count with an unprimed sha creates an empty ~/wl/src/<sha>, and wl_prime_base.sh then says "present".
