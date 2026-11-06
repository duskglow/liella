/* Spec §22 Notes (s22_n_2): pthread functions return error numbers and do not set errno; the
 * sem_* functions are the exception and report through errno. For a table of cheap failing
 * calls, errno is set to a sentinel first: each call must return its error number and leave
 * the sentinel in place. A succeeding call leaves it too. sem_trywait on a zero semaphore
 * returns -1 with errno EAGAIN. */
#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <pthread.h>
#include <semaphore.h>
#include <signal.h>
#include <stdio.h>

#define SENTINEL 4242 /* not an error number */

static int failures;

static void check(const char *what, int got, int want)
{
    int e = errno;
    if (got != want) {
        fprintf(stderr, "s22_errno: %s returned %d, want %d\n", what, got, want);
        failures++;
    }
    if (e != SENTINEL) {
        fprintf(stderr, "s22_errno: %s changed errno to %d\n", what, e);
        failures++;
    }
}

int main(void)
{
    pthread_mutex_t held = PTHREAD_MUTEX_INITIALIZER;
    pthread_mutex_t free_m = PTHREAD_MUTEX_INITIALIZER;
    pthread_mutexattr_t ma;
    pthread_mutex_t ec;
    pthread_attr_t a;
    sem_t sem;
    int r;

    if (pthread_mutex_lock(&held) != 0)
        return 2;
    if (pthread_mutexattr_init(&ma) != 0 || pthread_mutexattr_settype(&ma, PTHREAD_MUTEX_ERRORCHECK) != 0 ||
        pthread_mutex_init(&ec, &ma) != 0)
        return 3;
    if (pthread_attr_init(&a) != 0)
        return 4;

    errno = SENTINEL;
    r = pthread_mutex_trylock(&held);
    check("pthread_mutex_trylock (locked)", r, EBUSY);

    errno = SENTINEL;
    r = pthread_join(pthread_self(), NULL);
    check("pthread_join (self)", r, EDEADLK);

    errno = SENTINEL;
    r = pthread_mutex_unlock(&ec);
    check("pthread_mutex_unlock (errorcheck, unowned)", r, EPERM);

    errno = SENTINEL;
    r = pthread_attr_setstacksize(&a, PTHREAD_STACK_MIN - 1);
    check("pthread_attr_setstacksize (too small)", r, EINVAL);

    errno = SENTINEL;
    r = pthread_kill(pthread_self(), 65);
    check("pthread_kill (signal 65)", r, EINVAL);

    errno = SENTINEL;
    r = pthread_kill(pthread_self(), -1);
    check("pthread_kill (signal -1)", r, EINVAL);

    /* Success leaves errno alone too. */
    errno = SENTINEL;
    r = pthread_mutex_trylock(&free_m);
    check("pthread_mutex_trylock (free)", r, 0);
    errno = SENTINEL;
    r = pthread_mutex_unlock(&free_m);
    check("pthread_mutex_unlock (owned)", r, 0);

    pthread_attr_destroy(&a);
    pthread_mutex_destroy(&ec);
    pthread_mutexattr_destroy(&ma);
    pthread_mutex_unlock(&held);

    /* sem_* use errno: -1 and EAGAIN. */
    if (sem_init(&sem, 0, 0) != 0)
        return 5;
    errno = SENTINEL;
    r = sem_trywait(&sem);
    if (r != -1 || errno != EAGAIN) {
        fprintf(stderr, "s22_errno: sem_trywait (zero) returned %d errno %d, want -1 errno %d\n", r, errno, EAGAIN);
        failures++;
    }
    sem_destroy(&sem);

    return failures ? 1 : 0;
}
