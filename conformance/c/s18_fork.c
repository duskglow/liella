/* Spec §18.2 and §22.4: fork in a threaded process. atfork handlers run in order (prepare
 * newest first; parent and child oldest first); malloc works in the child while another
 * parent thread was allocating (the library's locks are taken around the fork); only the
 * calling thread exists in the child; _Fork runs no handlers; vfork suspends the parent until
 * the child exits; clone runs a function on a new stack; a process-shared robust mutex whose
 * owner process dies reports EOWNERDEAD. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <unistd.h>

static char log_[32];
static int nlog;
static void p1(void) { log_[nlog++] = 'p'; }
static void p2(void) { log_[nlog++] = 'q'; }
static void a1(void) { log_[nlog++] = 'a'; }
static void a2(void) { log_[nlog++] = 'b'; }
static void c1(void) { log_[nlog++] = 'c'; }
static void c2(void) { log_[nlog++] = 'd'; }

static volatile int stop;
static void *allocator(void *arg)
{
    (void)arg;
    while (!stop) {
        void *p = malloc(100);
        free(p);
        void *q = malloc(5000);
        free(q);
    }
    return NULL;
}

/* The status of the only child: its exit code, or 100 + signal. */
static int reap(pid_t pid)
{
    int st = 0;
    if (syscall(SYS_wait4, pid, &st, 0, NULL) != pid)
        return -1;
    if ((st & 0x7f) == 0)
        return (st >> 8) & 0xff;
    return 100 + (st & 0x7f);
}

static int clone_fn(void *arg)
{
    *(volatile int *)arg = 7; /* CLONE_VM: the parent sees it */
    return 3;
}

int main(void)
{
    if (pthread_atfork(p1, a1, c1) != 0 || pthread_atfork(p2, a2, c2) != 0)
        return 1;
    pthread_t t;
    pthread_create(&t, NULL, allocator, NULL);
    for (int round = 0; round < 50; round++) {
        nlog = 0;
        pid_t pid = fork();
        if (pid < 0)
            return 2;
        if (pid == 0) {
            if (nlog != 4 || memcmp(log_, "qpcd", 4) != 0)
                _exit(10);
            for (int i = 0; i < 1000; i++) {
                void *p = malloc((size_t)(i % 300) + 1);
                if (p == NULL)
                    _exit(11);
                free(p);
            }
            /* Only this thread: its TID is the process ID. */
            if (gettid() != getpid())
                _exit(12);
            _exit(0);
        }
        if (nlog != 4 || memcmp(log_, "qpab", 4) != 0)
            return 3;
        int st = reap(pid);
        if (st != 0)
            return 20 + st % 20;
    }
    stop = 1;
    pthread_join(t, NULL);

    nlog = 0;
    pid_t pid = _Fork();
    if (pid == 0)
        _exit(nlog == 0 ? 0 : 1);
    if (nlog != 0 || reap(pid) != 0)
        return 4;

    volatile int shared = 0;
    pid = vfork();
    if (pid == 0) {
        shared = 1;
        _exit(5);
    }
    if (reap(pid) != 5 || shared != 1)
        return 5; /* the child shares the parent's memory (D-18.1) */

    static char stack[64 * 1024];
    volatile int seen = 0;
    pid_t c = clone(clone_fn, stack + sizeof stack, CLONE_VM | SIGCHLD, (void *)&seen);
    if (c <= 0 || reap(c) != 3 || seen != 7)
        return 6;

    /* A process-shared robust mutex whose owner process exits holding it. */
    pthread_mutex_t *m = mmap(NULL, sizeof *m, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    if (m == MAP_FAILED)
        return 7;
    pthread_mutexattr_t ma;
    pthread_mutexattr_init(&ma);
    pthread_mutexattr_setpshared(&ma, PTHREAD_PROCESS_SHARED);
    pthread_mutexattr_setrobust(&ma, PTHREAD_MUTEX_ROBUST);
    pthread_mutex_init(m, &ma);
    pid = fork();
    if (pid == 0) {
        pthread_mutex_lock(m);
        _exit(0);
    }
    if (reap(pid) != 0)
        return 8;
    if (pthread_mutex_lock(m) != EOWNERDEAD || pthread_mutex_consistent(m) != 0 || pthread_mutex_unlock(m) != 0)
        return 9;
    return 0;
}
