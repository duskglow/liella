/* Spec §1.2 contract clause 5: __cxa_finalize(dso) runs that object's destructors now, once;
 * the others run at exit. */
#include <stdlib.h>
#include <unistd.h>

extern int __cxa_atexit(void (*fn)(void *), void *arg, void *dso);
extern void __cxa_finalize(void *dso);

static int dso1, dso2;
static int ran1, ran2;
static void f1(void *arg) { (void)arg; ran1++; }
static void f2(void *arg) { (void)arg; ran2++; }

__attribute__((destructor)) static void last(void)
{
    _exit(ran1 == 1 && ran2 == 1 ? 0 : 80 + ran1 * 10 + ran2);
}

int main(void)
{
    if (__cxa_atexit(f1, NULL, &dso1) != 0 || __cxa_atexit(f2, NULL, &dso2) != 0)
        return 1;
    __cxa_finalize(&dso1);
    if (ran1 != 1 || ran2 != 0)
        return 2;
    return 0;
}
