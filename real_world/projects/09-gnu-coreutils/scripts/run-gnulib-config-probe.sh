#!/usr/bin/env bash
set -u

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "$script_dir/.." && pwd)"
repo_root="$(cd "$project_dir/../../.." && pwd)"

upstream="$project_dir/upstream"
work_root="${RCC_COREUTILS_WORK_ROOT:-$project_dir/build/gnulib-config-probe}"
logs="$project_dir/logs/gnulib-config-probe"
scratch="$project_dir/scratch"
src_work="$work_root/src"
build_dir="$work_root/build"
install_dir="$work_root/install"
rcc="${RCC:-$repo_root/target/debug/rcc}"
host_cc="${HOST_CC:-cc}"

usage() {
    cat <<'USAGE'
Usage: run-gnulib-config-probe.sh [--dry-run]

Creates an ignored LF-normalized coreutils worktree, runs bootstrap/configure
when needed, then asks rcc to parse/lower a wrapper translation unit that
includes generated config.h and src/system.h.

Environment:
  RCC                         rcc binary path (default: target/debug/rcc)
  HOST_CC                     host compiler for configure (default: cc)
  RCC_COREUTILS_WORK_ROOT     ignored work directory
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

mkdir -p "$work_root" "$logs" "$scratch"

log_path() {
    printf '%s/%s' "$logs" "$1"
}

write_blocker() {
    local name="$1"
    local message="$2"
    {
        printf 'blocker=%s\n' "$name"
        printf 'task=tasks/16-linux-glibc-compat/16-gnu-coreutils-bootstrap-probe.md\n'
        printf 'message=%s\n' "$message"
    } >"$(log_path blocker.env)"
}

if [ ! -d "$upstream/.git" ]; then
    write_blocker "missing-upstream" "coreutils upstream clone is missing"
    echo "coreutils upstream clone is missing: $upstream" >&2
    exit 2
fi

if [ "$dry_run" -eq 1 ]; then
    cat <<EOF
upstream=$upstream
worktree=$src_work
build_dir=$build_dir
logs=$logs
wrapper=$scratch/gnulib-config-wrapper.c
rcc=$rcc
host_cc=$host_cc
EOF
    exit 0
fi

if [ ! -d "$src_work/.git" ]; then
    rm -rf "$src_work"
    git -c core.autocrlf=false clone --recurse-submodules "$upstream" "$src_work" \
        >"$(log_path clone.stdout)" 2>"$(log_path clone.stderr)" || exit $?
fi

if [ ! -f "$src_work/configure" ]; then
    missing_tools=()
    for tool in autoconf automake aclocal autopoint; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            missing_tools+=("$tool")
        fi
    done
    if [ "${#missing_tools[@]}" -ne 0 ]; then
        write_blocker "missing-bootstrap-tools" "missing tools: ${missing_tools[*]}"
        echo "missing bootstrap tools: ${missing_tools[*]}" >&2
        echo "see tasks/16-linux-glibc-compat/16-gnu-coreutils-bootstrap-probe.md" >&2
        exit 77
    fi
    (
        cd "$src_work" &&
        ./bootstrap --skip-po
    ) >"$(log_path bootstrap.stdout)" 2>"$(log_path bootstrap.stderr)" || exit $?
fi

if [ ! -f "$build_dir/config.status" ]; then
    mkdir -p "$build_dir"
    (
        cd "$build_dir" &&
        "$src_work/configure" \
            --disable-nls \
            --without-gmp \
            --without-selinux \
            --prefix="$install_dir" \
            CC="$host_cc"
    ) >"$(log_path configure.stdout)" 2>"$(log_path configure.stderr)" || exit $?
fi

config_dir=""
for candidate in "$build_dir/lib" "$build_dir"; do
    if [ -f "$candidate/config.h" ]; then
        config_dir="$candidate"
        break
    fi
done
if [ -z "$config_dir" ]; then
    write_blocker "missing-config-h" "configure completed but no generated config.h was found"
    echo "configure completed, but generated config.h was not found" >&2
    exit 2
fi

cat >"$scratch/gnulib-config-wrapper.c" <<'EOF'
#include "config.h"
#include "system.h"

int rcc_gnulib_config_probe(void) {
#ifdef HAVE_DIRENT_H
    return 0;
#else
    return 1;
#endif
}
EOF

"$rcc" \
    --target=x86_64-unknown-linux-gnu \
    --linux-gnu-hosted \
    --emit=hir \
    -I "$config_dir" \
    -I "$build_dir/lib" \
    -I "$src_work/lib" \
    -I "$src_work/src" \
    -I "$src_work/gl/lib" \
    -I "$src_work" \
    -o "$work_root/gnulib-config-wrapper.hir" \
    "$scratch/gnulib-config-wrapper.c" \
    >"$(log_path rcc-config-wrapper.stdout)" \
    2>"$(log_path rcc-config-wrapper.stderr)"
