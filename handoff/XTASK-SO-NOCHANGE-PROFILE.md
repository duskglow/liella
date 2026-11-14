# XTASK-SO-NOCHANGE-PROFILE (#5121): where a no-change `cargo xtask so` spent its time, and the fix

State: branch `task/xtask-so-nochange-profile`, from origin/next b31418181. Code: 056a5ed91 (the fix, test, plants
file), then the record, this note and the report. Decision: `records/pending/XTASK-SO-NOCHANGE-PROFILE-1.toml`
(D-NEW, series D-X; the coordinator assigns the id). Tooling only: the release products are byte-identical before
and after (all 23 files of `target/liella`, sha256 compared on keke). Fast products: 21 of 23 identical; `libm.a`
and `libm.so.6` differ, because libm's dependencies are now the tagged copies it shares with libc and the fast
profile's thin-LTO output (16 codegen units) depends on their bytes. Their behaviour is checked by the gate's libm
tests on these products; the bytes were not diffed (the old fast libm was overwritten). Not touched: the layout check,
hottext.rs.

## Profile (before)

Host keke (EPYC 7551, Zen 1), role `~/w/n1`, one 4-core gate slot, origin/next b31418181. Exec trace of each run:
`strace -f --seccomp-bpf -ttt -e trace=execve,clone,clone3,vfork,fork` (logs and traces in keke
`~/work/XTASK-SO-NOCHANGE-PROFILE/before/`; scripts `prof.sh`, `tl.py` there).

Nearly the whole no-change build was **rustc recompiling unchanged crates** (23 crate compiles per run). The cargo
no-op check, the xtask build, the post-link passes and the layout check (verdict cache hit) were each under 0.3 s
per process in the trace. Rebuilds per no-change run (seconds until the next exec):

| crate (build that rebuilt it) | fast | release |
|---|---|---|
| liella_mem, liella_rt, liella, liella_abi, liella_legacy (libc) | 1.0 + 1.3 + 13.4 + 0.7 + 5.9 | 1.0 + 14.8 + 0.4 + 5.7 |
| liella_boundary (libc) | 24.1 | 78.2 |
| `c`, the libc staticlib (libc) | 57.3 | 69.5 |
| liella_ld, liella_boundary (libm) | 1.5 + 24.2 | 1.5 + 80.4 |
| `m`, the libm staticlib (libm) | 19.6 | 51.0 |
| liella_mem, liella_rt, liella_abi, liella_legacy, liella_boundary (ld.so) | 1.0 + 14.9 + 0.6 + 5.5 + 26.0 | 1.0 + 15.2 + 0.4 + 5.8 + 79.8 |
| `ld_so` staticlib (ld.so) | 3.5 | 6.1 |
| whole run | 201.3 | 411.7 |

Cause: the libc, libm and interpreter `cargo rustc` builds shared the target directory `target/product` with three
different settings: libm ran without `LIELLA_BUILD_TAG` (read with `option_env!` by liella_boundary and liella_ld),
and the interpreter with `-Crelocation-model=pie`. Cargo keeps one copy of each dependency per target directory and
profile, so each build rebuilt what the previous one had built differently. The per-command times include the
post-processing that ran inside them (the trace analyser missed execs strace logged as `<unfinished>`; corrected for
the after-traces). The after-measurements give the split: the thrash was about 155-160 s of 200 s fast and about 375 s
of 400 s release. After the fix the traces show 0 crate compiles in either profile (23 per fast run before).

## The fix (056a5ed91)

`so::ProductBuild` (manifest, archive, target directory, own flags); `LIBC_BUILD`, `LIBM_BUILD`, `LD_BUILD` give all
three cargo commands and their archive paths. Every build sets the build tag, so libc and libm share their
dependencies (the libm products are unchanged). The interpreter builds in `target/product/ld-pie`. Test:
`so::tests::product_builds_sharing_a_target_directory_build_alike`. The first build after the change compiles the
interpreter's dependencies once in the new directory (fast 210.6 s, release 407.1 s on keke).
The old `target/product/<profile>/libld_so.a` stays on disk unused (nothing reads it: the test checks the archive path).

## Measurements (keke, one 4-core gate slot, wall seconds; identical runs vary by about +-10 s on this host)

| run | before | after |
|---|---|---|
| fast, no change | 201.3, 205.0 (straced), 198.7, 195.9 | 42.3, 47.4 (straced), 42.3 |
| release, no change | 411.7, 415.8 (straced), 394.4, 393.1 | 22.7, 27.3 (straced), 22.6 |
| fast, one-file change (comment appended to crates/core/src/getopt.rs) | 211.7 | 203.7 |
| fast, the file restored | 198.8 | 190.4 |
| release, one-file change | 400.2 | 406.0 |
| release, the file restored | 396.8 | 401.7 |

A one-file change costs about the same as before: any edit under `crates/`, `csys/` or `include/` changes the build
tag, so every tagged crate rebuilds in both target directories anyway. Jobs: `xtask-so-nochange-profile-prof1` and
`-onefile-before` (before code), `-after` (after; logs in keke `~/work/XTASK-SO-NOCHANGE-PROFILE/after/`).

## What is left (next step)

After the fix a no-change build is all post-processing: about 1400 `objcopy`, 770 `readelf`, 42 `ar` runs and a few
in-process passes over unchanged archives (release 23 s, fast 42 s; the fast archive has more sections). The largest
in-process gaps in the release trace were 1.6-2.3 s each. A cache keyed on the three raw cargo archives, the xtask binary,
the tools (as `layout_verdict::host_inputs` does) and the tree inputs, checked against the hashes of the
published products, would bring a no-change build down to the cargo no-op checks and the hashing. Not done here
(context checkpoint); the key's tree inputs need care: `compat.rs` walks all of `crates/`, `math.rs` and
`native.rs` read tables in `spec/` and `crates/`, and the `s29` legacy test reads
`target/product/placeholders/*.bodies.c`, an intermediate the cache must keep or verify.

## Tests, plants, gate

Plants `handoff/XTASK-SO-NOCHANGE-PROFILE-plants.txt`: 4 CAUGHT, 0 SURVIVED, 0 WARN (job
`xtask-so-nochange-profile-plants`): interpreter back in the shared directory, libm without the tag, archive read
from the shared directory, cargo told the shared directory. WARN-DEV-SHM printed (copy under /dev/shm; the test
mounts nothing). Clippy and the 12 `so::tests`: job `xtask-so-nochange-profile-lint2`, rc 0. The gate escalates to
FULL (`xtask/src/so.rs` is on the product path): job `xtask-so-nochange-profile-gate`.
