# 08 — QuickJS

Status: not started

Source: <https://bellard.org/quickjs/>

Start by downloading or cloning the selected upstream source into `upstream/`,
then create `plan.md` from the fetched build files. Do not edit upstream `.c` or
`.h` files. Any adaptation must live in this directory as wrapper scripts or
build-script-only patches.

Initial target: compile selected core objects only. Expect GNU and platform
assumptions; record each one explicitly instead of weakening the probe.

