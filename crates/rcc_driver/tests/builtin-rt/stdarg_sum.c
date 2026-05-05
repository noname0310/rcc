#include <stdarg.h>
#include <stdio.h>

int sum_ints(int count, ...) {
  va_list ap;
  int total = 0;
  va_start(ap, count);
  for (int i = 0; i < count; i = i + 1)
    total = total + va_arg(ap, int);
  va_end(ap);
  return total;
}

int main(void) {
  if (sum_ints(5, 1, 2, 3, 4, 5) != 15)
    return 1;
  puts("varargs ok");
  return 0;
}
