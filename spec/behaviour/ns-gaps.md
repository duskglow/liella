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

## `networks` through `dns` (gap, follow-up story)

glibc: `getnetbyaddr(192.0.2.0)` sends a PTR query for `0.2.0.192.in-addr.arpa` (the number padded
to four octets), returns the answer's name with `n_addrtype` 2 and `n_net` 0xc00002; `10` asks
`0.0.0.10.in-addr.arpa`. `getnetbyname(x)` sends a PTR query for `x` itself; a PTR answer there gave
TRY_AGAIN, a NOERROR answer with no records NO_DATA. Liella has no `dns` source for `networks`: every
lookup misses and sends nothing. Gain of the difference: none (a missing source, not a design choice), so
glibc's behaviour should be followed; but it needs a PTR query builder for the padded in-addr.arpa name, an
answer reader and the TRY_AGAIN/NO_DATA mapping in the networks lookup (roughly 100-150 lines in
`names`/`nss`), far over a few lines: **RULING NEEDED / a new story** (NS-NETWORKS-DNS). The test pins
the present state.

## Group-list merge (settled)

`getgrouplist` on a file with groups `a:50:u b:50:u c:60:u c:61:u e:70:u,u f:99:u`: base first, never
repeated; the file's other GIDs in file order; **a GID shared by two groups appears once per group**
(base 99: `99 50 50 60 61 70`); the same name with two GIDs lists both; a repeated member counts once.
Liella matches (`ns_gaps_initgroups_merge_disagreeing_groups`, `s17_3_c_4`): only the base GID is
never repeated.
