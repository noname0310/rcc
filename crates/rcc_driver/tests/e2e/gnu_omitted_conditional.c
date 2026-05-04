int main(void) {
  int i = 3;
  int a = (++i ?: 10);
  int b = (0 ?: 5);
  int c = (7 ?: 9);
  return a == 4 && i == 4 && b == 5 && c == 7 ? 0 : 1;
}
