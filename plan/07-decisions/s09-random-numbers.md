<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

#### §9: Random numbers

| ID | Spec | Ann. | Decision |
|---|---|---|---|
| D-9.1 | §9.1 | ARB | `getentropy` keeps the 256-byte limit with `EIO`, as the contract states for glibc (ABI behaviour). The internal `Csprng::fill` has no limit. |
| D-9.2 | §9.1 | HIST | `arc4random*`: a per-thread ChaCha20 keystream reseeded from `getrandom`, held in pages marked `MADV_WIPEONFORK`, so the output stays fork-safe (the contract). If the vDSO offers `getrandom`, it is used (⚑ KERN interface). It is never a failure path. Lack of entropy terminates, per the contract. |
| D-9.3 | §9.2–9.4 | HIST | The deterministic generators are explicit-state values (`Rng48`, `AdditiveFeedback`, and `rand_r`'s 32-bit state), because their algorithms are contracts. Per OD-13 the `random`/`rand`/`rand_r` sequences are Liella's own documented generators (unspecified by the spec); only the `drand48` family is bit-exact. The global states for `rand`/`random`/`drand48`/`seed48` live in `legacy::prng_global` behind one lock (`rand` stays MT-safe). |
| D-9.4 | §9.3 | ARB (spec silent) | `random_data.state` holds the caller's state buffer itself (word 0 is the generator's header); `rand_type`/`rand_deg`/`rand_sep` describe it and `fptr`/`rptr`/`end_ptr` bound it. A zeroed `drand48_data` takes the default parameters on first use (`__init`), as glibc's does. |
