#include <stddef.h>
#include <stdio.h>

struct T {
  int a;
  char b;
  int c;
  double d;
};

int main(void) {
  if (offsetof(struct T, a) != 0)
    return 1;
  if (offsetof(struct T, b) != 4)
    return 2;
  if (offsetof(struct T, c) != 8)
    return 3;
  if (offsetof(struct T, d) != 16)
    return 4;
  puts("offsetof ok");
  return 0;
}
