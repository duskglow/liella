/* C++17 source (LINK-CXX). Spec §22.5, plan S11: libstdc++ builds std::call_once on
 * pthread_once, and C++ requires that a callable that exits by an exception leaves the flag
 * unset, so a later call_once runs a callable again. Checked: the exception reaches the
 * caller's catch; a later call on the same flag runs its callable once; a thread that called
 * call_once on the flag while the throwing callable ran (or after it) runs its own callable,
 * and nothing hangs. An alarm turns a hang into a failure (SIGALRM) instead of a stuck suite. */
/* LINK-CXX: yes */
/* LINK-LIBM: yes */
/* LINK-UNWIND: yes */
#include <atomic>
#include <cstdio>
#include <mutex>
#include <stdexcept>
#include <thread>
#include <time.h>
#include <unistd.h>

static int failures;
static void check(bool ok, const char *what)
{
    if (!ok) {
        std::fprintf(stderr, "s22_once_cxx: FAIL %s\n", what);
        failures++;
    }
}

static void pause_ms(long ms)
{
    struct timespec ts = {0, ms * 1000000L};
    nanosleep(&ts, nullptr);
}

/* 1: throw, catch, retry on the same flag. */
static std::once_flag f1;
static int runs1_throw, runs1_ok;

/* 2: a waiter while the first callable throws. */
static std::once_flag f2;
static std::atomic<int> started2{0};
static std::atomic<int> runs2_waiter{0};

int main()
{
    alarm(20);

    bool caught = false;
    try {
        std::call_once(f1, [] { runs1_throw++; throw std::runtime_error("init failed"); });
    } catch (const std::runtime_error &) {
        caught = true;
    }
    check(caught, "the exception reaches the caller's catch");
    check(runs1_throw == 1, "the throwing callable ran once");
    std::call_once(f1, [] { runs1_ok++; });
    check(runs1_ok == 1, "a call after the throw runs its callable");
    std::call_once(f1, [] { runs1_ok++; });
    check(runs1_ok == 1, "a completed flag runs nothing more");

    std::thread thrower([] {
        try {
            std::call_once(f2, [] {
                started2.store(1);
                pause_ms(100); /* the waiter is (usually) blocked on the flag now */
                throw 42;
            });
        } catch (int v) {
            check(v == 42, "the thrower catches its value");
            return;
        }
        check(false, "the thrower's exception was lost");
    });
    while (started2.load() == 0)
        pause_ms(1);
    std::thread waiter([] { std::call_once(f2, [] { runs2_waiter++; }); });
    thrower.join();
    waiter.join();
    check(runs2_waiter.load() == 1, "the waiter runs its callable after the throw");
    std::call_once(f2, [] { runs2_waiter++; });
    check(runs2_waiter.load() == 1, "the flag is done after the waiter's run");

    if (failures == 0)
        std::puts("s22_once_cxx: ok");
    return failures == 0 ? 0 : 1;
}
