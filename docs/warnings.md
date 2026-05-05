# Warning Policy

This document defines the release-facing warning names and group membership for
`rcc`. Detector implementation is intentionally split into follow-up tasks; this
page is the stable policy surface they must use.

## Command-Line Controls

| Flag | Meaning |
|------|---------|
| `-w` | Suppress all warnings. |
| `-Wall` | Enable the common analysis warnings listed in the `-Wall` table. |
| `-Wextra` | Enable every `-Wall` warning plus the `-Wextra` table. |
| `-Wpedantic` | Record pedantic intent for later strictness checks. |
| `-Wname` | Enable one named warning. |
| `-Wno-name` | Disable one named warning, overriding groups. |
| `-Werror` | Promote every emitted warning to an error. |
| `-Werror=name` | Enable and promote one named warning. |
| `-Wno-error=name` | Stop promoting one named warning, overriding `-Werror`. |

Warning names are normalized by removing an optional `-W` prefix, removing a
leading `no-` only for control flags, replacing `_` with `-`, and lowercasing.
For example, `-Wunused_parameter` and `-Wunused-parameter` address the same
warning.

Source files can also adjust policy from the point of a directive onward with
`#pragma GCC diagnostic push`, `pop`, `ignored "-Wname"`, `warning "-Wname"`,
and `error "-Wname"`. The pragma state is source-positioned so downstream
parser/HIR/typeck warnings use the policy active at the diagnostic span.

## Default Warnings

Default warnings are emitted when their phase encounters the condition unless
the user disables them with `-Wno-name` or suppresses all warnings with `-w`.

| Name | Code |
|------|------|
| `unknown-pragma` | `W0001` |
| `float-overflow` | `W0002` |
| `multichar` | `W0003` |
| `duplicate-decl-specifier` | `W0004` |
| `old-style-definition` | `W0005` |
| `macro-redefined` | `W0006` |
| `enum-overflow` | `W0007` |
| `conversion` | `W0008` |
| `constant-overflow` | `W0009` |
| `division-by-zero` | `W0010` |
| `shift-count-overflow` | `W0011` |
| `complex-to-real` | `W0012` |

## Extension Warnings

Extension warnings are emitted by default in strict C99 mode when an accepted
GNU or compatibility construct is parsed. Feature flags such as
`-fgnu-statement-expressions` may suppress the compatibility warning at the
source phase; `-Wno-name` remains the warning-control escape hatch.

| Name | Code |
|------|------|
| `gnu-statement-expression` | `W0013` |
| `gnu-range-designator` | `W0014` |
| `gnu-attributes` | `W0015` |
| `gnu-inline-asm` | `W0016` |
| `gnu-omitted-conditional-operand` | `W0017` |
| `gnu-conditional-void-operand` | `W0018` |
| `gnu-case-ranges` | `W0019` |
| `gnu-labels-as-values` | `W0020` |
| `gnu-lvalue-comma` | `W0021` |
| `gnu-function-names` | `W0022` |
| `gnu-va-area` | `W0023` |
| `gnu-typeof` | `W0024` |
| `gnu-alignof` | `W0025` |

## `-Wall`

These warnings are opt-in analysis warnings. A detector must call
`WarningConfig::warning_enabled("<name>")` before emitting one.

| Name | Owner |
|------|-------|
| `implicit-function-declaration` (`W0029`) | `tasks/13-quality/03d-implicit-function-declaration-warning.md` |
| `unused-function` (`W0027`) | `tasks/13-quality/03b-unused-function-warning.md` |
| `unused-variable` (`W0026`) | `tasks/13-quality/03a-unused-variable-warning.md` |

## `-Wextra`

`-Wextra` enables every `-Wall` warning plus this extra set.

| Name | Owner |
|------|-------|
| `sign-compare` (`W0030`) | `tasks/13-quality/03e-sign-compare-warning.md` |
| `unreachable-code` (`W0031`) | `tasks/13-quality/03f-unreachable-code-warning.md` |
| `unused-parameter` (`W0028`) | `tasks/13-quality/03c-unused-parameter-warning.md` |

## Detector Contract

Each warning detector must:

- use the canonical names above for CLI control and tests;
- emit named warnings through `Handler` with a stable warning code so command
  line flags and `#pragma GCC diagnostic` source-positioned policy both apply;
- include the controlling spelling, such as `[-Wunused-variable]`, in the
  message, note, or help text;
- add at least one regression test for enable, suppress, and promote behavior.
