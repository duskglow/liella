# key-nomem

**Spec:** §22.6 (`pthread_setspecific`, `pthread_getspecific`).
**Clauses:** `s22_6_f_0`. Decision D-NEW (plan/07-decisions/s22-threads.md).

**Question:** with the address space exhausted, which keys does `pthread_setspecific` set on a
thread that has never stored a key value, and which fail with `ENOMEM`? What does
`pthread_getspecific` return after each? The same on the initial thread and on a new thread.

**Probe:** `key-nomem.probe.c` creates 1024 keys, then for each case forks a child that (on the
initial thread, or on a thread created before the limit) sets `RLIMIT_AS` to the process's size
plus 64 KiB, takes `malloc` blocks of 1 MiB down to 16 bytes and anonymous pages until each
fails, and then either sets the keys in creation order until one fails (`first-fail`) or sets
the one key of the given creation index (`idx=`), and reads it back. Run through the probe
service (`~/bin/probe`, probe hash 6e4cf0a03f48), against glibc 2.39 (dynamic) and Liella's
static products, on a Zen 1 host, 2026-11-06.

**Observed (glibc 2.39, `key-nomem.glibc.txt`):** creation indexes 0 to 31 are set and read back
on both threads; index 32 and every later index tried (33, 63, 64, 500, 1000) return `ENOMEM`,
and `pthread_getspecific` then returns NULL. Setting in creation order stops at index 32 with
every earlier value intact.

**Liella:** every key is set and read back on both threads (the probe's Liella output reads
`all-set` and `set=0 get=value` on every line): each thread's key value storage is part of its
mapping from creation (D-22.9), so the call never needs memory. Conformance program
`conformance/c/s22_key_nomem.c` holds Liella to that.
