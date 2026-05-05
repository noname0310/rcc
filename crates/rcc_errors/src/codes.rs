//! Stable error-code registry for the rcc C compiler.
//!
//! Every user-facing diagnostic should carry one of these codes so users
//! can look it up in `docs/error-codes.md`.
//!
//! Codes are allocated in contiguous blocks per subsystem:
//!   E0001..E0020  — lexer / preprocessor
//!   E0021..E0040  — parser          (reserved, future)
//!   E0041..E0060  — type-checking   (reserved, future)
//!   E0061..E0080  — HIR lowering    (reserved, future)
//!   E0081..E0100  — type-checking / codegen (E0081 spent on
//!                                            assignment compatibility,
//!                                            see task 07-05)
//!
//! Warning codes use the `WNNNN` spelling and live in their own
//! namespace; task 04-16 introduces the first, W0001 for unknown
//! `#pragma` directives, and task 07-12 introduces W0012 for
//! complex-to-real imaginary-part discard.
//!
//! The preprocessor block E0001..E0020 was filled during lexer work, so
//! task 04-03 borrows the first slot of the parser window for the
//! `#include` resolver. Downstream parser tasks should allocate from
//! E0022 onward; see the `## Notes (agent)` in
//! `tasks/04-preprocess/03-include-search-path.md`.
//!
//! Task 05-18 (declaration specifiers) spends E0060 and E0061 out of
//! the reserved-for-future type-checking block because the
//! corresponding constraints are first raised in the parser (conflict
//! of storage-class / type-specifier) — the type-checker never gets a
//! chance to reject them on its own.

/// Collects every registered error code for programmatic iteration.
///
/// Each entry is `(code, short_description)`.
pub const ALL_CODES: &[(&str, &str)] = &[
    (E0001, E0001_DESC),
    (E0002, E0002_DESC),
    (E0003, E0003_DESC),
    (E0004, E0004_DESC),
    (E0005, E0005_DESC),
    (E0006, E0006_DESC),
    (E0007, E0007_DESC),
    (E0008, E0008_DESC),
    (E0009, E0009_DESC),
    (E0010, E0010_DESC),
    (E0011, E0011_DESC),
    (E0012, E0012_DESC),
    (E0013, E0013_DESC),
    (E0014, E0014_DESC),
    (E0015, E0015_DESC),
    (E0016, E0016_DESC),
    (E0017, E0017_DESC),
    (E0018, E0018_DESC),
    (E0019, E0019_DESC),
    (E0020, E0020_DESC),
    (E0021, E0021_DESC),
    (E0022, E0022_DESC),
    (E0023, E0023_DESC),
    (E0024, E0024_DESC),
    (E0025, E0025_DESC),
    (E0026, E0026_DESC),
    (E0027, E0027_DESC),
    (E0028, E0028_DESC),
    (E0029, E0029_DESC),
    (E0030, E0030_DESC),
    (E0031, E0031_DESC),
    (E0032, E0032_DESC),
    (E0040, E0040_DESC),
    (E0041, E0041_DESC),
    (E0060, E0060_DESC),
    (E0061, E0061_DESC),
    (E0062, E0062_DESC),
    (E0063, E0063_DESC),
    (E0070, E0070_DESC),
    (E0071, E0071_DESC),
    (E0072, E0072_DESC),
    (E0073, E0073_DESC),
    (E0074, E0074_DESC),
    (E0075, E0075_DESC),
    (E0076, E0076_DESC),
    (E0077, E0077_DESC),
    (E0078, E0078_DESC),
    (E0079, E0079_DESC),
    (E0080, E0080_DESC),
    (E0081, E0081_DESC),
    (E0082, E0082_DESC),
    (E0083, E0083_DESC),
    (E0084, E0084_DESC),
    (E0085, E0085_DESC),
    (E0086, E0086_DESC),
    (E0087, E0087_DESC),
    (E0088, E0088_DESC),
    (W0001, W0001_DESC),
    (W0002, W0002_DESC),
    (W0003, W0003_DESC),
    (W0004, W0004_DESC),
    (W0005, W0005_DESC),
    (W0006, W0006_DESC),
    (W0007, W0007_DESC),
    (W0008, W0008_DESC),
    (W0009, W0009_DESC),
    (W0010, W0010_DESC),
    (W0011, W0011_DESC),
    (W0012, W0012_DESC),
    (W0013, W0013_DESC),
    (W0014, W0014_DESC),
    (W0015, W0015_DESC),
    (W0016, W0016_DESC),
    (W0017, W0017_DESC),
    (W0018, W0018_DESC),
    (W0019, W0019_DESC),
    (W0020, W0020_DESC),
    (W0021, W0021_DESC),
    (W0022, W0022_DESC),
    (W0023, W0023_DESC),
    (W0024, W0024_DESC),
    (W0025, W0025_DESC),
    (W0026, W0026_DESC),
];

// ── Lexer / preprocessor block: E0001..E0020 ────────────────────────

/// Unexpected character in source input.
pub const E0001: &str = "E0001";
const E0001_DESC: &str = "unexpected character";

/// Unterminated string literal.
pub const E0002: &str = "E0002";
const E0002_DESC: &str = "unterminated string literal";

/// Nested block comment (`/*` inside another `/* ... */`).
///
/// C99 block comments do not nest (§6.4.9). A nested `/*` is almost
/// always a typo — the outer comment is silently closed at the first
/// `*/`, leaking the remaining lines into regular source.
pub const E0003: &str = "E0003";
const E0003_DESC: &str = "nested block comment";

/// Unterminated block comment (`/* ... */`).
pub const E0004: &str = "E0004";
const E0004_DESC: &str = "unterminated block comment";

/// Invalid escape sequence in string or character literal.
pub const E0005: &str = "E0005";
const E0005_DESC: &str = "invalid escape sequence";

/// Unterminated character constant (`'...` with no closing `'`).
pub const E0006: &str = "E0006";
const E0006_DESC: &str = "unterminated character constant";

/// Invalid escape sequence in a string or character literal.
pub const E0007: &str = "E0007";
const E0007_DESC: &str = "invalid escape sequence";

/// Unterminated string literal (`"...` with no closing `"`).
pub const E0008: &str = "E0008";
const E0008_DESC: &str = "unterminated string literal";

/// Integer literal overflow.
pub const E0009: &str = "E0009";
const E0009_DESC: &str = "integer literal overflow";

/// Unterminated header name in `#include` directive.
///
/// A `<...>` or `"..."` header name was opened but the matching
/// closing delimiter was not found before the end of the logical
/// line or end of file (C99 §6.4.7).
pub const E0010: &str = "E0010";
const E0010_DESC: &str = "unterminated header name";

/// Invalid octal digit in integer literal.
pub const E0011: &str = "E0011";
const E0011_DESC: &str = "invalid octal digit";

/// Invalid hexadecimal escape in string/char literal.
pub const E0012: &str = "E0012";
const E0012_DESC: &str = "invalid hex escape";

/// `#include` expects `"FILENAME"` or `<FILENAME>`.
pub const E0013: &str = "E0013";
const E0013_DESC: &str = "malformed #include directive";

/// `#define` macro name is missing or invalid.
pub const E0014: &str = "E0014";
const E0014_DESC: &str = "invalid #define directive";

/// `#ifdef` / `#ifndef` expects an identifier.
pub const E0015: &str = "E0015";
const E0015_DESC: &str = "expected identifier after #ifdef/#ifndef";

/// Unmatched `#endif`.
pub const E0016: &str = "E0016";
const E0016_DESC: &str = "unmatched #endif";

/// Unmatched `#else` or `#elif`.
pub const E0017: &str = "E0017";
const E0017_DESC: &str = "unmatched #else/#elif";

/// Missing `#endif` at end of file.
pub const E0018: &str = "E0018";
const E0018_DESC: &str = "missing #endif at end of file";

/// Unknown preprocessor directive.
pub const E0019: &str = "E0019";
const E0019_DESC: &str = "unknown preprocessor directive";

/// `#error` directive encountered.
pub const E0020: &str = "E0020";
const E0020_DESC: &str = "#error directive encountered";

/// `#include` header could not be located in any search path.
///
/// For the `"..."` form the current source file's directory is
/// searched first, then `Session::opts.include_paths`; for the
/// `<...>` form only `include_paths` is consulted (C99 §6.10.2).
pub const E0021: &str = "E0021";
const E0021_DESC: &str = "cannot find header";

/// `#define` redefines a macro with a different replacement list.
///
/// C99 §6.10.3p1 permits "benign" redefinition — repeating an
/// identical `#define` is silently accepted — but any difference in
/// the replacement-list's token count, ordering, spelling, or
/// whitespace separation is ill-formed.
pub const E0022: &str = "E0022";
const E0022_DESC: &str = "macro redefined with a different body";

/// Duplicate parameter name in a function-like `#define`.
///
/// C99 §6.10.3p6: the identifiers naming the parameters of a
/// function-like macro "shall be distinct" — two identical names in
/// the same parameter list is a constraint violation.
pub const E0023: &str = "E0023";
const E0023_DESC: &str = "duplicate macro parameter name";

/// Stringize operator `#` not followed by a parameter name.
///
/// C99 §6.10.3.2p1: each `#` preprocessing token in the replacement
/// list for a function-like macro shall be followed by a parameter
/// name as the next preprocessing token in the replacement list.
pub const E0024: &str = "E0024";
const E0024_DESC: &str = "`#` is not followed by a macro parameter";

/// Token-paste operator `##` produced an invalid token.
///
/// C99 §6.10.3.3 — the concatenation of the two operand texts must
/// form a single valid preprocessing token. If the combined text
/// re-lexes to more than one pp-token the paste is ill-formed. This
/// code is also used for the C99 §6.10.3.3p1 positional constraint
/// violation (`##` at the very beginning or end of a replacement
/// list).
pub const E0025: &str = "E0025";
const E0025_DESC: &str = "pasting forms an invalid token";

/// `__VA_ARGS__` referenced outside a variadic function-like macro.
///
/// C99 §6.10.3p5: the identifier `__VA_ARGS__` shall occur only in
/// the replacement list of a function-like macro that uses the
/// ellipsis notation in the parameters. Any other use — inside an
/// object-like macro body, inside a non-variadic function-like
/// macro, or as an ordinary identifier in regular source — is a
/// constraint violation.
pub const E0026: &str = "E0026";
const E0026_DESC: &str = "`__VA_ARGS__` outside a variadic macro";

/// Attempt to `#define` or `#undef` a predefined macro.
///
/// C99 §6.10.8p2: the implementation shall not predefine the macro
/// `__cplusplus`, nor shall it define it in any standard header; and
/// the predefined macros listed in §6.10.8p1 — `__DATE__`,
/// `__FILE__`, `__LINE__`, `__STDC__`, `__STDC_HOSTED__`,
/// `__STDC_VERSION__`, `__TIME__` — "shall not be the subject of a
/// `#define` or `#undef` preprocessing directive". Doing so is a
/// constraint violation.
pub const E0027: &str = "E0027";
const E0027_DESC: &str = "cannot redefine or undefine a predefined macro";

/// Ill-formed `#if` / `#elif` controlling expression.
///
/// Covers C99 §6.10.1 constraint violations in the integer constant
/// expression evaluator: division or remainder by zero in a live
/// branch, unexpected tokens, missing operands, unbalanced parens,
/// and malformed integer literals.
pub const E0028: &str = "E0028";
const E0028_DESC: &str = "invalid #if expression";

/// `#line` argument out of range.
///
/// C99 §6.10.4p3: the digit sequence of a `#line` directive "shall
/// not specify zero, nor a number greater than 2147483647". Both
/// bounds are constraint violations and carry this code.
pub const E0029: &str = "E0029";
const E0029_DESC: &str = "`#line` argument out of range";

/// Unexpected token during parsing.
///
/// The parser encountered a token that does not belong to any valid
/// statement, declaration, or expression at the current position.
/// Recovery skips forward to the next `;` or `}` so that subsequent
/// constructs can still be diagnosed independently.
pub const E0030: &str = "E0030";
const E0030_DESC: &str = "unexpected token";

/// Malformed GNU `__attribute__((...))` syntax.
pub const E0031: &str = "E0031";
const E0031_DESC: &str = "malformed attribute syntax";

/// Malformed GNU inline assembly syntax.
pub const E0032: &str = "E0032";
const E0032_DESC: &str = "malformed inline assembly syntax";

/// Integer literal is too large to fit in the widest representable type.
///
/// `rcc` decodes every integer literal into a `u128` before the
/// typeck pass selects a concrete C type per the C99 §6.4.4.1p5
/// ladder. When the raw magnitude already overflows `u128` — well
/// above `unsigned long long` — the value is unrepresentable at any
/// standard C integer type, so we reject it at decode time rather
/// than silently wrap. Contrast with lexer code E0009, which covers
/// the narrower case of a literal that fits `u128` but still exceeds
/// the language-level widest type.
pub const E0040: &str = "E0040";
const E0040_DESC: &str = "integer literal too large";

/// Adjacent string literals have incompatible encoding prefixes.
///
/// C99 §6.4.5p5 concatenates adjacent string-literal tokens in
/// translation phase 6. A narrow (unprefixed) literal concatenates
/// with an `L`-prefixed wide literal — the result is wide — but any
/// other mix of distinct prefixes (`L` with `u`, `L` with `U`, `u`
/// with `U`, a bare narrow with `u`/`U`/`u8`) is undefined behavior
/// and `rcc` rejects it at parse time. The first incompatible token
/// carries the primary label; the preceding run is shown as
/// secondary context.
pub const E0041: &str = "E0041";
const E0041_DESC: &str = "incompatible string literal encodings";

/// Multiple, conflicting storage-class specifiers on a single
/// declaration.
///
/// C99 §6.7.1p2: "At most, one storage-class specifier may be given in
/// the declaration specifiers in a declaration." The parser flags any
/// second storage-class keyword — both the classic conflict
/// (`static extern`) and the self-duplicate (`static static`) — with
/// this code at the offending keyword.
pub const E0060: &str = "E0060";
const E0060_DESC: &str = "conflicting storage-class specifiers";

/// Invalid combination of type specifiers inside a single declaration.
///
/// C99 §6.7.2p2 enumerates the legal multisets of type-specifier
/// keywords (`int`, `signed int`, `unsigned long long`, `long double`,
/// `_Complex float`, …). Anything outside that table is a constraint
/// violation — e.g. `short long`, `long long long`, `int int`,
/// `float int`, a typedef-name after `unsigned`, or two struct/union
/// tags in one specifier list. The parser reports the first token
/// that breaks the combination.
pub const E0061: &str = "E0061";
const E0061_DESC: &str = "invalid combination of type specifiers";

/// Declarator carries a name where an abstract declarator was
/// required.
///
/// C99 §6.7.6 defines `type-name : specifier-qualifier-list
/// abstract-declarator?`. An *abstract* declarator — the kind that
/// appears inside `sizeof(T)`, casts `(T)e`, compound literals
/// `(T){...}`, and parameter-type lists — lacks the identifier atom
/// of a concrete declarator (§6.7.5). Writing a name there is a
/// constraint violation; the parser recovers by discarding the name
/// so the rest of the construct still lowers cleanly.
pub const E0062: &str = "E0062";
const E0062_DESC: &str = "abstract declarator cannot contain a name";

/// K&R declaration list references a name not in the identifier list.
///
/// C99 §6.9.1p6: each identifier in the declaration list of a K&R-
/// style function definition must match one of the identifiers in
/// the function declarator's identifier list. A declaration that
/// names a parameter not present in the list is a constraint
/// violation.
pub const E0063: &str = "E0063";
const E0063_DESC: &str = "K&R declaration names unknown parameter";

// ── HIR lowering block: E0070..E0080 ────────────────────────────────

/// Redeclaration of an identifier with conflicting linkage or type.
///
/// C99 §6.2.2p7: if within a translation unit the same identifier
/// appears with both internal and external linkage the behaviour is
/// undefined. `rcc` rejects this at lowering time rather than
/// silently accepting the ambiguity.
pub const E0070: &str = "E0070";
const E0070_DESC: &str = "conflicting redeclaration";

/// Use of an undeclared identifier.
///
/// C99 §6.5.1p2: an identifier shall designate an entity visible in
/// the current scope. When lookup finds no binding, this error is
/// emitted with a `help:` line suggesting similarly-named symbols if
/// any exist within edit-distance 3.
pub const E0071: &str = "E0071";
const E0071_DESC: &str = "undeclared identifier";

/// Tag kind mismatch: a tag was previously declared with a different
/// kind (e.g. `struct S` then `union S`).
///
/// C99 §6.7.2.3p1: each declaration of a structure, union, or
/// enumerated type that does not include a tag declares a distinct
/// type. Each declaration of a structure, union, or enumerated type
/// that **does** include a tag must agree on the kind of the tag.
/// Using `struct S` where `S` was previously declared as `union S`
/// (or vice versa, or struct/enum mismatch) is a constraint violation.
pub const E0072: &str = "E0072";
const E0072_DESC: &str = "tag kind mismatch";

/// Use of an undeclared label in a `goto` statement.
///
/// C99 §6.8.6.1p1: the identifier in a `goto` statement shall name a
/// label located somewhere in the enclosing function. Forward
/// references are permitted — the label may appear after the `goto` —
/// but it must exist somewhere in the same function body.
pub const E0073: &str = "E0073";
const E0073_DESC: &str = "undeclared label";

/// Duplicate label in the same function.
///
/// C99 §6.8.1p3: label names shall be unique within a function. A
/// second `name:` definition in the same function body is a
/// constraint violation.
pub const E0074: &str = "E0074";
const E0074_DESC: &str = "duplicate label";

/// Typedef cycle detected.
///
/// A typedef directly or indirectly refers to itself through a chain
/// of other typedefs. C99 §6.7.7 requires typedef names to denote a
/// complete, acyclic type. `rcc` detects cycles during expansion and
/// reports this error rather than looping forever.
pub const E0075: &str = "E0075";
const E0075_DESC: &str = "typedef cycle detected";

/// Illegal declarator form.
///
/// C99 §6.7.5 imposes several constraints on the shapes of
/// declarators. `rcc` rejects these at lowering time:
///
/// - `void x;` for an object declaration (only `void *` and
///   function-returning-void are legal).
/// - A function returning an array (`int f()[10]`).
/// - A function returning a function (`int f()(int)`).
///
/// Each violation is flagged with this code at the offending
/// declarator token.
pub const E0076: &str = "E0076";
const E0076_DESC: &str = "illegal declarator form";

/// Duplicate enumerator name in the same scope.
///
/// C99 §6.7.2.2p3 requires enumerators within a single enumerator list
/// to have distinct names, and §6.4.4.3 places enumerators in the
/// ordinary identifier namespace — so repeating an enumerator already
/// declared at the same scope (even via a previous `enum` definition)
/// is a constraint violation. `rcc` flags the offending enumerator and
/// keeps the first binding.
pub const E0078: &str = "E0078";
const E0078_DESC: &str = "duplicate enumerator name";

/// Invalid bit-field width.
///
/// C99 §6.7.2.1 constrains bit-field widths: the width shall be a
/// non-negative integer constant expression, and shall not exceed the
/// width of the underlying integer type. A width of zero is allowed
/// only for an anonymous bit-field (declarator-less separator that
/// forces alignment to the next storage unit). `rcc` rejects negative
/// widths, widths larger than the type's bit-width, and zero widths on
/// named bit-fields with this code.
pub const E0077: &str = "E0077";
const E0077_DESC: &str = "invalid bit-field width";

/// Invalid initializer designator or excess initializer.
///
/// C99 §6.7.8p7 constrains designators to match the current aggregate:
/// `[N]` selects array elements and `.name` selects members of a
/// struct/union. An initializer that uses the wrong designator kind,
/// names a missing field, or selects past a known bound cannot be
/// lowered without losing source intent, so `rcc` rejects it here.
pub const E0079: &str = "E0079";
const E0079_DESC: &str = "invalid initializer designator";

/// Assignment to an rvalue or other non-modifiable lvalue.
///
/// C99 §6.5.16p2 requires the left operand of a simple or compound
/// assignment to be a modifiable lvalue. The narrower constraint that
/// the LHS be an lvalue at all is checked first: writing to the
/// result of a cast (`(int)x = 1;`), an arithmetic expression, a
/// literal, or any other rvalue is rejected with this code. The
/// "modifiable" half (const-qualified objects, array types, etc.)
/// piggybacks on this code in task 07-05.
pub const E0080: &str = "E0080";
const E0080_DESC: &str = "assignment to non-modifiable lvalue";

/// Incompatible types in assignment.
///
/// C99 §6.5.16.1p1 enumerates the only legal RHS shapes for a simple
/// assignment, function-call argument, return statement, or
/// initializer:
///
/// - both operands are arithmetic types (the RHS may need
///   conversion — narrowing is flagged with W0008, not E0081);
/// - both operands are compatible struct or union types (modulo
///   qualifier expansion on the LHS pointee);
/// - both operands are pointers to compatible types, with the LHS's
///   pointee qualifier set including every qualifier on the RHS's
///   pointee;
/// - one operand is a pointer to an object/incomplete type and the
///   other is a pointer to (qualified or unqualified) `void`;
/// - the LHS is a pointer and the RHS is a null pointer constant
///   (an integer constant expression with value 0, optionally cast
///   to `void *`);
/// - the LHS is `_Bool` and the RHS is any pointer.
///
/// Anything else is a constraint violation. `rcc` reports it with
/// this code at the assignment / initializer / argument site.
pub const E0081: &str = "E0081";
const E0081_DESC: &str = "incompatible types in assignment";

/// Incompatible pointer conversion.
///
/// C99 §6.3.2.3 enumerates the only legal implicit conversions
/// between pointer types:
///
/// - any pointer to (qualified or unqualified) `void` may be
///   converted to/from a pointer to any object/incomplete type, with
///   qualifier additions on the destination side allowed but
///   qualifier *removals* requiring an explicit cast;
/// - a null pointer constant (the integer constant `0`, optionally
///   cast to `void *`) converts to any pointer type;
/// - two pointers to *compatible* types (in the §6.7.5 sense) are
///   interchangeable when the destination's pointee qualifier set
///   includes every qualifier of the source's pointee;
/// - two pointers to function types are interchangeable iff the
///   function types are compatible (return type + parameter list).
///
/// All other pointer-to-pointer conversions — `int*` ↔ `char*`,
/// dropping a `const` qualifier without an explicit cast,
/// converting between function pointers with mismatched signatures,
/// or going between integer and pointer without a cast — are
/// constraint violations and are reported with this code at the
/// conversion site.
pub const E0082: &str = "E0082";
const E0082_DESC: &str = "incompatible pointer conversion";

/// Invalid operands to a binary operator (C99 §6.5.5–§6.5.14).
///
/// Raised by the type-checker when the operand types of a binary
/// operator do not match any rule the operator allows. Examples:
///
/// * `s1 / s2` where `s1` and `s2` are struct values — the arithmetic
///   operators `* / + -` require arithmetic operands (or, for `+`/`-`,
///   one pointer + one integer).
/// * `p & q` where either operand is not an integer — the bitwise
///   operators `& | ^ << >>` require integer operands.
/// * `p % i` where `p` is a pointer — `%` is integer-only.
/// * `&&` / `||` / `?:` / `!` / equality / relational with operands
///   whose types cannot be brought into a common scalar type.
///
/// The diagnostic is emitted at the operator's span; the operand
/// types are included in the message so the user can see what the
/// type-checker inferred for each side.
pub const E0083: &str = "E0083";
const E0083_DESC: &str = "invalid operands to binary operator";

/// Non-constant expression in a static or thread-storage initializer.
///
/// C99 §6.7.8p4: "All the expressions in an initializer for an object
/// that has static or thread storage duration shall be constant
/// expressions or string literals." A global / file-scope object's
/// initializer must therefore reduce, after the type-checker's
/// implicit conversions, to a value the constant-expression evaluator
/// (C99 §6.6) can fold:
///
/// - an integer constant expression (§6.6p6),
/// - an arithmetic constant expression (§6.6p7), or
/// - an address constant (§6.6p8) — `&obj`, `&arr[ice]`, function
///   designator, or `(T*)0 + ice`.
///
/// Anything else — a function call, a reference to a non-static local,
/// a `*p` or `++x`, an arbitrary side-effecting comma — is a constraint
/// violation. `rcc` emits this code at the offending sub-expression's
/// span and continues so later passes still see the partially-typed
/// initializer.
pub const E0084: &str = "E0084";
const E0084_DESC: &str = "non-constant expression in static initializer";

/// `sizeof` operand has no complete object layout.
///
/// C99 §6.5.3.4 requires `sizeof` to operate on a complete object
/// type, except that VLA operands are evaluated at runtime and still
/// need a known element layout. The CFG pass emits this error before
/// codegen when layout cannot be computed without silently producing
/// an incorrect zero-size object.
pub const E0085: &str = "E0085";
const E0085_DESC: &str = "sizeof operand has no complete object layout";

/// Invalid `case` / `default` label placement inside switch lowering.
///
/// C99 §6.8.4.2 requires every `case` and `default` label to appear
/// within the body of an enclosing `switch`. Each switch may contain at
/// most one `default` label, and no two `case` labels in the same switch
/// may have the same constant value.
pub const E0086: &str = "E0086";
const E0086_DESC: &str = "invalid switch label";

/// Invalid struct or union member access.
///
/// C99 §6.5.2.3 constrains `.` to a struct/union object and `->` to a
/// pointer to a struct/union object. The named member must exist in that
/// record type. `rcc` resolves member names during type checking so CFG
/// receives only numeric field indices.
pub const E0087: &str = "E0087";
const E0087_DESC: &str = "invalid member access";

/// Typed-HIR invariant violation at the typeck -> CFG/codegen boundary.
///
/// This is an internal phase-boundary diagnostic rather than a primary
/// source-language constraint: if type checking completed without any
/// user-facing errors, no expression, definition, initializer leaf, or
/// unresolved placeholder is allowed to carry `Ty::Error` into CFG or
/// LLVM codegen. Seeing this code means a previous phase accepted a
/// construct without assigning it a real type or without feature-gating
/// the unsupported shape.
pub const E0088: &str = "E0088";
const E0088_DESC: &str = "typed HIR invariant violation";

// ── Warning block: W0001.. ──────────────────────────────────────────

/// Unknown `#pragma` directive — accepted but ignored.
///
/// C99 §6.10.6 allows implementation-defined pragmas; any pragma
/// `rcc` does not recognise (anything other than `once` or the
/// standard `STDC *` family) is dropped with a warning rather than
/// treated as an error. Does **not** count toward
/// `Handler::has_errors`.
pub const W0001: &str = "W0001";
const W0001_DESC: &str = "unknown #pragma directive";

/// Floating constant overflowed `double` and was clamped to `±infinity`.
///
/// C99 §6.4.4.2p3 says a floating constant whose value is outside the
/// range of representable values of its type has undefined behavior;
/// `rcc` follows the common host-parser convention of converting such
/// a literal to `±infinity` (IEEE 754) and warning the user rather
/// than hard-erroring. Emitted by `decode_float` whenever the
/// post-decode magnitude compares infinite while the source spelling
/// was a normal pp-number (the source grammar has no way to write
/// `infinity` directly).
pub const W0002: &str = "W0002";
const W0002_DESC: &str = "float literal overflow";

/// Multi-character character constant — implementation-defined value.
///
/// C99 §6.4.4.4p10: "An integer character constant has type `int`. The
/// value of an integer character constant containing a single character
/// that maps to a single-byte execution character is the numerical
/// value of the representation of the mapped character. The value of
/// an integer character constant containing more than one character
/// (e.g. `'ab'`), or containing a character or escape sequence that
/// does not map to a single-byte execution character, is
/// implementation-defined." `rcc` packs the constituent bytes
/// big-endian (so `'ab'` evaluates to `0x6162`) and warns — silently
/// picking an implementation-defined value is a well-known footgun
/// that has surprised users of every major C compiler.
pub const W0003: &str = "W0003";
const W0003_DESC: &str = "multi-character constant";

/// Redundant (duplicated) type qualifier or function specifier.
///
/// C99 §6.7.3p4 explicitly permits repeating the same type qualifier
/// in a declaration ("If the same qualifier appears more than once in
/// the same specifier-qualifier-list, either directly or via one or
/// more typedefs, the behavior is the same as if it appeared only
/// once"), and §6.7.4p5 says the same thing for `inline`. Repetition
/// is therefore well-formed, but it is almost always a mistake — the
/// parser accepts it and warns so the duplicate stands out in tooling
/// output.
pub const W0004: &str = "W0004";
const W0004_DESC: &str = "duplicate type qualifier or function specifier";

/// K&R-style (old-style) function definition.
///
/// C99 §6.9.1p6 still permits old-style (K&R) function definitions
/// where the parameter types are declared between the declarator's
/// closing `)` and the opening `{` of the body. This style is
/// obsolescent and should be rewritten using prototype syntax.
pub const W0005: &str = "W0005";
const W0005_DESC: &str = "K&R function definition is obsolete";

/// Permissive macro redefinition (GNU extension).
///
/// With `gnu_permissive_redefinition` enabled, a non-identical
/// `#define` that preserves the macro's kind (object-like ↔
/// object-like, or function-like with identical arity and
/// variadicity) is accepted with a warning instead of the strict
/// C99 E0022 error. The new definition silently replaces the old
/// one, matching GCC / Clang behaviour.
pub const W0006: &str = "W0006";
const W0006_DESC: &str = "macro redefined with a different body (permissive)";

/// Enumerator value is outside the range of `int`.
///
/// C99 §6.7.2.2p2 requires each enumerator's value to be representable
/// as `int`; §6.7.2.2p4 then lets the implementation pick any integer
/// type wide enough to hold every enumerator of the enumeration. In
/// M4 `rcc` always uses `int` as the underlying representation, so an
/// explicit value that falls outside `[INT_MIN, INT_MAX]` — or a
/// defaulted value that would overflow via `prev + 1` — is truncated
/// and flagged with this warning. M6 will promote the rule to the
/// §6.7.2.2p4 selection algorithm and drop the diagnostic.
pub const W0007: &str = "W0007";
const W0007_DESC: &str = "enumerator value outside the range of `int`";

/// Implicit narrowing conversion in an assignment / initializer /
/// argument / return.
///
/// The C99 §6.5.16.1 assignment compatibility rules accept any
/// arithmetic-to-arithmetic conversion, but a great many of those
/// conversions silently lose information at run time — `int x = 1.5;`
/// drops the fractional part, `unsigned char b = 300;` truncates to
/// `44`, `int n = 1ULL << 40;` discards the high bits. `rcc` follows
/// every other modern C compiler and warns at compile time when the
/// destination type cannot represent the full range / precision of the
/// source type. The conversion is still performed; the warning gives
/// the user a chance to add an explicit cast or fix the type. Task
/// 07-05 introduces the warning; task 07-07 wires it to the implicit
/// `Convert` insertion pass.
pub const W0008: &str = "W0008";
const W0008_DESC: &str = "implicit conversion narrows value";

/// Integer overflow while folding an integer constant expression.
///
/// C99 §6.5p5 makes signed integer overflow undefined behaviour, but the
/// constant-expression evaluator (C99 §6.6) folds constant arithmetic at
/// compile time on a 128-bit signed accumulator. When the result of `+`,
/// `-`, `*`, or `<<` overflows that accumulator — or, more commonly, the
/// destination type's range — the fold is abandoned (the expression is
/// not a usable integer constant expression) and this warning is emitted
/// at the offending operator's span. The diagnostic is informational:
/// runtime behaviour stays UB, but the user is warned that the literal
/// they wrote does not fit.
pub const W0009: &str = "W0009";
const W0009_DESC: &str = "integer overflow in constant expression";

/// Division or remainder by zero in a folded integer constant expression.
///
/// C99 §6.5.5p5 makes `a / 0` and `a % 0` undefined behaviour. The
/// constant-expression folder (C99 §6.6) cannot produce a result for
/// such an expression, so the fold returns `None` and emits this
/// warning at the operator's span. Use of the un-foldable expression in
/// a context that requires an integer constant expression — array
/// length, case label, enumerator initialiser, `#if` controlling
/// expression — escalates to a hard error in that context.
pub const W0010: &str = "W0010";
const W0010_DESC: &str = "division by zero in constant expression";

/// Shift count out of range in a folded integer constant expression.
///
/// C99 §6.5.7p3 makes `a << n` and `a >> n` undefined when the right
/// operand is negative or is greater than or equal to the width of the
/// promoted left operand's type. The constant-expression folder treats
/// any such shift as un-evaluable, returns `None`, and warns at the
/// operator's span so the user can spot the typo at compile time.
pub const W0011: &str = "W0011";
const W0011_DESC: &str = "shift count out of range in constant expression";

/// Imaginary part discarded in a complex-to-real conversion.
///
/// C99 §6.3.1.6 specifies that converting a value of complex type to a
/// real type drops the imaginary part. The conversion is well-formed,
/// but if the source value carried a non-zero imaginary component the
/// information is silently lost — the same footgun shape as W0008's
/// arithmetic narrowing. `rcc` emits this warning whenever the
/// type-checker inserts a `ConvertKind::ComplexToReal` wrapper, both
/// for explicit casts (`(double)complex_value`) and for the implicit
/// case (assigning a complex source into a real destination).
pub const W0012: &str = "W0012";
const W0012_DESC: &str = "imaginary part discarded in complex-to-real conversion";

/// GNU statement expression accepted in strict C99 mode.
///
/// `({ ... })` is a GNU C extension that evaluates a compound
/// statement as an expression. The parser accepts it so real-world
/// GNU-flavoured sources can keep their AST shape, but emits this
/// warning unless `Options::gnu_statement_expressions` is enabled.
pub const W0013: &str = "W0013";
const W0013_DESC: &str = "GNU statement expression extension";

/// GNU initializer range designator accepted in strict C99 mode.
///
/// `[lo ... hi] = value` is a GNU C designated-initializer extension
/// that initialises a contiguous subrange. The parser accepts it as an
/// explicit range node and emits this warning unless
/// `Options::gnu_range_designators` is enabled.
pub const W0014: &str = "W0014";
const W0014_DESC: &str = "GNU initializer range designator extension";

/// GNU `__attribute__((...))` accepted in strict C99 mode.
///
/// The parser preserves the syntax for later extension semantics, but
/// C99 does not define attributes. This warning is suppressed when
/// `Options::gnu_attributes` is enabled.
pub const W0015: &str = "W0015";
const W0015_DESC: &str = "GNU attribute syntax extension";

/// GNU inline assembly accepted in strict C99 mode.
///
/// The parser preserves the syntax for later extension semantics and
/// codegen lowering, but C99 does not define inline assembly. This
/// warning is suppressed when `Options::gnu_inline_asm` is enabled.
pub const W0016: &str = "W0016";
const W0016_DESC: &str = "GNU inline assembly syntax extension";

/// GNU omitted-middle conditional accepted in strict C99 mode.
///
/// `a ?: b` is a GNU C extension equivalent to `a ? a : b`, except
/// the first operand is evaluated exactly once. The parser accepts it
/// as an explicit node and emits this warning unless
/// `Options::gnu_omitted_conditional_operand` is enabled.
pub const W0017: &str = "W0017";
const W0017_DESC: &str = "GNU omitted conditional operand extension";

/// GNU conditional expression with exactly one void arm.
///
/// C99 requires both `?:` result operands to be void when either is void.
/// GNU C accepts a single void arm and gives the whole conditional type
/// `void`. The type checker emits this warning unless
/// `Options::gnu_conditional_void_operand` is enabled.
pub const W0018: &str = "W0018";
const W0018_DESC: &str = "GNU conditional expression with one void operand";

/// GNU case range accepted in strict C99 mode.
///
/// `case lo ... hi:` is a GNU C extension. The parser accepts it as an
/// explicit range node and emits this warning unless
/// `Options::gnu_case_ranges` is enabled.
pub const W0019: &str = "W0019";
const W0019_DESC: &str = "GNU case range extension";

/// GNU labels-as-values / computed goto accepted in strict C99 mode.
///
/// `&&label` and `goto *expr` are GNU C extensions. The parser accepts them
/// as explicit nodes and emits this warning unless
/// `Options::gnu_labels_as_values` is enabled.
pub const W0020: &str = "W0020";
const W0020_DESC: &str = "GNU labels-as-values extension";

/// GNU lvalue comma expression accepted in strict C99 mode.
///
/// GNU C treats a comma expression as an lvalue when its right operand is an
/// lvalue. C99 makes every comma expression an rvalue, so the type checker
/// emits this warning unless `Options::gnu_lvalue_comma` is enabled.
pub const W0021: &str = "W0021";
const W0021_DESC: &str = "GNU lvalue comma extension";

/// GNU `__FUNCTION__` predefined function name alias accepted in strict C99 mode.
///
/// C99 defines `__func__`, while GNU C also accepts `__FUNCTION__`. HIR
/// lowering preserves the alias and emits this warning unless
/// `Options::gnu_function_names` is enabled.
pub const W0022: &str = "W0022";
const W0022_DESC: &str = "GNU function name alias";

/// chibicc/GNU `__va_area__` compatibility builtin accepted in strict C99 mode.
///
/// C99 exposes variadic arguments through `<stdarg.h>` macros, not through a
/// magic identifier naming the ABI save area. HIR lowering accepts this inside
/// variadic functions for chibicc compatibility and emits this warning unless
/// `Options::gnu_va_area` is enabled.
pub const W0023: &str = "W0023";
const W0023_DESC: &str = "GNU __va_area__ compatibility builtin";

/// GNU `typeof` type specifier accepted in strict C99 mode.
///
/// `typeof (expr)` and `typeof (type-name)` are GNU C extensions. The parser
/// preserves them so compatibility declarations can reach HIR lowering and
/// emits this warning unless `Options::gnu_typeof` is enabled.
pub const W0024: &str = "W0024";
const W0024_DESC: &str = "GNU typeof type specifier extension";

/// GNU `__alignof__` expression accepted in strict C99 mode.
///
/// `__alignof__(expr)` and `__alignof__(type-name)` are GNU C extensions. The
/// parser preserves them so target-layout queries can reach HIR/CFG lowering
/// and emits this warning unless `Options::gnu_alignof` is enabled.
pub const W0025: &str = "W0025";
const W0025_DESC: &str = "GNU __alignof__ expression extension";

/// Local variable declared but never read.
///
/// Emitted only when `-Wall`, `-Wextra`, `-Wunused-variable`, or
/// `-Werror=unused-variable` enables the `unused-variable` analysis warning.
pub const W0026: &str = "W0026";
const W0026_DESC: &str = "unused local variable";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_codes_have_correct_format() {
        for &(code, desc) in ALL_CODES {
            let first = code.chars().next().expect("code is non-empty");
            assert!(
                first == 'E' || first == 'W',
                "code {code:?} must start with 'E' (error) or 'W' (warning)"
            );
            assert_eq!(code.len(), 5, "code {code:?} must be exactly 5 chars");
            assert!(
                code[1..].chars().all(|c| c.is_ascii_digit()),
                "code {code:?} digits portion must be all digits"
            );
            assert!(!desc.is_empty(), "description for {code} must not be empty");
        }
    }

    #[test]
    fn no_duplicate_codes() {
        let mut seen = std::collections::HashSet::new();
        for &(code, _) in ALL_CODES {
            assert!(seen.insert(code), "duplicate error code: {code}");
        }
    }

    #[test]
    fn codes_are_sorted_within_each_namespace() {
        // `E` and `W` codes live in disjoint spaces; the registry
        // lists every `E` first in numeric order, then every `W` in
        // numeric order. A single byte-wise sort would still hold
        // because `'E' < 'W'`, but keep the assertion per-namespace
        // so that introducing another prefix later does not quietly
        // bend the invariant.
        let check_sorted = |prefix: char| {
            let subset: Vec<&str> =
                ALL_CODES.iter().map(|&(c, _)| c).filter(|c| c.starts_with(prefix)).collect();
            for pair in subset.windows(2) {
                assert!(
                    pair[0] < pair[1],
                    "{prefix} codes must be sorted: {} should come before {}",
                    pair[0],
                    pair[1]
                );
            }
        };
        check_sorted('E');
        check_sorted('W');
    }
}
