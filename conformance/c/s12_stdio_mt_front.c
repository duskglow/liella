/* Spec §12, D-12.19, STDIO-MT-FPUTS: fputs and fwrite on a fully buffered stream while a second
 * thread is alive, called from a thread Liella set up itself (the process's main thread), where
 * the locked front path runs (the cargo test threads are not such threads and never reach it).
 * Short texts (the fputs leaf), long texts (fputs past its leaf) and fwrite. Checked: every byte
 * and the order of the bytes; the stream lock free to another thread after a call; a call nested
 * under the caller's own flockfile (the lock still held after it, free after funlockfile); a call
 * made while another thread holds the lock waits for it (that thread's bytes come first).
 * Self-checking: each failed check prints `FAIL <part>: line N: <check>` on standard error.
 * STDIO-LOCKED-PATH: a watchdog fails the program within a minute when a call or the final
 * fclose hangs (a lock left in a state no thread can take), and the lock state is checked
 * once more before the stream is closed (the caller's ftrylockfile excludes another thread).
 * The functions are called through volatile pointers, never as compiler builtins. */
#define _GNU_SOURCE
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static int failures;
static const char *current;

#define CHECK(cond) do { if (!(cond)) { failures++; fprintf(stderr, "FAIL %s: line %d: %s\n", current, __LINE__, #cond); } } while (0)

static int (*volatile fputs_f)(const char *, FILE *) = fputs;
static size_t (*volatile fwrite_f)(const void *, size_t, size_t, FILE *) = fwrite;
static int (*volatile fputs_unlocked_f)(const char *, FILE *) = fputs_unlocked;
static void (*volatile lock_f)(FILE *) = flockfile;
static int (*volatile trylock_f)(FILE *) = ftrylockfile;
static void (*volatile unlock_f)(FILE *) = funlockfile;

/* The watchdog: the whole program takes well under a second (the bounded wait for a holder
 * about 10 s); a hang fails it at once instead of at the harness's timeout. */
static void watchdog(int sig)
{
    static const char msg[] = "FAIL watchdog: the program hung (a stream lock that cannot be taken?)\n";
    (void)sig;
    (void)!write(2, msg, sizeof msg - 1);
    _exit(1);
}

/* What the stream must hold at the end, in order. */
static char expect[65536];
static size_t expect_len;

static void expect_add(const char *s, size_t n)
{
    if (expect_len + n <= sizeof expect)
        memcpy(expect + expect_len, s, n);
    expect_len += n;
}

/* The second thread: alive (blocked in read) until the write end of the pipe is closed. */
static int park[2];

static void *parked(void *arg)
{
    char c;
    (void)arg;
    while (read(park[0], &c, 1) > 0)
        ;
    return NULL;
}

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

/* Another thread takes the lock, says so, waits 50 ms, writes HELD and releases the lock. */
static int held;
static const char HELD[] = "[held by the other thread]";

static void *hold_then_put(void *arg)
{
    FILE *fp = arg;
    lock_f(fp);
    __atomic_store_n(&held, 1, __ATOMIC_RELEASE);
    struct timespec ts = { 0, 50 * 1000 * 1000 };
    nanosleep(&ts, NULL);
    fputs_unlocked_f(HELD, fp);
    unlock_f(fp);
    return NULL;
}

/* `put` while another thread holds the lock: it must wait, so HELD comes before `text`. */
static void while_held(FILE *fp, int use_fwrite, const char *text)
{
    pthread_t t;
    __atomic_store_n(&held, 0, __ATOMIC_RELAXED);
    CHECK(pthread_create(&t, NULL, hold_then_put, fp) == 0);
    /* At most about 10 s: a lock never released leaves the other thread waiting for good. */
    struct timespec tick = { 0, 1000 * 1000 };
    for (int k = 0; k < 10000 && !__atomic_load_n(&held, __ATOMIC_ACQUIRE); k++)
        nanosleep(&tick, NULL);
    if (!__atomic_load_n(&held, __ATOMIC_ACQUIRE)) {
        CHECK(!"the other thread could not take the lock");
        pthread_detach(t);
        return;
    }
    if (use_fwrite)
        CHECK(fwrite_f(text, 1, strlen(text), fp) == strlen(text));
    else
        CHECK(fputs_f(text, fp) >= 0);
    CHECK(pthread_join(t, NULL) == 0);
    expect_add(HELD, strlen(HELD));
    expect_add(text, strlen(text));
    CHECK(taken_by_other(fp) == 1);
}

static void put_fputs(FILE *fp, const char *s)
{
    CHECK(fputs_f(s, fp) >= 0);
    expect_add(s, strlen(s));
}

static void put_fwrite(FILE *fp, const char *s)
{
    size_t n = strlen(s);
    CHECK(fwrite_f(s, 1, n, fp) == n);
    expect_add(s, n);
}

int main(void)
{
    pthread_t second;
    current = "watchdog";
    CHECK(signal(SIGALRM, watchdog) != SIG_ERR);
    alarm(60);
    current = "second_thread";
    CHECK(pipe(park) == 0);
    CHECK(pthread_create(&second, NULL, parked, NULL) == 0);

    FILE *fp = tmpfile();
    current = "tmpfile";
    CHECK(fp != NULL);
    if (fp == NULL)
        return 1;
    /* The first byte sets the stream up and opens its write window (fully buffered). */
    CHECK(fputc('<', fp) == '<');
    expect_add("<", 1);

    /* After each call the lock is free again: another thread can take it. */
    current = "released_after_call";
    put_fputs(fp, "fputs 32 bytes 0123456789abcdef\n");
    CHECK(taken_by_other(fp) == 1);
    put_fwrite(fp, "fwrite 32 bytes 0123456789abcde\n");
    CHECK(taken_by_other(fp) == 1);
    put_fputs(fp, "fputs past its leaf: a text longer than sixty-four bytes, ending here.\n");
    CHECK(taken_by_other(fp) == 1);
    put_fwrite(fp, "fwrite of a longer text: more than sixty-four bytes in one call, ending here.\n");
    CHECK(taken_by_other(fp) == 1);

    /* Many calls in a row: every byte, in order (the buffer fills and is written out on the way). */
    current = "order";
    for (int i = 0; i < 300; i++) {
        char line[96];
        /* Short (one copy piece), medium (33 to 64 bytes: two overlapping pieces) and long. */
        snprintf(line, sizeof line, i % 3 == 2 ? "%04d long line, past the leaf of fputs: 0123456789abcdefghijklmnopqrstuvwxyz\n" : i % 6 == 1 ? "%04d medium line, two copy pieces: 0123456789abcd\n" : "%04d short line 0123456789ab\n", i);
        if (i % 2)
            put_fwrite(fp, line);
        else
            put_fputs(fp, line);
        if (i % 50 == 0)
            CHECK(taken_by_other(fp) == 1);
    }

    /* Under the caller's own flockfile: the call nests, the lock stays the caller's after it. */
    current = "nested_in_flockfile";
    lock_f(fp);
    put_fputs(fp, "[nested fputs]");
    CHECK(taken_by_other(fp) == 0);
    put_fwrite(fp, "[nested fwrite]");
    CHECK(taken_by_other(fp) == 0);
    lock_f(fp);
    put_fputs(fp, "[nested twice]");
    unlock_f(fp);
    CHECK(taken_by_other(fp) == 0);
    unlock_f(fp);
    CHECK(taken_by_other(fp) == 1);

    /* Another thread holds the lock: the call waits for it. */
    current = "waits_for_holder";
    while_held(fp, 0, "[fputs after the holder]");
    while_held(fp, 1, "[fwrite after the holder]");
    while_held(fp, 0, "[fputs after the holder, a text longer than sixty-four bytes in all]");

    /* The stream's contents: exactly the expected bytes, in order. */
    current = "contents";
    CHECK(expect_len <= sizeof expect);
    CHECK(fflush(fp) == 0);
    CHECK(fseek(fp, 0, SEEK_END) == 0);
    long size = ftell(fp);
    CHECK(size == (long)expect_len);
    rewind(fp);
    static char got[sizeof expect];
    size_t n = fread(got, 1, sizeof got, fp);
    CHECK(n == expect_len);
    if (n == expect_len && memcmp(got, expect, n) != 0) {
        size_t k = 0;
        while (got[k] == expect[k])
            k++;
        fprintf(stderr, "FAIL contents: first difference at byte %zu of %zu\n", k, n);
        failures++;
    }

    /* The lock state after every call: free, the caller's own after ftrylockfile (another
     * thread cannot take it then), free again after funlockfile. */
    current = "lock_state_at_end";
    CHECK(taken_by_other(fp) == 1);
    CHECK(trylock_f(fp) == 0);
    CHECK(taken_by_other(fp) == 0);
    unlock_f(fp);
    CHECK(taken_by_other(fp) == 1);
    CHECK(fclose(fp) == 0);

    current = "second_thread_end";
    CHECK(close(park[1]) == 0);
    CHECK(pthread_join(second, NULL) == 0);
    alarm(0);
    return failures ? 1 : 0;
}
