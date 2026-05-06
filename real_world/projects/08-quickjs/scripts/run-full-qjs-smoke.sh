#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${project_dir}/../../.." && pwd)"
upstream="${project_dir}/upstream"
build="${project_dir}/build/full"
logs="${project_dir}/logs"

host_cc="${HOST_CC:-cc}"
rcc="${RCC:-${repo_root}/target/debug/rcc}"
llvm_prefix="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}"

lib_sources=(
    quickjs.c
    dtoa.c
    libregexp.c
    libunicode.c
    cutils.c
    quickjs-libc.c
)

mkdir -p "${build}/host" "${build}/rcc" "${logs}"

if [ ! -f "${upstream}/quickjs.c" ] || [ ! -f "${upstream}/VERSION" ]; then
    echo "QuickJS upstream checkout is incomplete: ${upstream}" >&2
    exit 2
fi

if [ "${RCC_BUILD:-1}" != "0" ]; then
    (
        cd "${repo_root}"
        LLVM_SYS_181_PREFIX="${llvm_prefix}" cargo build -p rcc_driver --features rcc_codegen_llvm/llvm
    ) >"${logs}/full-cargo-build.stdout" 2>"${logs}/full-cargo-build.stderr"
fi

version="$(tr -d '\r\n' < "${upstream}/VERSION")"
common_flags=(
    -O2
    -fwrapv
    -funsigned-char
    -D_GNU_SOURCE
    "-DCONFIG_VERSION=\"${version}\""
    -I "${upstream}"
)
host_flags=(-std=gnu99)
rcc_flags=(
    -std=c99
    -fgnu-attributes
    -fgnu-range-designators
    -fgnu-labels-as-values
    -fgnu-inline-asm
    -fgnu-statement-expressions
    -fgnu-builtin-libcalls
)

for src in "${lib_sources[@]}" qjsc.c; do
    extra=()
    if [ "${src}" = qjsc.c ]; then
        extra=(-DCONFIG_CC=\"${host_cc}\" -DCONFIG_PREFIX=\"/usr/local\")
    fi
    "${host_cc}" "${host_flags[@]}" "${common_flags[@]}" "${extra[@]}" \
        -c "${upstream}/${src}" \
        -o "${build}/host/${src%.c}.o" \
        >"${logs}/full-host-${src}.stdout" \
        2>"${logs}/full-host-${src}.stderr"
done

"${host_cc}" -g -o "${build}/host/qjsc" \
    "${build}/host/qjsc.o" \
    "${build}/host/quickjs.o" \
    "${build}/host/dtoa.o" \
    "${build}/host/libregexp.o" \
    "${build}/host/libunicode.o" \
    "${build}/host/cutils.o" \
    "${build}/host/quickjs-libc.o" \
    -lm -ldl -lpthread \
    >"${logs}/full-host-qjsc-link.stdout" \
    2>"${logs}/full-host-qjsc-link.stderr"

"${build}/host/qjsc" -s -c -o "${build}/repl.c" -m "${upstream}/repl.js" \
    >"${logs}/full-host-qjsc-repl.stdout" \
    2>"${logs}/full-host-qjsc-repl.stderr"

for src in "${lib_sources[@]}" qjs.c; do
    LLVM_SYS_181_PREFIX="${llvm_prefix}" "${rcc}" \
        --target=x86_64-unknown-linux-gnu \
        --linux-gnu-hosted \
        "${rcc_flags[@]}" \
        "${common_flags[@]}" \
        -c "${upstream}/${src}" \
        -o "${build}/rcc/${src%.c}.o" \
        >"${logs}/full-rcc-${src}.stdout" \
        2>"${logs}/full-rcc-${src}.stderr"
done

LLVM_SYS_181_PREFIX="${llvm_prefix}" "${rcc}" \
    --target=x86_64-unknown-linux-gnu \
    --linux-gnu-hosted \
    "${rcc_flags[@]}" \
    "${common_flags[@]}" \
    -c "${build}/repl.c" \
    -o "${build}/rcc/repl.o" \
    >"${logs}/full-rcc-repl.c.stdout" \
    2>"${logs}/full-rcc-repl.c.stderr"

"${host_cc}" -g -o "${build}/rcc/qjs" \
    "${build}/rcc/qjs.o" \
    "${build}/rcc/repl.o" \
    "${build}/rcc/quickjs.o" \
    "${build}/rcc/dtoa.o" \
    "${build}/rcc/libregexp.o" \
    "${build}/rcc/libunicode.o" \
    "${build}/rcc/cutils.o" \
    "${build}/rcc/quickjs-libc.o" \
    -lm -ldl -lpthread \
    >"${logs}/full-rcc-qjs-link.stdout" \
    2>"${logs}/full-rcc-qjs-link.stderr"

"${build}/rcc/qjs" --help >"${logs}/full-rcc-qjs-help.stdout" 2>"${logs}/full-rcc-qjs-help.stderr" || true
"${build}/rcc/qjs" -e 'console.log(1 + 2)' \
    >"${logs}/full-rcc-qjs-smoke.stdout" \
    2>"${logs}/full-rcc-qjs-smoke.stderr"

actual="$(tr -d '\r\n' < "${logs}/full-rcc-qjs-smoke.stdout")"
if [ "${actual}" != "3" ]; then
    echo "QuickJS smoke expected 3, got ${actual}" >&2
    exit 1
fi

cat >"${build}/smoke.js" <<'EOF'
function fib(n) {
  return n < 2 ? n : fib(n - 1) + fib(n - 2);
}

const data = JSON.parse('{"answer":42,"items":[1,2,3]}');
let total = 0;
for (let i = 0; i < data.items.length; i++) {
  total += data.items[i];
}

console.log(["file", fib(10), data.answer, total, data.items.join("+")].join(":"));
EOF

"${build}/rcc/qjs" "${build}/smoke.js" \
    >"${logs}/full-rcc-qjs-file-smoke.stdout" \
    2>"${logs}/full-rcc-qjs-file-smoke.stderr"

actual="$(tr -d '\r\n' < "${logs}/full-rcc-qjs-file-smoke.stdout")"
if [ "${actual}" != "file:55:42:6:1+2+3" ]; then
    echo "QuickJS file smoke expected file:55:42:6:1+2+3, got ${actual}" >&2
    exit 1
fi

echo "quickjs full qjs smoke ok"
