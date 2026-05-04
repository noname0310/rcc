struct Pair {
  unsigned int a;
  unsigned int b;
};

static int take_pair(struct Pair p) {
  if (p.a != 3)
    return 1;
  if (p.b != 4)
    return 2;
  return 0;
}

int main(void) {
  struct Pair p;
  p.a = 3;
  p.b = 4;
  return take_pair(p);
}
