/* Test tool for conformance/run.sh, built with the host C library: run a program with exactly
 * the environment entries given, in order and with duplicates (`env -i` would merge repeated
 * names, but `execve` can pass them, and spec §1.3 says what a program then sees).
 *
 *   execenv NAME=VALUE... -- program arguments... */
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv)
{
    int sep = 1;
    while (sep < argc && strcmp(argv[sep], "--") != 0)
        sep++;
    if (sep + 1 >= argc) {
        fprintf(stderr, "usage: execenv NAME=VALUE... -- program [args...]\n");
        return 127;
    }
    char *envp[256];
    int n = 0;
    for (int i = 1; i < sep && n < 255; i++)
        envp[n++] = argv[i];
    envp[n] = NULL;
    argv[sep] = NULL;
    execve(argv[sep + 1], argv + sep + 1, envp);
    perror("execenv: execve");
    return 127;
}
