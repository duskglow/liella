/* Spec §12, D-12.19, FILE-LOCK-3: flockfile / ftrylockfile / funlockfile on threads Liella set
 * up itself (the process's main thread and pthread_create threads), where the direct lock path
 * runs; the cargo test threads are not such threads and always take the full path. Checked:
 * standard streams at their first use, nesting (the depth is kept), ftrylockfile from another
 * thread, funlockfile by a thread that does not hold the lock, and a byte copy of a FILE.
 * Self-checking: each failed check prints `FAIL <part>: line N: <check>` on standard error.
 * The functions are called through volatile pointers, never as compiler builtins. */
#define _GNU_SOURCE
#include <pthread.h>
#include <stdio.h>
#include <string.h>

static int failures;
static const char *current;

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

static void (*volatile lock_f)(FILE *) = flockfile;
static int (*volatile trylock_f)(FILE *) = ftrylockfile;
static void (*volatile unlock_f)(FILE *) = funlockfile;
static int (*volatile ferror_unlocked_f)(FILE *) = ferror_unlocked;

/* ftrylockfile from another thread: 1 when it took the lock (it releases it again), else 0. */
static void *other_try(void *arg)
{
    FILE *fp = arg;
    if (trylock_f(fp) != 0)
        return (void *)0;
    unlock_f(fp);
    return (void *)1;
}

static int taken_by_other(FILE *fp)
{
    pthread_t t;
    void *r = (void *)2;
    if (pthread_create(&t, NULL, other_try, fp) != 0 || pthread_join(t, &r) != 0)
        return -1;
    return (int)(long)r;
}

/* funlockfile from a thread that does not hold the lock, then ftrylockfile there: 1 when that
 * thread could take the lock (it releases it again), else 0. */
static void *other_unlock_then_try(void *arg)
{
    FILE *fp = arg;
    unlock_f(fp);
    if (trylock_f(fp) != 0)
        return (void *)0;
    unlock_f(fp);
    return (void *)1;
}

static FILE copy;

int main(void)
{
    /* First: the standard streams have not been used yet. flockfile at first use must take the
     * stream's own lock: after a call that sets the stream up without locking, another thread
     * still cannot take it. */
    FILE *std[2] = { stdin, stdout };
    const char *names[2] = { "stdin_first_use", "stdout_first_use" };
    for (int i = 0; i < 2; i++) {
        current = names[i];
        lock_f(std[i]);
        CHECK(ferror_unlocked_f(std[i]) == 0);
        CHECK(taken_by_other(std[i]) == 0);
        unlock_f(std[i]);
        CHECK(taken_by_other(std[i]) == 1);
    }

    FILE *fp = tmpfile();
    current = "tmpfile";
    CHECK(fp != NULL);
    if (fp == NULL)
        return 1;
    CHECK(fputc('x', fp) == 'x');

    /* Nesting: two flockfile and one ftrylockfile need three funlockfile. */
    current = "nesting";
    lock_f(fp);
    lock_f(fp);
    CHECK(trylock_f(fp) == 0);
    CHECK(taken_by_other(fp) == 0);
    unlock_f(fp);
    CHECK(taken_by_other(fp) == 0);
    unlock_f(fp);
    CHECK(taken_by_other(fp) == 0);
    unlock_f(fp);
    CHECK(taken_by_other(fp) == 1);

    /* ftrylockfile while another thread holds nothing: taken; while this thread holds it: the
     * other thread fails. */
    current = "trylock_other_thread";
    CHECK(trylock_f(fp) == 0);
    CHECK(taken_by_other(fp) == 0);
    unlock_f(fp);
    CHECK(taken_by_other(fp) == 1);

    /* funlockfile by a thread that does not hold the lock releases nothing. */
    current = "unlock_by_non_owner";
    lock_f(fp);
    {
        pthread_t t;
        void *r = (void *)2;
        CHECK(pthread_create(&t, NULL, other_unlock_then_try, fp) == 0 && pthread_join(t, &r) == 0);
        CHECK(r == (void *)0);
    }
    unlock_f(fp);
    CHECK(taken_by_other(fp) == 1);

    /* A byte copy of the FILE is not the stream: locking it must not lock the original. */
    current = "byte_copy";
    memcpy(&copy, fp, sizeof copy);
    lock_f(&copy);
    CHECK(taken_by_other(fp) == 1);
    unlock_f(&copy);
    lock_f(fp);
    CHECK(taken_by_other(fp) == 0);
    unlock_f(fp);
    CHECK(taken_by_other(fp) == 1);

    current = "fclose";
    CHECK(fclose(fp) == 0);
    return failures ? 1 : 0;
}
