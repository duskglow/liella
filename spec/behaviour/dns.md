# The DNS stub resolver, lookups through `dns`, and nsswitch.conf actions

**Spec:** §16.1 (NSS actions), §16.2 (`getaddrinfo`/`getnameinfo` through `dns`), §16.3
(`gethostbyname`/`gethostbyaddr` through `dns`, `h_errno`, `hstrerror`), §16.4 (the resolver,
`res_*`, `dn_*`, `ns_*`, `b64_*`, `p_*`). Public interfaces: `<resolv.h>`, `<arpa/nameser.h>`,
`<netdb.h>`. References (plan §1.2): RFC 1034, RFC 1035 (messages, compression), RFC 3596 (AAAA),
RFC 6891 (EDNS), RFC 3490/5891 and UTS #46 (IDNA), the `resolv.conf(5)`, `nsswitch.conf(5)` and
`resolver(3)` documentation.

**Clauses:** `s16_1_c_0`, `s16_1_c_1`, `s16_1_c_2`, `s16_1_g_1`, `s16_2_c_1`, `s16_2_c_2`,
`s16_2_c_2_1`, `s16_2_c_2_2`, `s16_2_c_2_3`, `s16_2_c_2_4`, `s16_2_c_2_5`, `s16_2_c_2_6`,
`s16_2_c_3`, `s16_2_c_4`, `s16_2_f_0`, `s16_2_g_0`, `s16_3_c_0`, `s16_3_f_0`, `s16_4_c_0`,
`s16_4_c_1`, `s16_4_c_1_1`, `s16_4_c_1_2`, `s16_4_c_1_3`, `s16_4_c_1_4`, `s16_4_c_2`, `s16_4_c_3`,
`s16_4_c_4`, `s16_4_g_1`, `s16_4_g_2`, `s16_4_g_3`, `s16_4_g_4`, `s16_4_f_0`.

**Probe:** `crates/core/tests/oracle/dns_diff.c`, run by `dns_run.sh PROGRAM SCRATCH`. **Record:**
`dns.glibc.txt`. The reference build links `-lresolv`. The first phase writes one fixture `etc/`
per scenario and runs the offline message functions. Each scenario then runs as root with its
fixture bind-mounted over `/etc`, in private mount and network namespaces where only `lo` is up;
the program starts its own DNS server (a forked child) on 127.0.0.1:53 there, answering from a
fixed script, so nothing depends on real DNS.

## The server's script

A/AAAA/MX/TXT/PTR answers for fixed names (all TTL 300, authoritative); `cname.test` → `a.test`;
`nx.test` NXDOMAIN; `servfail.test` SERVFAIL; `refused.test` REFUSED; `silent.test` no answer;
`wrongq.test` an answer for another question; `slow.test` drops every other UDP query; `trunc.test`
sets TC over UDP and answers over TCP; `badid.test` sends an answer with a wrong ID before the right
one; `ad.test` sets the AD bit; search-list names under `example.test`; `xn--bcher-kva.test`.

## Scenarios

| Scenario | `resolv.conf` | `nsswitch.conf` hosts |
|---|---|---|
| basic | 127.0.0.1, timeout 1, attempts 2 | `files dns` |
| search | search `example.test other.test`, ndots 2 | `dns` |
| trustad | `trust-ad` | `dns` |
| noaaaa | `no-aaaa` | `dns` |
| usevc | `use-vc` | `dns` |
| notfound_return | | `dns [NOTFOUND=return] files` |
| notfound_continue | | `dns files` |
| unavail | 127.0.0.2 (nothing listens) | `dns [!UNAVAIL=return] files` |

The fixture `hosts` has `localhost` and `192.0.2.200 local.test nx.test`.

## Observations the record shows directly

* Offline: `res_mkquery` sets RD and one question (the ID is random and not shown); an empty label
  or a small buffer is -1. `dn_comp` compresses against earlier names; `dn_expand` follows
  backward and forward pointers, rejects a pointer loop, and escapes `.`, `\`, and bytes outside
  printable ASCII (`\001`, `\032`). The name checks: `res_hnok` rejects a leading hyphen, spaces,
  `*`, empty labels, and accepts `_`, digits first, trailing hyphens; `res_ownok` also accepts a
  leading `*.`; `res_mailok`/`res_dnok` are more lenient.
* `ns_samename` compares case-insensitively and ignores a trailing dot, but an escaped dot is a
  different name; `ns_makecanon` keeps case and ends with one dot; `ns_name_pton` returns 1 for a
  fully qualified name. `ns_format_ttl` uses uppercase for a single unit and lowercase for
  several; `ns_parse_ttl` accepts units in any case and repeats, and rejects a trailing number
  without a unit. `ns_datetosecs` reads `YYYYMMDDhhmmss`. Base64 round-trips; an invalid
  character or a small buffer is -1. `hstrerror`, `p_type`, `p_class`, `p_rcode` texts as shown
  (unknown values printed as numbers).
* `res_init` reads the nameserver, search list and options into `_res`.
* Through `dns`: A and AAAA answers; `getaddrinfo` with `AF_UNSPEC` lists AAAA then A results, and
  `AI_CANONNAME` gives the CNAME target; `gethostbyname` gives a CNAME's owner as an alias; a
  name with only AAAA records is `NO_DATA` (4) for `gethostbyname`; names are answered in the
  case asked.
* NXDOMAIN is `HOST_NOT_FOUND`/`EAI_NONAME`; SERVFAIL is `TRY_AGAIN`/`EAI_AGAIN`; a truncated UDP
  answer is retried over TCP; a dropped query is retried within the attempts; an answer with a
  wrong ID is ignored and the right one used.
* `trust-ad` passes the AD bit through, otherwise it is cleared; `no-aaaa` suppresses AAAA queries
  (`NO_DATA`) and the AAAA results of `getaddrinfo`.
* Reverse lookups: `gethostbyaddr` and `getnameinfo` through PTR records for IPv4 and IPv6; a
  missing PTR with `NI_NAMEREQD` is `EAI_NONAME`.
* Search list: with `ndots 2`, `host` and `x.y` are tried with the search domains first
  (`host.example.test`, `x.y.example.test`), `x.y.` only as given; `res_querydomain` joins the
  two names.
* Actions: `dns files` falls through from NXDOMAIN to `files`; `[NOTFOUND=return]` stops after
  NXDOMAIN (and after `NO_DATA`); with no server reachable (UNAVAIL), `[!UNAVAIL=return]`
  continues to `files`.
* `AI_IDN` with a non-ASCII name in the "C" locale is `EAI_IDN_ENCODE`; without `AI_IDN` the raw
  bytes are looked up and not found.

## Observations that look like historical artifacts

* The same server answer gives different results over UDP and TCP: REFUSED is
  `TRY_AGAIN`/`EAI_AGAIN` over UDP but `NO_RECOVERY`/`EAI_NONAME` over TCP; an answer to the wrong
  question is `TRY_AGAIN` over UDP but `NO_DATA` over TCP.
* `res_search` leaves `h_errno` at `HOST_NOT_FOUND` (1) when it succeeds after trying another
  name first.

## What the record does not settle

* Timing (timeouts, retry intervals) is not shown; `rotate`, `edns0`, `single-request`,
  several nameservers, `sortlist`, `gai.conf` and RFC 6724 ordering beyond these cases,
  `RES_OPTIONS`/`LOCALDOMAIN`/`HOSTALIASES`, re-reading `resolv.conf`, and `getaddrinfo_a` are
  tested separately in phase 10.
