# Compat-only NSS internals (spec §16.1; phase 15 LG-7 `COMPAT-NSS-RESOLV`)

Black-box observations of the reference library (glibc 2.39, Ubuntu 24.04, x86_64), made on
2026-10-23 because §16.1 names `__nss_database_lookup`, `__nss_next` and the `__nss_*_lookup`
compat names in its title and gives them no contract or signature. All five exist only as
`name@GLIBC_2.2.5` (no default version) in `libc.so.6`.

Probe: `crates/core/tests/oracle/nss_resolv_compat_probe.c` (binds each compat version with
`.symver`; every call in a forked child). Record: [`nss-resolv-compat.glibc.txt`](nss-resolv-compat.glibc.txt).
No header declares these names, so the probe calls each with six pointer arguments: all NULL,
six 64-byte buffers of `0xAA`, or the strings `"passwd"`, `"files"`, `"files"` followed by buffers.
It prints the `int` result, `errno` (preset to 12345) and every buffer byte the call changed.
Nothing here describes or derives from the reference library's source code.

Clauses: none states these functions' contracts; the nearest is `s16_1_c_0` (§16.1's lookups,
whose title names them).

## Observations

* **`__nss_database_lookup`:** returns -1 and writes 8 zero bytes (a NULL pointer) to its fourth
  argument; nothing else is written and `errno` is unchanged. With a NULL fourth argument the
  call faults.
* **`__nss_next`:** returns -1; writes nothing; `errno` unchanged.
* **`__nss_group_lookup`, `__nss_hosts_lookup`, `__nss_passwd_lookup`:** return -1 with `errno`
  `ENOSYS`; write nothing.

## Liella

`crates/boundary/src/posix/s16_nameservices.rs`: `liella_compat___nss_database_lookup`,
`liella_compat___nss_next` and `liella_compat___nss_{group,hosts,passwd}_lookup` (one body each),
exported only as the compat versions by `cargo xtask compat-gen`'s stubs (the plan's behaviour
rows, `spec/abi/compat-plan.tsv`, R15-1), not in `libc.a` (R15-2). Same results as above, with one difference: a NULL
fourth argument to `__nss_database_lookup` is not written instead of faulting (the S6 rule
for NULL out-pointers, as in `digits-dots.md`).
Tests: `crates/boundary/tests/s16_nameservices_symbols.rs` `sym___nss_*`.

## libresolv.so.2 hosts-file walkers (LG-7 part 2, probe rerun on shiki 2026-10-23, job compat-nss-resolv-pt2-probe)

* **`_gethtbyname(name)`, `_gethtbyname2(name, af)`:** the first hosts-file line of family `af`
  (`AF_INET` for `_gethtbyname`) naming `name`; no switch, no DNS, no numeric parsing
  (`127.0.0.1` is not found). Failure: NULL, `h_errno` `HOST_NOT_FOUND`, also for an unknown
  family (`gethostbyname2` gives `NO_RECOVERY` there). `errno` is never changed.
* **`_gethtbyaddr(addr, len, af)`:** the first line of family `af` whose address begins with the
  `len` bytes (a 3-byte `127.0.0` finds the `127.0.0.1` line, where `gethostbyaddr` fails).
* **`_gethtent()`:** every hosts line, both families (7 on shiki where `gethostent` gave 3); NULL
  with `HOST_NOT_FOUND` at the end and after it; **`_sethtent(1)`** rewinds.
* All results are one static entry of their own (never `gethostbyname`'s buffer).
* **`res_gethostbyname`, `res_gethostbyname2`, `res_gethostbyaddr`:** DNS only (the hosts file is
  not read: `localhost` came back as the DNS search-domain name); numeric names are parsed.
  Settled on keke 2026-10-23 (COMPAT-NSS-RESOLV-PT2-2; probe under strace, and a controlled DNS
  answer from a responder in an unprivileged user+network namespace with its own resolv.conf):
  `res_gethostbyname2` ignores `af`: only an `A` query is sent and the entry is `AF_INET`, also for
  `AF_INET6` or 99. A name of digits and dots not ending in `.` is a dotted quad (`127.0.0.1`
  even for `AF_INET6`) or HOST_NOT_FOUND with no query (`1.2.3`, `1.2.3.4.5`, and a part of
  four or more digits: `0127.0.0.1`, `1234.0.0.1`, `4294967423.0.0.1`, a 20-digit part); a name
  with `:` is HOST_NOT_FOUND with no query; `""` is NO_RECOVERY with `errno` EMSGSIZE (the rerun
  without a search list left `errno` 0). Any other name is sent as it is, host name or not
  (rerun 2026-10-23, COMPAT-NSS-RESOLV-PT2-3, strace in a namespace whose only server refuses:
  `host_x.example.`, `bad_name!`, `1.2.3.4.` and a 63-byte label were each sent once, then
  HOST_NOT_FOUND with `errno` ECONNREFUSED); a 64-byte label or a name over 255 bytes is
  NO_RECOVERY with `errno` EMSGSIZE and no query. Answers: CNAME target
  as name, the asked name as alias, every address; trailing dot dropped; `h_errno` 0.
  `res_gethostbyaddr`: length/family errors as `gethostbyaddr` (NETDB_INTERNAL, EINVAL /
  EAFNOSUPPORT); an IPv6 address under `::ffff:0:0/96` or `::/96` asks `in-addr.arpa` for its
  last 4 bytes and (mapped form, observed) answers `AF_INET`; other IPv6 asks `ip6.arpa`, answers
  `AF_INET6`. The three share one result buffer, not `gethostbyname`'s.

Liella (`netdb.rs` `compat_res_gethostby*`, entries `liella_compat_res_gethostby*`, block
`users::RES_HOST`): as above over the thread's resolver (`resolv_compat`). Names are classified as
above (`res_name`) and every other name is asked for, host name or not. Differences: `errno` after a DNS exchange is whatever Liella's resolver leaves (the reference set 0); the
`::/96` answer shape is assumed `AF_INET` like the mapped form.
Tests: `sym_res_gethostbyname`, `sym_res_gethostbyname_names` (the probe table), `sym_res_gethostbyname2`,
`sym_res_gethostbyaddr` (no-DNS paths).

Liella (`crates/boundary/src/netdb.rs` `compat_*`, entries `liella_compat__gethtby*`,
`liella_compat__gethtent`, `liella_compat__sethtent` in `s16_nameservices.rs`): as above, with
one choice where nothing was observed: a `len` longer than the family's address never matches
(Liella reads at most 16 bytes). `_sethtent` rewinds whatever its argument. Exported only when
phase 14 builds `libresolv.so.2` (`spec/abi/compat-plan.tsv` rows, D-16.6).
Tests: `crates/boundary/tests/s16_nameservices_symbols.rs` `sym__gethtby*`, `sym__gethtent`, `sym__sethtent`.
