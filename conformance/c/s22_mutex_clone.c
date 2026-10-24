/* Spec §22.4, §18 clone: a single-threaded program that starts a task sharing its memory with
 * clone(CLONE_VM) (not through pthread_create) and contends a mutex with it loses no update
 * (MUTEX-3: the clone wrapper ends the single-thread mutex mode before the system call). A
 * CLONE_VM | CLONE_VFORK child (the caller suspended until it exits) keeps the mutex rules. */
#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

static pthread_mutex_t counter_lock = PTHREAD_MUTEX_INITIALIZER;
static long counter;
static volatile int started;
#define ROUNDS 1000000

static void count_up(void)
{
    for (int i = 0; i < ROUNDS; i++) {
        pthread_mutex_lock(&counter_lock);
        /* A read, a pause and a write: a lost exclusion loses updates. */
        long v = counter;
        __asm__ volatile("" ::: "memory");
        counter = v + 1;
        pthread_mutex_unlock(&counter_lock);
    }
}

static int sibling(void *arg)
{
    (void)arg;
    started = 1;
    count_up();
    return 0;
}

static int vfork_child(void *arg)
{
    pthread_mutex_t *m = arg;
    /* The caller is suspended: the mutex the child takes and releases keeps its rules. */
    if (pthread_mutex_lock(m) != 0 || pthread_mutex_trylock(m) != EBUSY || pthread_mutex_unlock(m) != 0)
        return 1;
    return 0;
}

static int run(int (*fn)(void *), int flags, void *arg)
{
    size_t size = 1 << 18;
    char *stack = malloc(size);
    if (stack == NULL)
        return -1;
    pid_t pid = clone(fn, stack + size, flags | SIGCHLD, arg);
    if (pid < 0)
        return -1;
    if (!(flags & CLONE_VFORK)) {
        /* Run beside the sibling: wait until it is running, then contend. */
        while (!started)
            sched_yield();
        count_up();
    }
    int st;
    if (waitpid(pid, &st, 0) != pid || !WIFEXITED(st))
        return -1;
    free(stack);
    return WEXITSTATUS(st);
}

int main(void)
{
    alarm(120);
    pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;
    if (run(vfork_child, CLONE_VM | CLONE_VFORK, &m) != 0)
        return 1;
    if (pthread_mutex_trylock(&m) != 0 || pthread_mutex_unlock(&m) != 0)
        return 2;
    if (run(sibling, CLONE_VM | CLONE_FS | CLONE_FILES, NULL) != 0)
        return 3;
    if (counter != 2L * ROUNDS)
        return 4;
    return 0;
}
