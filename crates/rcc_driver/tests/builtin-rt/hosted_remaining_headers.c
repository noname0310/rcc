#include <assert.h>
#include <errno.h>
#include <inttypes.h>
#include <locale.h>
#include <setjmp.h>
#include <signal.h>
#include <stdio.h>
#include <time.h>
#include <wctype.h>

#define STATIC_ASSERT(name, cond) typedef char static_assert_##name[(cond) ? 1 : -1]

STATIC_ASSERT(jmp_buf_exists, sizeof(jmp_buf) > 0);
STATIC_ASSERT(clock_t_exists, sizeof(clock_t) > 0);
STATIC_ASSERT(time_t_exists, sizeof(time_t) > 0);
STATIC_ASSERT(lconv_exists, sizeof(struct lconv) > 0);
STATIC_ASSERT(tm_exists, sizeof(struct tm) >= sizeof(int) * 9);

static int touch_signal_decls(void) {
  __rcc_sighandler_t handler = SIG_DFL;
  __rcc_sighandler_t (*signal_fn)(int, __rcc_sighandler_t) = signal;
  int (*raise_fn)(int) = raise;

  if (handler != SIG_DFL)
    return 1;
  if (signal_fn == 0 || raise_fn == 0)
    return 2;
  return 0;
}

static int touch_setjmp_decls(void) {
  int (*setjmp_fn)(jmp_buf) = setjmp;
  void (*longjmp_fn)(jmp_buf, int) = longjmp;

  if (setjmp_fn == 0 || longjmp_fn == 0)
    return 1;
  return 0;
}

static int touch_time_decls(void) {
  clock_t (*clock_fn)(void) = clock;
  double (*difftime_fn)(time_t, time_t) = difftime;
  time_t (*time_fn)(time_t *) = time;
  size_t (*strftime_fn)(char *, size_t, const char *, const struct tm *) = strftime;

  if (clock_fn == 0 || difftime_fn == 0 || time_fn == 0 || strftime_fn == 0)
    return 1;
  return 0;
}

static int touch_wctype_decls(void) {
  wint_t value = (wint_t)'A';
  wctype_t alpha = wctype("alpha");
  wctrans_t upper = wctrans("toupper");

  if (!iswalpha(value))
    return 1;
  if (!iswctype(value, alpha))
    return 2;
  if (towctrans((wint_t)'a', upper) != (wint_t)'A')
    return 3;
  if (towlower(value) != (wint_t)'a')
    return 4;
  if (towupper((wint_t)'z') != (wint_t)'Z')
    return 5;
  return 0;
}

int main(void) {
  char buf[64];
  char *end = 0;
  intmax_t parsed = strtoimax("123x", &end, 10);
  uintmax_t unsigned_parsed = strtoumax("456x", &end, 10);
  imaxdiv_t divided = imaxdiv((intmax_t)17, (intmax_t)5);
  struct lconv *lc;

  assert(1);
  errno = 0;

  if (EDOM == 0 || EILSEQ == 0 || ERANGE == 0)
    return 1;
  if (EINTR != 4 || EINVAL != 22 || ENOMEM != 12)
    return 13;
  if (errno != 0)
    return 2;
  if (parsed != (intmax_t)123 || unsigned_parsed != (uintmax_t)456)
    return 3;
  if (imaxabs((intmax_t)-7) != (intmax_t)7)
    return 4;
  if (divided.quot != (intmax_t)3 || divided.rem != (intmax_t)2)
    return 5;
  if (snprintf(buf, sizeof(buf), "%" PRIdMAX ":%" PRIuMAX, parsed, unsigned_parsed) < 0)
    return 6;

  if (setlocale(LC_ALL, "C") == 0)
    return 7;
  lc = localeconv();
  if (lc == 0 || lc->decimal_point == 0)
    return 8;

  if (touch_signal_decls() != 0)
    return 9;
  if (touch_setjmp_decls() != 0)
    return 10;
  if (touch_time_decls() != 0)
    return 11;
  if (touch_wctype_decls() != 0)
    return 12;

  puts("hosted remaining headers ok");
  return 0;
}
