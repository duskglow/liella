# Address conversion, interfaces, IPv6 options, Ethernet addresses

**Spec:** §15.3 (address conversion), §15.4 (interfaces), §15.5 (IPv6 option and routing-header
helpers), §15.6 (source filters), §15.7 (Ethernet addresses), §15.8 (`ruserok`). Public
interfaces: `<arpa/inet.h>`, `<net/if.h>`, `<ifaddrs.h>`, `<netinet/in.h>`, `<netinet/ether.h>`,
`<netdb.h>`. Normative references (plan §1.2): RFC 4291 (IPv6 text forms), RFC 5952 (canonical
IPv6 text), RFC 3542 (IPv6 options and routing headers), RFC 3678 (source filters), RFC 3549
(netlink, for interface enumeration).

**Clauses:** `s15_3_c_1`, `s15_3_c_2`, `s15_3_c_3`, `s15_3_c_4`, `s15_3_c2_0`, `s15_4_c_1`,
`s15_4_c_2`, `s15_4_c_3`, `s15_5_c_0`, `s15_6_c_0`, `s15_7_c_0`, `s15_8_c_3`, `s15_3_c_5`,
`s15_3_c_6`, `s15_3_c_7`.

**Probe:** `crates/core/tests/oracle/netaddr_diff.c`. **Record:** `netaddr.glibc.txt`. The
reference build links `-lresolv`: `inet_net_pton`, `inet_net_ntop` and `inet_neta` are exported
by `libresolv.so.2` (Appendix C), not by libc.

## What the probe does

* `inet_pton` over valid and malformed IPv4 and IPv6 texts (leading zeros, hex, short and long
  forms, whitespace, zone suffixes, embedded IPv4, misplaced `::`); the output buffer is
  prefilled with `ee` bytes so untouched output shows.
* `inet_ntop` of IPv6 addresses covering every RFC 5952 compression choice and the embedded IPv4
  forms, and of IPv4 and IPv6 addresses with buffer sizes around the text's length.
* `inet_aton`, `inet_addr` and `inet_network` over the historical forms (one to four parts, octal,
  hex, overflow, trailing text).
* The classful helpers, the shared `inet_ntoa` buffer.
* `inet_net_pton`/`inet_net_ntop` (prefix lengths, partial addresses, `default`), `inet_nsap_*`,
  `inet_neta`.
* `ether_aton_r`/`ether_ntoa_r`, the shared `ether_aton` buffer, `ether_line`.
* The RFC 3542 option builder and parser and the type-0 routing header (shown as hex).
* `if_nametoindex`, `if_indextoname`, `if_nameindex`, and `getifaddrs` restricted to the loopback
  interface.
* `in6addr_any`/`in6addr_loopback`, byte-order helpers, the error cases of
  `getipv4sourcefilter`, `ruserok` without configuration files.

Each call's result is taken before `errno` is read (argument evaluation order is unspecified).

## Observations the record shows directly

* `inet_pton` is strict: IPv4 needs exactly four decimal parts without leading zeros; IPv6
  rejects a zone suffix, five-digit groups and stray colons; an unknown family is -1 with
  `EAFNOSUPPORT`.
* `inet_ntop` follows RFC 5952: the longest run of zero groups is compressed (the first when two
  runs are equal), a single zero group is not, lowercase hex; embedded IPv4 text is used only
  for `::ffff:0:0/96` and the IPv4-compatible `::/96` form (but `::0.0.0.1` prints `::1`);
  `64:ff9b::/96` is printed in hex. A buffer too small is `ENOSPC`; an unknown family
  `EAFNOSUPPORT`.
* `inet_aton` accepts one to four parts in octal, decimal or hex, and stops at the first white
  space, so `"1.2.3.4 trailing"` is accepted; anything else after the number is rejected. A NULL
  output is accepted (validation only). `inet_addr` returns `INADDR_NONE` for invalid text.
  `inet_network` gives `ffffffff` for many forms `inet_aton` accepts (it reads at most four
  parts of one byte each, and packs short forms into the low bytes).
* `inet_makeaddr` builds class A, B or C addresses by the network number's size; a network
  number with high bits set is combined as observed (`ffffff01`, 2 → `ffffff03`).
* `inet_net_pton` returns the prefix length (32 when absent), writes only the bytes the prefix
  covers, rejects a prefix over 32 with `EMSGSIZE` and unparsable text with `ENOENT`; it does not
  accept `AF_INET6` (`EAFNOSUPPORT`). `inet_net_ntop` prints only the bytes the prefix covers
  and rejects a prefix over 32 with `EINVAL`, a small buffer with `EMSGSIZE`.
* `inet_nsap_addr` accepts hex digits with optional `.` separators and no `0x` prefix (a `0x`
  prefix gives 0); `inet_nsap_ntoa` prints uppercase groups.
* `ether_aton` accepts one or two hex digits per byte with `:` separators, stops after six bytes
  (trailing text ignored), and writes bytes as it goes (a failure leaves the parsed prefix);
  `ether_ntoa` prints lowercase without leading zeros. `ether_line` needs a host name after the
  address and stops the name at white space or `#`.
* RFC 3542: an option with an alignment requirement it cannot honour, or of type 0 or 1 (the pad
  types), is rejected; `inet6_opt_finish` pads to a multiple of 8 with PadN; `inet6_rth_add`
  beyond the declared segment count fails; `inet6_rth_reverse` reverses the address list; the
  type-0 header's length field counts 8-byte units.
* `if_nametoindex` of an unknown or overlong name is 0 with `ENODEV`; `if_indextoname` of an
  unknown index is NULL with `ENXIO`. `getifaddrs` lists, for `lo`, an `AF_PACKET` entry with
  statistics (`ifa_data`), then the IPv4 and IPv6 addresses with their netmasks.
* `getipv4sourcefilter` on a group the socket has not joined is `EADDRNOTAVAIL`; on a bad
  descriptor `EBADF`.

## What the record does not settle

* `rcmd`, `rexec`, `rresvport` and `bindresvport` need privileges or remote services;
  `ruserok` with `/etc/hosts.equiv` or `~/.rhosts` needs fixtures (phase 10 tests in a private
  mount namespace).
* Interfaces other than `lo` are machine-specific.
