# NS-NETWORKS-DNS / -2 handoff (COMPLETE)

State: branch task/ns-networks-dns-2, stacks on task/ns-spec-gaps-settle-2 (sync with --base origin/task/ns-spec-gaps-settle-2).
Done (D-16.1, D-16.2, COORDINATOR #1988, #1997): networks `dns` source (resolv_compat::net_*, netdb_compat::net_dns,
boundary netdb::with_networks); probe oracle/net_dns.c; tests ns_networks_dns_case_table, ns_networks_dns_damaged_answers.
Run 2: run 1's gate was green (kanon, rc 0). Damaged answer keeps HOST_NOT_FOUND (reference: NULL, h_errno 0): differences
row, COMPAT-13 registered (no implementation), both answers pinned (spec/behaviour/ns-gaps.md; probe and test).
Gate (sumire s1): ns-networks-dns-2-gate rc 0, 865 passed / 0 failed. clippy -D warnings and xtask static pass.
Plants (RAM copy, sumire zram): class-width, servfail-notfound, count-trusted all CAUGHT with the expected message
(job ns-networks-dns-2-plants rc 0; script ~/work/ns-networks-dns-2/plants.sh on sumire).
Not to redo: the probe table, plants. Next: land. Plan rows: D-16.1, D-16.2.
