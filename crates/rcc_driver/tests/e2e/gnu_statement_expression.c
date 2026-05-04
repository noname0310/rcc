int main(void) {
  int value = ({ int i = 2; i += 5; i; });
  int add_assign_result = ({ int i = 2; i += 5; });
  int outer = 3;
  int shadow = ({ int outer = 4; { int outer = 5; outer += 2; } outer; });
  int side_effect = 0;
  ({ int tmp = value; side_effect = tmp; ; });
  return value == 7 && add_assign_result == 7 && outer == 3 && shadow == 4 && side_effect == 7
             ? 0
             : 1;
}
