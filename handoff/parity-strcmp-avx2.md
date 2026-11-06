# PARITY-STRCMP-AVX2 (handoff)

State: branch task/parity-strcmp-avx2, code commit 38c60827 (csys/src/machine/scan.S AVX2 strcmp; new test in
crates/boundary/tests/machine_scanners.rs; plants file handoff/parity-strcmp-avx2.plants). Role keke:~/w/n1 (Zen 1, AVX2, no AVX-512).

## Profile (before the change; instructions per call, percall avx2_families.c, keke)
Liella's percall ladder equals the Haswell-EP numbers in the brief (Liella's count is the same on both AVX2 hosts).
- The harness buffers pc_a/pc_b are page-aligned, so every strcmp_eq_* row runs the kernel's page-aligned-start path
  (.Lstrcmp_v_start: failed head page test 4, NULL tests 4, set-up 2, OR page test 5, then 32-byte blocks and 64-byte
  groups), not the 16/16/32/32/32 head. Its counts fit the model exactly: 16/31 = 32 + 13 harness = 45; 32/33 = 53;
  64/65 = 75 (2 blocks, one group, 9-instruction locate, settle 8). So the 16-65 "head" gap of the ladder is the
  page-aligned path, not the head that ordinary strings take.
- Long rows: the 64-byte group costs 14 instructions per 64 bytes (blocks + fold 11, step 3): about 908 per 4 KiB page
  against about 740 for glibc (11844 / 16 pages at 65536). That is the whole long-row surplus (14765 vs 11844).

## Design and why
128-byte groups (22 instructions per 128 B, about 704 per page). A 128-byte group costs 9 more than a 64-byte one when the
stop is in its first half (the extra loads are wasted), so where the first group sits decides which short rows rise:
- From .Lstrcmp_v_top (index 128 after the head, every later page): four tested 32-byte blocks, then 128-byte groups.
  A stop in the four blocks costs the same or less than two blocks + a 64-byte group (strcmp_late_256, stop at 255:
  about 6 fewer); stops at 256-383 cost up to 11 more (first 128-byte group), 384 on fewer.
- From .Lstrcmp_v_start (page-aligned / near-page-end starts) the first page keeps the old 64-byte groups: a 128-byte
  group there regressed the 128 and 256 ladder rows (+11 / +5 by the model). Its later pages use the new path.
- .Lstrcmp_v_exact (OR test misjudged, or near a page end) is unchanged (old blocks + 64-byte groups).
Rejected (by the model, not built): one 128-byte loop behind the old two blocks (ladder 64/65 +9, strcmp_late_256 +9);
three blocks then 128 (ladder 128 +3, late_256 +17); routing page-aligned starts into the head (16 +1, 32 +2, 256 +25);
negative-index loop (+4..5 set-up on every short entry); vptest for the group test (changes the instruction mix: needs
timing, not tried).

Correction from the measured run: the ladder strings are not both page-aligned. One of the two 64-aligned buffers starts 64
bytes before a page end (head test fails, .Lstrcmp_v_exact, then .Lstrcmp_v_top at 64), so the ladder's long rows take
the new path from index 64 on. The 16-65 surplus (45 / 53 / 75 against 31 / 38 / 45) is that start path's fixed cost.

## Results (keke, Zen 1, AVX2 class without AVX-512; percall instructions per call, glibc / before / after)
strcmp_eq_16/31 31 / 45 / 45; 32/33 38 / 53 / 53; 64/65 45 / 75 / 75; 128/129 89 / 89 / 89; 256 112 / 113 / 113;
1024 250 / 297 / 278 (1.19 -> 1.11); 4096 804 / 965 / 793 (0.99); 16384 3012 / 3725 / 3025 (1.00);
65536 11844 / 14765 / 11953 (1.01). No row rose. Jobs: parity-strcmp-avx2-percall-before, parity-strcmp-avx2-percall
(head 38c60827; the later commit ec2ad25b changes tests only).
Bench counts: before = parity-strcmp-avx2-count-before (origin/next 4c33d4c8); after = parity-strcmp-avx2-count (runs
after gate-3; compare the strcmp/strncmp/strcasecmp rows, revert if any rose).
Plants (first run parity-strcmp-avx2-plants, superseded): fold-drop-block3, top-limit, group-step CAUGHT; locate-half
and group-count SURVIVED -> tests strengthened in ec2ad25b (rescan counter unchanged without a writer; 256-4093-byte
strings ending at a guard page). Final run: parity-strcmp-avx2-plants-3. Gate: parity-strcmp-avx2-gate-3 (direct tier).
clippy (workspace, all targets, all features) and static rc 0 on the role at 38c60827 (ec2ad25b only touches a test).

## Not done / next step
- strcasecmp/strncasecmp (cord loop: page test 8 + bound test 2 per 64 bytes; hoist both into a group count, as strcmp
  does: about -16 at 256) and the short strcmp/strncmp head rows: not started (budget).
- Skylake-SP and Ivy Bridge bench counts: not run (no role on those classes; the dispatcher must schedule them). The SSE2
  path (Ivy Bridge) is unchanged by construction (only the AVX2 branch of liella_machine_strcmp was edited).
- The Skylake-SP gap on short and long rows is glibc's AVX-512 path (inferred from counts); an EVEX strcmp level is a
  separate, larger story (dispatch level in cpu.rs, ymm16-31 to skip vzeroupper).
