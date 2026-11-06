# TIMING-N100-MEMCMP handoff (after -3: COMPLETE, the branch should land)

State: branch `task/timing-n100-memcmp-3` = task/timing-n100-memcmp-2 (ea0e8604) + merge of origin/next 957f58bd
(b4fb26e3). Role shiki:~/w/sh2. Plants file: handoff/TIMING-N100-MEMCMP-2-plants.txt. Decisions: no new D-row (local to
fold.S; reasons in its comments).

## -3: the CI conflict (merge_next rc 3 on csys/src/machine/fold.S)
next's TIMING-COV-MEMCMP-NONAVX2 rewrote the comment above `.Lcmp_sse_mid` (SSE2 65-128 class: block at 0 alone as
the early probe); this branch had added two sentences and the phase pin (`.p2align 6, 0xcc; .fill 20, 1, 0xcc`) in the
same comment. Resolved: next's comment text kept, this story's phase sentences and pin re-added after it; no
instruction of either side changed. Pin value checked by assembling on the build host (`gcc -c -Wa,-L`, nm of the
local labels; script ~/work/timing-n100-memcmp-3/phase.sh on shiki):
| label (mod 64) | next | branch (timed in -2) | merged |
|---|---|---|---|
| .Lcmp_long_loop | 21 | 52 | 52 |
| .Lcmp_huge_loop | - | 45 | 45 |
| .Lcmp_sse_mid | 20 | 20 | 20 |
| .Lcmp_sse_long | 61 | 48 | 61 |
So the AVX2 loops sit where -2 timed them and the SSE2 classes where next has them: 20 stays right (Ivy Bridge
unchanged from next by layout; not re-timed, the brief asked for the N100 rows only).

## -3 timing, N100 (nozomi:timing-n100-memcmp-3-n100, booking 94): staged trees from shiki, both commit b4fb26e3,
next = merged tree with next's fold.S (tree 2fe36397) vs merged (tree 96e00b33); 7 rounds interleaved, order alternating,
`bench --no-build --staged --rows` (adaptive 3-5 rounds per run, Liella static vs glibc dynamic); medians of 7 runs:
| row | next ns | merged ns | L/G next | L/G merged |
|---|---|---|---|---|
| memcmp_256 | 7.53 | 7.53 | 1.061 | 1.061 |
| memcmp_4k | 116.52 | 109.00 | 1.086 | 1.015 |
| memcmp_64k | 1977.84 | 1977.42 | 0.829 | 0.830 |
The -2 gain survives the merge unchanged (4k -6.5%).

## Jobs (final head b4fb26e3 code; later commits handoff/ only)
shiki: timing-n100-memcmp-3-gate (cargo clippy --workspace --all-targets --all-features -D warnings, xtask clippy,
static, direct-tier affected), timing-n100-memcmp-3-plants (9 plants: 8 CAUGHT, 1 EQUIVALENT, 0 WARN-RC-ONLY/NO-MSG; one
WARN-DEV-SHM, the plants dir was under /dev/shm, no planted test mounts /dev). nozomi: timing-n100-memcmp-3-n100.
Staging job shiki:timing-n100-memcmp-3-stage (tool job, not proof).

## From -2 (still true)
memcmp AVX2 long class: 256-byte one-branch step only for 289 B-8 KiB, old 128-byte two-branch step above (the
256-byte stride per load site raised N100 L2-hit demand loads 214 -> 308 per 64k call, cycles +6%: prefetch coverage).
SSE2 classes pinned at their 64-byte phase (a 257-byte shift cost Ivy Bridge memcmp_4k 2.3%). Ivy Bridge in -2: all
rows equal or faster. memcmp_64k +3 instructions per call (size test + jmp), timing at parity.

## Open / next
* memcmp_256 still 1.06 (N100) / 1.26 (Ivy Bridge): needs its own idea (the 129-256 interleave did nothing).
* If fold.S's SSE2 code before `.Lcmp_sse_mid` changes again, re-check the pin with phase.sh (keep next's phase).
Do not redo: 128-byte one-branch step, 129-256 interleave (both NEGATIVE, docs/saturation/03).
