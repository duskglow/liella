/* Spec §17.2: a credential change applies to every thread of the process (the broadcast of
 * D-17.7). Run as root in a user namespace with several IDs mapped. */
/* RUN-WRAP: unshare -U --map-auto --map-root-user */
#define _GNU_SOURCE
#include <grp.h>
#include <pthread.h>
#include <sys/fsuid.h>
#include <sys/syscall.h>
#include <unistd.h>

enum { N = 64 };
static pthread_barrier_t start, changed, done;
static unsigned uids[N], gids[N], euids[N];
static int ngroups[N];

static void *worker(void *arg)
{
    int i = (int)(long)arg;
    pthread_barrier_wait(&start);
    pthread_barrier_wait(&changed);
    /* The kernel's per-thread values, not a cached copy. */
    uids[i] = (unsigned)syscall(SYS_getuid);
    euids[i] = (unsigned)syscall(SYS_geteuid);
    gids[i] = (unsigned)syscall(SYS_getgid);
    ngroups[i] = (int)syscall(SYS_getgroups, 0, NULL);
    pthread_barrier_wait(&done);
    return NULL;
}

int main(void)
{
    if (getuid() != 0)
        return 1;
    pthread_barrier_init(&start, NULL, N + 1);
    pthread_barrier_init(&changed, NULL, N + 1);
    pthread_barrier_init(&done, NULL, N + 1);
    pthread_t t[N];
    for (long i = 0; i < N; i++)
        pthread_create(&t[i], NULL, worker, (void *)i);
    pthread_barrier_wait(&start);
    gid_t list[3] = {7, 8, 9};
    if (setgroups(3, list) != 0)
        return 2;
    if (setresgid(5, 5, 5) != 0)
        return 3;
    if (setreuid(4, 4) != 0)
        return 4;
    pthread_barrier_wait(&changed);
    pthread_barrier_wait(&done);
    for (int i = 0; i < N; i++) {
        if (uids[i] != 4 || euids[i] != 4)
            return 10 + i;
        if (gids[i] != 5 || ngroups[i] != 3)
            return 30 + i;
    }
    for (int i = 0; i < N; i++)
        pthread_join(t[i], NULL);
    /* Unprivileged now: a change to another ID fails everywhere, and nothing changes. */
    if (setuid(0) != -1)
        return 50;
    if (getuid() != 4)
        return 51;
    return 0;
}
