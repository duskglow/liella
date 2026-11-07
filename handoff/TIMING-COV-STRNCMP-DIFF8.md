# TIMING-COV-STRNCMP-DIFF8 (handoff)

State: branch task/timing-cov-strncmp-diff8; code commit 3695972e (csys/src/machine/scan.S strncmp SSE2 head, new test
in crates/boundary/tests/machine_scanners.rs), plants file 9dace255, then the D-NEW row and this handoff. Role
shiki:~/w/sh1 (Haswell-EP). Decisions in force: D-4.21, D-4.35, D-4.45, D-4.59, D-NEW (placeholder: brief reserved
no id; coordinator assigns).

## Step 1: where diff8's extra instructions went (SSE2 class, counted from the code, confirmed by counts)
Old SSE2 path, difference at byte 8, n = 64: entry OR page test 8 (always rejected on SSE2: mask 0) + .Lsnc_pt NULL
tests and a's exact page test 8 + index/zero setup 2 + step page tests of a and b 10 + one 16-byte block 8 + hit
bound 4 + byte decide 4 + order 3 = 47 in the function (56 per call with the bench's call overhead; glibc 39).
The surplus was all bound/page/NULL handling before the first compare (20 instructions); the NUL/difference search
(8) and the order (3) were already minimal. Cause: the SSE2 level ignored the entry's OR value, which already proves
both pointers non-NULL and 64 bytes inside both pages when below 4032.

## Step 2: change
.Lsnc_pt (the entry's taken target) first tests eax < 4032; then up to four 16-byte blocks addressed from a and b
(n exit before blocks 2-4), difference falls through to sbb/or/ret, equal NUL returns 0, writer re-read resumes
.Lsnc_byte with xmm3 zeroed, n > 64 resumes .Lsnc_step at 64. Old NULL/exact-page code is now .Lsnc_slow. AVX2
level: reaches .Lsnc_pt only with eax >= 4032, so +1 fused cmp/jae on its OR-rejected pairs; no measured row takes it.
Test added: strncmp_kernel_stays_in_the_terminators_page_from_every_offset_near_its_end (terminated texts ending at
a guard page, every start offset in the last 81 bytes, n = SIZE_MAX, difference at each position with 0xE9 vs 'x'
(unsigned order), n = pos and pos + 1, common NUL before a difference, other side near its page end or mid-page).
Existing tests already cover n = 0, n below the difference, every alignment pair.

## Counts (cbench_hot instructions per call, Liella vs glibc, --runs 3, shiki)
SSE2 class (--tunables glibc.cpu.hwcaps=-AVX2): diff8 56.0 -> 36.0 (39.0); eq16 67.0 -> 48.0 (66.0);
names64 89.0 -> 68.0 (83.0). Build-host time ratio diff8 1.50 -> 1.00 (shared host, not a verdict).
Default class (AVX2): eq16 35.0 -> 35.0, diff8 34.0 -> 34.0, names64 42.0 -> 42.0 (glibc 34/34/43): unchanged.
No strncmp row slower by instructions on either class.

## Jobs (shiki)
timing-cov-strncmp-diff8-gate rc 0 (clippy, static, affected --base origin/next; code 3695972e),
timing-cov-strncmp-diff8-plants-2 rc 0 (9 plants: 8 CAUGHT, 1 EQUIVALENT (s16 n-exit, performance only), no WARN),
timing-cov-strncmp-diff8-counts-before rc 0 (origin/next a09927ac), timing-cov-strncmp-diff8-counts-after,
timing-cov-strncmp-diff8-counts-after rc 0, timing-cov-strncmp-diff8-stage-2 (started 07:3xZ, writes
shiki:~/work/TIMING-COV-STRNCMP-DIFF8/stage.tar; not waited for).
SUPERSEDED: timing-cov-strncmp-diff8-stage (rc 1: --stage needs the tarball path), timing-cov-strncmp-diff8-plants (rc 0, 1 CAUGHT + 7 WARN-NO-MSG: regex missed the postcondition abort).

## Not done / next step
Timing (step 3) PENDING, not started: budget. Recipe: copy the tarball written by job -stage (shiki ~/w/sh1) to
nico (Ivy Bridge, idle at 06:00Z) and nozomi (N100); install handoff/TIMING-COV-STRNCMP-DIFF8-timing.sh there as
~/work/TIMING-COV-STRNCMP-DIFF8/timing.sh (it unpacks the tarball whole into a new directory and runs
`./xtask-bin bench --no-build --compare --runs 5 [--tunables glibc.cpu.hwcaps=-AVX2] cbench_hot`), start with
`~/bin/timing timing-cov-strncmp-diff8-<ivb|n100> sh ~/work/TIMING-COV-STRNCMP-DIFF8/timing.sh <tarball> <tag> [tunables]`.
Saturation register row (docs/saturation/03) not updated.
