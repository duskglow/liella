/* Black-box probe: what the reference does with an object whose PT_GNU_STACK asks for an
 * executable stack. Prints the permissions of the main stack and of a thread stack made before
 * the dlopen, the dlopen result, and the same after. No library source consulted. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static char perms_of(uintptr_t a, char out[5]) {
    FILE *f = fopen("/proc/self/maps", "r");
    char line[512];
    strcpy(out, "????");
    while (f && fgets(line, sizeof line, f)) {
        unsigned long lo, hi;
        char p[5];
        if (sscanf(line, "%lx-%lx %4s", &lo, &hi, p) == 3 && a >= lo && a < hi) {
            memcpy(out, p, 5);
            break;
        }
    }
    if (f) fclose(f);
    return out[2];
}

static volatile int go;
static volatile uintptr_t tstack;
static void *thr(void *arg) {
    volatile char here;
    (void)arg;
    tstack = (uintptr_t)&here;
    while (!go) usleep(1000);
    return 0;
}

int main(int argc, char **argv) {
    volatile char here;
    char m[5], t[5];
    pthread_t th;
    pthread_create(&th, 0, thr, 0);
    while (!tstack) usleep(1000);
    perms_of((uintptr_t)&here, m);
    perms_of(tstack, t);
    printf("before main-stack %s thread-stack %s\n", m, t);
    if (argc > 1) {
        void *h = dlopen(argv[1], RTLD_NOW);
        printf("dlopen %s: %s\n", argv[1], h ? "ok" : dlerror());
        perms_of((uintptr_t)&here, m);
        perms_of(tstack, t);
        printf("after main-stack %s thread-stack %s\n", m, t);
    }
    go = 1;
    pthread_join(th, 0);
    return 0;
}
