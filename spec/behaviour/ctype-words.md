# The per-thread ctype table pointers (`__ctype_b_loc` and its siblings)

Spec §5.1 (COMPAT note), §7.2-7.4; decisions D-5.5, D-7.8. Task PJ-22, 2026-10-07.

**Clauses:** `s5_1_c_0`, `s7_1_c_p1`, `s7_2_t_0`.

Black-box probes of glibc 2.39 (Ubuntu 24.04, Zen 1) through the probe service
(`tools/probe/remote.sh`, hashes 6ce8863ef33f and 174e7c05e286), run with
`LOCPATH` naming a directory holding `de_DE.ISO-8859-1` compiled with `localedef` (0xe9 is a
lowercase letter there, 0xc9 its uppercase; neither is a letter in "C").

## Observations

1. A pointer fetched once from `__ctype_b_loc`, `__ctype_toupper_loc` or `__ctype_tolower_loc`
   is the address of a per-thread word that stays the same; the word always names the table of
   the thread's current locale: it follows `setlocale` in the same thread (while the thread
   uses the global locale), `uselocale` of an object, and `uselocale(LC_GLOBAL_LOCALE)`. While
   the thread uses its own object, `setlocale` does not change it. The functions (`isalpha`,
   `toupper`, `tolower`) agree with the word in every case.
2. `newlocale` given the thread's active object as its base leaves the thread's words (and the
   functions' answers) unchanged until the next `uselocale`. (POSIX: the application must stop
   using the base first; the conformance test leaves this case out.)
3. After `freelocale` of the active object the word still names its tables.
4. Another thread's `setlocale` does NOT reach a thread using the global locale: neither its
   fetched pointer, nor a fresh `__ctype_b_loc()`, nor `isalpha`/`toupper` in that thread see
   the new tables (Liella differs deliberately, D-7.8, docs/differences-from-glibc.md).
5. A new thread starts with the global locale's tables, also when its creator uses its own
   object.
6. A forked child keeps its parent thread's choice and words; `uselocale` and `setlocale` in the
   child act on it as in observation 1.
7. Entry -1 (`EOF`): class 0, `toupper` -1, `tolower` -1.

Liella before PJ-22 (main a0e6694) answered observation 1 differently: its words were refreshed
only by the next `__ctype_*_loc` call, so a fetched pointer kept the old tables (probe
6ce8863ef33f: DIFFERENT). PJ-22 fixes that (D-5.5); `conformance/c/s05_ctype_words.c` checks
observations 1, 3, 5, 6, 7 and D-7.8.

## Recorded output (glibc)

```
== PJ-22 ctype words one thread: fetched __ctype_b_loc/_toupper_loc/_tolower_loc across setlocale, uselocale, LC_GLOBAL_LOCALE, freelocale exit=0
stdout=[initial                slot_b=0 slot_u=0xe9 slot_l=0xc9 | same_addr=111 | table_is_initial=1 | fn isalpha=0 toupper=0xe9 tolower=0xc9
setlocale de: ok
after setlocale de     slot_b=1 slot_u=0xc9 slot_l=0xe9 | same_addr=111 | table_is_initial=0 | fn isalpha=1 toupper=0xc9 tolower=0xe9
uselocale(C obj)       slot_b=0 slot_u=0xe9 slot_l=0xc9 | same_addr=111 | table_is_initial=1 | fn isalpha=0 toupper=0xe9 tolower=0xc9
own C, global->C       slot_b=0 slot_u=0xe9 slot_l=0xc9 | same_addr=111 | table_is_initial=1 | fn isalpha=0 toupper=0xe9 tolower=0xc9
own C, global->de      slot_b=0 slot_u=0xe9 slot_l=0xc9 | same_addr=111 | table_is_initial=1 | fn isalpha=0 toupper=0xe9 tolower=0xc9
uselocale(GLOBAL)      slot_b=1 slot_u=0xc9 slot_l=0xe9 | same_addr=111 | table_is_initial=0 | fn isalpha=1 toupper=0xc9 tolower=0xe9
global->C again        slot_b=0 slot_u=0xe9 slot_l=0xc9 | same_addr=111 | table_is_initial=1 | fn isalpha=0 toupper=0xe9 tolower=0xc9
uselocale(de obj)      slot_b=1 slot_u=0xc9 slot_l=0xe9 | same_addr=111 | table_is_initial=0 | fn isalpha=1 toupper=0xc9 tolower=0xe9
newlocale(C, base=active) slot_b=1 slot_u=0xc9 slot_l=0xe9 | same_addr=111 | table_is_initial=0 | fn isalpha=1 toupper=0xc9 tolower=0xe9
uselocale(result)      slot_b=0 slot_u=0xe9 slot_l=0xc9 | same_addr=111 | table_is_initial=1 | fn isalpha=0 toupper=0xe9 tolower=0xc9
uselocale(de2)         slot_b=1 slot_u=0xc9 slot_l=0xe9 | same_addr=111 | table_is_initial=0 | fn isalpha=1 toupper=0xc9 tolower=0xe9
back to global C       slot_b=0 slot_u=0xe9 slot_l=0xc9 | same_addr=111 | table_is_initial=1 | fn isalpha=0 toupper=0xe9 tolower=0xc9
after free(active)     slot_b=1 slot_u=0xc9
EOF: b=0 u=-1 l=-1
]
stderr=[]
== PJ-22 ctype words threads: another thread's setlocale vs global-following and uselocale threads, new thread initial value, fork exit=0
stdout=[newlocale de: ok
global   after main setlocale de    slot_b=0 slot_u=0xe9 fresh_b=0 fn isalpha=0 toupper=0xe9
own-de   after main setlocale de    slot_b=1 slot_u=0xc9 fresh_b=1 fn isalpha=1 toupper=0xc9
own-de   after main setlocale C     slot_b=1 slot_u=0xc9 fresh_b=1 fn isalpha=1 toupper=0xc9
own-de   uselocale(GLOBAL)          slot_b=0 slot_u=0xe9 fresh_b=0 fn isalpha=0 toupper=0xe9
global   after main setlocale C     slot_b=0 slot_u=0xe9 fresh_b=0 fn isalpha=0 toupper=0xe9
own-de   global again, main->de     slot_b=0 slot_u=0xe9 fresh_b=0 fn isalpha=0 toupper=0xe9
new      created under global de    slot_b=1 slot_u=0xc9 fresh_b=1 fn isalpha=1 toupper=0xc9
new      creator uses own C         slot_b=1 slot_u=0xc9 fresh_b=1 fn isalpha=1 toupper=0xc9
child    after fork (own C)         slot_b=0 slot_u=0xe9 fresh_b=0 fn isalpha=0 toupper=0xe9
child    child uselocale(GLOBAL)    slot_b=1 slot_u=0xc9 fresh_b=1 fn isalpha=1 toupper=0xc9
child    after fork (global de)     slot_b=1 slot_u=0xc9 fresh_b=1 fn isalpha=1 toupper=0xc9
child    child setlocale C          slot_b=0 slot_u=0xe9 fresh_b=0 fn isalpha=0 toupper=0xe9
parent   after children             slot_b=1 slot_u=0xc9 fresh_b=1 fn isalpha=1 toupper=0xc9
]
stderr=[]
```

## Probe sources

```c
/* PJ-22 ctype words one thread: fetched __ctype_b_loc/_toupper_loc/_tolower_loc across setlocale, uselocale, LC_GLOBAL_LOCALE, freelocale */
#define _GNU_SOURCE
#include <ctype.h>
#include <locale.h>
#include <stdio.h>

#define DE "de_DE.ISO-8859-1"
static const unsigned short **pb;
static const int **pu, **pl;
static const unsigned short *t0;

static void show(const char *tag) {
    /* through the fetched slot addresses, fresh calls, and the functions */
    printf("%-22s slot_b=%d slot_u=%#x slot_l=%#x | same_addr=%d%d%d | table_is_initial=%d | fn isalpha=%d toupper=%#x tolower=%#x\n", tag,
           ((*pb)[0xe9] & _ISalpha) != 0, (unsigned)(*pu)[0xe9], (unsigned)(*pl)[0xc9],
           __ctype_b_loc() == pb, __ctype_toupper_loc() == pu, __ctype_tolower_loc() == pl, *pb == t0,
           (isalpha)(0xe9) != 0, (unsigned)(toupper)(0xe9), (unsigned)(tolower)(0xc9));
}

int main(void) {
    pb = __ctype_b_loc(); pu = __ctype_toupper_loc(); pl = __ctype_tolower_loc();
    t0 = *pb;
    show("initial");
    printf("setlocale de: %s\n", setlocale(LC_ALL, DE) ? "ok" : "FAILED");
    show("after setlocale de");
    locale_t c = newlocale(LC_ALL_MASK, "C", (locale_t)0);
    uselocale(c);
    show("uselocale(C obj)");
    setlocale(LC_ALL, "C");
    show("own C, global->C");
    setlocale(LC_ALL, DE);
    show("own C, global->de");
    uselocale(LC_GLOBAL_LOCALE);
    show("uselocale(GLOBAL)");
    setlocale(LC_ALL, "C");
    show("global->C again");
    locale_t d = newlocale(LC_ALL_MASK, DE, (locale_t)0);
    uselocale(d);
    show("uselocale(de obj)");
    locale_t d2 = newlocale(LC_CTYPE_MASK, "C", d); /* base consumed: is the active object changed? */
    show("newlocale(C, base=active)");
    uselocale(d2);
    show("uselocale(result)");
    locale_t e = newlocale(LC_ALL_MASK, DE, (locale_t)0);
    uselocale(e);
    freelocale(d2);
    show("uselocale(de2)");
    uselocale(LC_GLOBAL_LOCALE);
    freelocale(c);
    show("back to global C");
    /* freelocale of the active object: read only through the slot */
    uselocale(e);
    freelocale(e);
    printf("%-22s slot_b=%d slot_u=%#x\n", "after free(active)", ((*pb)[0xe9] & _ISalpha) != 0, (unsigned)(*pu)[0xe9]);
    printf("EOF: b=%d u=%d l=%d\n", (*pb)[-1], (*pu)[-1], (*pl)[-1]);
    return 0;
}
```

```c
/* PJ-22 ctype words threads: another thread's setlocale vs global-following and uselocale threads, new thread initial value, fork */
#define _GNU_SOURCE
#include <ctype.h>
#include <locale.h>
#include <pthread.h>
#include <stdio.h>
#include <sys/wait.h>
#include <unistd.h>

#define DE "de_DE.ISO-8859-1"
static pthread_barrier_t bar;
static locale_t de;

static void show(const char *who, const char *tag, const unsigned short **pb, const int **pu) {
    printf("%-8s %-26s slot_b=%d slot_u=%#x fresh_b=%d fn isalpha=%d toupper=%#x\n", who, tag,
           ((*pb)[0xe9] & _ISalpha) != 0, (unsigned)(*pu)[0xe9], ((*__ctype_b_loc())[0xe9] & _ISalpha) != 0,
           (isalpha)(0xe9) != 0, (unsigned)(toupper)(0xe9));
    fflush(stdout);
}

/* step: main and the threads take turns, separated by barrier waits */
static void *follower(void *a) {
    (void)a;
    const unsigned short **pb = __ctype_b_loc(); const int **pu = __ctype_toupper_loc();
    pthread_barrier_wait(&bar);              /* 1: main set de */
    pthread_barrier_wait(&bar);
    show("global", "after main setlocale de", pb, pu);
    pthread_barrier_wait(&bar);              /* 2: main set C */
    pthread_barrier_wait(&bar);
    show("global", "after main setlocale C", pb, pu);
    pthread_barrier_wait(&bar);              /* 3 */
    pthread_barrier_wait(&bar);
    return 0;
}

static void *own(void *a) {
    (void)a;
    const unsigned short **pb = __ctype_b_loc(); const int **pu = __ctype_toupper_loc();
    uselocale(de);
    pthread_barrier_wait(&bar);
    pthread_barrier_wait(&bar);
    show("own-de", "after main setlocale de", pb, pu);
    pthread_barrier_wait(&bar);
    pthread_barrier_wait(&bar);
    show("own-de", "after main setlocale C", pb, pu);
    uselocale(LC_GLOBAL_LOCALE);
    show("own-de", "uselocale(GLOBAL)", pb, pu);
    pthread_barrier_wait(&bar);              /* 3: main set de */
    pthread_barrier_wait(&bar);
    show("own-de", "global again, main->de", pb, pu);
    return 0;
}

static void *fresh(void *a) {
    show("new", (const char *)a, __ctype_b_loc(), __ctype_toupper_loc());
    return 0;
}

int main(void) {
    de = newlocale(LC_ALL_MASK, DE, (locale_t)0);
    printf("newlocale de: %s\n", de ? "ok" : "FAILED");
    pthread_barrier_init(&bar, 0, 3);
    pthread_t t1, t2, t3;
    pthread_create(&t1, 0, follower, 0);
    pthread_create(&t2, 0, own, 0);
    setlocale(LC_ALL, DE);
    pthread_barrier_wait(&bar);
    pthread_barrier_wait(&bar);
    pthread_barrier_wait(&bar);
    setlocale(LC_ALL, "C");
    pthread_barrier_wait(&bar);
    pthread_barrier_wait(&bar);
    setlocale(LC_ALL, DE);
    pthread_barrier_wait(&bar);
    pthread_join(t1, 0);
    pthread_join(t2, 0);
    pthread_create(&t3, 0, fresh, "created under global de");
    pthread_join(t3, 0);
    uselocale(newlocale(LC_ALL_MASK, "C", (locale_t)0));
    pthread_create(&t3, 0, fresh, "creator uses own C");
    pthread_join(t3, 0);
    /* fork: the child is a copy of this thread (own C over global de) */
    const unsigned short **pb = __ctype_b_loc(); const int **pu = __ctype_toupper_loc();
    pid_t p = fork();
    if (p == 0) {
        show("child", "after fork (own C)", pb, pu);
        uselocale(LC_GLOBAL_LOCALE);
        show("child", "child uselocale(GLOBAL)", pb, pu);
        _exit(0);
    }
    waitpid(p, 0, 0);
    uselocale(LC_GLOBAL_LOCALE);
    p = fork();
    if (p == 0) {
        show("child", "after fork (global de)", pb, pu);
        setlocale(LC_ALL, "C");
        show("child", "child setlocale C", pb, pu);
        _exit(0);
    }
    waitpid(p, 0, 0);
    show("parent", "after children", pb, pu);
    return 0;
}
```
