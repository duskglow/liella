#!/bin/sh
# The DNS search list beyond MAXDNSRCH (spec §16.4; milestone 3, finding p2-4): a resolv.conf with
# eight search domains, a stub DNS server on 127.0.0.1:53 that logs each query and answers
# NXDOMAIN, and a program that prints how many names _res.dnsrch holds and runs res_search and
# getaddrinfo for an unqualified name. All of it in a private user, network and mount namespace
# (unshare -r -n -m), so the system's resolver configuration is never touched.
# Usage: sh specman_dns.sh [static-link-objects...]  (no argument: the host C library; with
# Liella's liella-crt1.o and libc.a: the products).
set -eu
dir=$(mktemp -d "${TMPDIR:-/tmp}/specman-dns.XXXXXX")
trap 'rm -rf "$dir"' EXIT
cd "$dir"
cat > resolv.conf <<'EOF'
nameserver 127.0.0.1
search d1.test d2.test d3.test d4.test d5.test d6.test d7.test d8.test
options attempts:1 timeout:1
EOF
cat > srv.py <<'EOF'
import socket, sys
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.bind(("127.0.0.1", 53))
s.settimeout(8)
out = open(sys.argv[1], "w")
try:
    while True:
        d, a = s.recvfrom(512)
        i, labels = 12, []
        while d[i]:
            labels.append(d[i + 1:i + 1 + d[i]].decode())
            i += 1 + d[i]
        out.write(".".join(labels) + "\n")
        out.flush()
        r = bytearray(d[:12])
        r[2], r[3] = 0x81, 0x83
        s.sendto(bytes(r) + d[12:], a)
except socket.timeout:
    pass
EOF
cat > q.c <<'EOF'
#include <netdb.h>
#include <resolv.h>
#include <stdio.h>
int main(void)
{
    res_init();
    int n = 0;
    while (n < 7 && _res.dnsrch[n])
        n++;
    printf("_res.dnsrch holds %d names (MAXDNSRCH %d)\n", n, MAXDNSRCH);
    unsigned char ans[512];
    printf("res_search = %d\n", res_search("host", C_IN, T_A, ans, sizeof ans));
    struct addrinfo *ai;
    printf("getaddrinfo = %d\n", getaddrinfo("host2", NULL, NULL, &ai));
    return 0;
}
EOF
if [ $# -eq 0 ]; then
    cc -w -o q q.c -lresolv
else
    cc -std=gnu17 -O1 -w -static -nostdlib -fno-builtin "$1" q.c "$2" -lgcc -o q
fi
unshare -r -n -m sh -c "ip link set lo up && mount --bind $dir/resolv.conf /etc/resolv.conf && (python3 srv.py queries.txt &) && sleep 0.5 && ./q && sleep 9"
echo "queries:"
cat queries.txt
