#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "$script_dir/.." && pwd)"
repo_root="$(cd "$project_dir/../../.." && pwd)"
upstream="$project_dir/upstream"
build="$project_dir/build"
logs="$project_dir/logs"
artifacts="$project_dir/artifacts"

host_cc="${HOST_CC:-cc}"
rcc="${RCC:-$repo_root/target/debug/rcc}"
llvm_prefix="${LLVM_SYS_181_PREFIX:-/usr/lib/llvm-18}"

mkdir -p "$build" "$logs" "$artifacts"

if [ ! -f "$upstream/main.c" ] || [ ! -f "$upstream/one.c" ]; then
    echo "MuJS upstream checkout is incomplete: $upstream" >&2
    exit 2
fi

if [ "${RCC_BUILD:-1}" != "0" ]; then
    (
        cd "$repo_root"
        LLVM_SYS_181_PREFIX="$llvm_prefix" cargo build -p rcc_driver --features rcc_codegen_llvm/llvm
    ) >"$logs/cargo-build-rcc.stdout" 2>"$logs/cargo-build-rcc.stderr"
fi

cat >"$build/smoke.js" <<'JS'
function assertEq(name, actual, expected) {
    if (actual !== expected) {
        print("FAIL " + name + ": " + actual + " !== " + expected);
        throw new Error(name);
    }
    print("ok " + name);
}

var sum = 0;
for (var i = 0; i < 10; ++i)
    sum += i;
assertEq("loop", sum, 45);

function makeAdder(n) {
    return function(x) { return n + x; };
}
assertEq("closure", makeAdder(5)(7), 12);

var obj = { a: 1, b: 2 };
assertEq("object", obj.a + obj.b, 3);

var arr = [1, 2, 3];
arr.push(4);
assertEq("array", arr.join(","), "1,2,3,4");

assertEq("json", JSON.parse("{\"x\":[1,2,3]}").x[2], 3);
assertEq("regexp", /foo([0-9]+)/.exec("foo42")[1], "42");
assertEq("string", "a,b,c".split(",").reverse().join(""), "cba");
assertEq("math", Math.floor(Math.sin(Math.PI / 2) * 10), 10);

print("mujs smoke ok");
JS

"$host_cc" \
    -std=c99 \
    -O2 \
    -I "$upstream" \
    "$upstream/main.c" \
    "$upstream/one.c" \
    -lm \
    -o "$build/mujs-host" \
    >"$logs/host-build.stdout" \
    2>"$logs/host-build.stderr"

LLVM_SYS_181_PREFIX="$llvm_prefix" "$rcc" \
    --target=x86_64-unknown-linux-gnu \
    --linux-gnu-hosted \
    -std=c99 \
    -O2 \
    -I "$upstream" \
    "$upstream/main.c" \
    "$upstream/one.c" \
    -lm \
    -o "$build/mujs-rcc" \
    >"$logs/rcc-build.stdout" \
    2>"$logs/rcc-build.stderr"

"$build/mujs-host" "$build/smoke.js" >"$artifacts/host-mujs-smoke.stdout"
"$build/mujs-rcc" "$build/smoke.js" >"$artifacts/rcc-mujs-smoke.stdout"

diff -u "$artifacts/host-mujs-smoke.stdout" "$artifacts/rcc-mujs-smoke.stdout" \
    >"$logs/smoke-output.diff"

grep -qx 'mujs smoke ok' "$artifacts/rcc-mujs-smoke.stdout"
cat "$artifacts/rcc-mujs-smoke.stdout"
