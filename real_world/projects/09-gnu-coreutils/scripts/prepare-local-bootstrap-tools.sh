#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "$script_dir/.." && pwd)"
tools_root="$project_dir/build/local-tools"
debs="$project_dir/build/local-debs"

packages=(
    autoconf
    automake
    autopoint
    autotools-dev
    bison
    gperf
    help2man
    libintl-perl
    m4
    texinfo
)

mkdir -p "$debs" "$tools_root"
(
    cd "$debs"
    apt-get download "${packages[@]}"
)

rm -rf "$tools_root"
mkdir -p "$tools_root"
for deb in "$debs"/*.deb; do
    dpkg-deb -x "$deb" "$tools_root"
done

bin="$tools_root/usr/bin"
ln -sf automake-1.16 "$bin/automake"
ln -sf aclocal-1.16 "$bin/aclocal"

autoconf_cfg="$tools_root/usr/share/autoconf/autom4te.cfg"
automake_cfg="$tools_root/usr/share/automake-1.16/Automake/Config.pm"
autoconf_share="$tools_root/usr/share/autoconf"
automake_share="$tools_root/usr/share/automake-1.16"

python3 - "$autoconf_cfg" "$autoconf_share" "$automake_cfg" "$automake_share" <<'PY'
from pathlib import Path
import sys

autoconf_cfg, autoconf_share, automake_cfg, automake_share = map(Path, sys.argv[1:])
autoconf_cfg.write_text(
    autoconf_cfg.read_text().replace("/usr/share/autoconf", str(autoconf_share)),
)
automake_cfg.write_text(
    automake_cfg.read_text().replace("/usr/share/automake-1.16", str(automake_share)),
)
PY

cat >"$project_dir/build/local-bootstrap-env.sh" <<EOF
export PATH="$bin:\$PATH"
export PERL5LIB="$tools_root/usr/share/autoconf:$tools_root/usr/share/automake-1.16:$tools_root/usr/share/texinfo:$tools_root/usr/share/perl5:$tools_root/usr/share/texinfo/lib/libintl-perl/lib:$tools_root/usr/share/texinfo/lib/Unicode-EastAsianWidth/lib\${PERL5LIB:+:\$PERL5LIB}"
export autom4te_perllibdir="$tools_root/usr/share/autoconf"
export AC_MACRODIR="$tools_root/usr/share/autoconf"
export AUTOMAKE_LIBDIR="$tools_root/usr/share/automake-1.16"
export AUTOM4TE="$bin/autom4te"
export AUTOCONF="$bin/autoconf"
export AUTOHEADER="$bin/autoheader"
export AUTOMAKE="$bin/automake"
export ACLOCAL="aclocal --automake-acdir=$tools_root/usr/share/aclocal-1.16 --system-acdir=$tools_root/usr/share/aclocal"
export trailer_m4="$tools_root/usr/share/autoconf/autoconf/trailer.m4"
export gettext_datadir="$tools_root/usr/share/gettext"
EOF

echo "wrote $project_dir/build/local-bootstrap-env.sh"
