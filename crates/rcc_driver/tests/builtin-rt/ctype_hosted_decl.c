#include <ctype.h>
#include <stdio.h>

int main(void) {
  if (!isspace(' '))
    return 1;
  if (!isdigit('7'))
    return 2;
  if (tolower('A') != 'a')
    return 3;
  if (toupper('z') != 'Z')
    return 4;
  puts("ctype ok");
  return 0;
}
