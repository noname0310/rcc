#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${project_dir}/../../.." && pwd)"

curl_url="${CURL_URL:-https://github.com/curl/curl.git}"
curl_rev="${CURL_REV:-9c9a4f3eabbb6f24277538d28a00afa25ba2839a}"
default_curl_src="${project_dir}/upstream"
curl_src="${CURL_SRC:-${default_curl_src}}"
build_dir="${project_dir}/build"
logs_dir="${project_dir}/logs"
artifacts_dir="${project_dir}/artifacts"

rcc_bin="${RCC:-${repo_root}/target/debug/rcc}"
host_cc="${HOST_CC:-cc}"
llvm_prefix="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}"
linker_driver="${RCC_LINKER_DRIVER:-/usr/bin/clang-18}"
make_jobs="${MAKEFLAGS_J:-4}"
network_smoke="${NETWORK_SMOKE:-0}"

mkdir -p "${build_dir}" "${logs_dir}" "${artifacts_dir}"

ensure_default_curl_source() {
    if [[ "${curl_src}" != "${default_curl_src}" ]]; then
        return
    fi

    if [[ ! -d "${curl_src}/.git" ]]; then
        rm -rf "${curl_src}"
        git -c core.autocrlf=false init "${curl_src}" \
            >"${logs_dir}/git-init.stdout" \
            2>"${logs_dir}/git-init.stderr"
        git -C "${curl_src}" remote add origin "${curl_url}"
    fi

    git -C "${curl_src}" config core.autocrlf false
    if ! git -C "${curl_src}" cat-file -e "${curl_rev}^{commit}" 2>/dev/null; then
        git -C "${curl_src}" fetch --depth 1 origin "${curl_rev}" \
            >"${logs_dir}/git-fetch.stdout" \
            2>"${logs_dir}/git-fetch.stderr"
    fi
    git -C "${curl_src}" reset --hard "${curl_rev}" \
        >"${logs_dir}/git-reset.stdout" \
        2>"${logs_dir}/git-reset.stderr"
    git -C "${curl_src}" clean -fdx \
        >"${logs_dir}/git-clean.stdout" \
        2>"${logs_dir}/git-clean.stderr"
}

ensure_default_curl_source

if [[ ! -f "${curl_src}/CMakeLists.txt" ]] || [[ ! -d "${curl_src}/lib" ]] \
    || [[ ! -d "${curl_src}/src" ]]; then
    echo "curl source tree is incomplete: ${curl_src}" >&2
    echo "Set CURL_SRC to a clone of ${curl_url}" >&2
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
host_c_flags="-std=gnu99 -D_GNU_SOURCE -fvisibility=hidden"

cmake_common=(
    -DCMAKE_C_FLAGS_RELEASE="-DNDEBUG"
    -DCMAKE_BUILD_TYPE=Release
    -DBUILD_SHARED_LIBS=OFF
    -DCMAKE_DISABLE_FIND_PACKAGE_Threads=TRUE
    -DCURL_USE_OPENSSL=OFF
    -DCURL_USE_LIBSSH2=OFF
    -DCURL_USE_LIBPSL=OFF
    -DCURL_ZLIB=OFF
    -DCURL_BROTLI=OFF
    -DCURL_ZSTD=OFF
    -DUSE_LIBIDN2=OFF
    -DUSE_NGHTTP2=OFF
    -DENABLE_THREADED_RESOLVER=OFF
    -DENABLE_UNIX_SOCKETS=OFF
    -DENABLE_IPV6=OFF
    -DCURL_DISABLE_LDAP=ON
    -DCURL_DISABLE_LDAPS=ON
    -DCURL_DISABLE_FTP=ON
    -DCURL_DISABLE_SMTP=ON
    -DCURL_DISABLE_IMAP=ON
    -DCURL_DISABLE_POP3=ON
    -DCURL_DISABLE_GOPHER=ON
    -DCURL_DISABLE_RTSP=ON
    -DCURL_DISABLE_TELNET=ON
    -DCURL_DISABLE_TFTP=ON
    -DCURL_DISABLE_DICT=ON
    -DCURL_DISABLE_FILE=ON
    -DCURL_DISABLE_MQTT=ON
    -DCURL_DISABLE_PROXY=ON
    -DCURL_DISABLE_HTTP_AUTH=ON
    -DCURL_DISABLE_KERBEROS_AUTH=ON
    -DCURL_DISABLE_NEGOTIATE_AUTH=ON
    -DCURL_DISABLE_ALTSVC=ON
    -DCURL_DISABLE_HSTS=ON
    -DCURL_DISABLE_WEBSOCKETS=ON
    -DBUILD_TESTING=OFF
    -DBUILD_CURL_EXE=ON
    -DENABLE_CURL_MANUAL=OFF
)

configure_and_build() {
    local name="$1"
    local compiler="$2"
    local flags="$3"
    local cmake_dir="${build_dir}/${name}-cmake"

    rm -rf "${cmake_dir}"
    mkdir -p "${cmake_dir}"

    (
        cd "${cmake_dir}"
        LLVM_SYS_181_PREFIX="${llvm_prefix}" \
        RCC_LINKER_DRIVER="${linker_driver}" \
        cmake \
            -DCMAKE_C_COMPILER="${compiler}" \
            -DCMAKE_C_FLAGS="${flags}" \
            "${cmake_common[@]}" \
            "${curl_src}"
    ) >"${logs_dir}/${name}-cmake-configure.stdout" \
      2>"${logs_dir}/${name}-cmake-configure.stderr"

    (
        cd "${cmake_dir}"
        LLVM_SYS_181_PREFIX="${llvm_prefix}" \
        RCC_LINKER_DRIVER="${linker_driver}" \
        make -j"${make_jobs}"
    ) >"${logs_dir}/${name}-make.stdout" \
      2>"${logs_dir}/${name}-make.stderr"
}

configure_and_build host "${host_cc}" "${host_c_flags}"
configure_and_build rcc "${rcc_bin}" "${c_flags}"

host_cli="${build_dir}/host-cmake/src/curl"
rcc_cli="${build_dir}/rcc-cmake/src/curl"
rcc_lib_archive="${build_dir}/rcc-cmake/lib/libcurl.a"

if [[ ! -x "${host_cli}" ]]; then
    echo "host curl CLI not produced at ${host_cli}" >&2
    exit 3
fi
if [[ ! -x "${rcc_cli}" ]]; then
    echo "rcc curl CLI not produced at ${rcc_cli}" >&2
    exit 3
fi
if [[ ! -f "${rcc_lib_archive}" ]]; then
    echo "rcc libcurl.a not produced at ${rcc_lib_archive}" >&2
    exit 3
fi

"${host_cli}" --version \
    >"${artifacts_dir}/host-curl-version.stdout" \
    2>"${artifacts_dir}/host-curl-version.stderr"
"${rcc_cli}" --version \
    >"${artifacts_dir}/rcc-curl-version.stdout" \
    2>"${artifacts_dir}/rcc-curl-version.stderr"

grep -q '^curl ' "${artifacts_dir}/host-curl-version.stdout"
grep -q '^curl ' "${artifacts_dir}/rcc-curl-version.stdout"

server_pid=""
cleanup() {
    if [[ -n "${server_pid}" ]] && kill -0 "${server_pid}" 2>/dev/null; then
        kill "${server_pid}" 2>/dev/null || true
        wait "${server_pid}" 2>/dev/null || true
    fi
}
trap cleanup EXIT

http_root="${build_dir}/http-root"
port_file="${build_dir}/http-port"
rm -rf "${http_root}"
mkdir -p "${http_root}"
cat >"${http_root}/index.html" <<'HTML'
<!doctype html>
<html>
<head><title>rcc curl smoke</title></head>
<body>curl local oracle: rcc and host should fetch identical bytes.</body>
</html>
HTML
rm -f "${port_file}"

python3 - "${http_root}" "${port_file}" <<'PY' &
import functools
import http.server
import sys

root, port_file = sys.argv[1], sys.argv[2]

class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

handler = functools.partial(QuietHandler, directory=root)
with http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler) as httpd:
    with open(port_file, "w", encoding="ascii") as f:
        f.write(str(httpd.server_address[1]))
    httpd.serve_forever()
PY
server_pid=$!

for _ in {1..100}; do
    if [[ -s "${port_file}" ]]; then
        break
    fi
    sleep 0.05
done
if [[ ! -s "${port_file}" ]]; then
    echo "local HTTP oracle failed to start" >&2
    exit 4
fi

local_url="http://127.0.0.1:$(cat "${port_file}")/"

"${host_cli}" -sS \
    -o "${artifacts_dir}/host-local.html" \
    -w 'status=%{http_code} size=%{size_download}\n' \
    "${local_url}" \
    >"${artifacts_dir}/host-local.stdout" \
    2>"${artifacts_dir}/host-local.stderr"
"${rcc_cli}" -sS \
    -o "${artifacts_dir}/rcc-local.html" \
    -w 'status=%{http_code} size=%{size_download}\n' \
    "${local_url}" \
    >"${artifacts_dir}/rcc-local.stdout" \
    2>"${artifacts_dir}/rcc-local.stderr"

diff -u "${artifacts_dir}/host-local.stdout" "${artifacts_dir}/rcc-local.stdout" \
    >"${logs_dir}/local-http-stdout.diff"
diff -u "${artifacts_dir}/host-local.html" "${artifacts_dir}/rcc-local.html" \
    >"${logs_dir}/local-http-body.diff"

if [[ "${network_smoke}" -eq 1 ]]; then
    "${rcc_cli}" -sS \
        -o "${artifacts_dir}/rcc-example.html" \
        -w 'status=%{http_code} size=%{size_download} time=%{time_total}\n' \
        http://example.com/ \
        >"${artifacts_dir}/rcc-example.stdout" \
        2>"${artifacts_dir}/rcc-example.stderr"

    grep -q '^status=200 ' "${artifacts_dir}/rcc-example.stdout"
    grep -q '<title>Example Domain</title>' "${artifacts_dir}/rcc-example.html"
fi

cp -f "${rcc_lib_archive}" "${artifacts_dir}/libcurl.a"
cp -f "${rcc_cli}" "${artifacts_dir}/curl"

cat "${artifacts_dir}/rcc-curl-version.stdout"
cat "${artifacts_dir}/rcc-local.stdout"
if [[ "${network_smoke}" -eq 1 ]]; then
    cat "${artifacts_dir}/rcc-example.stdout"
fi
