# 05 — Lua

Status: blocked on rcc runtime codegen bug

Source: <https://www.lua.org/source/>

Start by downloading or cloning the selected official source into `upstream/`,
then create `plan.md` from the fetched build files. Do not edit upstream `.c` or
`.h` files. Any adaptation must live in this directory as wrapper scripts or
build-script-only patches.

Initial target: compile the Lua core library before attempting the interpreter.

Current target: Lua 5.5.0 official tarball. The core library and interpreter
compile/link with `rcc`; executing a Lua chunk is blocked by
`tasks/09-codegen-llvm/31-lua-parser-runtime-regression.md`.
