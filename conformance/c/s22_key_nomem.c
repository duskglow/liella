/* Spec §22.6 Failure (s22_6_f_0): pthread_setspecific never fails with ENOMEM in Liella. With
 * the address space exhausted (RLIMIT_AS at the process's size plus 64 KiB, then malloc and
 * mmap taken until they fail), a thread that has never stored a key value sets, and reads back,
 * the value of each key in the table: keys inside the reference library's in-thread range
 * (creation index 0 to 31, which never fail there) and keys outside it (32 and up, which may
 * fail there with ENOMEM; spec/behaviour/key-nomem.md). Every case runs on the initial thread and
 * on a thread created before the limit, each in its own child process, so no case sees memory or
 * values another case left. */
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

enum { NKEYS = 1024 };
static pthread_key_t keys[NKEYS];
static int nkeys;

/* -1: set every key in creation order; otherwise the one creation index to set. */
static int target;

static long vm_bytes(void)
{
    char b[128];
    int fd = open("/proc/self/statm", O_RDONLY);
    if (fd < 0)
        return -1;
    long n = read(fd, b, sizeof b - 1);
    close(fd);
    if (n <= 0)
        return -1;
    b[n] = 0;
    return atol(b) * 4096L;
}

/* Exhaust the address space; 0 once a page can no longer be mapped. */
static int exhaust(void)
{
    struct rlimit r;
    long size = vm_bytes();
    if (size < 0 || getrlimit(RLIMIT_AS, &r) != 0)
        return 1;
    r.rlim_cur = (rlim_t)size + 65536;
    if (setrlimit(RLIMIT_AS, &r) != 0)
        return 1;
    static const size_t sizes[] = {1 << 20, 65536, 4096, 256, 16};
    for (unsigned i = 0; i < sizeof sizes / sizeof sizes[0]; i++)
        for (int n = 0; n < 1000000 && malloc(sizes[i]) != NULL; n++) {
        }
    for (int n = 0; n < 100000; n++)
        if (mmap(0, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0) == MAP_FAILED)
            return errno == ENOMEM ? 0 : 1;
    return 1;
}

static int set_and_check(int i)
{
    void *v = (void *)(long)(0x1000 + i);
    int rc = pthread_setspecific(keys[i], v);
    if (rc != 0) {
        fprintf(stderr, "s22_key_nomem: key index %d: pthread_setspecific returned %d (%s)\n", i, rc,
                rc == ENOMEM ? "ENOMEM" : "other");
        return 1;
    }
    if (pthread_getspecific(keys[i]) != v) {
        fprintf(stderr, "s22_key_nomem: key index %d: pthread_getspecific lost the value\n", i);
        return 1;
    }
    return 0;
}

static void *body(void *arg)
{
    (void)arg;
    if (exhaust() != 0) {
        fprintf(stderr, "s22_key_nomem: the address space was not exhausted\n");
        return (void *)1;
    }
    int bad = 0;
    if (target >= 0) {
        bad = set_and_check(target);
    } else {
        for (int i = 0; i < nkeys && !bad; i++)
            bad = set_and_check(i);
        for (int i = 0; i < nkeys && !bad; i++)
            if (pthread_getspecific(keys[i]) != (void *)(long)(0x1000 + i)) {
                fprintf(stderr, "s22_key_nomem: key index %d changed after later sets\n", i);
                bad = 1;
            }
    }
    return (void *)(long)bad;
}

static int run(int threaded, int tgt)
{
    pid_t p = fork();
    if (p < 0)
        return 1;
    if (p == 0) {
        void *res = 0;
        target = tgt;
        if (threaded) {
            pthread_t t;
            if (pthread_create(&t, 0, body, 0) != 0 || pthread_join(t, &res) != 0)
                _exit(2);
        } else {
            res = body(0);
        }
        _exit(res != 0);
    }
    int st;
    if (waitpid(p, &st, 0) != p || !WIFEXITED(st) || WEXITSTATUS(st) != 0) {
        fprintf(stderr, "s22_key_nomem: FAIL %s thread, %s %d\n", threaded ? "a new" : "the initial",
                tgt < 0 ? "keys 0 up to" : "key index", tgt < 0 ? nkeys - 1 : tgt);
        return 1;
    }
    return 0;
}

int main(void)
{
    for (nkeys = 0; nkeys < NKEYS; nkeys++)
        if (pthread_key_create(&keys[nkeys], 0) != 0)
            break;
    if (nkeys != NKEYS) {
        fprintf(stderr, "s22_key_nomem: only %d keys created\n", nkeys);
        return 1;
    }
    /* Inside the reference's in-thread range, its edge, and outside it. */
    static const int table[] = {0, 1, 15, 31, 32, 33, 63, 64, 500, 1023};
    int fails = 0;
    for (int th = 0; th < 2; th++) {
        fails += run(th, -1);
        for (unsigned i = 0; i < sizeof table / sizeof table[0]; i++)
            fails += run(th, table[i]);
    }
    if (fails != 0) {
        fprintf(stderr, "s22_key_nomem: %d case(s) failed\n", fails);
        return 1;
    }
    puts("s22_key_nomem: ok");
    return 0;
}
