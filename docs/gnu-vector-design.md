# GNU Vector Extension Design Slice

This note scopes the `11-15s` gcc-torture vector cluster. The current compiler
parses GNU attributes, but `vector_size(N)` is not represented in HIR. As a
result, typedefs such as `typedef int v4si __attribute__((vector_size(16)));`
flow through as scalar `int`, compile, and then fail at runtime. These are
vector-extension gaps, not generic conformance failures.

## Cluster Baseline

Measured with WSL LLVM 18 and the full gcc-torture adapter:

| Case | 15s7 result | Primary feature |
| --- | --- | --- |
| `20050316-1` | pass | vector/scalar bitcasts, vector returns |
| `20050316-2` | pass | int/float vector bitcasts |
| `20050316-3` | pass | signed/unsigned vector bitcasts |
| `20050604-1` | pass | vector compound literals, `+=` |
| `pr92618` | pass | may-alias vector pointer stores, vector returns |
| `scal-to-vec1` | pass | scalar-vector arithmetic splats |
| `scal-to-vec2` | pass | scalar function result splats |
| `scal-to-vec3` | pass | integer literal splats into float/double vectors |
| `simd-4` | pass | vector-to-integer bitcast |
| `simd-6` | pass | vector multiply, memcmp byte view |

15s7 WSL verification:

```text
cargo run -p rcc_conformance --bin rcc_conformance_run -- \
  --rcc target/wsl/debug/rcc \
  --suite gcc-torture --include-gpl \
  --case gcc-torture::execute::20050316-1 \
  --case gcc-torture::execute::20050316-2 \
  --case gcc-torture::execute::20050316-3 \
  --case gcc-torture::execute::20050604-1 \
  --case gcc-torture::execute::pr92618 \
  --case gcc-torture::execute::scal-to-vec1 \
  --case gcc-torture::execute::scal-to-vec2 \
  --case gcc-torture::execute::scal-to-vec3 \
  --case gcc-torture::execute::simd-4 \
  --case gcc-torture::execute::simd-6 \
  --output target/wsl/gcc-vector-cluster-15s7.json
```

Expected result: `10 pass, 0 fail, 0 xfail, 0 skip`.

## Type Model

Add a first-class HIR type:

```rust
Ty::Vector {
    elem: TyId,
    lanes: u32,
    bytes: u64,
}
```

Rules:
- `vector_size(B)` attaches to object scalar typedefs and declarations.
- `B` is the total vector byte size.
- `lanes = B / sizeof(elem)` and must be non-zero with no remainder.
- Element types in this slice are integer and floating scalar types only.
- `sizeof(vector)` is `bytes`; alignment is at least `bytes` up to the target's
  natural vector alignment cap. The initial LP64/SysV slice matches LLVM's
  fixed-vector ABI for the 32-bit, 64-bit, and 128-bit vectors exercised by the
  gcc-torture vector cluster.

## Lowering

`vector_size` is a type attribute, unlike field-level `aligned`. Lowering must
apply it after the base declarator type is known:

1. Parse raw attribute tokens as today.
2. Add an attribute expression evaluator for integer constant expressions used
   by `vector_size`, including `sizeof(type)` because gcc-torture uses
   `vector_size((elcount) * sizeof(type))`.
3. Wrap the target scalar type in `Ty::Vector`.
4. Preserve ordinary object qualifiers outside the vector element type.

## Type Checking

Initial operators:
- Vector object lvalue-to-rvalue is a vector rvalue.
- Vector assignment requires compatible vector type or a defined vector
  conversion.
- Vector list initializers fill lanes in order and zero-fill omitted lanes.
- Compound literals of vector type materialize a temporary vector object.
- Binary arithmetic on two equal vector types is elementwise.
- Binary arithmetic between vector and scalar splats the scalar to the vector
  element type.
- Integer vector bitwise/shifts are elementwise.
- Casts between same-size vector and scalar integer are bitcasts.
- Casts between same-lane integer/float vectors are elementwise conversion when
  GNU expects it, otherwise bitcast only when the source and destination sizes
  match.

## CFG Contract

Do not model a vector as an aggregate record. CFG should carry vector-typed
values as scalar-like SSA values, while memory operations still use vector object
size and alignment for loads/stores, copies, and pointer casts.

Required MIR extensions:
- `Rvalue::VectorSplat`
- `Rvalue::VectorInit`
- vector-aware `Unary`, `Binary`, and `Cast`
- place load/store of `Ty::Vector`

## LLVM Mapping

Map `Ty::Vector { elem, lanes }` to an LLVM fixed vector:

```text
<lanes x elem-llvm-type>
```

Emit:
- vector constants for initializers and compound literals,
- `bitcast` for same-size scalar/vector and vector/vector bitcasts,
- `insertelement` or constant vector construction for list initializers,
- elementwise LLVM arithmetic for vector operators,
- SysV ABI parameter/return classification using vector registers where
  LLVM's function type can represent the vector directly.

## Task Split

The implementation is split into `11-15s1` through `11-15s7`. Each task must add
at least one reduced fixture and must not mark a gcc-torture vector failure as
generic xfail.
