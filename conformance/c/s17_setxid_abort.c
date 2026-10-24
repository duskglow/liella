/* Spec §17.2: a credential change that succeeds in the caller but fails in another thread
 * ends the process (it cannot leave threads with different credentials). A worker drops its
 * own privilege with the raw system call (credentials are per thread to the kernel); the
 * main thread's setgid then succeeds for itself and fails in the worker's replay. */
/* RUN-WRAP: unshare -U --map-auto --map-root-user */
/* EXPECT-SIGNAL: 6 */
/* EXPECT-STDERR: setxid failed on another thread */
#define _GNU_SOURCE
#include <pthread.h>
#include <sys/syscall.h>
#include <unistd.h>
#include "expect_crash.h"

static pthread_barrier_t dropped, never;

static void *worker(void *arg)
{
    (void)arg;
    if (syscall(SYS_setresuid, 5, 5, 5) != 0)
        _exit(2);
    pthread_barrier_wait(&dropped);
    pthread_barrier_wait(&never); /* the replay interrupts this wait */
    return NULL;
}

int main(void)
{
    if (getuid() != 0)
        return 1;
    pthread_barrier_init(&dropped, NULL, 2);
    pthread_barrier_init(&never, NULL, 2);
    pthread_t t;
    pthread_create(&t, NULL, worker, NULL);
    pthread_barrier_wait(&dropped);
    expect_crash();
    setgid(6); /* succeeds here, EPERM in the worker: the process must end */
    return 3;
}
