# TLS-GET-ADDR-LATE-MODULE: handoff (BLOCKED on phase 14)

* Branch `task/tls-get-addr-late-module`, cut from `origin/next` at c3424c99; this run commits only this note,
  `handoff/tls-get-addr-late-module-report.md` and `handoff/reports/TLS-GET-ADDR-LATE-MODULE.json`. No code or test change.
* State: **BLOCKED.** Clause `s27_3_c_0` (lazy TLS block for a module dlopened after the thread started) has
  no Liella code to test or plant. Liella has no `dlopen` and no `__tls_get_addr` yet: `crates/ld/src/tls.rs` and
  `crates/ld/src/api.rs` say "Implementation phase: 14 ... Not yet implemented", `docs/spec-symbol-gaps.tsv` lists
  `__tls_get_addr` as "phase 14", and `plan/ph14/design.md` §7 lists s27_3_c_0 / s27_3_f_0 among its clauses.
  Conformance programs link statically against `libc.a` (`conformance/run.sh`), so a dlopen program does not link.
  A test that always skips on Liella would mark the clause as covered while proving nothing (CLAUDE.md rule 4),
  so none was added. The brief's plants (a) shared block, (b) `.tdata` not copied, (c) `.tbss` left as 0xAA need
  the lazy allocator, which does not exist yet.
* Done: the test design from the brief, as a black-box probe validated against glibc 2.39 (200/200 passes at
  load 16, 9 threads, 4 rounds), and a check that its assertions catch each of the three defects (acted out by
  helper-module variants: all CAUGHT with the expected message). Source and results are in the report.
* Probe finding to keep: a thread created after the early threads have **exited** can legitimately get an
  address one of them used (the block is freed and reused). The first probe draft failed 30/50 on glibc for
  this reason. Keep the early threads parked on a barrier while the late thread checks address distinctness.
* Decisions in force: none new. Plan rows to read: `plan/ph14/design.md` §7 (TLS), D-22.10.
* Not to redo: the glibc probe and the detection check (results in the report).
* Open: RULING NEEDED: requeue this story once phase-14 §7 (`liella_ld::tls`, `__tls_get_addr`, dlopen)
  lands, or fold it into that story as its test (recommended). `s27_3_f_0` (abort on allocation failure) needs
  OOM injection and stays out of scope, as the brief says.
* Exact next step (after phase 14): turn the probe into a conformance program with a helper `.so` built by
  the runner (needs dynamic-link support in `conformance/run.sh`), replace the `s27_3_c_0` stub with
  `conformance(&["s27_tls_late"])`, then plant (a)/(b)/(c) in `liella_ld::tls`.
