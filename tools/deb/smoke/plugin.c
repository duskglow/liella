/* The plugin that dlopen.c loads (built with -shared -fPIC). */
int plugin_answer(int x) {
    return x * 2 + 1;
}
