int puts(const char *);

int sink(int a, int b) {
  return a + b;
}

int main(void) {
  sink(puts("left"), puts("right"));
  return 0;
}
