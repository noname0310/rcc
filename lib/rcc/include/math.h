#ifndef __RCC_MATH_H
#define __RCC_MATH_H

typedef float float_t;
typedef double double_t;

#define FP_NAN 0
#define FP_INFINITE 1
#define FP_ZERO 2
#define FP_SUBNORMAL 3
#define FP_NORMAL 4

#define HUGE_VAL 1e999
#define HUGE_VALF 1e999F
#define HUGE_VALL 1e999L
#define INFINITY HUGE_VALF
#define NAN (0.0F / 0.0F)

extern int __fpclassify(double);
extern int __fpclassifyf(float);
extern int __fpclassifyl(long double);
extern int __signbit(double);
extern int __signbitf(float);
extern int __signbitl(long double);
extern int finite(double);
extern int finitef(float);
extern int finitel(long double);
extern int isinf(double);
extern int isinff(float);
extern int isinfl(long double);
extern int isnan(double);
extern int isnanf(float);
extern int isnanl(long double);

#define __rcc_math_select(x, double_fn, float_fn, long_double_fn)                                 \
    (sizeof(x) == sizeof(float)                                                                  \
            ? float_fn((float)(x))                                                               \
            : (sizeof(x) == sizeof(long double) ? long_double_fn((long double)(x))                \
                                                : double_fn((double)(x))))

#define fpclassify(x) __rcc_math_select((x), __fpclassify, __fpclassifyf, __fpclassifyl)
#define isfinite(x) __rcc_math_select((x), finite, finitef, finitel)
#define isinf(x) __rcc_math_select((x), isinf, isinff, isinfl)
#define isnan(x) __rcc_math_select((x), isnan, isnanf, isnanl)
#define isnormal(x) (fpclassify(x) == FP_NORMAL)
#define signbit(x) __rcc_math_select((x), __signbit, __signbitf, __signbitl)

#define isgreater(x, y) ((x) > (y))
#define isgreaterequal(x, y) ((x) >= (y))
#define isless(x, y) ((x) < (y))
#define islessequal(x, y) ((x) <= (y))
#define islessgreater(x, y) (((x) < (y)) || ((x) > (y)))
#define isunordered(x, y) (isnan(x) || isnan(y))

extern double acos(double);
extern double asin(double);
extern double atan(double);
extern double atan2(double, double);
extern double cos(double);
extern double sin(double);
extern double tan(double);

extern double acosh(double);
extern double asinh(double);
extern double atanh(double);
extern double cosh(double);
extern double sinh(double);
extern double tanh(double);

extern double exp(double);
extern double exp2(double);
extern double expm1(double);
extern double frexp(double, int *);
extern int ilogb(double);
extern double ldexp(double, int);
extern double log(double);
extern double log10(double);
extern double log1p(double);
extern double log2(double);
extern double logb(double);
extern double modf(double, double *);
extern double scalbn(double, int);
extern double scalbln(double, long);

extern double cbrt(double);
extern double fabs(double);
extern double hypot(double, double);
extern double pow(double, double);
extern double sqrt(double);

extern double erf(double);
extern double erfc(double);
extern double lgamma(double);
extern double tgamma(double);

extern double ceil(double);
extern double floor(double);
extern double nearbyint(double);
extern double rint(double);
extern long lrint(double);
extern long long llrint(double);
extern double round(double);
extern long lround(double);
extern long long llround(double);
extern double trunc(double);

extern double fmod(double, double);
extern double remainder(double, double);
extern double remquo(double, double, int *);

extern double copysign(double, double);
extern double nan(const char *);
extern double nextafter(double, double);
extern double nexttoward(double, long double);

extern double fdim(double, double);
extern double fmax(double, double);
extern double fmin(double, double);
extern double fma(double, double, double);

extern float acosf(float);
extern float asinf(float);
extern float atanf(float);
extern float atan2f(float, float);
extern float cosf(float);
extern float sinf(float);
extern float tanf(float);

extern float acoshf(float);
extern float asinhf(float);
extern float atanhf(float);
extern float coshf(float);
extern float sinhf(float);
extern float tanhf(float);

extern float expf(float);
extern float exp2f(float);
extern float expm1f(float);
extern float frexpf(float, int *);
extern int ilogbf(float);
extern float ldexpf(float, int);
extern float logf(float);
extern float log10f(float);
extern float log1pf(float);
extern float log2f(float);
extern float logbf(float);
extern float modff(float, float *);
extern float scalbnf(float, int);
extern float scalblnf(float, long);

extern float cbrtf(float);
extern float fabsf(float);
extern float hypotf(float, float);
extern float powf(float, float);
extern float sqrtf(float);

extern float erff(float);
extern float erfcf(float);
extern float lgammaf(float);
extern float tgammaf(float);

extern float ceilf(float);
extern float floorf(float);
extern float nearbyintf(float);
extern float rintf(float);
extern long lrintf(float);
extern long long llrintf(float);
extern float roundf(float);
extern long lroundf(float);
extern long long llroundf(float);
extern float truncf(float);

extern float fmodf(float, float);
extern float remainderf(float, float);
extern float remquof(float, float, int *);

extern float copysignf(float, float);
extern float nanf(const char *);
extern float nextafterf(float, float);
extern float nexttowardf(float, long double);

extern float fdimf(float, float);
extern float fmaxf(float, float);
extern float fminf(float, float);
extern float fmaf(float, float, float);

extern long double acosl(long double);
extern long double asinl(long double);
extern long double atanl(long double);
extern long double atan2l(long double, long double);
extern long double cosl(long double);
extern long double sinl(long double);
extern long double tanl(long double);

extern long double acoshl(long double);
extern long double asinhl(long double);
extern long double atanhl(long double);
extern long double coshl(long double);
extern long double sinhl(long double);
extern long double tanhl(long double);

extern long double expl(long double);
extern long double exp2l(long double);
extern long double expm1l(long double);
extern long double frexpl(long double, int *);
extern int ilogbl(long double);
extern long double ldexpl(long double, int);
extern long double logl(long double);
extern long double log10l(long double);
extern long double log1pl(long double);
extern long double log2l(long double);
extern long double logbl(long double);
extern long double modfl(long double, long double *);
extern long double scalbnl(long double, int);
extern long double scalblnl(long double, long);

extern long double cbrtl(long double);
extern long double fabsl(long double);
extern long double hypotl(long double, long double);
extern long double powl(long double, long double);
extern long double sqrtl(long double);

extern long double erfl(long double);
extern long double erfcl(long double);
extern long double lgammal(long double);
extern long double tgammal(long double);

extern long double ceill(long double);
extern long double floorl(long double);
extern long double nearbyintl(long double);
extern long double rintl(long double);
extern long lrintl(long double);
extern long long llrintl(long double);
extern long double roundl(long double);
extern long lroundl(long double);
extern long long llroundl(long double);
extern long double truncl(long double);

extern long double fmodl(long double, long double);
extern long double remainderl(long double, long double);
extern long double remquol(long double, long double, int *);

extern long double copysignl(long double, long double);
extern long double nanl(const char *);
extern long double nextafterl(long double, long double);
extern long double nexttowardl(long double, long double);

extern long double fdiml(long double, long double);
extern long double fmaxl(long double, long double);
extern long double fminl(long double, long double);
extern long double fmal(long double, long double, long double);

#endif
