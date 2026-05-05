# 16-linux-glibc-compat: index

Linux hosted compatibility: glibc/POSIX/GNU header shims, `-pthread`, runtime
library linking, and real-world probes that depend on host glibc instead of
rcc-owned libc bodies.

## Upstream deps

- 14-lang-extensions
- 15-builtin-rt
- 10-driver
- 09-codegen-llvm

## Tasks (pick in order)

- [x] [01-hosted-linux-surface-audit](01-hosted-linux-surface-audit.md)
- [x] [02-compat-mode-and-policy](02-compat-mode-and-policy.md)
- [x] [03-feature-test-macro-model](03-feature-test-macro-model.md)
- [x] [04-resource-header-overlay-order](04-resource-header-overlay-order.md)
- [x] [05-glibc-common-macro-shims](05-glibc-common-macro-shims.md)
- [x] [06-gnu-header-attribute-tolerance](06-gnu-header-attribute-tolerance.md)
- [x] [07-restrict-and-qualifier-aliases](07-restrict-and-qualifier-aliases.md)
- [x] [08-pthread-driver-flag](08-pthread-driver-flag.md)
- [x] [09-pthread-header-shim](09-pthread-header-shim.md)
- [x] [10-posix-core-type-shims](10-posix-core-type-shims.md)
- [x] [11-fcntl-dirent-stat-shims](11-fcntl-dirent-stat-shims.md)
- [x] [12-dlfcn-and-runtime-linking](12-dlfcn-and-runtime-linking.md)
- [x] [13-gnulib-config-header-probe](13-gnulib-config-header-probe.md)
- [x] [14-glibc-system-header-parse-gate](14-glibc-system-header-parse-gate.md)
- [x] [15-mujs-hosted-smoke](15-mujs-hosted-smoke.md)
- [x] [16-gnu-coreutils-bootstrap-probe](16-gnu-coreutils-bootstrap-probe.md)
- [x] [17-gnu-coreutils-single-utility-probe](17-gnu-coreutils-single-utility-probe.md)
- [x] [18-posix-thread-runtime-smoke](18-posix-thread-runtime-smoke.md)
- [x] [19-header-shim-audit-docs](19-header-shim-audit-docs.md)
- [ ] [20-real-world-glibc-dashboard](20-real-world-glibc-dashboard.md)
- [ ] [21-gnu-include-next-directive](21-gnu-include-next-directive.md)
- [ ] [22-gnulib-funcdecl-macro-surface](22-gnulib-funcdecl-macro-surface.md)
- [ ] [23-coreutils-posix-declaration-sweep](23-coreutils-posix-declaration-sweep.md)
- [ ] [24-coreutils-true-runtime-oracle](24-coreutils-true-runtime-oracle.md)

## Downstream

- 11-conformance
- real_world/projects/07-mujs
- real_world/projects/09-gnu-coreutils
- future POSIX-threaded hosted Linux project probes
