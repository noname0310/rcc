# TinyCC Binary Floating Literals

Decision: `rcc` does not implement TinyCC binary floating constants.

The C99 floating literal surface is decimal floating constants and hexadecimal
floating constants. `tcc-tests2::70_floating_point_literals` also contains a
`__TINYC__`-guarded section for TinyCC-only binary floating constants such as
`0B.110101100P12L`. That syntax is not C99 and is not shared by GCC or Clang as
a general GNU C extension.

`rcc` therefore keeps the `70_floating_point_literals` case as an xfail with a
policy reason instead of defining `__TINYC__` or adding a compatibility flag.
The non-`__TINYC__` portions of the fixture remain useful coverage for ordinary
C99 decimal and hexadecimal floating constants; those paths are still compiled
and executed by the conformance run before the expected-output mismatch is
classified as policy xfail.

Revisit this only if `rcc` grows an explicit TinyCC-compatibility mode. Such a
mode must be opt-in and must not define `__TINYC__` in the default C99 or GNU
compatibility modes.
