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

- [ ] Each shim has an owner, source failure, and semantic status.
- [ ] The docs distinguish declarations, macros, opaque types, and layout-known
      structs.
- [ ] Runtime ownership is consistently assigned to host libraries.
- [ ] Future agents can decide whether to add a shim or fix the parser.
