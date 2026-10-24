/* Spec §1.2 failure clause 3: exit from a multi-threaded process terminates every thread
 * without running their cleanup handlers. Two threads wait inside pthread_cleanup_push regions
 * (one blocked in a system call, one spinning); a cleanup handler that ran would end the process
 * with status 99 first, and a thread that survived exit would keep it alive until the alarm
 * (SIGALRM). The atexit handler shows that exit itself ran. */
/* EXPECT-EXIT: 3 */
/* EXPECT-STDERR: [atexit] */
#include <pthread.h>
#include <stdlib.h>
#include <unistd.h>

static volatile int started;

static void cleanup(void *arg)
{
    (void)arg;
    write(2, "[cleanup]\n", 10);
    _exit(99);
}

static void *blocked(void *arg)
{
    pthread_cleanup_push(cleanup, arg);
    __atomic_add_fetch(&started, 1, __ATOMIC_SEQ_CST);
    for (;;)
        pause();
    pthread_cleanup_pop(0);
    return NULL;
}

static void *spinning(void *arg)
{
    pthread_cleanup_push(cleanup, arg);
    __atomic_add_fetch(&started, 1, __ATOMIC_SEQ_CST);
    for (;;)
        __atomic_load_n(&started, __ATOMIC_SEQ_CST);
    pthread_cleanup_pop(0);
    return NULL;
}

static void at_exit(void) { write(2, "[atexit]\n", 9); }

int main(void)
{
    alarm(20);
    if (atexit(at_exit) != 0)
        return 1;
    pthread_t a, b;
    if (pthread_create(&a, NULL, blocked, NULL) != 0 || pthread_create(&b, NULL, spinning, NULL) != 0)
        return 2;
    while (__atomic_load_n(&started, __ATOMIC_SEQ_CST) < 2)
        usleep(1000);
    usleep(20000);
    exit(3);
}
