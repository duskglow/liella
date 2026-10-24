/* Spec §22.5: an exception that leaves a pthread_once/call_once routine (a C++ std::call_once
 * routine that throws; libstdc++ builds call_once on pthread_once) passes through the call to
 * the caller's handler, and the object is new again: a later call runs the routine, and a
 * thread that waited during the throwing run runs it itself. Nested objects are both reset. An
 * exception the search phase finds no handler for leaves the object alone. The exception is
 * raised with the compiler's unwinder (<unwind.h>, libgcc_eh), as a C++ throw is; the
 * "handler" is a stop function that jumps back to the frame that called pthread_once.
 * The same source is the glibc probe (cc -O1 s22_once_unwind.c -lgcc_s -pthread: the shared
 * unwinder, as a C++ program has; it passes on glibc 2.39). An alarm turns a hang (an object
 * left running, so a later call waits forever) into a failure (SIGALRM) instead of a stuck suite. */
/* LINK-UNWIND: yes */
#define _GNU_SOURCE
#include <pthread.h>
#include <setjmp.h>
#include <stdint.h>
#include <string.h>
#include <threads.h>
#include <time.h>
#include <unwind.h>
#include <unistd.h>

static jmp_buf catcher;
static uintptr_t catcher_frame; /* a local of the frame that catches */
static struct _Unwind_Exception exc;
static int caught;

static _Unwind_Reason_Code stop(int version, _Unwind_Action actions, _Unwind_Exception_Class cls,
                                struct _Unwind_Exception *e, struct _Unwind_Context *ctx, void *arg)
{
    (void)version; (void)cls; (void)e; (void)arg;
    if (actions & _UA_END_OF_STACK)
        return _URC_FATAL_PHASE2_ERROR;
    /* The catching frame: every frame below it (pthread_once's included) is unwound. */
    if (_Unwind_GetCFA(ctx) > catcher_frame)
        longjmp(catcher, 1);
    return _URC_NO_REASON;
}

static void throw_now(void)
{
    memset(&exc, 0, sizeof exc);
    exc.exception_class = 0x4c49454c4c410000ULL; /* not C++'s: no handler but ours */
    _Unwind_ForcedUnwind(&exc, stop, 0);
}

/* "try { pthread_once(o, init); } catch (...)": 1 when the exception arrived here. */
static __attribute__((noinline)) int try_once(pthread_once_t *o, void (*init)(void))
{
    volatile int marker = 0;
    catcher_frame = (uintptr_t)&marker;
    if (setjmp(catcher) != 0)
        return 1;
    if (pthread_once(o, init) != 0)
        return 2;
    return marker;
}

static __attribute__((noinline)) int try_call_once(once_flag *f, void (*init)(void))
{
    volatile int marker = 0;
    catcher_frame = (uintptr_t)&marker;
    if (setjmp(catcher) != 0)
        return 1;
    call_once(f, init);
    return marker;
}

/* 1: throw on the first run, complete on the second. */
static pthread_once_t o1 = PTHREAD_ONCE_INIT;
static int runs1;
static void init1(void)
{
    if (++runs1 == 1)
        throw_now();
}

/* 2: a waiter during the throwing run retries and runs the routine itself. */
static pthread_once_t o2 = PTHREAD_ONCE_INIT;
static volatile int runs2_waiter, waiter_ret = -1;
static pthread_t waiter;
static void init2_waiter(void) { runs2_waiter++; }
static void *wait_on_o2(void *arg)
{
    (void)arg;
    waiter_ret = pthread_once(&o2, init2_waiter);
    return 0;
}
static void init2_throw(void)
{
    if (pthread_create(&waiter, 0, wait_on_o2, 0) != 0)
        return;
    struct timespec t = {0, 100000000}; /* the waiter blocks in pthread_once meanwhile */
    nanosleep(&t, 0);
    throw_now();
}

/* 3: nested: the inner routine throws through both objects' calls. */
static pthread_once_t o3 = PTHREAD_ONCE_INIT, o4 = PTHREAD_ONCE_INIT;
static int runs3, runs4;
static void init4(void)
{
    if (++runs4 == 1)
        throw_now();
}
static void init3(void)
{
    runs3++;
    pthread_once(&o4, init4);
}

/* 4: call_once, as 1. */
static once_flag f5 = ONCE_FLAG_INIT;
static int runs5;
static void init5(void)
{
    if (++runs5 == 1)
        throw_now();
}

/* 5: a raise no handler takes: the search phase ends, nothing is reset, the routine goes on. */
static pthread_once_t o6 = PTHREAD_ONCE_INIT;
static int runs6, raise6 = -1;
static void init6(void)
{
    runs6++;
    memset(&exc, 0, sizeof exc);
    exc.exception_class = 0x4c49454c4c410001ULL;
    raise6 = _Unwind_RaiseException(&exc);
}

/* 6: plain C, no exception. */
static pthread_once_t o7 = PTHREAD_ONCE_INIT;
static int runs7;
static void init7(void) { runs7++; }

int main(void)
{
    alarm(20);

    if (try_once(&o1, init1) != 1 || runs1 != 1)
        return 1;
    if (try_once(&o1, init1) != 0 || runs1 != 2)
        return 2;
    if (pthread_once(&o1, init1) != 0 || runs1 != 2)
        return 3;

    if (try_once(&o2, init2_throw) != 1)
        return 4;
    if (pthread_join(waiter, 0) != 0 || waiter_ret != 0 || runs2_waiter != 1)
        return 5;
    if (pthread_once(&o2, init2_throw) != 0 || runs2_waiter != 1)
        return 6;

    if (try_once(&o3, init3) != 1 || runs3 != 1 || runs4 != 1)
        return 7;
    if (try_once(&o3, init3) != 0 || runs3 != 2 || runs4 != 2)
        return 8;
    if (pthread_once(&o4, init4) != 0 || runs4 != 2)
        return 9;

    if (try_call_once(&f5, init5) != 1 || runs5 != 1)
        return 10;
    if (try_call_once(&f5, init5) != 0 || runs5 != 2)
        return 11;

    if (pthread_once(&o6, init6) != 0 || runs6 != 1 || raise6 != _URC_END_OF_STACK)
        return 12;
    if (pthread_once(&o6, init6) != 0 || runs6 != 1)
        return 13;

    if (pthread_once(&o7, init7) != 0 || pthread_once(&o7, init7) != 0 || runs7 != 1)
        return 14;
    return 0;
}
