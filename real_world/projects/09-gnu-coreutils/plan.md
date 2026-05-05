# GNU coreutils probe plan

## Snapshot

- Upstream: <https://github.com/coreutils/coreutils>
- Commit: `d719413160b48136e3c3d6e5822f241eabddafb9`
- Local source: `real_world/projects/09-gnu-coreutils/upstream/`
- Source policy: never modify upstream `.c`, `.h`, `configure.ac`, `Makefile.am`,
  or generated files in place.

## Why This Project

GNU coreutils is a deliberately hard hosted-Linux target:

- It depends on glibc/POSIX/GNU headers through gnulib.
- It uses feature-test macros and generated `config.h` heavily.
- It exercises file-system, locale, getopt, signal, stat, dirent, fcntl, time,
  and process-related declarations.
- It is still mostly C99-compatible at the language level once the hosted
  environment is correctly modeled.

## Probe Order

1. Run the project's bootstrap/configure path with the host compiler in a local
   build directory and record the generated include paths and `config.h`
   surface.
2. Pick one small utility, starting with `src/true.c` because it is simple but
   still includes `config.h`, `system.h`, `<stdio.h>`, and `<sys/types.h>`.
3. Compile the selected translation unit with `rcc` using the same include
   order and feature macros as the host build.
4. If compilation fails because `rcc` is wrong or incomplete, stop the project
   probe and create or execute the corresponding compiler task under
   `tasks/16-linux-glibc-compat/`.
5. Once the first utility links and runs, compare a small runtime command
   against the host-built utility.

## Generated Include Order

The reproducible probe entrypoint is:

```sh
bash real_world/projects/09-gnu-coreutils/scripts/prepare-local-bootstrap-tools.sh
bash real_world/projects/09-gnu-coreutils/scripts/run-gnulib-config-probe.sh
```

The script does not modify `upstream/`.  It creates an ignored LF-normalized
worktree under `build/gnulib-config-probe/src`, bootstraps/configures into
`build/gnulib-config-probe/build`, writes logs under
`logs/gnulib-config-probe/`, and asks `rcc` to lower a wrapper translation unit
that includes generated `config.h` and `src/system.h`.

The exact host commands are encoded in the scripts:

```sh
git -c core.autocrlf=false clone --recurse-submodules upstream build/gnulib-config-probe/src
(cd build/gnulib-config-probe/src && ./bootstrap --skip-po)
(cd build/gnulib-config-probe/build && \
  build/gnulib-config-probe/src/configure \
    --disable-nls --without-gmp --without-selinux \
    --prefix=build/gnulib-config-probe/install CC=cc)
```

For the first `rcc` compile, use this include order:

1. generated `config.h` directory: `build/gnulib-config-probe/build/lib`
   when present, otherwise `build/gnulib-config-probe/build`;
2. generated gnulib replacement headers: `build/gnulib-config-probe/build/lib`;
3. source replacement headers: `build/gnulib-config-probe/src/lib`;
4. selected utility headers: `build/gnulib-config-probe/src/src`;
5. gnulib source headers: `build/gnulib-config-probe/src/gl/lib`;
6. project root fallback: `build/gnulib-config-probe/src`.

Current local environment note: the checked-out ignored `upstream/` tree has
CRLF shell scripts on Windows, so the probe deliberately clones a fresh
`core.autocrlf=false` worktree before bootstrap.  If bootstrap tools are
missing, the script exits `77` and writes `logs/gnulib-config-probe/blocker.env`
linked to `tasks/16-linux-glibc-compat/16-gnu-coreutils-bootstrap-probe.md`.
On the current WSL machine, `prepare-local-bootstrap-tools.sh` extracts the
needed Debian packages into ignored `build/local-tools/`; no sudo/system install
is required.

Observed generated config:

- `build/gnulib-config-probe/build/lib/config.h`
- `PACKAGE_STRING`: `GNU coreutils UNKNOWN`
- representative generated macros: `HAVE_DIRENT_H=1`

Host runtime oracle command for the first utility:

```sh
make -C real_world/projects/09-gnu-coreutils/build/gnulib-config-probe/build -j2 src/true
real_world/projects/09-gnu-coreutils/build/gnulib-config-probe/build/src/true
```

Current host-build status: configure succeeds, but `make src/true` fails in the
host compiler before producing the oracle binary.  The failure is recorded in
`logs/gnulib-config-probe/make-true.stderr` and is not an `rcc` compiler
failure: `lib/hard-locale.c` sees missing `SETLOCALE_NULL_MAX` /
`setlocale_null_r` declarations from the generated gnulib include chain.  The
next coreutils task must either resolve that host-build input issue or select a
smaller generated-header probe before using the utility as an oracle.

Current `rcc` config-wrapper status: after generated `config.h` exists, `rcc`
progresses into `src/system.h`/gnulib replacement headers and fails on missing
hosted declarations/macros such as `fputs_unlocked`, `fwrite_unlocked`,
`fchownat`, `fchmodat`, `vasprintf`, `mbrtowc` helpers, `S_TYPEISSHM`, and
`S_TYPEISTMO`.  These are compiler/header-surface inputs for
`tasks/16-linux-glibc-compat/17-gnu-coreutils-single-utility-probe.md` and the
header audit task, not broad xfails.

## Non-Goals

- Do not vendor glibc, gnulib, or Linux kernel headers into `rcc`.
- Do not patch coreutils source to avoid compiler bugs.
- Do not count a weakened probe as success. Every workaround must be either a
  build-script-only adaptation or a compiler task.

## Expected First Blockers

- Generated `config.h` and gnulib replacement-header include order.
- glibc-specific macro spellings such as `__THROW`, `__wur`, `__nonnull`,
  `__attribute_malloc__`, and feature-test conditionals.
- POSIX file-system declarations from `sys/types.h`, `sys/stat.h`, `fcntl.h`,
  `dirent.h`, and `unistd.h`.
- Linker/runtime flags for libc-provided functions, not rcc-owned function
  bodies.
