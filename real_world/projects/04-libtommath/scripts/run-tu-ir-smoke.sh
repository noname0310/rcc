#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${project_dir}/../../.." && pwd)"
upstream="${project_dir}/upstream"

if [[ ! -d "${upstream}/.git" ]]; then
  git clone -c core.autocrlf=false --depth 1 https://github.com/libtom/libtommath.git "${upstream}"
fi
git -C "${upstream}" config core.autocrlf false

mkdir -p "${project_dir}/build/rcc/ir" "${project_dir}/logs/tu"
rm -f "${project_dir}/logs/tu/status.tsv"

rcc_bin="${RCC:-${repo_root}/target/release/rcc}"
if [[ ! -x "${rcc_bin}" ]]; then
  LLVM_SYS_181_PREFIX="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}" \
    cargo build --release -p rcc_driver --bin rcc --features llvm \
    --manifest-path "${repo_root}/Cargo.toml"
fi

cd "${upstream}"
find . -maxdepth 1 \( -name 'mp_*.c' -o -name 's_*.c' \) -printf '%f\n' | sort > ../logs/tu/sources.txt

jobs="${RCC_TU_JOBS:-4}"
timeout_s="${RCC_TU_TIMEOUT:-60s}"

compile_one() {
  local f="$1"
  local stem="${f%.c}"
  local stdout="../logs/tu/${stem}.stdout"
  local stderr="../logs/tu/${stem}.stderr"
  local out="../build/rcc/ir/${stem}.ll"
  local status=0

  timeout "${timeout_s}" "${rcc_bin}" --std=c99 -Wall --emit=llvm-ir -I. "${f}" -o "${out}" \
    > "${stdout}" 2> "${stderr}" || status=$?
  printf '%s\t%s\t%s\n' "${f}" "${status}" "$(wc -c < "${stderr}")"
}

export -f compile_one
export rcc_bin timeout_s

xargs -n1 -P "${jobs}" bash -c 'compile_one "$1"' _ < ../logs/tu/sources.txt \
  | sort -k2,2n -k1,1 > ../logs/tu/status.tsv

cat ../logs/tu/status.tsv

failures="$(awk -F '\t' '$2 != 0 { c++ } END { print c + 0 }' ../logs/tu/status.tsv)"
if [[ "${failures}" != "0" ]]; then
  echo "libtommath TU IR smoke: ${failures} failures" >&2
  awk -F '\t' '$2 != 0 { print $1 }' ../logs/tu/status.tsv | while read -r f; do
    stem="${f%.c}"
    echo "== ${f} ==" >&2
    tail -80 "../logs/tu/${stem}.stderr" >&2
  done
  exit 1
fi

echo "libtommath TU IR smoke: all translation units compiled"
