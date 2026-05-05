#include <stdio.h>
#include <tgmath.h>

int main(void) {
  float sf = sqrt(4.0F);
  double sd = sqrt(9.0);
  long double sl = sqrt(16.0L);

  double complex z = 2.0 + 3.0 * I;
  double complex root = sqrt(-1.0 + 0.0 * I);
  float complex cf = 5.0F + I;

  if (fabsf(sf - 2.0F) > 0.0001F)
    return 1;
  if (fabs(sd - 3.0) > 0.0000001)
    return 2;
  if (fabsl(sl - 4.0L) > 0.0000001L)
    return 3;
  if (fabs(cimag(root) - 1.0) > 0.0000001)
    return 4;
  if (fabs(fabs(z) - 3.605551275463989) > 0.0000001)
    return 5;
  if (fabsf(creal(cf) - 5.0F) > 0.0001F)
    return 6;

  puts("tgmath ok");
  return 0;
}
