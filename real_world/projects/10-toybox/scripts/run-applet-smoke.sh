#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${project_dir}/../../.." && pwd)"
upstream="${project_dir}/upstream"
build="${project_dir}/build/applet-smoke"
logs="${project_dir}/logs"

upstream_url="${TOYBOX_URL:-https://github.com/landley/toybox}"
host_cc="${HOST_CC:-cc}"
rcc="${RCC:-${repo_root}/target/debug/rcc}"
llvm_prefix="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}"
applets=(true false echo cat wc)

ensure_upstream() {
    if [ ! -d "${upstream}/.git" ]; then
        git -c core.autocrlf=false clone --depth 1 "${upstream_url}" "${upstream}"
    fi
    git -C "${upstream}" config core.autocrlf false
    git -C "${upstream}" reset --hard HEAD >/dev/null
}

build_rcc_if_needed() {
    if [ "${RCC_BUILD:-1}" = "0" ]; then
        return
    fi
    (
        cd "${repo_root}"
        LLVM_SYS_181_PREFIX="${llvm_prefix}" \
            cargo build -p rcc_driver --features rcc_codegen_llvm/llvm
    ) >"${logs}/cargo-build-rcc.stdout" 2>"${logs}/cargo-build-rcc.stderr"
}

build_applets() {
    local name="$1"
    local cc="$2"
    local out_dir="${build}/${name}"

    rm -rf "${out_dir}"
    mkdir -p "${out_dir}"
    (
        cd "${upstream}"
        make distclean >/dev/null 2>&1 || true
        make defconfig
        PREFIX="${out_dir}/" CC="${cc}" HOSTCC="${host_cc}" \
            scripts/single.sh "${applets[@]}"
    ) >"${logs}/${name}-build.stdout" 2>"${logs}/${name}-build.stderr"
}

capture() {
    local name="$1"
    local applet="$2"
    shift 2
    local out="${logs}/${name}-${applet}.stdout"
    local err="${logs}/${name}-${applet}.stderr"
    local status_file="${logs}/${name}-${applet}.status"

    set +e
    "$@" >"${out}" 2>"${err}"
    local status=$?
    set -e
    printf "%s\n" "${status}" >"${status_file}"
}

run_applets() {
    local name="$1"
    local dir="${build}/${name}"
    local input="${build}/input.txt"
    printf "abc\ndef\n" >"${input}"

    capture "${name}" true "${dir}/true"
    capture "${name}" false "${dir}/false"
    capture "${name}" echo "${dir}/echo" hello toybox
    capture "${name}" cat "${dir}/cat" "${input}"
    capture "${name}" wc "${dir}/wc" "${input}"
}

compare_one() {
    local applet="$1"
    cmp -s "${logs}/host-${applet}.status" "${logs}/rcc-${applet}.status"
    cmp -s "${logs}/host-${applet}.stdout" "${logs}/rcc-${applet}.stdout"
    cmp -s "${logs}/host-${applet}.stderr" "${logs}/rcc-${applet}.stderr"
}

main() {
    mkdir -p "${build}" "${logs}"
    ensure_upstream
    build_rcc_if_needed

    export RCC="${rcc}"
    local rcc_cc="${script_dir}/rcc-cc.sh"

    build_applets host "${host_cc}"
    build_applets rcc "${rcc_cc}"
    run_applets host
    run_applets rcc

    for applet in "${applets[@]}"; do
        compare_one "${applet}"
    done

    echo "toybox applet smoke ok (${applets[*]})"
}

main "$@"
