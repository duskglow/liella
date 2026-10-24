<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 12: File system (high level) and patterns

* **Scope:**
  * §13.1–13.3 and §13.8–13.9: cwd, directory streams, scandir, ftw/nftw/fts, realpath,
    mkstemp and friends;
  * §14.7–14.10: fnmatch, glob, regex, wordexp.
  * §13.13: the mount-table functions (moved here by D-13.10; added to the scope list
    2026-10-03, when the phase-12 file-system part found the list did not name them).
* **Symbols:** about 110.
* **Clauses:** about 70.
* **Needs:** P10 (`~user` expansion), P11 (wordexp command substitution).
* **Test surface:**
  * Directory iteration while entries are created and removed (the "exactly once" guarantee
    of §13.2).
  * Walks over deep trees longer than `PATH_MAX` (D-13.5).
  * Symlink loops.
  * realpath prefix-on-failure.
  * mkstemp exhaustion (`EEXIST`).
  * Regex: the POSIX leftmost-longest corpus, back-references, and a linear-time guarantee
    for patterns without back-references (D-14.5).
  * `REG_STARTEND` with embedded NUL.
  * glob: every flag, and both versioned contracts.
  * wordexp error codes.
* **Exit:** in-scope clauses and symbols green.
* **Delivered** (exit met, with the deviations listed):
  * **Owner additions in this phase (2026-10-03):** the integrated name-service cache replacing
    nscd (D-16.5 closed; D-16.10: a per-user shared region with the owner's security amendments,
    stat-based change detection rate-limited to one check a second after a validating first
    lookup, a native API in `LIELLA_NATIVE_1`); secure memory (D-X.11: `liella_secure_zero`, proven
    to survive LTO by disassembly, and `liella_heap_alloc_secure`, zeroed on every release path);
    the three-way rule for spec disagreements (§1.2), the decide-or-ask guidance, the security pass
    after phase 15, and `spec/security/threat-model.md` started with the environment of secure
    processes.
  * **Method (§1.4):** behaviour documents first (`fstree.md`, `patterns.md`, `mntent.md`, with
    their records; directory-order observations name their filesystem, owner rule), a regex
    submatch analysis against 206,863 recorded cases (`regex-submatch-analysis.md`) before any
    regex design, then fresh sessions per part.
  * **Directory trees (§13.1–13.3, §13.8–13.9; D-13.2, D-13.13, D-13.14):** descriptor-relative
    walks with no depth limit that never abort; `readdir` reports the kernel's `ENOENT` for a
    removed directory (owner ruling); `realpath` without a length limit; `get_current_dir_name`
    trusts only a plain absolute `$PWD`, and none in secure mode (owner rulings).
  * **Mount tables (§13.13; D-13.15):** no hangs, crashes or silent truncation; §13.13's escape
    list corrected (three-way rule).
  * **Patterns (§14.7–14.10; D-14.4, D-14.5, D-14.7):** a linear-time regex engine with POSIX XBD
    9.1 submatches (owner ruling), structured for a glibc-compatible mode that is designed but not
    built; fnmatch and glob sharing one collating-element parser (`[[.é.]]` accepted); wordexp
    with the owner's rulings (`WRDE_NOCMD` fails closed, full POSIX arithmetic with errors for
    undefined cases, assignment operators refused, `${X:=w}` local to the call).
  * **Security (owner rulings):** the resolver's environment ignored in secure processes, and the
    secure-mode environment list rebuilt from ld.so(8) and a set-group-ID probe (34 names, cleared
    before constructors); the earlier list's origin recorded in §1.2's clean-room section.
  * **Spec corrections (three-way rule or owner ruling, each with its evidence):** §13.8 (the
    `realpath` buffer after a failure), §13.13 (the escapes), §14.7 (invalid multibyte text),
    §14.9 (XBD 9.1 cited), Appendix D (the collation-sequence tables).
  * **Tests:** §13 and §14 have every in-scope clause and symbol specified. Differential tests
    against the host glibc: `fstree_products`, `patterns_products` (54,950 regex lines, every
    differing one checked against an independent POSIX reference), `mntent_products`,
    `nscache_products`, and every phase-10/11 differential test run with the cache on and off;
    `tests/secure_env.rs` (a set-group-ID copy, no root). The full gate also exposed and fixed
    six timing races in earlier tests (the getaddrinfo_a queue shared by two tests, the
    resolver probe's rounding of elapsed time, waitid WCONTINUED in the process probe and in a
    clause test, `ptsname`'s shared buffer, the cache test's first-lookup assumption).
  * **Budgets:** `benchmarks/progress.md` entry 10 and the review before phase 13 (93 budgets
    met); fixed: `realpath` (14.6 → 8.6 µs), `getmntent`, fnmatch (about 1.7×), glob. Deferred
    with causes: fnmatch and wordexp engines (11–21×, 7–9×), the regex overall-match pass (a lazy
    DFA), the cache's attach cost, `fts` node mirroring.
  * **Also fixed in this phase:** `fopen(path, "a")` started at 0 instead of the end (D-12.16);
    `wordexp` left `we_offs` unset without `WRDE_DOOFFS`, crashing `wordfree`.
  * **Deviations and open items for the owner** (batched for the phase-13 report).
