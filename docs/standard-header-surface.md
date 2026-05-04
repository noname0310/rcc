# Standard Header Surface

`rcc` does not implement a C standard library. Hosted builds use the target
system's C runtime and libraries for the actual symbols, while `rcc` ships a
small compiler-provided include directory with declarations that the frontend can
parse and type-check.

The files under `lib/rcc/include/` are therefore ABI-facing declaration shims,
not libc implementations. They should contain only the C99 declarations needed
by current conformance fixtures and compiler-owned builtins. Adding a prototype
here is acceptable when:

- the symbol is provided by the host C runtime or an explicitly linked host
  library such as `libm`;
- the declaration is stable for the current target ABI;
- the compiler needs the declaration to parse, type-check, lower, or call the
  symbol correctly.

Do not add function bodies, data structure internals, or broad glibc/musl
surface area just to make unknown programs compile. New declarations should be
driven by a concrete conformance fixture or a separate task.

For math functions, the driver must still pass the requested library flag (for
example `-lm`) through to the linker. Header declarations only make the frontend
aware of the function type; they do not satisfy linking by themselves.
