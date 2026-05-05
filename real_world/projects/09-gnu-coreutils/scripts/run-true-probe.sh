#!/usr/bin/env bash
set -u

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "$script_dir/.." && pwd)"
repo_root="$(cd "$project_dir/../../.." && pwd)"

work_root="${RCC_COREUTILS_WORK_ROOT:-$project_dir/build/gnulib-config-probe}"
logs="$project_dir/logs/true-probe"
src_work="$work_root/src"
build_dir="$work_root/build"
rcc="${RCC:-$repo_root/target/debug/rcc}"
llvm_prefix="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}"

usage() {
    cat <<'USAGE'
Usage: run-true-probe.sh [--dry-run]

Uses the ignored GNU coreutils bootstrap/configure worktree from the gnulib
config probe, generates the small replacement headers needed by src/true.c,
then asks rcc to lower src/true.c.  The script never edits upstream sources or
generated headers in place; it only invokes project build targets and writes
logs under real_world/projects/09-gnu-coreutils/logs/true-probe/.

Environment:
  RCC                         rcc binary path (default: target/debug/rcc)
  LLVM_SYS_181_PREFIX         LLVM prefix for local rcc builds
  RCC_BUILD                   build rcc first unless set to 0
  RCC_COREUTILS_WORK_ROOT     ignored gnulib-config-probe work directory
USAGE
}

dry_run=0
for arg in "$@"; do
    case "$arg" in
        --dry-run) dry_run=1 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown argument: $arg" >&2; usage >&2; exit 2 ;;
    esac
done

mkdir -p "$logs"

log_path() {
    printf '%s/%s' "$logs" "$1"
}

config_h="$build_dir/lib/config.h"
if [ ! -f "$config_h" ]; then
    bash "$script_dir/run-gnulib-config-probe.sh" \
        >"$(log_path setup-config.stdout)" \
        2>"$(log_path setup-config.stderr)"
    setup_status=$?
    if [ ! -f "$config_h" ]; then
        echo "generated config.h is missing after setup probe" >&2
        exit "$setup_status"
    fi
fi

generated_targets=(
    lib/configmake.h
    lib/stdio.h
    lib/string.h
    lib/uchar.h
    lib/unistd.h
    lib/wchar.h
    src/version.h
)

if [ "$dry_run" -eq 1 ]; then
    cat <<EOF
worktree=$src_work
build_dir=$build_dir
logs=$logs
rcc=$rcc
generated_targets=${generated_targets[*]}
EOF
    exit 0
fi

if [ ! -f "$src_work/src/true.c" ]; then
    echo "coreutils true.c is missing: $src_work/src/true.c" >&2
    exit 2
fi

if [ "${RCC_BUILD:-1}" != "0" ]; then
    (
        cd "$repo_root" &&
        LLVM_SYS_181_PREFIX="$llvm_prefix" cargo build -p rcc_driver --features rcc_codegen_llvm/llvm
    ) >"$(log_path cargo-build.stdout)" 2>"$(log_path cargo-build.stderr)" || exit $?
fi

make -C "$build_dir" "${generated_targets[@]}" \
    >"$(log_path make-generated-headers.stdout)" \
    2>"$(log_path make-generated-headers.stderr)" || exit $?

"$rcc" \
    --target=x86_64-unknown-linux-gnu \
    --linux-gnu-hosted \
    --emit=hir \
    -I "$build_dir/lib" \
    -I "$build_dir/src" \
    -I "$src_work/lib" \
    -I "$src_work/src" \
    -I "$src_work/gl/lib" \
    -I "$src_work" \
    -o "$work_root/true.hir" \
    "$src_work/src/true.c" \
    >"$(log_path rcc-true.stdout)" \
    2>"$(log_path rcc-true.stderr)"
