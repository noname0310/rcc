# GNU89 Legacy Policy

`rcc` remains an ISO C99/C11 compiler, not a GNU89 compiler. A small GNU89
compatibility slice is supported when it is already part of the C99
obsolescent surface or needed to classify gcc-torture behavior:

- implicit `int` on old code paths is accepted as compatibility syntax;
- K&R-style function definitions are parsed with W0005;
- K&R parameter declaration lists determine both the function body's parameter
  locals and the function definition ABI type;
- calls through a prototype-less function type still use default argument
  promotions.

This is not a full `-std=gnu89` mode. Future GNU89 additions need explicit
tasks instead of being treated as ordinary C99 bugs.
