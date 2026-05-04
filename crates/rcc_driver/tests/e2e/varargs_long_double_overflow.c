#include <stdarg.h>

int f(int n, ...) {
  va_list ap;
  va_start(ap, n);
  if (va_arg(ap, long double) != 3.141592L)
    return 1;
  if (va_arg(ap, long double) != 2.71827L)
    return 2;
  va_end(ap);
  return 0;
}

int main(void) {
  return f(2, 3.141592L, 2.71827L);
}
