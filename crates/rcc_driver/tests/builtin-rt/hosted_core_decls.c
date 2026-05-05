#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int compare_ints(const void *lhs, const void *rhs) {
  int a = *(const int *)lhs;
  int b = *(const int *)rhs;
  return (a > b) - (a < b);
}

int main(void) {
  char buffer[64];
  char moved[16] = "abcdef";
  char token_input[16] = "a,b";
  char *end = 0;
  int values[4] = {4, 1, 3, 2};
  int key = 3;
  int *hit = 0;
  div_t qr;
  ldiv_t lqr;
  lldiv_t llqr;
  double parsed = strtod("12.5x", &end);
  long signed_value = strtol("-9", 0, 10);
  unsigned long unsigned_value = strtoul("17", 0, 10);
  int scanned = 0;

  if (sizeof(fpos_t) == 0)
    return 17;
  if ((int)(parsed * 10.0) != 125 || *end != 'x')
    return 1;
  if (signed_value != -9 || unsigned_value != 17UL)
    return 2;
  if (snprintf(buffer, sizeof(buffer), "%ld:%lu", signed_value, unsigned_value) < 0)
    return 3;
  if (sscanf(buffer, "%d", &scanned) != 1 || scanned != -9)
    return 4;

  memmove(moved + 1, moved, 3);
  if (memcmp(moved, "aabcef", 6) != 0)
    return 5;
  if (memchr(moved, 'c', sizeof(moved)) == 0)
    return 6;
  if (strstr("hello hosted world", "hosted") == 0)
    return 7;
  if (strspn("abc123", "abc") != 3)
    return 8;
  if (strcspn("abc123", "0123456789") != 3)
    return 9;
  if (strpbrk("abc123", "xyz3") == 0)
    return 10;
  if (strtok(token_input, ",") == 0 || strtok(0, ",") == 0)
    return 11;
  if (strerror(0) == 0)
    return 12;

  qsort(values, 4, sizeof(values[0]), compare_ints);
  hit = (int *)bsearch(&key, values, 4, sizeof(values[0]), compare_ints);
  if (hit == 0 || *hit != 3)
    return 13;

  qr = div(7, 3);
  lqr = ldiv(9L, 4L);
  llqr = lldiv(11LL, 5LL);
  if (abs(-3) != 3 || labs(-4L) != 4L || llabs(-5LL) != 5LL)
    return 14;
  if (qr.quot != 2 || qr.rem != 1 || lqr.quot != 2 || lqr.rem != 1)
    return 15;
  if (llqr.quot != 2 || llqr.rem != 1)
    return 16;

  puts("hosted core ok");
  return 0;
}
