// A static C++ program that throws and catches with no call into the C library on the way
// (crates/core/tests/cxx_throw_products.rs, STATIC-CXX-THROW-PTHREAD-ONCE). Without
// WITH_THREAD the program names no thread function at all, so the thread functions its link
// takes are the ones the C library's own members and the compiler's runtime pull in. With
// WITH_THREAD a second thread (pthread_create) throws and catches too.
// Prints "catches=<n> dtors=<n>" and exits 0 when every throw was caught with its destructors run.
#include <cstdio>
#ifdef WITH_THREAD
#include <pthread.h>
#endif

struct Thrown {
    int code;
};

static int dtors;
struct Guard {
    ~Guard() { __atomic_add_fetch(&dtors, 1, __ATOMIC_RELAXED); }
};

__attribute__((noinline)) static int depth(int n)
{
    Guard g;
    if (n == 0)
        throw Thrown{7};
    return depth(n - 1) + 1;
}

static int catches;
static void body()
{
    try {
        depth(3);
    } catch (const Thrown &t) {
        if (t.code == 7)
            __atomic_add_fetch(&catches, 1, __ATOMIC_RELAXED);
    }
}

#ifdef WITH_THREAD
static void *thread_main(void *)
{
    body();
    return nullptr;
}
#endif

int main()
{
    int want = 1;
#ifdef WITH_THREAD
    pthread_t t;
    if (pthread_create(&t, nullptr, thread_main, nullptr) != 0 || pthread_join(t, nullptr) != 0) {
        std::printf("thread failed\n");
        return 2;
    }
    want = 2;
#endif
    body();
    std::printf("catches=%d dtors=%d\n", catches, dtors);
    return catches == want && dtors == 4 * want ? 0 : 1;
}
