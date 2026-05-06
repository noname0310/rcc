# 11 -- libuv

Status: planned; start after Toybox establishes the next hosted Linux baseline

Source: <https://github.com/libuv/libuv>

libuv is a cross-platform asynchronous I/O library. It is a good follow-up to
Toybox because it heavily exercises pthreads, event loops, pipes, process
spawning, signals, filesystem APIs, and platform feature-test macros.

Do not edit files under `upstream/`. Any adaptation must live in this directory
as wrapper scripts, generated config inputs, or build-script-only patches.

Initial target:

1. Clone upstream into `upstream/` and record the resolved commit in `plan.md`.
2. Inspect CMake/autotools entry points and choose the smallest Linux static
   library target.
3. Build a host baseline static library and one small event-loop smoke program.
4. Compile the selected source set with `rcc`, link against host glibc/pthread,
   and compare runtime behavior with the host baseline.

Entry point will be added when Toybox either passes or exposes a committed
compiler task that blocks further expansion.
