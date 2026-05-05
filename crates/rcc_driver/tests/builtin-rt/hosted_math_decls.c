#include <math.h>
#include <stdio.h>

static int near(double lhs, double rhs) {
  double diff = fabs(lhs - rhs);
  return diff < 0.000001;
}

int main(void) {
  int exponent = 0;
  int quotient = 0;
  double integral = 0.0;
  double fraction = 0.0;
  float f = sqrtf(9.0f);
  long double ld = sqrtl(16.0L);

  if (!near(atan2(1.0, 1.0), 0.7853981633974483))
    return 1;
  if (!near(exp2(3.0), 8.0))
    return 2;
  if (!near(log1p(1.0), log(2.0)))
    return 3;
  if (!near(ldexp(0.5, 4), 8.0))
    return 4;
  if (frexp(8.0, &exponent) < 0.49 || exponent != 4)
    return 5;
  if (ilogb(8.0) != 3)
    return 6;
  fraction = modf(3.25, &integral);
  if (!near(integral, 3.0) || !near(fraction, 0.25))
    return 7;
  if (!near(scalbn(1.0, 3), 8.0))
    return 8;
  if (!near(cbrt(27.0), 3.0))
    return 9;
  if (!near(hypot(3.0, 4.0), 5.0))
    return 10;
  if (!near(erf(0.0), 0.0))
    return 11;
  if (!near(tgamma(5.0), 24.0))
    return 12;
  if (lrint(2.0) != 2 || llround(2.4) != 2)
    return 13;
  if (!near(trunc(2.9), 2.0))
    return 14;
  if (!near(fmod(5.5, 2.0), 1.5))
    return 15;
  if (!near(remainder(5.0, 2.0), 1.0))
    return 16;
  if (!near(remquo(5.0, 2.0, &quotient), 1.0))
    return 17;
  if (!near(copysign(2.0, -1.0), -2.0))
    return 18;
  if (!near(nextafter(1.0, 2.0) > 1.0 ? 1.0 : 0.0, 1.0))
    return 19;
  if (!near(fdim(5.0, 2.0), 3.0))
    return 20;
  if (!near(fmax(2.0, 3.0), 3.0) || !near(fmin(2.0, 3.0), 2.0))
    return 21;
  if (!near(fma(2.0, 3.0, 4.0), 10.0))
    return 22;
  if ((int)f != 3)
    return 23;
  if ((int)ld != 4)
    return 24;

  puts("hosted math ok");
  return 0;
}
