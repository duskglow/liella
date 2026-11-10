
### X-3b `REGEX-BACKREF-FIXED`: ring of eight for back-reference patterns

`regexec_backref` (`\(b[a-z]*\)@\1`, 13-byte subject): a pattern with back-references set up a
ring of sixteen frontiers per call; it now starts with eight (`BR_RING`), and a thread sent further
than that waits on the pending list as before. Instructions:u per call on a Skylake-class build
host, `cargo xtask bench --compare --counters --runs 3 cbench_patterns cbench_opt`:
24,493 to 22,913 (glibc 21,970: ratio 1.11 to 1.04). No other regex row moved (every row within 0.1%).
Not at parity: the matcher's own steps (`run`, `insert`, `walk`) are the rest and are not fixed cost.
Not tried: lazily built frontiers (the pool layout is fixed up front). A back-reference of eight
bytes or more costs one pending-list allocation (`regex_backref_ring_*` tests cover 0 to 40 bytes).
