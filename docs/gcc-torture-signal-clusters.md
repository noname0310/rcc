# gcc-torture runtime signal clusters

Source report: `target/wsl/gcc-torture-full-15e-after.json`

Resweep report after `11-15i` and `11-15j`:
`target/wsl/gcc-torture-15k-signal-resweep.json`

## Summary

The 15e full gcc-torture run had 61 cases whose reason was
`non-zero exit code: killed by signal`. After the aligned-record and
bit-field/aggregate fixes and the GNU vector cluster work, 19 of those now pass
and 42 remain failing.

Already fixed by follow-up work:

| Case | Fix |
| --- | --- |
| `20010904-1` | `11-15i`, GNU `aligned(N)` record stride |
| `20010904-2` | `11-15i`, GNU `aligned(N)` record stride |
| `20011113-1` | `11-15j`, bit-field physical layout before aggregate copy/byval |
| `20081117-1` | `11-15j`, bit-field storage coalescing |
| `990326-1` | `11-15j`, explicit aggregate layout side effect |
| `991118-1` | `11-15j`, explicit bit-field storage side effect |
| `bitfld-4` | `11-15j`, bit-field storage side effect |
| `pr49768` | `11-15j`, unnamed bit-field storage side effect |
| `pr71700` | `11-15j`, bit-field aggregate copy side effect |
| `20050316-1` | `11-15s1`-`11-15s7`, GNU vector type/lowering/casts/ABI |
| `20050316-2` | `11-15s1`-`11-15s7`, GNU vector type/lowering/casts/ABI |
| `20050316-3` | `11-15s1`-`11-15s7`, GNU vector type/lowering/casts/ABI |
| `20050604-1` | `11-15s1`-`11-15s7`, GNU vector compound literals, arithmetic, and float lane zero-fill |
| `pr92618` | `11-15s1`-`11-15s7`, GNU vector memory operations and returns |
| `scal-to-vec1` | `11-15s1`-`11-15s7`, GNU scalar-to-vector splats |
| `scal-to-vec2` | `11-15s1`-`11-15s7`, GNU scalar-to-vector splats |
| `scal-to-vec3` | `11-15s1`-`11-15s7`, GNU scalar-to-vector splats |
| `simd-4` | `11-15s1`-`11-15s7`, GNU vector-to-integer bitcasts |
| `simd-6` | `11-15s1`-`11-15s7`, GNU vector arithmetic and byte views |

## Remaining Clusters

| Cluster | Cases | Reason | Next task |
| --- | --- | --- | --- |
| Bit-field precision, signedness, and stores | `bf-sign-2`, `bitfld-1`, `bitfld-3`, `bitfld-5`, `pr31448-2`, `pr32244-1`, `pr34971`, `pr58984`, `struct-ini-2` | Shared storage layout now exists, but expression typing still loses bit-field width/signedness and stores/loads need precision-aware truncation/sign extension. | `11-15l` |
| Scalar conversion and integer edge semantics | `20030916-1`, `990222-1`, `20031003-1`, `20060110-1`, `20060110-2` | Mixture of C99 integer wrapping/conversion bugs and GCC edge cases around signed shifts and out-of-range float-to-int casts. | `11-15m` |
| VLA lifetime and VLA parameter side effects | `20040811-1`, `vla-dealloc-1`, `pr77767` | Runtime VLAs are not fully restored/deallocated across backward gotos, and VLA parameter bound side effects are not preserved exactly. | `11-15n` |
| Block-scope `extern` resolution | `scope-1` | Inner `extern int v;` should bind to the file-scope object, not the shadowing block local. | `11-15o` |
| Varargs and `va_list` runtime behavior | `pr64979`, `va-arg-21`, `va-arg-5`, `va-arg-6` | Remaining SysV `va_list` materialization and pointer-to-`va_list` cases. | `11-15p` |
| Aggregate, pointer, and byte-layout runtime bugs | `pr37573`, `pr49390`, `pr65401` | Large aggregate copies, char-byte views of objects, and aggregate ABI/pointer alias interactions still need smaller reductions. | `11-15q` |
| GNU field attributes and record member alignment | `pr23467` | Field-level `__attribute__((aligned(N)))` should raise member offset/alignment; `11-15i` only handled record-level alignment. | `11-15r` |
| GNU builtins, libc, and fortify wrappers | `20021127-1`, `fprintf-chk-1`, `printf-chk-1`, `vfprintf-1`, `vfprintf-chk-1`, `vprintf-1`, `vprintf-chk-1`, `pr103255` | GCC builtin/libcall behavior is only partially modeled; fortify and `__builtin_offsetof` need explicit handling or gating. | `11-15t` |
| GNU inline asm and instrumentation attributes | `20030222-1`, `990130-1`, `pr49279`, `pr85156`, `eeprof-1` | Inline asm operands/clobbers and instrumentation/noipa/noclone attributes are parsed more broadly than they are semantically implemented. | `11-15u` |
| GNU89 legacy cases | `920428-1`, `931018-1` | These rely on implicit int and K&R definitions. They are outside C99 and should stay gated behind an explicit GNU89 compatibility decision. | `11-15v` |
| GNU scalar storage order attribute | `20230630-2` | `scalar_storage_order` changes bit-field byte order; this is a target-specific GNU attribute and not C99. | `11-15w` |

## Reduced Fixtures Checked During Triage

All three reductions pass with host `cc -std=c99` and abort with current rcc
before their follow-up task is implemented.

### `scope-1`: block-scope extern

```c
void abort(void);
int v = 3;
int main(void) {
  int v = 4;
  { extern int v; if (v != 3) abort(); }
  return 0;
}
```

Observed on WSL:

```text
scope_extern cc=0
scope_extern rcc=134
```

### `pr77767`: VLA parameter side effects

```c
void abort(void);
void foo(int a, int b[a++], int c, int d[c++]) {
  if (a != 2 || c != 2) abort();
}
int main(void) { int e[10]; foo(1, e, 1, e); return 0; }
```

Observed on WSL:

```text
vla_param_side_effect cc=0
vla_param_side_effect rcc=134
```

### `990222-1`: prefix decrement plus compound assignment

```c
void abort(void);
char line[4] = { '1', '9', '9', '\0' };
int main(void) {
  char *ptr = line + 3;
  while ((*--ptr += 1) > '9') *ptr = '0';
  if (line[0] != '2' || line[1] != '0' || line[2] != '0') abort();
  return 0;
}
```

Observed on WSL:

```text
char_wrap cc=0
char_wrap rcc=134
```
