#include <stdio.h>
int main(void) { char b[4096]; size_t n; while ((n = fread(b, 1, sizeof b, stdin)) > 0) fwrite(b, 1, n, stdout); return 0; }
