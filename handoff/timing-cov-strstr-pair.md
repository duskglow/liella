# TIMING-COV-STRSTR-PAIR handoff (strstr on 64-byte haystacks)

State: branch task/timing-cov-strstr-pair (from origin/next a09927ac). Role keke:~/w/n2 (Zen 1, AVX2). Commits:
2d04b39a (direct entry + Rust continuation + test), plants file commit, alignment fix (`.p2align 6`, D-4.33).
Report: handoff/reports/TIMING-COV-STRSTR-PAIR.json. Plants: handoff/TIMING-COV-STRSTR-PAIR-plants.txt.
No new D-row: a local entry change; strstr's behaviour and the strpair scan contract are unchanged. strpair's header
now states a register contract (no stack; never writes rdi, rsi, rcx, xmm13-xmm15) that the new entry relies on.

## Where the instructions went (before; from handoff/timing-n100-strstr-64.md's profile, confirmed by the change)
hit_64 = 125.5 on the AVX2 level: strpair kernel ~45 (entry, two head blocks, stop), Rust ~80 (5 push/5 pop + frame,
the needle/prefix loop at 4 instructions per needle byte, cpu::ready, argument setup, the candidate compare, the
24-byte KResult<Option<usize>> returned through the stack and unpacked by ret::found).

## Change
`liella_machine_strstr` (csys/src/machine/scan.S) is now the exported `strstr` (Rust twin `posix::strstr` calls
it, so strstr_find and every other strstr test run it):
- h - 1 and n - 1 each at a page offset below 4080 (NULL never passes): one 16-byte load of n measures a needle of
  up to 15 bytes (m == 0 returns h); one 16-byte load of h checks bytes [0, m - 1) for a NUL (NULL answer).
- call strpair(h + last, last, n[0], n[last]) with h + last / last kept in rdi / rsi, the needle in xmm15, n pushed.
- answer 2k: one 16-byte load at h + k (page offset <= 4080) compared over bits [0, m): return h + k. Answer 2k + 1:
  the NUL re-read, 0 -> NULL.
- everything else tail-jumps to Rust: `liella_hot_strstr_slow(h, n)` (the old path, strz::find_str_n) before any
  scan; `liella_hot_strstr_next(h, n, m, r)` (strz::find_str_after -> strz::settle, the old find_known tail, which
  find_known now shares) after it.
New test: strstr_needle_ending_on_the_last_mapped_byte_is_measured_inside_its_page (needle NUL on the last mapped
byte, m 0..21, both scan paths): the only test that reaches the needle load's page test.

## Counts (cargo xtask bench --compare --counters --runs 3 cbench_memref, Zen 1 build host, instructions per call)
| row | before (next a09927ac) | after | glibc on this host | glibc Intel (brief) |
|---|---|---|---|---|
| strstr_hit_64 | 125.5 | 100.5 | 164.6 | 99.4 |
| strstr_miss_64 | 128.5 | 88.5 | 337.7 | 173.6 |
| strstr_hit_4k | 651.3 | 626.1 | 826.4 | 2243 |
| strstr_miss_4k | 911.2 | 872.0 | 7523.2 | 6446 |
All other 113 rows within +-1.9 (untouched code: noise of the per-call subtraction on 4k-64k rows).
hit_64 is at parity by instructions with the Intel glibc figure (100.5 vs 99.4, +1.1%), from 1.26x.
SSE2 level: NOT COUNTED. `GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2` changed nothing for Liella under xtask bench on this
host (both sections identical), so the "-AVX2" section is the AVX2 path again. The entry's own instructions are the
same on both levels (SSE2 only), so the -25 / -40 carry over by construction; count it on an Intel build host as
TIMING-COV-STRSTR-NONAVX2 did (shiki, -AVX2) to confirm.

## Jobs (keke)
Final head 9e5ac7f6 (commits after 2d04b39a: plants file, `.p2align 6`, clippy useless_vec in the test, MACHINE_EXPORTS
row for strstr in xtask/src/affected.rs). Jobs on it: gate3 (~/bin/gate 16G: clippy, static, affected --run --base
origin/next) then counts-final2 (~/work/TIMING-COV-STRSTR-PAIR/chain3.sh), plants4 (8 plants, RAM dir plants-4).
counts-before rc 0 (base). Superseded: counts-after rc 1 (alignment check), counts-after2 rc 0 (product identical to
the final head; the numbers above), plants / plants2 / plants3 killed (older heads; plants2 had 5 of 5 CAUGHT so far,
no WARN), gate rc 1 (clippy useless_vec), gate2 rc 1 (static machine-exports), counts-final killed.
Timing NOT run (budget): no Ivy Bridge / N100 booking was made.

## Next step
1. Read keke `~/bin/jobwait timing-cov-strstr-pair-gate timing-cov-strstr-pair-plants2`.
2. Booked timing of strstr_* (both classes, base vs head vs glibc, >= 3 rounds); the instruction mix is unchanged in
   kind (SSE2 compares replace scalar loops), so the periodic sweep may serve.
3. -AVX2 count on an Intel build host (above). If the SSE2 hit_64 is still above 1.2x, the SSE2 kernel's four head
   blocks are the next target.
Not to redo: the Rust frame around the first scan (this story), the kernel heads (TIMING-N100-STRSTR-64,
TIMING-COV-STRSTR-NONAVX2).
