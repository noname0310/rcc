#include <fenv.h>
#include <math.h>
#include <stdio.h>

int main(void) {
  fenv_t env;
  fexcept_t flag;
  int round = fegetround();
  int (*fegetenv_fn)(fenv_t *) = fegetenv;
  int (*feholdexcept_fn)(fenv_t *) = feholdexcept;
  int (*fesetenv_fn)(const fenv_t *) = fesetenv;
  int (*feupdateenv_fn)(const fenv_t *) = feupdateenv;

  if ((FE_ALL_EXCEPT & FE_DIVBYZERO) == 0)
    return 1;
  if (round != FE_TONEAREST && round != FE_DOWNWARD && round != FE_UPWARD &&
      round != FE_TOWARDZERO)
    return 2;
  if (sizeof(env) < 32 || sizeof(flag) != 2)
    return 3;
  if (feclearexcept(FE_ALL_EXCEPT) != 0)
    return 4;
  if (fegetexceptflag(&flag, FE_ALL_EXCEPT) != 0)
    return 5;
  if (fesetexceptflag(&flag, FE_ALL_EXCEPT) != 0)
    return 6;
  if (fesetround(round) != 0)
    return 7;
  if (feraiseexcept(0) != 0)
    return 8;
  if (fetestexcept(FE_ALL_EXCEPT) != 0)
    return 9;
  if (fegetenv_fn == 0 || feholdexcept_fn == 0 || fesetenv_fn == 0 || feupdateenv_fn == 0)
    return 10;

  puts("fenv ok");
  return 0;
}
