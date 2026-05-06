#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../../../.." && pwd)"
rcc="${RCC:-${repo_root}/target/debug/rcc}"
host_cc="${HOST_CC:-cc}"

if [ "${1:-}" = "--version" ]; then
    echo "rcc toybox wrapper"
    exit 0
fi

for arg in "$@"; do
    case "${arg}" in
        -|-E|-dM|-xc)
            exec "${host_cc}" "$@"
            ;;
    esac
done

filtered=()
for arg in "$@"; do
    case "${arg}" in
        -O|-O[0-9sgz]*|-Wall|-W*|-ffunction-sections|-fdata-sections|\
        -fno-asynchronous-unwind-tables|-fno-strict-aliasing|-funsigned-char)
            ;;
        *)
            filtered+=("${arg}")
            ;;
    esac
done

exec "${rcc}" \
    --target=x86_64-unknown-linux-gnu \
    --linux-gnu-hosted \
    -std=c11 \
    -D_GNU_SOURCE \
    -D_DEFAULT_SOURCE \
    -D_XOPEN_SOURCE=700 \
    -funsigned-char \
    -fgnu-attributes \
    -fgnu-named-variadic \
    -fgnu-permissive-redefinition \
    -fgnu-statement-expressions \
    -fgnu-labels-as-values \
    -fgnu-inline-asm \
    -fgnu-builtin-libcalls \
    "${filtered[@]}"
