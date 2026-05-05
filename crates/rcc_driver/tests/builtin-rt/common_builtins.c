#include <stdint.h>
#include <stdio.h>

#define STATIC_ASSERT(name, cond) typedef char static_assert_##name[(cond) ? 1 : -1]

STATIC_ASSERT(type_compat_int, __builtin_types_compatible_p(int, int));
STATIC_ASSERT(type_compat_negative, !__builtin_types_compatible_p(int, long));
STATIC_ASSERT(constant_p_literal, __builtin_constant_p(123));

int main(void) {
  uint32_t value = __builtin_bswap32(0x01020304U);
  if (value != 0x04030201U)
    return 1;
  if (__builtin_expect(value == 0x04030201U, 1) == 0)
    return 2;
  puts("builtins ok");
  return 0;
}
