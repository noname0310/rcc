int strcmp(char *, char *);
int printf(char *, ...);

char *f(void) {
    return __func__;
}

char *g(void) {
    return __FUNCTION__;
}

int main(void) {
    if (sizeof(__func__) != 5)
        return 1;
    if (strcmp(__func__, "main"))
        return 2;
    if (strcmp(f(), "f"))
        return 3;
    if (strcmp(g(), "g"))
        return 4;
    printf("OK\n");
    return 0;
}
