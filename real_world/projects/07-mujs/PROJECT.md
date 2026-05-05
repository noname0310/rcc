# 07 — MuJS

Status: hosted smoke passes on WSL/Linux

Source: <https://mujs.com/introduction.html>

Upstream commit: `0b4ed7e4ba37030fdd00f6a17b6de75cd7d7954b`

Do not edit upstream `.c` or `.h` files. Any adaptation must live in this
directory as wrapper scripts or build-script-only patches.

Current target: compile `main.c` + `one.c`, link with host `libm`, and run a
`print(1+2)` JavaScript smoke through both host and `rcc` binaries.

Entry point:

```sh
bash real_world/projects/07-mujs/scripts/run-smoke.sh
```
