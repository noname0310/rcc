typedef unsigned long size_t;
void *malloc(size_t);
void free(void *);
int puts(const char *);

typedef unsigned int uint32_t;
typedef unsigned short uint16_t;
typedef unsigned char uint8_t;

typedef uint32_t JSAtom;
typedef enum {
  C0,
  C1,
  C2,
  C3,
  C4,
  C5,
  C6,
  C7
} ClosureType;

typedef struct {
  ClosureType closure_type : 3;
  uint8_t is_lexical : 1;
  uint8_t is_const : 1;
  uint8_t var_kind : 4;
  uint16_t var_idx;
  JSAtom var_name;
} ClosureVar;

int main(void) {
  ClosureVar *vars = malloc(sizeof(ClosureVar) * 2);
  if (!vars)
    return 1;
  if (sizeof(ClosureVar) != 8)
    return 2;
  if ((unsigned long)((unsigned char *)&vars[1] - (unsigned char *)&vars[0]) !=
      sizeof(ClosureVar))
    return 3;

  vars[0].var_idx = 0;
  vars[0].var_name = 609;
  vars[0].closure_type = C4;
  vars[0].is_const = 0;
  vars[0].is_lexical = 0;
  vars[0].var_kind = 0;

  vars[1].var_idx = 0;
  vars[1].var_name = 603;
  vars[1].closure_type = C5;
  vars[1].is_const = 0;
  vars[1].is_lexical = 0;
  vars[1].var_kind = 0;

  if (vars[0].closure_type != C4)
    return 4;
  if (vars[1].closure_type != C5)
    return 5;
  if (vars[0].var_name != 609)
    return 6;
  if (vars[1].var_name != 603)
    return 7;

  free(vars);
  puts("bitfield-stride");
  return 0;
}
