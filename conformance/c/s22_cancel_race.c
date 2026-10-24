/* Spec §22.3: a cancellation request that arrives while a cancellation point blocks acts
 * either before the call's side effect (nothing consumed) or after it (the call returns its
 * result and the request is acted on at the next point), never in between. A reader blocks in
 * read() on a pipe; the main thread writes one byte and cancels it, with a varying delay
 * between the two, many times. Every round the byte is either still in the pipe (cancelled in
 * read) or recorded by the reader (cancelled at the following pthread_testcancel): never lost. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <time.h>
#include <unistd.h>

enum { ROUNDS = 3000 };
static int pfd[2];
static volatile int got;
static volatile int blocked;

static void *reader(void *arg)
{
    (void)arg;
    char c;
    blocked = 1;
    ssize_t n = read(pfd[0], &c, 1); /* cancellation point */
    if (n == 1)
        got = 1;
    pthread_testcancel(); /* the next point: a request that arrived after read completed */
    (void)!read(pfd[0], &c, 1); /* or this one, if the request is not there yet: the pipe is empty */
    return NULL;
}

int main(void)
{
    int before = 0, after = 0;
    for (int round = 0; round < ROUNDS; round++) {
        if (pipe(pfd) != 0)
            return 1;
        got = 0;
        blocked = 0;
        pthread_t t;
        pthread_create(&t, NULL, reader, NULL);
        while (!blocked) {
            /* Block, not yield: on one core the new reader may not run until this thread sleeps. */
            struct timespec nap = {0, 10000};
            nanosleep(&nap, NULL);
        }
        for (volatile int spin = 0; spin < (round % 64) * 50; spin++)
            ; /* let the reader reach the kernel, or not */
        /* Alternate which is sent first, with a varying gap: the wake-up or the request. */
        if (round % 2) {
            if (write(pfd[1], "x", 1) != 1)
                return 2;
            for (volatile int spin = 0; spin < (round % 7) * 20; spin++)
                ;
            pthread_cancel(t);
        } else {
            pthread_cancel(t);
            for (volatile int spin = 0; spin < (round % 7) * 20; spin++)
                ;
            if (round % 4 == 0) {
                /* Let the reader run first (on one core it would not otherwise). */
                struct timespec nap = {0, 10000};
                nanosleep(&nap, NULL);
            }
            if (write(pfd[1], "x", 1) != 1)
                return 2;
        }
        void *r;
        pthread_join(t, &r);
        if (r != PTHREAD_CANCELED)
            return 3;
        fcntl(pfd[0], F_SETFL, O_NONBLOCK);
        char c;
        ssize_t left = read(pfd[0], &c, 1);
        if (got) {
            if (left != -1 || errno != EAGAIN)
                return 4; /* read twice? */
            after++;
        } else {
            if (left != 1)
                return 5; /* consumed but not returned: lost */
            before++;
        }
        close(pfd[0]);
        close(pfd[1]);
    }
    /* Both orders are expected over this many rounds; neither is required by the contract. */
    (void)before;
    (void)after;
    return 0;
}
