# SPAWN-STRAY-FD (branch task/spawn-stray-fd) - NOT READY

Cause (by inspection, NOT reproduced): s18_4_c_1 probed descriptors 5-12 in the child for any open one. A popen stream
(popen without the e mode is not close-on-exec, by design: crates/boundary/src/spawn.rs popen_locked) held by another test
in the same test process is inherited by every concurrent spawn and can sit on fd 8 briefly. Product behaviour is intended;
it is test isolation (candidate 3, with a popen stream rather than the harness: the harness lock on 8 is already handled
by keep_from_children). Fix: the child reports a descriptor only when its link names the insub file or the sub directory.
Repeat loops on kanon (RAM, old test + readlink): s18_4 subset 0/400 hits, full binary 0/300 hits (load 24): not reproduced,
so no plant and no before/after hit rate. Gate (direct tier, jobs spawn-stray-fd-gate2): 114 passed, 0 failed.
First fix attempt had a wrong path pattern (gate red, spawn-stray-fd-gate), corrected in the second commit.
Not verified: final-head clippy/static (job spawn-stray-fd-build3 rc 1 at an unknown step; the earlier build2 on the first
commit had clippy and static clean), the after loop on the final binary (spawn-stray-fd-after2 may have run a stale binary),
plants. Next step: rerun clippy+static, rebuild, loop 300 on the new binary; a deterministic test (popen stream held while
spawning) would prove the cause.
