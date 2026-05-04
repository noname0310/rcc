#include <stdarg.h>

static int consume(va_list ap) {
  return va_arg(ap, int);
}

static int f(int tag, ...) {
  va_list ap0;
  va_list ap2;
  va_list *ap_array[3];
  va_list **ap_ptr = ap_array;
  int first;
  int second;

  ap_array[0] = &ap0;
  ap_array[1] = 0;
  ap_array[2] = &ap2;

  va_start(*ap_array[0], tag);
  first = consume(**ap_ptr);
  va_end(**ap_ptr++);

  ap_ptr++;

  va_start(*ap_array[2], tag);
  second = consume(**ap_ptr);
  va_end(**ap_ptr);

  if (*ap_ptr == 0)
    return 3;
  if (first != 11)
    return 1;
  if (second != 11)
    return 2;
  return 0;
}

int main(void) {
  return f(0, 11);
}
