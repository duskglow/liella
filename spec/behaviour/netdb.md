# The netdb databases through the `files` source, and getaddrinfo without DNS

**Spec:** §16.1 (NSS, the `files` source), §16.2 (`getaddrinfo`, `getnameinfo`, `gai_strerror`),
§16.3 (legacy host and network lookups), §16.5 (services, protocols, RPC, netgroups), §16.6
(aliases), §15.7 (`ether_hostton`/`ether_ntohost`). Public interfaces: `<netdb.h>`,
`<aliases.h>`, `<rpc/netdb.h>`, `<netinet/ether.h>`. References (plan §1.2): POSIX
(`getaddrinfo`, `getnameinfo`, `gethostbyname`), RFC 3493, the `hosts(5)`, `networks(5)`,
`services(5)`, `protocols(5)`, `rpc(5)`, `ethers(5)`, `netgroup(5)`, `aliases(5)` and
`nsswitch.conf(5)` documentation.

**Clauses:** `s15_7_c_0`, `s16_1_c_0`, `s16_1_c_1`, `s16_1_g_1`, `s16_2_c_1`, `s16_2_c_2`,
`s16_2_c_2_1`, `s16_2_c_2_2`, `s16_2_c_2_3`, `s16_2_c_2_4`, `s16_2_c_2_5`, `s16_2_c_3`, `s16_2_c_4`,
`s16_2_f_0`, `s16_2_g_0`, `s16_3_c_0`, `s16_3_f_0`, `s16_5_c_0`, `s16_5_f_0`, `s16_6_c_0`.

**Probe:** `crates/core/tests/oracle/netdb_diff.c`, run by `netdb_run.sh PROGRAM SCRATCH`.
**Record:** `netdb.glibc.txt`. The first phase writes a fixture `etc/` in the scratch directory;
the second runs as root with it bind-mounted over `/etc`, in private mount and network namespaces
where only `lo` is up, so no lookup reads the system's configuration or leaves the machine. The
fixture's `nsswitch.conf` names only `files`; there is no `host.conf`.

## What the probe does

* `gethostbyname`/`gethostbyname2(AF_INET6)` for names, aliases, case variants, names on
  several lines, an empty name, numeric texts (IPv4, IPv6, the short `1.2` form);
  `gethostbyaddr` for IPv4 and IPv6; the `ERANGE` protocol of `gethostbyname_r`; `gethostent`.
* `getnetbyname`/`getnetbyaddr`/`getnetent`, `getservbyname` (with and without a protocol,
  aliases, case) / `getservbyport` / `getservent`, `getprotobyname`/`getprotobynumber`,
  `getrpcbyname`/`getrpcbynumber`, `ether_hostton`/`ether_ntohost`, `innetgr` (nested and
  looping netgroups) and the netgroup enumeration, `getaliasbyname`/`getaliasent`.
* `getaddrinfo` over flag, family and socket-type combinations, numeric and service forms,
  `AI_PASSIVE`, `AI_V4MAPPED`/`AI_ALL`, `AI_ADDRCONFIG` (only `lo` up), invalid flags, families
  and ports; `getnameinfo` with each flag and the buffer and length errors; every `gai_strerror`
  code.

## Observations the record shows directly

* `files` hosts: a name or alias matches case-insensitively and the first matching line wins
  (no `host.conf`, so lines are not merged): `beta` gives 192.0.2.2 only, and `gamma` gives only
  the family of its first line for `AF_UNSPEC`. A line with an invalid address is skipped. A
  numeric text is answered without the file (`1.2` as `inet_aton` reads it, 1.0.0.2). The
  canonical name is the line's first name (`ALPHA` → `alpha`, `upper` → `UPPER`).
* `h_errno` is `HOST_NOT_FOUND` (1) for every miss; the `_r` forms report `ERANGE` with
  `h_errno` -1 (`NETDB_INTERNAL`) while the buffer is too small.
* `getnetbyname` stores the network left-aligned (`127` → `7f000000`, `192.0.2` → `c0000200`);
  `getnetbyaddr` with the right-aligned number does not find it. A non-numeric network is
  accepted as `ffffffff`.
* `services`: a name matches only with its exact case; the first entry of the name wins when no
  protocol is given; a port above 65535 is truncated (70000 → 4464); an entry without a protocol
  is accepted with an empty protocol.
* `protocols` names match case-insensitively; `rpc` aliases match; `ethers` entries are found
  both ways.
* Netgroups: triples with an empty or `-` field match any or none as written; nested groups are
  expanded; a loop ends without a match; the enumeration lists the triples of the group (not of
  its nested groups).
* Aliases: continuation lines are joined, `:include:` entries are skipped, members are split at
  commas and trimmed.
* `getaddrinfo`: without a socket type, one result per type (stream, datagram, raw); `AI_PASSIVE`
  with no node gives the wildcard addresses (IPv4 first), without it the loopback addresses
  (IPv6 first); `AI_NUMERICHOST` with a name is `EAI_NONAME`; an IPv6 text with `AF_INET` is
  `EAI_ADDRFAMILY`; an unknown service `EAI_SERVICE`, a service with `SOCK_RAW` `EAI_SERVICE`, an
  unknown family `EAI_FAMILY`, unknown flag bits `EAI_BADFLAGS`; a port of 65536 is accepted as
  0 and -1 rejected; a zone suffix is accepted with `AI_NUMERICHOST`; `AI_ADDRCONFIG` with only
  loopback still returns IPv4.
* `getnameinfo`: a reverse miss gives the numeric host unless `NI_NAMEREQD` (`EAI_NONAME`); an
  unknown port gives the number; unknown flag bits are `EAI_BADFLAGS`; a host buffer too small is
  `EAI_OVERFLOW`; a bad address length `EAI_FAMILY`; asking for neither host nor service
  succeeds.
* `gai_strerror` texts for every code, including the asynchronous ones (-100 to -105).

## Observations that look like historical artifacts

The spec does not decide these; an implementation may choose a cleaner, consistent behaviour and
record it:

* `gethostbyname("")` matches a hosts line that has an address and no name.
* `gethostent` returns the `::1` line as `127.0.0.1` and skips the other IPv6 lines.
* `getnetbyaddr` cannot find what `getnetbyname` returns (left- versus right-aligned numbers).
* A port above 65535 is truncated in `services` and accepted as 0 by `getaddrinfo`; a non-numeric
  network number becomes `ffffffff`.

## What the record does not settle

* Sources other than `files`, the action rules of `nsswitch.conf`, DNS and the resolver are the
  subject of the DNS probe; `host.conf` (`multi on`) is not exercised.
