/* A dynamic program that needs the host's libm and libc: verdict_cache checks that the key of a
   C-oracle sweep holds every shared object such a program loads (D-10.51). */
#include <math.h>
#include <stdio.h>

int main(void) {
    volatile double x = 2.0;
    printf("%a\n", cbrt(x));
    return 0;
}
