# PERF-NFTW-ENTRY-11 (continues -10; fts_physical_1000; branch task/perf-nftw-entry-11, role keke r3)

INCOMPLETE: no gain found; row stays at 1.053 (target 1.05), nftw_phys_1000 at 1.02.

Done: (1) reverted the -10 cold side vector (ruling #2886) and its plants file. (2) Tried the next step named by -10: status bytes
straight into the block (Stat -> 144 bytes by `FtsMem::stat_raw`). Result 6 instr/entry fewer (6686035.6 vs 6691980.7; ratio 1.0529 vs
1.0533): neutral, so reverted too. It also failed `cargo xtask static` (unsafe category `slice` not permitted in dirtree.rs) and its
plant stat-raw-short survived (the size of the byte view is not checked by any test). The branch head is the reverted state: fts.rs as in -9.
Counts on keke (Skylake-SP), jobs pne11-counts3 (with the copy) and pne11-counts4 (head): see the register entry in
docs/saturation/13-next-perf-stories-from-the-10.md (PERF-NFTW-ENTRY-11).
Gate: pne11-gate5 (static, affected: 0 suites, no base), pne11-gate6 (clippy -D warnings, affected --base origin/next --run).
Plants: pne11-plants3 on the reverted-away copy only (3 CAUGHT, 1 SURVIVED); the head has no new code, so no plants.

Do not redo: cold side vector (-10), status byte copy (-11), Node hot/cold split gains (~1 instr/entry).
Next step: the build closure (~230 instr/entry vs ~60 estimated minimum): merge the two TryVec pushes per entry, hoist the type test.
If that moves under 0.3%, record the row as a first-principles bound with the per-phase split and stop.
Rulings: none needed.
