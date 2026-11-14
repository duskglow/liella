/* Smoke program for the reference packages: dlopen the plugin named by argv[1], call it. */
#include <dlfcn.h>
#include <stdio.h>

int main(int argc, char **argv) {
    if (argc < 2) {
        puts("dlopen: usage: dlopen PLUGIN");
        return 2;
    }
    void *h = dlopen(argv[1], RTLD_NOW);
    if (h == NULL) {
        printf("dlopen: %s\n", dlerror());
        return 1;
    }
    int (*f)(int) = (int (*)(int))dlsym(h, "plugin_answer");
    if (f == NULL) {
        printf("dlsym: %s\n", dlerror());
        return 1;
    }
    int r = f(20);
    printf("dlopen: plugin_answer(20) = %d\n", r);
    dlclose(h);
    return r == 41 ? 0 : 1;
}
