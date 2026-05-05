#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <limits.h>
#include <stdio.h>

#define STATIC_ASSERT(name, cond) typedef char static_assert_##name[(cond) ? 1 : -1]

STATIC_ASSERT(int_is_32_bits, sizeof(int32_t) == 4);
STATIC_ASSERT(uint64_is_8_bytes, sizeof(uint64_t) == 8);
STATIC_ASSERT(size_t_matches_pointer, sizeof(size_t) == sizeof(void *));
STATIC_ASSERT(int_max_value, INT_MAX == 2147483647);
STATIC_ASSERT(char_bit_value, CHAR_BIT == 8);

int main(void) {
  bool ok = true;
  void *p = NULL;
  if (!ok)
    return 1;
  if (p != NULL)
    return 2;
  puts("headers ok");
  return 0;
}
