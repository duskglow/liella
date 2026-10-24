# `__nss_hostname_digits_dots` (spec §16.1)

Black-box observations of the reference library (glibc 2.39, Ubuntu 24.04, x86_64), made on
2026-10-04 (milestone 5) because the spec names the function in §16.1's title and gives it no
contract or signature, and no installed header declares it (a spec gap since phase 10).

Probe: `crates/core/tests/oracle/digits_dots_probe.c` (its caller buffers 8-aligned). Record:
[`digits-dots.glibc.txt`](digits-dots.glibc.txt). The probe fills every argument with a
recognisable value (`0xAA` bytes, 12345, 777, the pointer value 1) and prints what the call
changed. Nothing here describes or derives from the reference library's source code.

Clauses: none states this function's contract; the nearest is `s16_1_c_0` (§16.1's lookups,
whose title names it). A spec addition would add its own clause IDs.

## The signature the probe confirms

```c
int __nss_hostname_digits_dots(const char *name, struct hostent *resbuf, char **buffer,
                               size_t *buffer_size, size_t buflen, struct hostent **result,
                               enum nss_status *status, int af, int *h_errnop);
```

The hypothesis came from public descriptions of the function (the 2015 GHOST advisory names its
parameters). Each parameter's role is visible in the record: `resbuf` is filled, `*buffer` is
allocated (when `buffer_size` is given) or used (when it is NULL), `*result` and `*status` are
written in one form each, `af` selects the family, and `*h_errnop` receives the `h_errno` value.

## Observations

* **Return value:** 1 when `name` is a numeric address in the dotted forms `inet_aton` accepts
  (`1.2.3.4`, `1.2.3` = 1.2.0.3, `123` = 0.0.0.123) or an IPv6 literal, whether the address
  could be returned or not; 0 otherwise (`example.com`, an empty name, a trailing dot
  `1.2.3.4.`, a hexadecimal part `0x7f.1`, a letter `1.2.3.a`, a scope `fe80::1%1`). With 0 nothing
  is written.
* **Two forms.**
  * *Allocating* (`buffer_size` not NULL, the `gethostbyname` style): `*buffer` is set to a
    `malloc` block (`*buffer_size` > 0) holding the strings; `*result` is `resbuf` on success
    or NULL; `*status` is not written.
  * *Caller buffer* (`buffer_size` NULL, `buflen` bytes at `*buffer`, the `gethostbyname_r`
    style): `*result` is not written; `*status` is 1 (`NSS_STATUS_SUCCESS`), 0
    (`NSS_STATUS_NOTFOUND`) or -2 (`NSS_STATUS_TRYAGAIN`). The strings are in the caller's
    buffer.
* **The entry:** `h_name` is `name` as given; no aliases; one address; `h_addrtype`/`h_length`
  are `AF_INET`/4 or `AF_INET6`/16. `*h_errnop` is 0 on success.
* **The family:** `AF_INET6` accepts IPv6 literals (an IPv4-mapped one too); an IPv4 text with
  `AF_INET6`, or an IPv6 text with `AF_INET`, is handled (1) but not found: `h_errno` 1
  (`HOST_NOT_FOUND`), `*result` NULL or status 0. Any `af` other than `AF_INET6` (0, 99) acts
  as `AF_INET`. Out-of-range parts (`1.2.3.256`, five parts) are handled and not found.
* **A short caller buffer** (`10.20.30.40`: 48 bytes fail, 56 succeed): 1 with status -2,
  `h_errno` -1 (`NETDB_INTERNAL`) and `errno` `ERANGE`.
* **NULL `status` and `h_errnop`** in the caller-buffer form: the process is killed by
  `SIGSEGV` (seen in an earlier run; the recorded probe leaves the case out).
* `errno` is not written except for `ERANGE`.

## Status

**Ruled 2026-10-05 (owner): accepted.** §16.1 now states this contract (clauses `s16_1_c_3`
to `s16_1_c_6`); Liella exports the function (D-16.11), and `digits_dots_products.rs` builds the
probe against both libraries and compares the outputs line by line (identical). Not in the
record: the reference also enlarges `*buffer` in the allocating form for a non-numeric name
(answered with 0); Liella does the same.

## Under the three-way rule (as reported before the ruling)

The spec is silent (only the name), the reference behaves as above, and there is no official
documentation (no manual page, not in the glibc manual). This is therefore **RULING NEEDED**.

**Recommendation:** add the observed contract to §16.1 as a compatibility entry (it is a
`GLIBC_2.2.5` export that old NSS modules and statically linked resolvers call), with the
signature above, and implement it as a thin layer over Liella's numeric-address parsing
(`inet_aton` and `inet_pton`, §15.3) and the `struct hostent` packing that `gethostbyname_r`
already uses, reproducing the two forms, the family rule, `ERANGE` and the return value. One
deliberate difference is proposed: a NULL `status` or `h_errnop` is not dereferenced (S6: a
detectable invalid argument is not a crash). (Done, see Status.)
