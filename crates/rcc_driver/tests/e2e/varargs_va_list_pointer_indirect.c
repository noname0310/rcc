#include <stdarg.h>

static int consume(va_list *ap) {
  int i;
  for (i = 0; i < 10; i = i + 1) {
    if (va_arg(*ap, int) != i)
      return 1;
  }
  if (va_arg(*ap, double) != 0.5)
    return 2;
  return 0;
}

static int f(int tag, ...) {
  va_list ap;
  int r;
  va_start(ap, tag);
  r = consume(&ap);
  va_end(ap);
  return r;
}

int main(void) {
  return f(100, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0.5);
}
