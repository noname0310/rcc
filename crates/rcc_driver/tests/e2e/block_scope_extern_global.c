int v = 3;

int main(void) {
  int v = 4;
  {
    extern int v;
    if (v != 3)
      return 1;
  }
  if (v != 4)
    return 2;
  return 0;
}
