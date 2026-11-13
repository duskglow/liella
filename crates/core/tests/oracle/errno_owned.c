/* D-X.58/D-X.60 (s2_1_c_0): in a program the runtime started, a thread's errno is its thread
 * block's word at 0x80 (rt::tls::ERRNO_OFFSET): __errno_location() is the thread pointer
 * (%fs:0) plus 0x80, in the initial thread and in a thread pthread_create made, and a store to
 * errno lands there. Linked statically against the products (liella-crt1.o, libc.a): never
 * built against another C library, whose block uses 0x80 for its own data.
 * Exit 0: both threads agree; 1: the initial thread; 2: the created thread; 3: no thread. */
#include <errno.h>
#include <pthread.h>
#include <stdint.h>

static uintptr_t tp(void)
{
    uintptr_t v;
    __asm__ volatile("movq %%fs:0, %0" : "=r"(v));
    return v;
}

static int owned_errno(int v)
{
    errno = v;
    uintptr_t at = tp() + 0x80;
    return (uintptr_t)__errno_location() == at && *(volatile int *)at == v;
}

static void *body(void *arg)
{
    (void)arg;
    return (void *)(intptr_t)!owned_errno(92);
}

int main(void)
{
    if (!owned_errno(91))
        return 1;
    pthread_t t;
    void *r;
    if (pthread_create(&t, 0, body, 0) != 0 || pthread_join(t, &r) != 0)
        return 3;
    return r ? 2 : 0;
}
