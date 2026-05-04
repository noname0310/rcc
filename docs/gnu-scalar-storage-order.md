# GNU scalar_storage_order policy

`scalar_storage_order("big-endian")` and
`scalar_storage_order("little-endian")` are GNU record attributes, not C99.
rcc preserves the attribute through HIR as record metadata and currently
implements the bit-field storage semantics needed by GCC torture
`20230630-2`.

On little-endian targets, `scalar_storage_order("big-endian")` changes
bit-field storage in two ways:

- bit-fields are allocated from the most-significant bit toward the
  least-significant bit inside the coalesced storage unit;
- LLVM loads, stores, and constant initializers byte-swap the storage unit so
  the object bytes match the requested scalar storage order.

The same mechanism is represented target-neutrally in HIR layout metadata, but
rcc does not yet implement general reversed-endian scalar object access for
ordinary non-bit-field integer members. That broader GNU extension is outside
C99 and should be added as a separate task if a conformance case requires it.

The implementation also fixes mixed-underlying-type bit-field coalescing for
the baseline SysV-style layout. For example, `short i:12; char c:1; ...`
shares one 16-bit storage unit when the later smaller bit-fields fit in the
remaining bits.
