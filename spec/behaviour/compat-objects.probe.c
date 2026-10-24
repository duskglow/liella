/* LEGACY-OBJECTS (LG-6) black-box probe: glibc's compat data objects, bound by version. */
#include <stdio.h>
#include <string.h>

#define BIND(local, sym, ver) __asm__(".symver " #local "," #sym "@" #ver)

extern const char *const el_225[]; BIND(el_225, sys_errlist, GLIBC_2.2.5);
extern const char *const el_23[]; BIND(el_23, sys_errlist, GLIBC_2.3);
extern const char *const el_24[]; BIND(el_24, sys_errlist, GLIBC_2.4);
extern const char *const el_212[]; BIND(el_212, sys_errlist, GLIBC_2.12);
extern const char *const uel_212[]; BIND(uel_212, _sys_errlist, GLIBC_2.12);
extern const char *const uel_225[]; BIND(uel_225, _sys_errlist, GLIBC_2.2.5);
extern const int ne_225; BIND(ne_225, sys_nerr, GLIBC_2.2.5);
extern const int ne_23; BIND(ne_23, sys_nerr, GLIBC_2.3);
extern const int ne_24; BIND(ne_24, sys_nerr, GLIBC_2.4);
extern const int ne_212; BIND(ne_212, sys_nerr, GLIBC_2.12);
extern const int une_225; BIND(une_225, _sys_nerr, GLIBC_2.2.5);
extern const int une_212; BIND(une_212, _sys_nerr, GLIBC_2.12);
extern const char *const sl_225[]; BIND(sl_225, sys_siglist, GLIBC_2.2.5);
extern const char *const sl_233[]; BIND(sl_233, sys_siglist, GLIBC_2.3.3);
extern const char *const usl_233[]; BIND(usl_233, _sys_siglist, GLIBC_2.3.3);
extern const char *const sa_225[]; BIND(sa_225, sys_sigabbrev, GLIBC_2.2.5);
extern const char *const sa_233[]; BIND(sa_233, sys_sigabbrev, GLIBC_2.3.3);
extern void *mh; BIND(mh, __malloc_hook, GLIBC_2.2.5);
extern void *fh; BIND(fh, __free_hook, GLIBC_2.2.5);
extern void *rh; BIND(rh, __realloc_hook, GLIBC_2.2.5);
extern void *ah; BIND(ah, __memalign_hook, GLIBC_2.2.5);
extern void *amh; BIND(amh, __after_morecore_hook, GLIBC_2.2.5);
extern void *mih; BIND(mih, __malloc_initialize_hook, GLIBC_2.2.5);
extern void *mc; BIND(mc, __morecore, GLIBC_2.2.5);
extern void *mw; BIND(mw, mallwatch, GLIBC_2.2.5);
extern unsigned char na[24]; BIND(na, _null_auth, GLIBC_2.2.5);

static void table(const char *name, const char *const *t, int n) {
    for (int i = 0; i < n; i++)
        printf("%s[%d]=%s\n", name, i, t[i] ? t[i] : "(null)");
}

static void same(const char *name, const char *const *a, const char *const *b, int n) {
    int diff = 0;
    for (int i = 0; i < n; i++)
        diff += (a[i] == NULL) != (b[i] == NULL) || (a[i] && strcmp(a[i], b[i]) != 0);
    printf("%s: %d of %d entries differ\n", name, diff, n);
}

int main(void) {
    printf("sys_nerr 2.2.5=%d 2.3=%d 2.4=%d 2.12=%d; _sys_nerr 2.2.5=%d 2.12=%d\n", ne_225, ne_23, ne_24, ne_212, une_225, une_212);
    table("sys_errlist@GLIBC_2.12", el_212, 135);
    same("sys_errlist@GLIBC_2.2.5 vs 2.12 prefix", el_225, el_212, 125);
    same("sys_errlist@GLIBC_2.3 vs 2.12 prefix", el_23, el_212, 126);
    same("sys_errlist@GLIBC_2.4 vs 2.12 prefix", el_24, el_212, 132);
    same("_sys_errlist@GLIBC_2.12 vs sys_errlist@GLIBC_2.12", uel_212, el_212, 135);
    same("_sys_errlist@GLIBC_2.2.5 vs sys_errlist@GLIBC_2.12 prefix", uel_225, el_212, 125);
    table("sys_siglist@GLIBC_2.3.3", sl_233, 65);
    same("sys_siglist@GLIBC_2.2.5 vs 2.3.3 prefix", sl_225, sl_233, 64);
    same("_sys_siglist@GLIBC_2.3.3 vs sys_siglist@GLIBC_2.3.3", usl_233, sl_233, 65);
    table("sys_sigabbrev@GLIBC_2.3.3", sa_233, 65);
    same("sys_sigabbrev@GLIBC_2.2.5 vs 2.3.3 prefix", sa_225, sa_233, 64);
    printf("hooks null: malloc=%d free=%d realloc=%d memalign=%d after_morecore=%d malloc_initialize=%d morecore=%d mallwatch=%d\n",
           mh == NULL, fh == NULL, rh == NULL, ah == NULL, amh == NULL, mih == NULL, mc == NULL, mw == NULL);
    int nz = 0;
    for (int i = 0; i < 24; i++) nz += na[i] != 0;
    printf("_null_auth nonzero bytes=%d\n", nz);
    return 0;
}
