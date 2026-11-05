# PROFIL-SIGPROF-HISTOGRAM (clause s28_1_c_3)

State: branch task/profil-sigprof-histogram (from origin/next). Test only; no product code changed.
Host/role: keke:~/w/n3.

## What was done
* `crates/core/tests/s28_profiling_clauses.rs`: the `s28_1_c_3` stub is now a real test (file-level lint
  allowance like the other clause files). It runs profil over a 64 KiB text window starting 16 KiB below its own
  `spin` loop, at scale 0x8000. It spins in fixed 2M-iteration chunks (the support/timeout.rs deadline is only a hang
  guard) until 3 samples land in the buckets the offset and scale map `spin` to, and most samples must land there.
  Then: `profil(NULL,0,0,0)` disarms ITIMER_PROF (getitimer all zero), puts back the caller's earlier SIGPROF
  handler, and no counter moves and no SIGPROF arrives over 2x the profiled spin. A handler installed after the stop
  is still installed after a second stop. sprofil with two adjacent, non-overlapping regions (the spin window and the
  16 KiB below it) gets the same checks, plus a second start/stop cycle.
* What the glibc 2.39 black-box probe settled (keke, gcc): profil arms ITIMER_PROF at 10 ms with its own SA_SIGINFO
  action. profil(NULL) and sprofil(NULL,0,..) disarm it and restore the earlier action and flags. A stop while idle
  is a no-op and leaves a handler installed after the stop alone. **Scale 0 does not stop sampling** (the timer stays
  armed and samples are counted), so the brief's "or scale 0" stop is not glibc behaviour. Liella matches (scale 0
  maps every pc to bucket 0), and the test does not cover scale 0.

## Results (head in the report json)
* gate `profil-sigprof-histogram-gate`: affected direct tier with --base origin/next: 2 suites ok, 2 passed,
  0 failed; peak 1.1G; rc 0.
* plants `profil-sigprof-histogram-plants` (/dev/shm copy plants-1): (a) `profil_bin` `>> 16` changed to `>> 15`
  (wrong scale formula): CAUGHT, msg "no samples in spin's buckets"; (b) `timer(0)` dropped from the stop path
  (timer left armed): CAUGHT, msg "ITIMER_PROF is disarmed". Baseline and restored runs pass; no WARN lines; job rc 0.
* Code head for the gate and plants: 8ececa8f. Later commits touch only handoff/.
* `cargo xtask clippy` and `cargo xtask static` are clean on the role.

## Open / informational
* sprofil difference (probe): glibc 2.39 `sprofil(.., &tv, 0)` wrote tv = 0 and armed ITIMER_PROF with a 1 us
  interval. Liella uses 10 ms and reports tv = 10 ms. Not tested, not changed. Needs a story to decide
  (recommendation: probe further and match glibc, or add a differences row).
* Plant (a) is caught by the hang guard's message after about 10 s x the load scale.

## Next step
None for this clause. A follow-up story could cover sprofil's timer period and tv output (above) and PROF_UINT.
