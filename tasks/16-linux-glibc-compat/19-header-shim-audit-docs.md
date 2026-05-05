> ✓ done — 2026-05-06

# 16-19: Header Shim Audit Docs

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-18-posix-thread-runtime-smoke  
**Milestone:** hosted-linux

## Goal

Document every hosted header shim and the reason it exists, so the project does
not drift into maintaining a private libc.

## Scope

- In: `docs/hosted-linux.md`, resource header inventory, and links to project
  failures that justified each shim.
- In: removal criteria for shims when the parser can handle host headers
  directly.
- Out: broad libc compatibility promises.

## Acceptance

- [x] Each shim has an owner, source failure, and semantic status.
- [x] The docs distinguish declarations, macros, opaque types, and layout-known
      structs.
- [x] Runtime ownership is consistently assigned to host libraries.
- [x] Future agents can decide whether to add a shim or fix the parser.

## Result

`docs/hosted-linux.md` now has a `Header Shim Inventory` table.  Each hosted
shim group records:

- kind: declaration, macro, opaque type, scalar typedef, or layout-known struct;
- source failure / owner: the real-world or conformance probe that justified it;
- semantic status: what rcc owns versus what host libc/libm/libpthread/libdl
  owns;
- removal criterion: when a parser/header fix can shrink or remove the shim.

The table intentionally groups ordinary C99 declaration headers separately from
glibc/POSIX-specific overlays so future agents can tell whether a failure needs
a small shim, a parser fix, or a runtime/linker change.
