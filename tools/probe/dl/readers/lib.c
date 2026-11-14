/* lib.c - one helper object for tools/probe/dl/readers (DL-16c). NAME is set by -D. */
#define CAT2(a, b) a##b
#define CAT(a, b) CAT2(a, b)
int CAT(fn_, NAME)(void) { return 7; }
