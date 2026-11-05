# NSCACHE-SECOND-LOOKUP (handoff, run 1)

**State.** Branch `task/nscache-second-lookup` from `origin/next` ff125cce; code commit 4507467b (hot-text probe
`tools/hottext/probes/p_nss.c`, regenerated `crates/libc-so/hot-text.list` (fast profile, +58 keys, 28 existing keys moved),
four cache functions added to `init_cold_startup_functions_are_grouped`). Role shiki:~/w/sh1 (Haswell-EP, 2.6 GHz).
**Decisions in force.** D-16.10 unchanged (first lookup uncached, attach + first validation on the second, every
amendment check kept). No new D-row: layout of the products only, no behaviour or trust change.

## Profile (step 1; static Liella probe, per call: perf_event_open self-counters, getrusage, markers)
Scratch probes: `~/w/.scratch/NSCACHE-SECOND-LOOKUP/{percall.c,steps.c,measure.sh,reattach.sh,hotjob.sh}`, the
same files on shiki `~/work/nscache-second-lookup/`. `percall N user [x]` (x: `setuid(getuid())` before call 3
forces a re-attach with warm code); `steps <region>` measures each attach system call alone.

Steady state (region exists, entry present), base next ff125cce:

| call | kernel instr | user instr | minflt | ns (unbooked, noisy) |
|---|---|---|---|---|
| cached 2nd | 171.5k | 18.5k | 11 | 79-86k |
| cached 3rd+ (hit) | 4.0k (= the marker/counter floor) | 7.1k | 0 | 1.8-2.7k |
| uncached 2nd | 19.1k | 39.8k | 0 | 15.0k |
| re-attach, warm code (3rd call after setuid) | 100.1k (incl. ~15k unmap/close of the old mapping) | 16.4k | 3 | |

Where the cached 2nd call's 171.5k kernel instructions go (steps.c, fresh process; strace of the call):
- the first validation: 5 stats (passwd, nsswitch.conf, host.conf, resolv.conf, gai.conf): 24.4k
- `/proc/self/ns/user` stat (T13): 26.3k in a fresh process (13.7k warm: the first walk of /proc/self costs the rest)
- boot ID (open+read+close of /proc/sys/kernel/random/boot_id): 13.4k
- open region 6.3k, fstat 1.2k, mmap 8 MiB shared 7.3k, 4 id calls 1.5k
- region page faults (header, slot, entry: 3 different 64 KiB windows): 11-15k
- **first touch of the attach / validate / hit code and a heap page: 8 faults, about 71k** (171.5k − 100.1k
  re-attach). This was the largest share that does not touch a security check, so run 1 went after it.
Not on the path: no MAP_POPULATE, no full-file validation (the reader touches 3 pages), no account lookup (the
cheap stamp path holds), no F_DUPFD (the descriptor is already above 2).

## Change (step 2) and result
The attach, validation and hit functions go into the hot-text group (`p_nss`: getpwuid+getgrgid ×3, as in ls -l or id),
so they share the fault-around windows of code that is already mapped. Same per-call probe, same host:

| call | before (kernel / user / faults) | after |
|---|---|---|
| cached 2nd | 171.5k / 18.5k / 11 | **121.6k / 18.5k / 7** |
| cached 1st | ~397k / 64.7k / 37 | ~300k / 64.7k / 28 (NSS files path grouped too) |
| uncached 1st | ~398k / 64.7k / 37 | ~298k / 64.7k / 29 |
| uncached 2nd, hit | unchanged (19.1k / 39.8k / 0; 4.0k / 7.1k / 0) | unchanged |

**Break-even by instructions** (n lookups in one process; cached cheaper when n > 2 + (c2 − u)/(u − h); c2 = cached 2nd,
u = uncached later call 58.9k, h = hit 11.1k): before 2 + 131.0/47.8 = 4.7 → **from the 5th lookup**; after
2 + 81.2/47.8 = 3.7 → **from the 4th lookup**. The first lookup's ~97k gain applies to every process with one or more
lookups, cache on or off. By time: not measured on a booked host (see Not run). Unbooked shiki times of the 2nd call
did not separate (80-138 µs after, 79-241 µs before).
**Target (2nd cached ≤ uncached) not reached and not reachable by layout:** the remaining 121.6k kernel instructions are
the stats, /proc and region syscalls listed above (~95k) plus 4 faults. With every amendment check kept, the
attach floor is about 2× the uncached call (58.9k). Security properties: none touched. The region checks (O_NOFOLLOW,
fstat type/owner/mode/size, AT_SECURE and ID checks, initial user namespace, descriptor above 2, header stamp with boot ID,
seqlock and entry checksums) run as before. Only the section names of their functions changed.

## Gate / plants / counts (this run, shiki)
- `cargo xtask clippy` rc 0, `cargo xtask static` rc 0 on 4507467b.
- Gate `nscache-second-lookup-gate` (`cargo xtask affected --base origin/next --run`): xtask itself escalated to
  FULL ("crates/libc-so/hot-text.list: a product crate or its linker input"); still running at hand-back (clippy ok,
  then `cargo xtask test`, which includes hot_text_products and its fault bounds). Re-attach: `ssh <shiki> ~/bin/jobwait
  nscache-second-lookup-gate`. The role's tree is 4507467b (the later commits touch handoff/ only; the sync was skipped while the gate ran).
- Plants `nscache-second-lookup-plants3` (rc 0): `sec-mode` (connect accepts a region with the wrong mode) CAUGHT by
  squatted_regions_are_refused; `hot-region-path` (one cache key dropped from the list) CAUGHT by
  init_cold_startup_functions_are_grouped. 2 CAUGHT, 0 WARN. Superseded: `-plants` (baseline: package named
  `liella-core`, it is `liella`), `-plants2` (role sync skipped while the gate ran, old file).
- Counts: `nscache-second-lookup-prof-base` (before) and `nscache-second-lookup-hot1` (regen + after), rc 0.

## Not run (and why)
- Booked timing on both timing CPU classes (Ivy Bridge, AVX2): budget. Needs ~45-75 min of booking wait plus staging.
- Full-profile regen (`LIELLA_FULL_PROFILE=1 cargo xtask so` + `regen.sh`, union): only the fast profile was traced, so
  release-build names that differ are simply not grouped (no harm, less gain).

## RULING NEEDED (not implemented)
1. Boot ID on a tmpfs region: a file in tmpfs cannot outlive a boot, so reading `/proc/sys/kernel/random/boot_id` (13.4k
   per attach) proves nothing there. Option: `fstatfs` the region (about 1k) and on TMPFS_MAGIC skip the read and the
   header comparison of the boot words, but keep writing them. This changes a check of the owner's amendment, so it needs
   the owner. Recommendation: allow it.
2. Attach at break-even instead of at the 2nd lookup (D-16.10 as-implemented item 6): moves the cost rather than cutting it.
   Recommendation: no.

## Next step
Book Ivy Bridge and an AVX2 timing host, stage per handoff/timing-sweep-1105.md, time the static `percall` (before =
ff125cce products, after = this branch) interleaved, and print the break-even in µs. Then regen the full profile and
union it. Rows: `grep -rn "^| D-16.10 " plan/`.
