#include <math.h>
#include <stdio.h>

static int calls;

static double one_with_side_effect(void) {
  calls = calls + 1;
  return 1.0;
}

int main(void) {
  float finite_f = 1.0F;
  double finite_d = 2.0;
  long double finite_l = 3.0L;
  double nan_value = NAN;

  if (fpclassify(0.0) != FP_ZERO)
    return 1;
  if (fpclassify(INFINITY) != FP_INFINITE)
    return 2;
  if (fpclassify(NAN) != FP_NAN)
    return 3;
  if (!isfinite(finite_f) || !isfinite(finite_d) || !isfinite(finite_l))
    return 4;
  if (!isnormal(finite_d))
    return 5;
  if (!isinf(HUGE_VAL) || !isinf(HUGE_VALF) || !isinf(HUGE_VALL))
    return 6;
  if (!isnan(nan_value))
    return 7;
  if (!signbit(-0.0))
    return 8;
  if (!isgreater(3.0, 2.0) || !isgreaterequal(3.0, 3.0))
    return 9;
  if (!isless(2.0, 3.0) || !islessequal(3.0, 3.0))
    return 10;
  if (!islessgreater(2.0, 3.0))
    return 11;
  if (!isunordered(nan_value, 1.0))
    return 12;
  if (isgreater(nan_value, 1.0) || isless(nan_value, 1.0))
    return 13;
  if (!isfinite(one_with_side_effect()))
    return 14;
  if (calls != 1)
    return 15;

  puts("math classification ok");
  return 0;
}
