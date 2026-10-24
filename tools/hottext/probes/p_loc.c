#include <stdio.h>
#include <locale.h>
#include <stdlib.h>
int main(void) { setlocale(LC_ALL, ""); char *p = malloc(100); printf("%s %d\n", p ? "a" : "b", 5); free(p); return 0; }
