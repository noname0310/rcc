struct S {
  unsigned short s[2];
};

int main(void) {
  struct S x;
  unsigned char *p = (unsigned char *) x.s;

  x.s[0] = 0x1234;
  if (p[0] != 0x34)
    return 1;
  if (p[1] != 0x12)
    return 2;

  p[0] = 0x78;
  p[1] = 0x56;
  if (x.s[0] != 0x5678)
    return 3;
  return 0;
}
