#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${project_dir}/../../.." && pwd)"

curl_src="${CURL_SRC:-${HOME}/work-curl-rcc-20260505/curl}"
build_dir="${project_dir}/build"
logs_dir="${project_dir}/logs"
artifacts_dir="${project_dir}/artifacts"

rcc_bin="${RCC:-${repo_root}/target/debug/rcc}"
llvm_prefix="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}"
linker_driver="${RCC_LINKER_DRIVER:-/usr/bin/clang-18}"
make_jobs="${MAKEFLAGS_J:-4}"
network_smoke="${NETWORK_SMOKE:-1}"

mkdir -p "${build_dir}" "${logs_dir}" "${artifacts_dir}"

if [[ ! -f "${curl_src}/CMakeLists.txt" ]] || [[ ! -d "${curl_src}/lib" ]] \
    || [[ ! -d "${curl_src}/src" ]]; then
    echo "curl source tree is incomplete: ${curl_src}" >&2
    echo "Set CURL_SRC to a clone of https://github.com/curl/curl" >&2
    exit 2
fi

rcc_build_mode="${RCC_BUILD:-auto}"
case "${rcc_build_mode}" in
    1 | true | yes)
        should_build_rcc=1
        ;;
    0 | false | no)
        should_build_rcc=0
        ;;
    auto)
        if [[ -x "${rcc_bin}" ]]; then
            should_build_rcc=0
        else
            should_build_rcc=1
        fi
        ;;
    *)
        echo "Unsupported RCC_BUILD mode: ${rcc_build_mode}" >&2
        echo "Use RCC_BUILD=auto, RCC_BUILD=1, or RCC_BUILD=0." >&2
        exit 2
        ;;
esac

if [[ "${should_build_rcc}" -eq 1 ]]; then
    (
        cd "${repo_root}"
        LLVM_SYS_181_PREFIX="${llvm_prefix}" \
        RCC_LINKER_DRIVER="${linker_driver}" \
            cargo build -p rcc_driver --bin rcc --features llvm
    ) >"${logs_dir}/cargo-build-rcc.stdout" \
     2>"${logs_dir}/cargo-build-rcc.stderr"
fi

gnu_flags=(
    -fgnu-named-variadic
    -fgnu-va-args-elision
    -fgnu-permissive-paste
    -fgnu-attributes
    -fgnu-typeof
    -fgnu-alignof
    -fgnu-statement-expressions
    -fgnu-omitted-conditional-operand
    -fgnu-conditional-void-operand
    -fgnu-range-designators
    -fgnu-case-ranges
    -fgnu-labels-as-values
    -fgnu-lvalue-comma
    -fgnu-pragma-pack
    -fgnu-function-names
    -fgnu-va-area
    -fgnu-builtin-libcalls
)

c_flags="${gnu_flags[*]} -std=c99 -D_GNU_SOURCE -fvisibility=hidden"

cmake_dir="${build_dir}/cmake"
rm -rf "${cmake_dir}"
mkdir -p "${cmake_dir}"

(
    cd "${cmake_dir}"
    LLVM_SYS_181_PREFIX="${llvm_prefix}" \
    RCC_LINKER_DRIVER="${linker_driver}" \
        cmake \
            -DCMAKE_C_COMPILER="${rcc_bin}" \
            -DCMAKE_C_FLAGS="${c_flags}" \
            -DCMAKE_C_FLAGS_RELEASE="-DNDEBUG" \
            -DCMAKE_BUILD_TYPE=Release \
            -DBUILD_SHARED_LIBS=OFF \
            -DCMAKE_DISABLE_FIND_PACKAGE_Threads=TRUE \
            -DCURL_USE_OPENSSL=OFF \
            -DCURL_USE_LIBSSH2=OFF \
            -DCURL_USE_LIBPSL=OFF \
            -DCURL_ZLIB=OFF \
            -DCURL_BROTLI=OFF \
            -DCURL_ZSTD=OFF \
            -DUSE_LIBIDN2=OFF \
            -DUSE_NGHTTP2=OFF \
            -DENABLE_THREADED_RESOLVER=OFF \
            -DENABLE_UNIX_SOCKETS=OFF \
            -DENABLE_IPV6=OFF \
            -DCURL_DISABLE_LDAP=ON \
            -DCURL_DISABLE_LDAPS=ON \
            -DCURL_DISABLE_FTP=ON \
            -DCURL_DISABLE_SMTP=ON \
            -DCURL_DISABLE_IMAP=ON \
            -DCURL_DISABLE_POP3=ON \
            -DCURL_DISABLE_GOPHER=ON \
            -DCURL_DISABLE_RTSP=ON \
            -DCURL_DISABLE_TELNET=ON \
            -DCURL_DISABLE_TFTP=ON \
            -DCURL_DISABLE_DICT=ON \
            -DCURL_DISABLE_FILE=ON \
            -DCURL_DISABLE_MQTT=ON \
            -DCURL_DISABLE_PROXY=ON \
            -DCURL_DISABLE_HTTP_AUTH=ON \
            -DCURL_DISABLE_KERBEROS_AUTH=ON \
            -DCURL_DISABLE_NEGOTIATE_AUTH=ON \
            -DCURL_DISABLE_ALTSVC=ON \
            -DCURL_DISABLE_HSTS=ON \
            -DCURL_DISABLE_WEBSOCKETS=ON \
            -DBUILD_TESTING=OFF \
            -DBUILD_CURL_EXE=ON \
            -DENABLE_CURL_MANUAL=OFF \
            "${curl_src}"
) >"${logs_dir}/cmake-configure.stdout" \
  2>"${logs_dir}/cmake-configure.stderr"

(
    cd "${cmake_dir}"
    LLVM_SYS_181_PREFIX="${llvm_prefix}" \
    RCC_LINKER_DRIVER="${linker_driver}" \
        make -j"${make_jobs}"
) >"${logs_dir}/make.stdout" \
  2>"${logs_dir}/make.stderr"

cli_bin="${cmake_dir}/src/curl"
lib_archive="${cmake_dir}/lib/libcurl.a"

if [[ ! -x "${cli_bin}" ]]; then
    echo "curl CLI not produced at ${cli_bin}" >&2
    exit 3
fi
if [[ ! -f "${lib_archive}" ]]; then
    echo "libcurl.a not produced at ${lib_archive}" >&2
    exit 3
fi

"${cli_bin}" --version \
    >"${artifacts_dir}/curl-version.stdout" \
    2>"${artifacts_dir}/curl-version.stderr"

grep -q '^curl ' "${artifacts_dir}/curl-version.stdout"

if [[ "${network_smoke}" -eq 1 ]]; then
    "${cli_bin}" -sS \
        -o "${artifacts_dir}/example.html" \
        -w 'status=%{http_code} size=%{size_download} time=%{time_total}\n' \
        http://example.com/ \
        >"${artifacts_dir}/curl-example.stdout" \
        2>"${artifacts_dir}/curl-example.stderr"

    grep -q '^status=200 size=528 ' "${artifacts_dir}/curl-example.stdout"
    grep -q '<title>Example Domain</title>' "${artifacts_dir}/example.html"
fi

cp -f "${lib_archive}" "${artifacts_dir}/libcurl.a"
cp -f "${cli_bin}" "${artifacts_dir}/curl"

cat "${artifacts_dir}/curl-version.stdout"
if [[ "${network_smoke}" -eq 1 ]]; then
    cat "${artifacts_dir}/curl-example.stdout"
fi
