struct S {
  unsigned int s1;
  unsigned int s2;
};

struct T {
  unsigned int t1;
  struct S t2;
};

struct U {
  unsigned short u1;
  unsigned short u2;
};

struct V {
  struct U v1;
  struct T v2;
};

union UView {
  char b[64];
  struct V v;
};

union UView u;

int main(void) {
  char *b;
  struct T *d;

  u.v.v2.t2.s1 = 8192;
  b = u.b;
  d = (struct T *) (b + __builtin_offsetof(struct V, v2));

  if ((void *) d != (void *) &u.v.v2)
    return 1;
  if (d->t2.s1 != 8192)
    return 2;
  return 0;
}
