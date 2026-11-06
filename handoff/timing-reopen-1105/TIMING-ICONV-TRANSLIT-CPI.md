# TIMING-ICONV-TRANSLIT-CPI: iconv UTF-8 -> ASCII//TRANSLIT runs fewer instructions but more cycles

Id: TIMING-ICONV-TRANSLIT-CPI
Kind: code
Priority: 70
Size: M
Host: -
Stacks-on: -
Model: opus
Effort: high
BUDGET: 80 calls / 140k

## Rows (TIMING-SWEEP-1105; instr / cycles / time)
cbench_locale/iconv_utf8_ascii_translit_4k: Ivy Bridge 0.864 / 1.298 / 1.299; N100 0.864 / 1.453 / 1.454.

## Profile evidence (TIMING-REOPEN-1105, booked Ivy Bridge host, whole process with CBENCH_ROWS, user mode)
cycles 30.06G vs 23.28G (1.29x), instructions 0.86x, uops issued 1.01x (Liella issues more uops per instruction).
Liella cycle samples: Conv::convert 53% (hot offsets near +0x335e: a ~13 KiB function), unicode::decomposition 30%,
conv::internal_move 10%, Codec::encode 6%. Contributions, as event difference per cycle of the gap:
- front-end: idq_uops_not_delivered 7.0x (2.12 slots per gap cycle), idq.mite_uops 4.4x, idq.dsb_uops 0.94x: code
  delivered from the legacy decoders, consistent with a hot loop spread over a large function (DSB capacity/misses);
- uops_issued.flags_merge 1.08G vs ~0 (0.16 per gap cycle): partial-flags merge uops (an instruction that writes only
  some flags, e.g. inc/dec or a shift by cl, followed by a reader of CF and others); glibc has none;
- ld_blocks.store_forward 35.6M vs ~0 (each ~10+ cycles: on the order of 5% of the gap or more); ild_stall.lcp
  1.25M vs ~0 (16-bit immediates; small); slow_lea 3.7x;
- back end: resource_stalls.rs 1.38x (0.39), cycles_no_execute 2.5x (0.35), ports 0/1/5 each +0.4-0.5 uops per gap cycle.
Branch misses 0.16x of glibc (not the cause).

## Hypothesis
CPI, not instruction count: (1) the hot loop's code is decoded by MITE instead of streamed from the uop cache
(large convert body); (2) flags-merge uops on every iteration; (3) store-forward blocks (a narrow store read back by a
wider load, or the reverse) in the per-character path. The N100 ratio (1.45) is worse, so not Ivy Bridge specific.

## The change
1. Locate the flags-merge and store-forward sites: `perf record -e uops_issued.flags_merge:u` and
   `-e ld_blocks.store_forward:u` on the Liella binary, annotate Liella (objdump on a build host).
2. Split the steady ASCII/translit loop out of Conv::convert into a small hot function (cold paths out of line);
   remove the partial-flag pattern and the narrow-store/wide-load pair the samples point at.
3. Re-count instructions (must not rise) and time on both hosts.

## JOBS
gate (clippy, static, affected --base origin/next); plants via `cargo xtask plants` on the changed conversion code
(translit replacement dropped, byte count off by one, state not carried across buffers); instruction counts
before/after (cbench_locale iconv rows) as the regression gate; booked timing check of the row on the Ivy Bridge
timing host (`ops timing-book nico <min> --story <ID>`) and on the N100.
