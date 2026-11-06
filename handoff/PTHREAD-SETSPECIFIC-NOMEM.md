# PTHREAD-SETSPECIFIC-NOMEM: handoff (pthread_setspecific and ENOMEM)

Branch task/pthread-setspecific-nomem (on top of task/search-nomem-tls, base origin/next c101c4c6). Code head bf94d6cf
(spec, test, docs; NO product code change). Role shiki:~/w/sh1. Decisions in force: D-22.5, D-22.9, D-22.10, D-NEW
(placeholder: the coordinator reserves the id and runs tools/assign_placeholder.sh before landing).

## Probe (black box, step 1; spec/behaviour/key-nomem.{md,probe.c,glibc.txt}; probe service hash 6e4cf0a03f48, keke)
1024 keys created, then per case a forked child exhausts the address space (RLIMIT_AS = size + 64 KiB, malloc 1 MiB..16 B
and pages until each fails) on the initial thread or on a thread made before the limit, then sets keys:

| case | glibc 2.39 | Liella (static products, ~/w/nightly 2d289a42) |
|---|---|---|
| set in creation order | first fail at index 32, ENOMEM, get NULL; 0..31 intact | all 1024 set, all read back |
| one key, index 0, 1, 15, 16, 30, 31 | set 0, get value | set 0, get value |
| one key, index 32, 33, 63, 64, 500, 1000 | ENOMEM, get NULL | set 0, get value |

Same on the initial thread and on a new thread. Boundary observed: creation index 32.

## Finding: the brief's premise holds only for test processes
Liella's products already never allocate in pthread_setspecific: every Liella thread (and the initial thread, static or
dynamic) holds the full key area (KEYS = 4096, 64 KiB) in its own mapping from creation (D-22.9;
crates/boundary/src/thread.rs `init_initial`, `take_mapping`/`adopt`). The ENOMEM SEARCH-NOMEM-TLS saw came from a TEST
process: glibc made its threads, and Liella "adopts" such a thread (maps its descriptor + key area) on first use (D-22.10,
`thread::desc()` -> `adopt()`). Products have no such thread. So no product change was made, and no counts are needed
(the setspecific/getspecific code is unchanged; before = after).

## What the branch adds (bf94d6cf)
- Spec §22.6 **Failure** (clause s22_6_f_0): Liella never fails with ENOMEM; glibc's observed rule quoted.
  spec/clauses.tsv regenerated with `cargo xtask stubs` (only that file changed).
- D-NEW row (plan/07-decisions/s22-threads.md), differences row (docs/differences-from-glibc.md; no compat option, reason
  in the row: a may-fail resource error no program can rely on; Liella succeeds wherever glibc does).
- conformance/c/s22_key_nomem.c + test fn s22_6_f_0 (crates/core/tests/s22_threads_clauses.rs): table of keys 0, 1, 15,
  31, 32, 33, 63, 64, 500, 1023 plus all 1024 in order, initial and new thread, each in a forked child under an exhausted
  address space (checks the exhaustion: a page mmap must fail with ENOMEM). Passes on the products (shiki prep job).
- search_nomem.rs: the settle wait STAYS (step 4's "or say why"): libtest's first wait also allocates std's thread handle
  and the channel's waiter entry, so even a non-allocating setspecific would leave the child exposed to the limit. The
  comment now says where the setspecific need comes from (test process, D-22.10). No 500x loops run: the test is
  unchanged in behaviour.

## Jobs (shiki)
- pthread-setspecific-nomem-prep rc 0 (stubs, so, conformance s22_key_nomem: 1 passed). Evidence, not final.
- pthread-setspecific-nomem-gate (RUNNING at hand-back, on bf94d6cf): clippy and static passed (affected started after them), then `affected --run`. affected ESCALATED TO FULL by its own rule
  ("spec/clauses.tsv: generated table or its input"); a new clause cannot avoid that file.
- pthread-setspecific-nomem-plants-2 rc 0, 2 CAUGHT with their msg (no WARN): high-keys-enomem (ENOMEM from key 32 on, the reference's bound) and
  low-key-allocates (`specific()` adopts a fresh mapping on every call = allocating for a low key).
- SUPERSEDED: pthread-setspecific-nomem-plants (rc 0 but both plants WARN-NO-MSG: the conformance runner writes the
  program's stderr to conformance/build/<name>.log.stderr, not to the log; -2's command prints that file).

## RULING NEEDED
- Brief step 2 (low keys' values in the thread block) is not needed for the product and would only help test processes.
  There it cannot even match glibc's 32 keys: Liella's block fields in a glibc-made block must stay inside glibc's reserved
  area (D-22.10), and only 0x180-0x280 (256 bytes) is free, while 32 keys need 384+ bytes (value + sequence number).
  Recommend: do not change the block layout. Keep search_nomem's wait (needed for std's own allocations anyway).

## Next step
Read the gate and plants-2 rows in shiki ~/logs/jobs.tsv; both rc 0 -> reserve D-NEW and land.
