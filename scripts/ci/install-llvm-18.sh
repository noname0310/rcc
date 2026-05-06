#!/usr/bin/env bash
set -euo pipefail

prefix=/usr/lib/llvm-18
lib_dir=${prefix}/lib

emit_prefix() {
  if [[ -n "${GITHUB_ENV:-}" ]]; then
    echo "LLVM_SYS_181_PREFIX=${prefix}" >> "${GITHUB_ENV}"
  fi
  echo "LLVM_SYS_181_PREFIX=${prefix}"
}

has_complete_llvm_18() {
  [[ -x "${prefix}/bin/llvm-config" ]] &&
    [[ -f "${lib_dir}/libPolly.a" ]] &&
    [[ -f "${lib_dir}/libPollyISL.a" ]]
}

if has_complete_llvm_18; then
  "${prefix}/bin/llvm-config" --version
  emit_prefix
  exit 0
fi

retry() {
  local max_attempts="$1"
  shift
  local attempt=1
  until "$@"; do
    if (( attempt >= max_attempts )); then
      return 1
    fi
    echo "command failed, retrying (${attempt}/${max_attempts}): $*" >&2
    sleep $((attempt * 5))
    attempt=$((attempt + 1))
  done
}

export DEBIAN_FRONTEND=noninteractive

# Ubuntu 24.04 ships LLVM 18 packages. Prefer the runner's default apt
# sources before adding apt.llvm.org; this avoids the flaky llvm.sh path when
# the hosted image already has suitable package metadata.
retry 2 timeout 180s sudo apt-get -o Acquire::Retries=3 update || true
if ! retry 3 timeout 240s sudo apt-get install -y --no-install-recommends \
  llvm-18-dev \
  libpolly-18-dev \
  clang-18 \
  lld-18; then
  echo "default Ubuntu LLVM 18 install failed; falling back to apt.llvm.org" >&2
  curl -fsSL --retry 5 --retry-delay 2 https://apt.llvm.org/llvm.sh -o llvm.sh
  chmod +x llvm.sh
  retry 2 timeout 360s sudo ./llvm.sh 18
  retry 3 timeout 240s sudo apt-get install -y --no-install-recommends \
    llvm-18-dev \
    libpolly-18-dev \
    clang-18 \
    lld-18
fi

if ! has_complete_llvm_18; then
  echo "LLVM 18 install is incomplete: expected llvm-config plus libPolly.a and libPollyISL.a under ${prefix}" >&2
  exit 1
fi

"${prefix}/bin/llvm-config" --version
emit_prefix
