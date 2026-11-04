# compat-behaviour (LG-2) handoff

State: branch task/compat-behaviour-g2, code commit 268338d8 (on origin/next f3b3584a). Group 1 (memcpy, realpath,
glob, glob64, fmemopen @GLIBC_2.2.5) was already in the tree.

Done (group 2, partial): regexec@GLIBC_2.2.5 (only REG_NOTBOL/REG_NOTEOL honoured; STARTEND and unknown bits
ignored), nftw@/nftw64@GLIBC_2.2.5 (only FTW_PHYS/MOUNT/CHDIR/DEPTH; ACTIONRETVAL and unknown bits ignored),
quick_exit@GLIBC_2.10 (thread_local dtors, then quick-exit entries). Rules in crates/legacy/src/versioned.rs, entries
liella_compat_* in boundary s13/s14/s01 + process::quick_exit_2_10, rows removed from spec/abi/compat-missing.tsv,
compat_aliases.S regenerated (compat-gen on shiki sh1). Tests: compat_* in boundary tests s01/s13/s14 and versioned.rs
unit tests, each tied to the compat.glibc.txt probe lines.

Verification: gate job shiki:compat-behaviour-g2-gate2 on 16d00791 (clippy, static, so, affected --base origin/next) started detached;
still running at hand-back; superseded gate1 failed to compile a test (fixed). Plants NOT RUN (context budget).

Remaining: posix_spawn@ and posix_spawnp@GLIBC_2.2.5 (old: a file the kernel refuses with ENOEXEC runs with /bin/sh;
spawnp hands sh the bare name, record line "no #! script on PATH: child exit 2"; whether sh gets `file` or `argv[0]`
is not settled by the record: both were "script". Needs one more probe with argv[0] != file before coding).
The spawn child's Program::Search has no shell fallback today (crates/core/src/process.rs Program).

Next step: read the gate job; run plants (drop each mask in versioned.rs; drop run_thread_dtors in quick_exit_2_10;
expect compat_* tests to fail) from a RAM copy; then posix_spawn/p.
Plan rows: plan/06-phases/18-phase-15-legacy-tier-and.md LG-2; spec/abi/compat-plan.tsv rows 244,245,260,261,372,374.
