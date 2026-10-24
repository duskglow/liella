# compat-objects

**Spec:** §2 (`sys_errlist`, `_sys_errlist`, `sys_nerr`, `_sys_nerr`), §20.8 (`sys_siglist`,
`_sys_siglist`, `sys_sigabbrev`), §3 (the inert malloc hook variables, `__morecore`, `mallwatch`),
§29 (`_null_auth`).
**Clauses:** `s0_6_c0_6_0` (the compat-only names old binaries were linked against). Plan row LG-6 `LEGACY-OBJECTS` (plan/06-phases/18-phase-15-legacy-tier-and.md).

**Question:** what do glibc 2.39's compat-only data objects hold, version by version? An old binary
that names one gets a copy relocation of the object's `st_size` bytes, so both the size and the
contents are ABI. This is a fact record.

**Probe:** `compat-objects.probe.c` binds every version with `.symver` and prints the contents;
`readelf --dyn-syms` of the probe gives the sizes it copied. **Record:** `compat-objects.glibc.txt`
(sumire, glibc 2.39, 2026-10-23, job `legacy-objects-probe`).

**Findings:**

* Sizes are the manifest's (`spec/abi/versions.tsv`): `sys_errlist`/`_sys_errlist` 1000, 1008, 1056,
  1080 bytes at `GLIBC_2.2.5`, `2.3`, `2.4`, `2.12` (125, 126, 132, 135 pointers); `sys_nerr`/`_sys_nerr`
  4 bytes holding 125, 126, 132, 135; `sys_siglist`/`_sys_siglist`/`sys_sigabbrev` 512 at `GLIBC_2.2.5`
  and 520 at `GLIBC_2.3.3` (64 and 65 pointers); the hooks, `__morecore` and `mallwatch` 8; `_null_auth` 24.
* Every older version is a prefix of the newest: no entry differs between versions or between a name
  and its underscore twin.
* `sys_errlist[n]` is the `strerror` text of `n`, except NULL where `strerror` says "Unknown error n"
  (41 and 58) and at 134 (past the last errno with a text).
* `sys_siglist[n]` and `sys_sigabbrev[n]` hold the description and the abbreviation without `SIG` for
  1 to 31 (`spec/abi/sigmsgs.tsv`), NULL at 0 and for 32 to 64.
* The hooks, `__morecore` and `mallwatch` are NULL; `_null_auth` is 24 zero bytes.
