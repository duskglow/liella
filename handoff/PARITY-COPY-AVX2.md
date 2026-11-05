# PARITY-COPY-AVX2 (continued by PARITY-COPY-AVX2-2): handoff

State: branch `task/parity-copy-avx2-2` (continues `task/parity-copy-avx2`), from origin/next f8b77d70. Code commit 1e8ffb15
(AVX2 copy: held 128-byte end block); handoff/plants commits 9453c446, c0bff978; docs wording a86b97cb; this handoff on top.
Role this run: kanon:~/w/kn1 (Broadwell-EP E5-2699 v4: AVX2 + ERMS, no FSRM, no AVX-512 -> `AVX2_ERMS_NARROW` window).

## Done (both runs)
1. Counter probe: `rep movsb`/`rep stosb` = ONE instruction on Skylake-SP, Haswell-EP, Ivy Bridge (docs/benchmarks.md 2a,
   register 03). Ruling #3046 / owner 11-05: rep-string copy rows are judged by timing on a booked host (same-host ratio);
   vector-vs-vector rows are gated on instructions, verdict from the timing sweep. a86b97cb aligns 2a and register row 9.
2. Profile of a 1 KiB AVX2 copy (by reading, 170 vs 137 Haswell-EP): entry/dispatch 25-30; overlap/direction 15; alignment
   setup 10; 128-byte loop 77; 32-byte tail loop 14 (the pure-overhead phase); ends 5.
3. Change 1e8ffb15: each direction loads the 128-byte block at the end it runs towards before any store, stores it last;
   the 32-byte loop is gone. Expected -9 per call at 32-aligned power-of-two sizes, +3 where n mod 128 is 1..32 past the
   aligned start. Kernel contract n >= 128 (all production callers pass >= 128). SSE2 path untouched.

## Jobs
- keke:parity-copy-avx2-gate2: rc 0 (direct tier, clippy + static + affected --base origin/next) on tree 9453c446;
  product identical to the final head (later commits touch handoff/ and docs/ only).
- kanon:parity-copy-avx2-2-lint: rc 0 (cargo xtask clippy + static) on c0bff978. Static re-run in the foreground on the
  role on a86b97cb: rc 0 (log kanon:~/work/parity-copy-avx2-2/static2.log; no jobrun possible, see below).
- kanon:parity-copy-avx2-2-plants: rc 0, 6 plants, 6 CAUGHT, 0 SURVIVED, 0 WARN (handoff/PARITY-COPY-AVX2-plants.txt,
  `cargo xtask plants`, dir /dev/shm/claude/parity-copy-avx2-2/plants-1).
- Percall NOT MEASURED. kanon:parity-copy-avx2-2-percall rc 1: my command did not source ~/.cargo/env (`cargo: command
  not found`). -percall2 and -percall3 have NO jobs.tsv row: the slot queue refused them, `slots: REFUSED ... only 14G free
  on /home/claude's filesystem (floor 15G); free space first` (seen in percall3's jobrun stderr). kanon / is 48G, 14G free;
  ~/wl/src holds 12G of cached trees from many runs (not mine except f8b77d70 57M and c0bff978 303M; ram-sweep does not
  cover ~/wl). A CI job (ci-pre-65e1bc3aa, queued 19:10:33) vanished the same way. Every jobrun on kanon is blocked until
  space is freed; I did not bypass the floor or delete others' caches.
- Base f8b77d70 is now primed on BOTH kanon and keke (`~/wl/src/f8b77d70...`): the keke percall2 failure cause is fixed.

## Next step (exact)
1. On a host with >= 15G free (keke preferred: Zen 1, no ERMS, every 257 B-64 KiB copy runs the AVX2 kernel; or kanon
   after its ~/wl/src cache is pruned), from the synced role:
   `setsid nohup ~/bin/jobrun <story>-percall count -- sh -c '. ~/.cargo/env; cd ~/w/<role> && bash validation/wl_percall.sh --spec validation/percall/avx2_families.c --cases memcpy_16,...,memcpy_65536,memmove_16,...,memmove_65536 .' >/dev/null 2>&1 &`
   (sizes 16 31 32 33 64 65 128 129 256 1024 4096 16384 65536). Judge base vs new: any ladder size that rises (129 and
   other n mod 128 in 1..32 sizes are the risk) -> revert 1e8ffb15 and hand back DO NOT LAND with the data.
2. Skylake-SP count of memmove_4k (436.8 vs 413.0; expect about 428) on a count job there.
3. Timing of the kernel change on a booked AVX2 timing host (3 extra loads/stores at entry change the mix).
4. 1 KiB still about 24 over after the change: profile the entry/dispatch chain (25-30) with wl_trace.sh.

## Not to redo
The rep-string probe; the plants (6/6 CAUGHT on c0bff978's code); gate2 (product unchanged since).
Process slip (run 1): one line of scan.S was edited with `sed -i`; content reviewed, correct.
