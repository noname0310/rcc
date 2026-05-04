int failed;

void foo(int a, int b[a++], int c, int d[c++]) {
  b[0] = a;
  d[0] = c;
  if (a != 4)
    failed = 1;
  if (c != 7)
    failed = 2;
}

int main(void) {
  int b[8];
  int d[8];
  foo(3, b, 6, d);
  if (b[0] != 4)
    return 3;
  if (d[0] != 7)
    return 4;
  return failed;
}
