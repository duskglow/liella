# Phase 10's name-service spec gaps, settled by probe

**Spec:** §16.3 (`networks`, `initgroups`), §16.4 (`fp_nquery`, `p_*`). **Clauses:** `s16_3_c_0`,
`s16_3_f_0`, `s16_4_f_0`.

**Probes:** `crates/core/tests/oracle/ns_gaps.c` (offline, `-lresolv`) and `ns_gaps2.c` (its own
user, mount and network namespaces, unprivileged: fixture files bind-mounted over `/etc`, a UDP
server on 127.0.0.1:53). **Record:** `ns-gaps.glibc.txt`. **Tests:** `crates/core/tests/ns_gaps_probe.rs`.

## `fp_nquery` with a non-zero `pfcode` (settled)

The counts line needs HEAD1 (0x100); flags HEAD2 (0x200); the `;; ->>HEADER<<-` line HEADX (0x800,
followed by a lone `;` line). A section heading (`;; QUERY SECTION:`, with a blank line after the
records) is printed only with HEAD1 **and only when the section has records**: an empty authority or
additional section has no heading and no blank line. Liella printed the empty headings (extrapolated);
fixed in `text::nquery` (one line), pinned by `ns_gaps_nquery_headings`.

## MINFO and RP with single-label names (settled)

Names print with a trailing dot (`r. e.`, `mbox. txt.`); a root name prints as `.`. Liella matches
(`ns_gaps_minfo_rp_single_label`).

## `networks` through `dns` (settled, NS-NETWORKS-DNS)

Probe `crates/core/tests/oracle/net_dns.c`. `getnetbyaddr(n)` sends one PTR question: `n` without its
trailing zero octets, padded with zero octets to four and reversed under `in-addr.arpa` (`10` and
`0x0a000000` ask `0.0.0.10`, `0xac10` and `0xac100000` ask `0.0.16.172`; no class rule); `n_net` is
the number without those octets, the first PTR target the name, the others aliases. `getnetbyname(x)`
sends a PTR question for `x` through the search rules; the first target under `in-addr.arpa` gives
`n_net` (labels reversed, `2.0.192.in-addr.arpa` is 0xc00002) and is the alias. NXDOMAIN gives
HOST_NOT_FOUND, NOERROR without records NO_DATA, SERVFAIL, REFUSED, a truncated answer that cannot be
retried, or an answer without a usable PTR record TRY_AGAIN. An answer count beyond the packet gives
the records that fit. Liella follows this (`ns_networks_dns_case_table`, `ns_networks_dns_damaged_answers`);
its official name for `getnetbyname` is the name asked.

## Group-list merge (settled)

`getgrouplist` on a file with groups `a:50:u b:50:u c:60:u c:61:u e:70:u,u f:99:u`: base first, never
repeated; the file's other GIDs in file order; **a GID shared by two groups appears once per group**
(base 99: `99 50 50 60 61 70`); the same name with two GIDs lists both; a repeated member counts once.
Liella matches (`ns_gaps_initgroups_merge_disagreeing_groups`, `s17_3_c_4`): only the base GID is
never repeated.
