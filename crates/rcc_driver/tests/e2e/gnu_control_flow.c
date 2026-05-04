int main(void) {
    int v = 0;
    switch (7) {
    case 0 ... 5:
        v = 1;
        break;
    case 6 ... 20:
        v = 2;
        break;
    }
    if (v != 2)
        return 1;

    void *p = &&direct;
    goto *p;
    return 2;
direct:
    ;

    static void *table[] = { &&first, &&second };
    int i = 0;
    goto *table[1];
first:
    i = 10;
second:
    i = i + 1;
    if (i != 1)
        return 3;

    int lhs = 0;
    int rhs = 0;
    (lhs = 5, rhs) = 6;
    if (lhs != 5 || rhs != 6)
        return 4;

    return 0;
}
