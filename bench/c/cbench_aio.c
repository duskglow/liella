/* POSIX asynchronous I/O (phase 13, §11.11; D-11.8): the latency of one request on a regular file
 * whose pages are cached (submit, wait, collect), and a list of 16. The same file builds against
 * the products (`cargo xtask bench`) and against the host C library (`cc -O2 -pthread
 * bench/c/cbench_aio.c`), so benchmarks/progress.md compares identical code. Prints
 * `name<TAB>ns/op` lines, the best of several rounds, formatted by hand.
 *
 * Liella runs the requests on io_uring where the kernel permits it (a helper thread submits;
 * the caller sleeps until the completion), else on worker threads; the reference always on
 * worker threads. `pread_4k` is the synchronous call the requests stand for. The file is made
 * under /tmp and unlinked at once; every call is checked before it is timed. */
#define _GNU_SOURCE
#include <aio.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static volatile long sink;

static double now(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e9 + (double)ts.tv_nsec;
}

static void put(const char *name, double ns)
{
    char buf[64];
    int n = 0;
    while (name[n] && n < 40) {
        buf[n] = name[n];
        n++;
    }
    buf[n++] = '\t';
    unsigned long v = (unsigned long)(ns * 100.0 + 0.5);
    char digits[24];
    int d = 0;
    do {
        digits[d++] = (char)('0' + v % 10);
        v /= 10;
    } while (v || d < 3);
    while (d > 2)
        buf[n++] = digits[--d];
    buf[n++] = '.';
    buf[n++] = digits[1];
    buf[n++] = digits[0];
    buf[n++] = '\n';
    (void)!write(1, buf, (size_t)n);
}

static void fail(const char *what)
{
    (void)!write(2, "cbench_aio: unexpected result: ", 31);
    (void)!write(2, what, strlen(what));
    (void)!write(2, "\n", 1);
    exit(1);
}

#include "cbench_rows.h"

#define MEASURE(name, iters, expr)                         \
    do {                                                   \
        if (cb_skip(name))                                 \
            break;                                         \
        double best = 1e300;                               \
        for (int round = 0; round < 7; round++) {          \
            double t0 = now();                             \
            long acc = 0;                                  \
            for (long i = 0; i < (iters); i++)             \
                acc += (long)(expr);                       \
            sink = acc;                                    \
            double per = (now() - t0) / (double)(iters);   \
            if (per < best)                                \
                best = per;                                \
        }                                                  \
        put(name, best);                                   \
    } while (0)

static int fd;
static unsigned char buf[16][4096];
static struct aiocb cbs[16];

/* One request: submit, wait, collect; 0 when it read `n` bytes. */
static long one(size_t n)
{
    struct aiocb *cb = &cbs[0];
    memset(cb, 0, sizeof *cb);
    cb->aio_fildes = fd;
    cb->aio_buf = buf[0];
    cb->aio_nbytes = n;
    cb->aio_offset = 4096;
    cb->aio_sigevent.sigev_notify = SIGEV_NONE;
    if (aio_read(cb) != 0)
        return 1;
    const struct aiocb *l[1] = {cb};
    while (aio_error(cb) == EINPROGRESS)
        if (aio_suspend(l, 1, NULL) != 0 && errno != EINTR)
            return 2;
    return aio_return(cb) == (ssize_t)n ? 0 : 3;
}

/* A list of 16 reads of 4 KiB, waited for together. */
static long list16(void)
{
    struct aiocb *l[16];
    for (int k = 0; k < 16; k++) {
        memset(&cbs[k], 0, sizeof cbs[k]);
        cbs[k].aio_fildes = fd;
        cbs[k].aio_buf = buf[k];
        cbs[k].aio_nbytes = 4096;
        cbs[k].aio_offset = (off_t)k * 4096;
        cbs[k].aio_lio_opcode = LIO_READ;
        cbs[k].aio_sigevent.sigev_notify = SIGEV_NONE;
        l[k] = &cbs[k];
    }
    if (lio_listio(LIO_WAIT, l, 16, NULL) != 0)
        return 1;
    long bad = 0;
    for (int k = 0; k < 16; k++)
        bad += aio_return(&cbs[k]) != 4096;
    return bad;
}

static long sync_read(void)
{
    return pread(fd, buf[0], 4096, 4096) == 4096 ? 0 : 1;
}

int main(void)
{
    char path[] = "/tmp/cbench_aio.XXXXXX";
    fd = mkstemp(path);
    if (fd < 0)
        fail("mkstemp");
    unlink(path);
    static unsigned char block[64 * 1024];
    memset(block, 'a', sizeof block);
    if (write(fd, block, sizeof block) != (ssize_t)sizeof block)
        fail("write");
    if (sync_read() != 0)
        fail("pread");
    MEASURE("pread_4k", 200000, sync_read());
    if (one(4096) != 0 || one(64) != 0)
        fail("aio_read");
    MEASURE("aio_read_suspend_64", 20000, one(64));
    MEASURE("aio_read_suspend_4k", 20000, one(4096));
    if (list16() != 0)
        fail("lio_listio");
    MEASURE("lio_listio_wait_16x4k", 5000, list16());
    close(fd);
    return 0;
}
