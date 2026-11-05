# PARITY-COPY-AVX2: handoff

State: branch `task/parity-copy-avx2` from origin/next f8b77d70; code commit 1e8ffb15, docs/handoff commit on top. Role keke:~/w/r1
(Zen 1, AVX2, no ERMS: every 257 B to 64 KiB copy runs the AVX2 kernel there, so it counts the kernel change directly).

## 1. The counter probe (done first)
Probe: a static program running 100,000 `rep movsb` / `rep stosb` at n = 0, 1, 64, 1 KiB, 4 KiB, 16 KiB, 64 KiB under
`perf stat -e instructions:u` (scratch, job label `parity-copy-avx2-repprobe` on mei, shiki, kotori). Result: 9.00 instructions
per iteration at every size on Skylake-SP (mei), Haswell-EP (shiki) and Ivy Bridge (kotori): the rep op counts as ONE
instruction on all three. The brief's Ivy Bridge hypothesis (each iteration counted) is wrong: Ivy Bridge memcpy_4k
26.3 is the reference taking `rep movsb` at 4 KiB (count about 27) against Liella's SSE2 loop (718; `COPY_ERMS_MIN` 6144).
Written to docs/benchmarks.md section 2a and register 03 (3 rows). Per-row rule (coordinator note): memcpy_4k (both
classes), memcpy_64k (both), Haswell 16/64 KiB = rep rows -> cycles; memmove_4k, memcpy_64, memmove_64, 1 KiB -> instructions.

## 2. Profile of a 1 KiB AVX2 copy (by reading the kernel; dst 32-aligned), 170 vs 137 on Haswell-EP
Entry/dispatch (fold.S `.Lmove_big` -> `move_rest` -> `move_large`, `avx2_erms` window test): about 25-30.
Kernel: overlap/direction tests 15, forward alignment setup 10, 128-byte loop 7 x 11 = 77, **32-byte tail loop 14** (2 vectors
x 6 + exit 2, plus its `leaq`), ends 5. The tail loop is the one phase with a pure overhead surplus.

## 3. Change (1e8ffb15)
Each direction now loads the 128-byte block at the end it runs towards (forward: last 128; backward: first 128) before any
store and stores it last; the 128-byte steps stop when <= 128 bytes are left, which always lie inside the held block. The 32-byte
loop is gone. Expected: -9 per call where the loop ran 2 vectors (every 32-aligned power-of-two size), -15 with 3, -3 with 1,
+3 with 0 (n mod 128 in 1..32 past the aligned start). Kernel contract n >= 64 -> n >= 128 (all production callers pass >= 128;
selftest wrappers and the two AVX2 kernel tests follow; new size list AVX2_KERNEL_SIZES covers 128-289 at every 32-byte step).
SSE2 path untouched (Ivy Bridge counts cannot move: the kernel never runs without AVX2).
Not changed: memcpy_64/memmove_64 (one over; an AVX2 33-64 class needs the flag test on the shared entry: +2 on SSE2, register
row 03); AVX2_COPY_END and the rep windows (D-4.14 is by cycles).

## Jobs (keke unless noted)
parity-copy-avx2-gate (clippy + static + affected --base origin/next), parity-copy-avx2-plants (6 plants,
handoff/PARITY-COPY-AVX2-plants.txt), parity-copy-avx2-percall (wl_percall default base = merge-base: before and after in
one run; started behind the gate). Results: read `~/logs/<label>.log`; not seen by me at hand-back if still queued.
NOT RUN: Skylake-SP `cargo xtask bench --compare --counters` (no role on that class; the dispatcher must give a count job)
and the Ivy Bridge count (timing host, no building; SSE2 code unchanged by construction).

## Open / next step
1. Read the percall: 1 KiB / 4 KiB rows must drop about 9; check no ladder size rose. If a row rose, revert 1e8ffb15.
2. Skylake-SP count of memmove_4k (436.8 vs 413.0 expected -> about 428) on a count job there.
3. Cycle check of the kernel change on a booked AVX2 timing host (3 extra loads/stores at entry change the mix slightly).
4. 1 KiB still about 24 over after this: the entry/dispatch chain (25-30) is the next phase to profile with wl_trace.sh.
