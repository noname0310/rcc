#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "$script_dir/.." && pwd)"
repo_root="$(cd "$project_dir/../../.." && pwd)"
upstream="$project_dir/upstream"
build="$project_dir/build"
logs="$project_dir/logs"

host_cc="${HOST_CC:-cc}"
rcc="${RCC:-$repo_root/target/debug/rcc}"
llvm_prefix="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}"

sources=(
    quickjs.c
    dtoa.c
    libregexp.c
    libunicode.c
    cutils.c
    quickjs-libc.c
)

mkdir -p "$build/host" "$build/rcc" "$logs"

if [ ! -f "$upstream/quickjs.c" ] || [ ! -f "$upstream/VERSION" ]; then
    echo "QuickJS upstream checkout is incomplete: $upstream" >&2
    exit 2
fi

if [ "${RCC_BUILD:-1}" != "0" ]; then
    (
        cd "$repo_root"
        LLVM_SYS_181_PREFIX="$llvm_prefix" cargo build -p rcc_driver --features rcc_codegen_llvm/llvm
    ) >"$logs/cargo-build-rcc.stdout" 2>"$logs/cargo-build-rcc.stderr"
fi

version="$(tr -d '\r\n' < "$upstream/VERSION")"
common_flags=(
    -O2
    -fwrapv
    -funsigned-char
    -D_GNU_SOURCE
    "-DCONFIG_VERSION=\"$version\""
    -I "$upstream"
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

for src in "${sources[@]}"; do
    obj="${src%.c}.o"
    echo "host $src"
    "$host_cc" "${host_flags[@]}" "${common_flags[@]}" -c "$upstream/$src" -o "$build/host/$obj" \
        >"$logs/host-$src.stdout" 2>"$logs/host-$src.stderr"

    echo "rcc  $src"
    LLVM_SYS_181_PREFIX="$llvm_prefix" "$rcc" \
        --target=x86_64-unknown-linux-gnu \
        --linux-gnu-hosted \
        "${rcc_flags[@]}" \
        "${common_flags[@]}" \
        -c "$upstream/$src" \
        -o "$build/rcc/$obj" \
        >"$logs/rcc-$src.stdout" \
        2>"$logs/rcc-$src.stderr"
done

echo "quickjs object probe ok (${#sources[@]} translation units)"
