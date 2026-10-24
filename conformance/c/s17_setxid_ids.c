/* Spec §17.2: every credential setter applies to every thread (the broadcast of D-17.7):
 * setgid, setegid, setregid, setfsgid, setfsuid, setresuid, seteuid and an unprivileged setuid.
 * Run as root in a user namespace with several IDs mapped. */
/* RUN-WRAP: unshare -U --map-auto --map-root-user */
#define _GNU_SOURCE
#include <pthread.h>
#include <sys/fsuid.h>
#include <sys/syscall.h>
#include <unistd.h>

enum { N = 8, STEPS = 9 };
static pthread_barrier_t changed, recorded;
/* Per step and thread: real, effective, saved uid; real, effective, saved gid; fsuid; fsgid. */
static unsigned ids[STEPS][N + 1][8];

static void record(int step, int i)
{
    unsigned *r = ids[step][i];
    /* The kernel's per-thread values, read without the library. */
    syscall(SYS_getresuid, &r[0], &r[1], &r[2]);
    syscall(SYS_getresgid, &r[3], &r[4], &r[5]);
    r[6] = (unsigned)syscall(SYS_setfsuid, -1);
    r[7] = (unsigned)syscall(SYS_setfsgid, -1);
}

static void *worker(void *arg)
{
    int i = (int)(long)arg;
    for (int step = 0; step < STEPS; step++) {
        pthread_barrier_wait(&changed);
        record(step, i);
        pthread_barrier_wait(&recorded);
    }
    return NULL;
}

/* Every thread (main = index N) agrees with `want` on the fields whose bit is set in `mask`. */
static int same(int step, const unsigned want[8], unsigned mask)
{
    for (int i = 0; i <= N; i++)
        for (int f = 0; f < 8; f++)
            if ((mask >> f & 1) && ids[step][i][f] != want[f])
                return 0;
    return 1;
}

int main(void)
{
    if (getuid() != 0)
        return 1;
    pthread_barrier_init(&changed, NULL, N + 1);
    pthread_barrier_init(&recorded, NULL, N + 1);
    pthread_t t[N];
    for (long i = 0; i < N; i++)
        pthread_create(&t[i], NULL, worker, (void *)i);

    int rc[STEPS];
    int prev_fsgid = -2, prev_fsuid = -2;
    for (int step = 0; step < STEPS; step++) {
        switch (step) {
        case 0: rc[0] = setgid(6); break;
        case 1: rc[1] = setegid(7); break;
        case 2: rc[2] = setregid(8, 9); break;
        case 3: prev_fsgid = setfsgid(11); rc[3] = 0; break;
        case 4: prev_fsuid = setfsuid(12); rc[4] = 0; break;
        case 5: rc[5] = setresuid(3, 0, 13); break;
        case 6: rc[6] = seteuid(14); break; /* drops privilege: euid 14 */
        case 7: rc[7] = seteuid(13); break; /* unprivileged: the saved ID */
        case 8: rc[8] = setuid(3); break;   /* unprivileged: the effective ID only */
        }
        record(step, N);
        pthread_barrier_wait(&changed);
        pthread_barrier_wait(&recorded);
    }
    for (int i = 0; i < N; i++)
        pthread_join(t[i], NULL);
    for (int step = 0; step < STEPS; step++)
        if (rc[step] != 0)
            return 10 + step;

    /* Fields: 0 ruid, 1 euid, 2 suid, 3 rgid, 4 egid, 5 sgid, 6 fsuid, 7 fsgid. */
    static const unsigned want[STEPS][8] = {
        {0, 0, 0, 6, 6, 6, 0, 6},
        {0, 0, 0, 6, 7, 6, 0, 7},
        {0, 0, 0, 8, 9, 9, 0, 9},
        {0, 0, 0, 8, 9, 9, 0, 11},
        {0, 0, 0, 8, 9, 9, 12, 11},
        {3, 0, 13, 8, 9, 9, 0, 11},
        {3, 14, 13, 8, 9, 9, 14, 11},
        {3, 13, 13, 8, 9, 9, 13, 11},
        {3, 3, 13, 8, 9, 9, 3, 11},
    };
    for (int step = 0; step < STEPS; step++)
        if (!same(step, want[step], 0xff))
            return 30 + step;
    /* setfsgid/setfsuid return the previous value. */
    if (prev_fsgid != 9 || prev_fsuid != 0)
        return 50;
    /* Unprivileged: a change to an unrelated ID fails and changes nothing anywhere. */
    if (seteuid(20) != -1 || geteuid() != 3)
        return 51;
    return 0;
}
