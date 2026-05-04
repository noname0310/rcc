#include <stdarg.h>

static int f(int n, ...) {
  va_list ap;
  int a;
  long long b;
  int c;
  long double d;
  int e;
  int g;
  long long h;
  int i;
  double j;

  va_start(ap, n);
  a = va_arg(ap, int);
  b = va_arg(ap, long long);
  c = va_arg(ap, int);
  d = va_arg(ap, long double);
  e = va_arg(ap, int);
  g = va_arg(ap, int);
  h = va_arg(ap, long long);
  i = va_arg(ap, int);
  j = va_arg(ap, double);
  va_end(ap);

  if (a != 10)
    return 1;
  if (b != 10000000000LL)
    return 2;
  if (c != 11)
    return 3;
  if (d != 3.14L)
    return 4;
  if (e != 12)
    return 5;
  if (g != 13)
    return 6;
  if (h != 20000000000LL)
    return 7;
  if (i != 14)
    return 8;
  if (j != 2.72)
    return 9;
  return 0;
}

int main(void) {
  return f(4, 10, 10000000000LL, 11, 3.14L, 12, 13, 20000000000LL, 14, 2.72);
}
