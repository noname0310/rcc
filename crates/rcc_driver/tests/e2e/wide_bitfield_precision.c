struct s {
  unsigned long long u33:33;
  unsigned long long u40:40;
  unsigned long long u41:41;
};

struct one {
  unsigned long long b:40;
} x;

struct s a = { 0x100000, 0x100000, 0x100000 };
struct s b = { 0x100000000ULL, 0x100000000ULL, 0x100000000ULL };
struct s c = { 0x1FFFFFFFFULL, 0, 0 };

int main(void) {
  if (a.u33 * a.u33 != 0)
    return 1;
  if (a.u33 * a.u41 != 0x10000000000ULL)
    return 2;
  if (b.u33 + b.u33 != 0)
    return 3;
  if (a.u33 - b.u40 != 0xFF00100000ULL)
    return 4;
  if (++c.u33 != 0)
    return 5;
  if (--c.u40 != 0xFFFFFFFFFFULL)
    return 6;
  if (c.u41-- != 0)
    return 7;

  x.b = 0x0100;
  if (x.b << 32 != 0)
    return 8;
  x.b = 0x0100000001ULL;
  if ((x.b << 8) + (x.b >> 32) != 0x0000000101ULL)
    return 9;
  return 0;
}
