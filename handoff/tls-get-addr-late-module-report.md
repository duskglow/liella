# TLS-GET-ADDR-LATE-MODULE: report

Result: BLOCKED. No Liella implementation exists for clause s27_3_c_0 (phase 14), so no test or plant could run
against Liella. The test design from the brief is ready as a black-box probe, validated against glibc 2.39.

## Commits
* handoff-only commit (this report, `handoff/tls-get-addr-late-module.md`, `handoff/reports/TLS-GET-ADDR-LATE-MODULE.json`).

## Jobs
* None under jobrun. No gate: no code or test changed. No plants: the code under plant does not exist.
  The probe below ran by hand on the build host (Ubuntu glibc 2.39, `cc -O2`), outside the gate.

## Probe-settled behaviour (glibc 2.39, black box)
* 8 threads parked on a barrier before `dlopen(RTLD_NOW|RTLD_LOCAL)` of a module with 61 bytes of `.tdata`
  (bytes 1..61) and a 4099-byte `.tbss` tail (`PT_TLS` filesz 0x3d, memsz 0x1043, align 16; accessed through
  `R_X86_64_DTPMOD64`/`DTPOFF64`, i.e. `__tls_get_addr`), plus the main thread: on first access each thread
  sees the exact `.tdata` image and an all-zero `.tbss`, and gets its own address. Across 4 barrier-separated
  write/check rounds each thread sees only its own pattern, and the address stays the same.
* A thread created after the dlopen, while the early threads are still alive, sees the initial image at a
  new address.
* 200/200 passes, load average about 16 on a 60-vCPU host.
* Pitfall: if the early threads have already been joined, the late thread may get an address one of them used
  (freed and reused). That is correct behaviour, and an early draft that checked it failed 30/50 runs.

## Detection check (the probe's assertions, not Liella plants)
Helper-module variants that act out each brief plant, run under the same probe:

| variant | acts out | result |
|---|---|---|
| MUT_SHARED | (a) one block shared between threads | CAUGHT: "two threads share a block", "late thread shares a block" |
| MUT_NOCOPY | (b) `.tdata` not copied on lazy allocation | CAUGHT: ".tdata byte differs from the image" (all 10 threads) |
| MUT_TBSS_AA | (c) `.tbss` left uninitialised (0xAA) | CAUGHT: ".tbss byte not zero" (all 10 threads) |

These only show that the checks can detect the three defects. The real plants go in `liella_ld::tls` once it exists.

## RULING NEEDED
* Requeue this story after phase-14 §7 lands, or fold it into the phase-14 TLS story as its test. Recommended: fold it in,
  so the plants run against the new allocator in the same change.

## INFORMATIONAL
* `conformance/run.sh` links only statically against `libc.a`. A late-module test also needs the runner to build a
  helper `.so` and link the program dynamically against Liella's `libc.so.6` and `ld.so`. That is runner work for phase 14.
* `s27_3_f_0` (abort on allocation failure) needs OOM injection and was left out, as the brief says.
* The `s22_6_c_4` comment already notes that dynamically loaded TLS comes with phase 14.

## Refusals
* None.

## Appendix: probe sources (for the phase-14 conformance program)
Build: `cc -fPIC -shared -o tlsmod.so tlsmod.c; cc -o tlsprobe tlsprobe.c -ldl -pthread; ./tlsprobe ./tlsmod.so`.

### tlsprobe.c
```c
/* s27_3_c_0 black-box probe: threads started before dlopen get their own lazily allocated block
 * of the module's TLS, initialised from the image (.tdata copied, .tbss zero); a thread created
 * after the dlopen sees the initial image too. Barrier-ordered, fixed iterations, no sleeps. */
#include <dlfcn.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define NTHREADS 8
#define ROUNDS 4
#define DATA_LEN 61
#define TAIL_LEN 4099

typedef unsigned char *(*getter)(void);
static getter get_data, get_tail;
static pthread_barrier_t parked, released, written, checked, hold;
static uintptr_t addr_data[NTHREADS + 1], addr_tail[NTHREADS + 1];
static int fails;

static void fail(const char *what, int id) {
    printf("FAIL %s (thread %d)\n", what, id);
    __atomic_add_fetch(&fails, 1, __ATOMIC_RELAXED);
}

static int initial_image(int id) {
    unsigned char *d = get_data(), *t = get_tail();
    int ok = 1;
    for (int i = 0; i < DATA_LEN; i++)
        if (d[i] != (unsigned char)(i + 1)) { fail(".tdata byte differs from the image", id); ok = 0; break; }
    for (int i = 0; i < TAIL_LEN; i++)
        if (t[i] != 0) { fail(".tbss byte not zero", id); ok = 0; break; }
    return ok;
}

static unsigned char pat(int id, int r, int i) { return (unsigned char)(0x80 ^ (id * 37 + r * 11 + i)); }

static void write_pattern(int id, int r) {
    unsigned char *d = get_data(), *t = get_tail();
    for (int i = 0; i < DATA_LEN; i++) d[i] = pat(id, r, i);
    for (int i = 0; i < TAIL_LEN; i++) t[i] = pat(id, r, i + 1);
}

static void check_pattern(int id, int r) {
    unsigned char *d = get_data(), *t = get_tail();
    if ((uintptr_t)d != addr_data[id] || (uintptr_t)t != addr_tail[id]) fail("address changed", id);
    for (int i = 0; i < DATA_LEN; i++)
        if (d[i] != pat(id, r, i)) { fail("another thread's .tdata pattern seen", id); return; }
    for (int i = 0; i < TAIL_LEN; i++)
        if (t[i] != pat(id, r, i + 1)) { fail("another thread's .tbss pattern seen", id); return; }
}

static void *worker(void *arg) {
    int id = (int)(intptr_t)arg;
    pthread_barrier_wait(&parked);   /* started before the module exists */
    pthread_barrier_wait(&released); /* dlopen done */
    initial_image(id);               /* first access: lazy allocation */
    addr_data[id] = (uintptr_t)get_data();
    addr_tail[id] = (uintptr_t)get_tail();
    for (int r = 0; r < ROUNDS; r++) {
        write_pattern(id, r);
        pthread_barrier_wait(&written);
        check_pattern(id, r);
        pthread_barrier_wait(&checked);
    }
    pthread_barrier_wait(&hold); /* stay alive while the late thread runs: its block must be new */
    return NULL;
}

static void *late(void *arg) {
    (void)arg;
    initial_image(NTHREADS + 1);
    for (int i = 0; i <= NTHREADS; i++)
        if ((uintptr_t)get_data() == addr_data[i]) fail("late thread shares a block", NTHREADS + 1);
    return NULL;
}

int main(int argc, char **argv) {
    const char *mod = argc > 1 ? argv[1] : "./tlsmod.so";
    pthread_t th[NTHREADS];
    pthread_barrier_init(&parked, NULL, NTHREADS + 1);
    pthread_barrier_init(&released, NULL, NTHREADS + 1);
    pthread_barrier_init(&written, NULL, NTHREADS + 1);
    pthread_barrier_init(&checked, NULL, NTHREADS + 1);
    pthread_barrier_init(&hold, NULL, NTHREADS + 1);
    for (int i = 0; i < NTHREADS; i++)
        if (pthread_create(&th[i], NULL, worker, (void *)(intptr_t)i)) { puts("FAIL pthread_create"); return 2; }
    pthread_barrier_wait(&parked);
    void *h = dlopen(mod, RTLD_NOW | RTLD_LOCAL);
    if (!h) { printf("FAIL dlopen: %s\n", dlerror()); return 2; }
    get_data = (getter)dlsym(h, "mod_data");
    get_tail = (getter)dlsym(h, "mod_tail");
    if (!get_data || !get_tail) { puts("FAIL dlsym"); return 2; }
    pthread_barrier_wait(&released);
    /* main thread also predates the module */
    initial_image(NTHREADS);
    addr_data[NTHREADS] = (uintptr_t)get_data();
    addr_tail[NTHREADS] = (uintptr_t)get_tail();
    for (int r = 0; r < ROUNDS; r++) {
        write_pattern(NTHREADS, r);
        pthread_barrier_wait(&written);
        check_pattern(NTHREADS, r);
        pthread_barrier_wait(&checked);
    }
    for (int i = 0; i <= NTHREADS; i++)
        for (int j = i + 1; j <= NTHREADS; j++)
            if (addr_data[i] == addr_data[j] || addr_tail[i] == addr_tail[j]) fail("two threads share a block", i);
    pthread_t lt;
    if (pthread_create(&lt, NULL, late, NULL)) { puts("FAIL pthread_create late"); return 2; }
    pthread_join(lt, NULL);
    pthread_barrier_wait(&hold);
    for (int i = 0; i < NTHREADS; i++) pthread_join(th[i], NULL);
    printf("%s threads=%d rounds=%d fails=%d\n", fails ? "FAIL" : "PASS", NTHREADS + 1, ROUNDS, fails);
    return fails ? 1 : 0;
}
```

### tlsmod.c
```c
/* Helper module for the late-module TLS probe: a non-zero .tdata image and a .tbss tail. */
#define DATA_LEN 61
#define TAIL_LEN 4099
__thread unsigned char tls_data[DATA_LEN] = {
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50,
    51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61};
__thread unsigned char tls_tail[TAIL_LEN];
unsigned char *mod_data(void) { return tls_data; }
unsigned char *mod_tail(void) { return tls_tail; }
```

### tlsmod_mut.c (detection check variants)
```c
/* Detection check for the probe (not a libc plant): each MUT_* variant acts out one loader defect. */
#define DATA_LEN 61
#define TAIL_LEN 4099
#define IMG {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, \
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, \
    51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61}
#if defined(MUT_SHARED) /* (a) one block shared by every thread */
unsigned char g_data[DATA_LEN] = IMG;
unsigned char g_tail[TAIL_LEN];
unsigned char *mod_data(void) { return g_data; }
unsigned char *mod_tail(void) { return g_tail; }
#elif defined(MUT_NOCOPY) /* (b) lazy block allocated but .tdata never copied in */
__thread unsigned char tls_data[DATA_LEN];
__thread unsigned char tls_tail[TAIL_LEN];
unsigned char *mod_data(void) { return tls_data; }
unsigned char *mod_tail(void) { return tls_tail; }
#elif defined(MUT_TBSS_AA) /* (c) .tbss tail left uninitialised (0xAA) */
__thread unsigned char tls_data[DATA_LEN] = IMG;
__thread unsigned char tls_tail[TAIL_LEN] = {[0 ... TAIL_LEN - 1] = 0xAA};
unsigned char *mod_data(void) { return tls_data; }
unsigned char *mod_tail(void) { return tls_tail; }
#else
#error pick a MUT_ variant
#endif
```
