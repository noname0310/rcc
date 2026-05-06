# Standard Header Surface

`rcc` does not implement a C standard library. Hosted builds use the target
system's C runtime, libraries, and headers for libc/POSIX/Linux declarations.
The small compiler-provided include directory is reserved for headers that need
direct compiler cooperation.

The current declaration coverage audit lives in
[`docs/hosted-c99-header-audit.md`](hosted-c99-header-audit.md).

The files under `lib/rcc/include/` are therefore compiler-owned resource
headers, not libc implementations and not approximate glibc/musl/POSIX header
replacements. Adding a file here is acceptable when:

- the header describes frontend builtins or language support, such as
  `stddef.h`, `stdarg.h`, `stdint.h`, `stdbool.h`, `stdalign.h`,
  `stdatomic.h`, `iso646.h`, or target scalar limits;
- the contents are generated from rcc's target model or are otherwise part of
  the compiler contract;
- using the host header would make compiler-owned behavior ambiguous.

Do not add `stdio.h`, `stdlib.h`, `string.h`, `pthread.h`, `unistd.h`,
`sys/*.h`, networking headers, function bodies, data structure internals, or
broad glibc/musl surface area just to make unknown programs compile. A failure
inside a real host header should become a minimized preprocessor/parser/lowering
task, not a copied declaration shim.

For math functions, the driver must still pass the requested library flag (for
example `-lm`) through to the linker. Header declarations only make the frontend
aware of the function type; they do not satisfy linking by themselves.
