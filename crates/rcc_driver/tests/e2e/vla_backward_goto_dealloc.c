void *volatile sink;

int before_label_loop(int limit) {
  int n = 0;
lab:;
  int x[n % 1000 + 1];
  x[0] = 1;
  x[n % 1000] = 2;
  sink = x;
  n++;
  if (n < limit)
    goto lab;
  return n;
}

int inner_label_loop(int limit) {
  int n = 0;
  if (0) {
  lab:;
  }
  int x[n % 1000 + 1];
  x[0] = 1;
  x[n % 1000] = 2;
  sink = x;
  n++;
  if (n < limit)
    goto lab;
  return n;
}

int main(void) {
  if (before_label_loop(20000) != 20000)
    return 1;
  if (inner_label_loop(20000) != 20000)
    return 2;
  return 0;
}
