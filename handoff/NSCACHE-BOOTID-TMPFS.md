# NSCACHE-BOOTID-TMPFS (handoff)

**Status: INCOMPLETE.** The code, tests, D-16.10 amendment and plants file are pushed. Gate, plants and counts are
QUEUED on keke (the host was full: 28/28 cores busy). None of them has run, so nothing here is verified on a build
yet, not even compilation (this host has no cargo).

**State.** Branch `task/nscache-bootid-tmpfs`, stacked on `task/nscache-second-lookup-2` (3ff284ae). Code commit
7bd7966d; the handoff commit is on top. Role keke:~/w/n3, synced from 7bd7966d plus the plants file
(`--base origin/task/nscache-second-lookup-2`, tree 7c632434...-8cf8acdd).

## The change (owner ruling #3233)
- `crates/legacy/src/nscache.rs` `map_region`: for an existing region (`!created`), `fstatfs(fd)`. On
  `TMPFS_MAGIC` the stamp takes the header's boot words (`Table::boot()`), so `/proc/sys/kernel/random/boot_id` is
  not opened or read and the boot comparison always passes. If the header is busy (`None`), it falls back to reading
  `/proc`. When the region is created, or is reinitialised under the OFD lock (`!ok || !consistent`), the boot ID is
  read from `/proc` and written, so the header always holds a real boot ID and a region on another file system is
  still compared.
- `crates/core/src/names/cache.rs`: `Table::boot()`. `crates/kernel/src/uapi.rs`: `TMPFS_MAGIC` (0x01021994, from
  statfs(2)).
- Cost per attach to an existing region: one `fstatfs` replaces openat, read and close of boot_id. A created
  region does neither `fstatfs` nor the skip.

**Checks that are unchanged** (none were edited; the diff only touches the boot-word source):

| check | status |
|---|---|
| AT_SECURE | unchanged |
| real vs effective UID/GID | unchanged |
| overflow UID | unchanged |
| initial user namespace | unchanged |
| O_NOFOLLOW / O_NONBLOCK open and O_EXCL create | unchanged |
| descriptor above the standard ones | unchanged |
| fstat regular file, owner == euid, mode 0600 | unchanged |
| size bounds and geometry | unchanged |
| header magic, layout, size, owner and account stamp | unchanged |
| stamp files | unchanged |
| seqlock and checksums | unchanged |

## Tests (`crates/boundary/tests/nscache.rs`, clauses s17_3_c_1, s16_1_g_3)
- `a_region_on_tmpfs_attaches_without_reading_the_boot_id`: region under /dev/shm (skipped with a printed reason if
  it is not tmpfs). Read count from `/proc/thread-self/io` `syscr`, one pread per sample: a lookup right after an
  attach makes the same number of reads as a lookup without one. A stale boot word (0x5a x16) is still answered and
  is not rewritten. With the magic damaged, the region is reinitialised with the boot ID it was made with.
- `a_region_on_another_file_system_compares_the_boot_id`: the first directory that is not tmpfs out of
  CARGO_TARGET_TMPDIR, HOME and /var/tmp (skipped with a reason if none qualifies). An attach makes exactly one more
  read. A stale boot word means no hit, and the header is rewritten with the real boot ID.
- Assumption not yet checked: the warm reattach takes the cheap account path, so it reads no passwd. If the read
  counts differ for another reason, the gate will show it.

## Plants (`handoff/NSCACHE-BOOTID-TMPFS-plants.txt`)
1. `boot-skip-everywhere` (the required one): the skip also happens off tmpfs.
2. `boot-never-skipped`
3. `reinit-keeps-stale-boot`

## Jobs (keke, queued at 00:16Z; collect with `~/bin/jobwait <label>`, logs in `~/logs/<label>.log`)
- `nscache-bootid-tmpfs-gate`: clippy, static, then `gate 12G cargo xtask affected` (direct tier).
- `nscache-bootid-tmpfs-plants`: `cargo xtask plants handoff/NSCACHE-BOOTID-TMPFS-plants.txt --dir /dev/shm/claude/NSCACHE-BOOTID-TMPFS/plants-1`.
- `nscache-bootid-tmpfs-counts`: `cargo xtask so` (fast, then full profile), then
  `validation/probes/nscache/measure.sh ~/w/n3 after ~/work/NSCACHE-BOOTID-TMPFS`. **Before** = NSCACHE-SECOND-LOOKUP-2's
  release numbers: cached 2nd call 129.5k kernel instructions and 8 faults; boot_id step 13.4k. **After** is in the
  counts log. Break-even before: 8th lookup on Ivy Bridge, 10th on N100. Re-timing needs a booked `timing.sh` run.
- `nscache-bootid-tmpfs-check` (a duplicate of the gate) was cancelled while still queued.

## Next step
Collect the three jobs. If the gate is red, fix it and rerun only `-p liella-boundary --test nscache`. Plants must
be all CAUGHT. Put the after counts and the new break-even next to the before numbers. Rows to read:
`grep -rn "^| D-16.10 " plan/` ("Boot ID on tmpfs").
