# 06 — SQLite amalgamation

Status: not started

Source: <https://www.sqlite.org/howtocompile.html>

Start by downloading the selected official amalgamation into `upstream/`, then
create `plan.md` from the fetched files. Do not edit upstream `.c` or `.h`
files. Any adaptation must live in this directory as wrapper scripts or
build-script-only patches.

Initial target: compile `sqlite3.c` as an object before attempting shell
linkage.

