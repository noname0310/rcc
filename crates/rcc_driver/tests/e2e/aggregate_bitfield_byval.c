typedef unsigned long size_t;
extern void *memcpy(void *, const void *, size_t);

typedef struct {
  unsigned a : 16;
  unsigned b : 8;
  unsigned c : 8;
  long d[4];
} S;

typedef struct {
  long r[3];
} U;

S s = { 26, 0, 0, { 0, 21, 22, 23 } };

int check(U u) {
  return !(u.r[0] == 21 && u.r[1] == 22 && u.r[2] == 23);
}

int main(void) {
  U u, v;
  memcpy(&u, &s.d[1], sizeof u);
  v = u;
  return check(v);
}
