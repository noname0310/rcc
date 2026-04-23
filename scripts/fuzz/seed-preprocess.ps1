<#
.SYNOPSIS
    Populate fuzz/corpus/preprocess/ from the vendored chibicc test suite.

.DESCRIPTION
    Windows-side mirror of scripts/fuzz/seed-preprocess.sh. Both scripts
    copy the same curated set of small preprocessor-heavy .c files so
    Linux CI and local Windows dev end up with identical corpora.

.NOTES
    Task: tasks/04-preprocess/19-fuzz-target.md.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot  = Resolve-Path (Join-Path $ScriptDir '..\..')

$SrcDir = Join-Path $RepoRoot 'third_party\testsuites\chibicc\test'
$DstDir = Join-Path $RepoRoot 'fuzz\corpus\preprocess'

if (-not (Test-Path -LiteralPath $SrcDir -PathType Container)) {
    Write-Error "chibicc suite not vendored at $SrcDir. Run 'cargo xtask fetch-testsuites --only chibicc'."
}

New-Item -ItemType Directory -Force -Path $DstDir | Out-Null

# Curated seeds — chosen for preprocessor diversity (typedef + header
# include, the full macro corpus, #line, #pragma once, common symbol
# declarations, small compat / extern / offsetof surface). The sibling
# .h files give libFuzzer a template for header-shaped inputs so
# mutations around `#include "..."` start from realistic content.
#
# Keep this list small; libFuzzer mutates aggressively.
$Seeds = @(
    'typedef.c',      # 486 B — typedef forms + `#include "test.h"`
    'macro.c',        # 6.5 KiB — full chibicc macro corpus (GNU ext OK)
    'line.c',         # 357 B — `#line` directive
    'pragma-once.c',  # 119 B — `#pragma once`
    'const.c',        # 306 B — small TU with predefined macros usage
    'commonsym.c',    # 264 B — tentative defs + comments
    'compat.c',       # 396 B — pragma pack + misc attrs
    'extern.c',       # 351 B — extern + forward decls
    'offsetof.c',     # 284 B — `#include <stddef.h>` style header usage
    'include1.h',     # 114 B — header chained via `#include "include2.h"`
    'include2.h'      #  19 B — terminal header in the include chain
)

foreach ($name in $Seeds) {
    $src = Join-Path $SrcDir $name
    $dst = Join-Path $DstDir $name
    if (-not (Test-Path -LiteralPath $src -PathType Leaf)) {
        Write-Warning "seed $src not found, skipping"
        continue
    }
    Copy-Item -LiteralPath $src -Destination $dst -Force
}

$Count = (Get-ChildItem -LiteralPath $DstDir -File).Count
Write-Host "seeded $Count files into $DstDir"
