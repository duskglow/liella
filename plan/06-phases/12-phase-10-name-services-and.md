<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 10: Name services and databases

* **Scope:**
  * §16, NSS, getaddrinfo/getnameinfo (plus the async forms), the legacy host and network
    lookups, the resolver, services/protocols/rpc/netgroups/aliases (185 symbols);
  * §17.3–17.6, the passwd/group/shadow/gshadow databases, getlogin, getusershell, utmp
    (about 95 symbols);
  * §15.3–15.7, address conversion, interfaces (netlink), IPv6 option helpers, ether, rcmd;
  * §24.5, gethostid.
* **Symbols:** about 360.
* **Clauses:** about 110.
* **Needs:** P9. The resolver needs timeouts and clocks; `getaddrinfo_a` needs threads.
* **Builds:** IP-25 (the NSS engine with the built-in `files` and `dns` sources; plugin
  modules wait for P14), the DNS client, RFC 6724 sorting, IDNA (UTS #46 tables, D-16.4),
  `_res`/`h_errno` state, and the `_r` packers.
* **Test surface:**
  * Config-file fixtures in a private mount namespace (`/etc/hosts`, `nsswitch.conf`
    variants covering every action and status combination).
  * A loopback DNS test server (written in the test suite) covering retries, truncation,
    TCP fallback, the `trust-ad` and `no-aaaa` options, response validation, and
    `ndots`/search-list order.
  * The `ERANGE` retry protocol for `_r` lookups.
  * Every `EAI_*` code.
  * utmp record locking with the timeout (D-17.6).
* **Exit:** in-scope clauses and symbols green (plugin-module tests deferred to P14).
* **Delivered** (exit met, with the deviations listed):
  * **Method (§1.4):** behaviour documents first (`spec/behaviour/netaddr.md`, `users.md`,
    `netdb.md`, `dns.md`, with their recorded outputs), probing the reference library in private
    mount and network namespaces with fixture `/etc` trees and a DNS server the probe runs itself
    on `127.0.0.1`; then four fresh sessions implemented the parts from the spec, the RFCs and
    man pages (allowed from 2026-10-02, §1.2) and those documents. Part 1 was restarted after a
    power failure; its draft was reviewed and completed by a fresh session.
  * **Addresses, interfaces, rcmd, hostid (§15.3–15.8, §24.5; D-15.1 to D-15.5, D-24.9):** strict
    address texts in `liella::net` (RFC 4291/5952), the historical forms and the rcmd family in
    legacy; interfaces over netlink only; per-thread slots for `inet_ntoa`/`ether_*`.
    `/etc/hostid`'s format observed with an owner-approved root probe (2026-10-03).
  * **Users and accounting (§17.3–17.6; D-17.3 to D-17.5, D-17.8):** borrowed record views and
    one `_r` packer; per-thread record blocks; locked process-wide cursors; OFD locks with timed
    retries (no `SIGALRM`) for `lckpwdf` and utmp; `getpass`, `login_tty`, `initgroups`.
  * **Name services (IP-25; D-16.1, D-16.2, D-16.7):** the NSS engine (typed `nsswitch.conf`,
    every STATUS=ACTION rule, merge, reload on change, `__nss_configure_lookup`), the built-in
    `files` source for every database, getaddrinfo/getnameinfo with RFC 6724 sorting (source
    addresses from a netlink route query) and `AI_ADDRCONFIG` from the same interface view. The
    interim `files` module of parts 1–2 (D-15.6) is retired: every lookup goes through the engine.
  * **DNS (D-16.3, D-16.4, D-16.8, D-16.9):** the stub resolver (unbounded configuration, the
    568-byte `_res` per thread and re-read on change, UDP/TCP with ID and question checks, parallel
    A/AAAA, EDNS, `trust-ad`, `rotate`, search lists, `HOSTALIASES`/`LOCALDOMAIN`/`RES_OPTIONS`),
    the message, name and text functions of libresolv (in `libc.a` with an empty `libresolv.a`,
    D-16.6), the `dns` source for hosts, IDNA as built-in UTS #46 tables (Unicode 15.1, no
    libidn2), and `getaddrinfo_a` with worker threads and both notification kinds.
  * **Tests:** §15.3–15.8: 15 clauses, 71 symbols; §17.3–17.6: 31 clauses, 83 symbols
    (`login_tty` included); §16: 37 clauses and 171 symbols specified. Differential tests against
    the host glibc: `netaddr_products` (1 decision), `users_products` (1), `netdb_products`
    (13 KNOWN, 1 ADDED, 2 DROPPED), `dns_products` (7 + 6 KNOWN, 1 DROPPED), each with its reason;
    conformance program `s17_initgroups` (in a user namespace). `cargo xtask test`: plain,
    export+selftest, conformance; clippy clean.
  * **Budgets:** `benchmarks/progress.md` entry 8 (`cbench_netdb`, nine sanity rows; 46 budgets
    met in three runs). The P9 review before phase 11 fixed numeric getaddrinfo/getnameinfo
    (no configuration `stat` when nothing is looked up: 5,969 → 464 ns, glibc 133), the user and
    services lookups (parity or faster than glibc) and `dn_comp`; the rest is deferred with causes.
  * **Deviations and open items for the owner:**
    * Spec over reference and cleaner choices, recorded in `docs/differences-from-glibc.md`
      (D-15.2, D-15.4, D-16.1 to D-16.4, D-16.8, D-16.9, D-17.3 to D-17.5, D-17.8).
    * **nscd (D-16.5) is open.** nscd is not installed on the VM and no client was written;
      lookups are answered directly (clause `s16_1_g_3` ignored). Deciding it needs an inspection
      of the nscd protocol, which would mean installing nscd on the VM.
    * **For the owner:** `getpass` has no length limit (man page, P-lift) against the spec's
      historical note "limited"; the source of DNS query IDs (`getrandom` per query, kept; seeding
      a generator once would be faster but is a security choice).
    * Spec gaps found: `ttyslot`'s terminal table (chosen: `/etc/ttys`); `__nss_hostname_digits_dots`
      has no signature (pending); merge when merged groups disagree on name or GID (the earlier
      wins); `host.conf` environment overrides and `_res_hconf` trim/reorder (not implemented);
      the `networks` database through `dns` (unavailable); `fp_nquery` headings with a non-zero
      `pfcode` (extrapolated); MINFO/RP records with single-label names (unverified).
    * Untested for lack of privilege or a remote service: the rcmd success path and an `rresvport`
      bind, `sethostid`'s `EPERM`, the exported `login`/`logout`/`logwtmp` writing the system files.
    * Compat-only symbols (`_gethtby*`, `__nss_*_lookup`, `res_gethostby*`, the 2.2.5 versions of
      `login`/`logout`/`logwtmp`) wait for phase 15; NSS plugin modules for phase 14.
