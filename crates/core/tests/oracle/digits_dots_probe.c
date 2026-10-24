/* __nss_hostname_digits_dots, probed black-box (spec §16.1 names it; no installed header declares
 * it; milestone 5, 2026-10-04). The signature below is the hypothesis the probe tests: every
 * argument is filled with a recognisable value, and what the call changes is printed. Public
 * interface only. Build: cc -std=gnu17 -O1 digits_dots_probe.c; digits_dots_products.rs also
 * builds it against Liella (§16.1, owner ruling 2026-10-05). The caller buffers are 8-aligned, so
 * the size threshold does not depend on where the stack puts them. */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <netdb.h>
#include <stdio.h>
#include <string.h>

extern int __nss_hostname_digits_dots(const char *name, struct hostent *resbuf, char **buffer,
                                      size_t *buffer_size, size_t buflen, struct hostent **result,
                                      int *status, int af, int *h_errnop);

static void show(const char *what, int rc, struct hostent *h, struct hostent *res, int st, int he, int e)
{
    printf("%s: rc=%d result=%s status=%d h_errno=%d errno=%d", what, rc,
           res == h ? "resbuf" : res == NULL ? "NULL" : res == (void *)1 ? "untouched" : "other", st, he, e);
    if (h->h_name != (char *)0xAAAAAAAAAAAAAAAA) {
        printf(" name=[%s] type=%d len=%d", h->h_name ? h->h_name : "(null)", h->h_addrtype, h->h_length);
        int n = 0;
        for (char **a = h->h_aliases; a && *a; a++)
            n++;
        printf(" aliases=%d", n);
        for (char **a = h->h_addr_list; a && *a; a++) {
            char t[64];
            inet_ntop(h->h_addrtype, *a, t, sizeof t);
            printf(" addr=%s", t);
        }
    }
    printf("\n");
}

int main(void)
{
    static const struct { const char *name; int af; } cases[] = {
        {"1.2.3.4", AF_INET}, {"1.2.3.4", AF_INET6}, {"::1", AF_INET6}, {"::1", AF_INET},
        {"1.2.3", AF_INET}, {"1.2.3.4.", AF_INET}, {"1.2.3.4.5", AF_INET}, {"123", AF_INET},
        {"example.com", AF_INET}, {"1.2.3.256", AF_INET}, {"", AF_INET}, {"1", AF_INET},
        {"0x7f.1", AF_INET}, {"fe80::1%1", AF_INET6}, {"1.2.3.a", AF_INET}, {"::ffff:1.2.3.4", AF_INET6},
        {"1.2.3.4", 0}, {"1.2.3.4", 99},
    };
    for (size_t i = 0; i < sizeof cases / sizeof cases[0]; i++) {
        /* The allocating form: *buffer NULL and buffer_size given. */
        struct hostent h;
        memset(&h, 0xAA, sizeof h);
        char *bufp = NULL;
        size_t bs = 0;
        struct hostent *res = (void *)1;
        int st = 12345, he = 777;
        errno = 0;
        int rc = __nss_hostname_digits_dots(cases[i].name, &h, &bufp, &bs, 0, &res, &st, cases[i].af, &he);
        int e = errno;
        char w[96];
        snprintf(w, sizeof w, "alloc [%s] af=%d buffer=%s size>0=%d", cases[i].name, cases[i].af, bufp ? "set" : "NULL", bs > 0);
        show(w, rc, &h, res, st, he, e);
        /* The caller-buffer form: buffer_size NULL, buflen 512. */
        memset(&h, 0xAA, sizeof h);
        _Alignas(8) char store[512];
        char *sp = store;
        res = (void *)1;
        st = 12345, he = 777;
        errno = 0;
        rc = __nss_hostname_digits_dots(cases[i].name, &h, &sp, NULL, sizeof store, &res, &st, cases[i].af, &he);
        e = errno;
        snprintf(w, sizeof w, "buffer [%s] af=%d in-buffer=%d", cases[i].name, cases[i].af,
                 h.h_name != (char *)0xAAAAAAAAAAAAAAAA && h.h_name >= store && h.h_name < store + sizeof store);
        show(w, rc, &h, res, st, he, e);
    }
    for (size_t len = 0; len <= 64; len += 8) {
        struct hostent h;
        memset(&h, 0xAA, sizeof h);
        _Alignas(8) char store[64];
        char *sp = store;
        struct hostent *res = (void *)1;
        int st = 12345, he = 777;
        errno = 0;
        int rc = __nss_hostname_digits_dots("10.20.30.40", &h, &sp, NULL, len, &res, &st, AF_INET, &he);
        int e = errno;
        char w[64];
        snprintf(w, sizeof w, "buflen %zu", len);
        show(w, rc, &h, res, st, he, e);
    }
    return 0;
}
