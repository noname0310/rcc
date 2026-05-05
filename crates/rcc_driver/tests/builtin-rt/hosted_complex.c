#include <complex.h>
#include <stdio.h>

int main(void) {
  float complex unit = I;
  double complex z = 2.0 + 3.0 * I;
  double complex square = I * I;
  double complex reflected = conj(z);

  if (cimagf(unit) != 1.0F)
    return 1;
  if (creal(z) != 2.0 || cimag(z) != 3.0)
    return 2;
  if (creal(square) != -1.0 || cimag(square) != 0.0)
    return 3;
  if (creal(reflected) != 2.0 || cimag(reflected) != -3.0)
    return 4;

  puts("complex ok");
  return 0;
}
