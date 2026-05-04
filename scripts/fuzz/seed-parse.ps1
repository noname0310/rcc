<#
.SYNOPSIS
    Populate fuzz/corpus/parse/.

.DESCRIPTION
    Windows-side mirror of scripts/fuzz/seed-parse.sh. The parse fuzzer
    uses translation-unit shaped inputs from c-testsuite and chibicc.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot  = Resolve-Path (Join-Path $ScriptDir '..\..')

$CTestDir   = Join-Path $RepoRoot 'third_party\testsuites\c-testsuite\tests\single-exec'
$ChibiccDir = Join-Path $RepoRoot 'third_party\testsuites\chibicc\test'
$DstDir     = Join-Path $RepoRoot 'fuzz\corpus\parse'

New-Item -ItemType Directory -Force -Path $DstDir | Out-Null

function Copy-Seed {
    param(
        [Parameter(Mandatory=$true)][string]$Source,
        [Parameter(Mandatory=$true)][string]$Name
    )

    if (Test-Path -LiteralPath $Source -PathType Leaf) {
        Copy-Item -LiteralPath $Source -Destination (Join-Path $DstDir $Name) -Force
    } else {
        Write-Warning "seed $Source not found, skipping"
    }
}

foreach ($name in @(
    '00001.c', '00002.c', '00003.c', '00005.c', '00011.c', '00012.c',
    '00023.c', '00061.c', '00094.c', '00098.c', '00112.c', '00114.c'
)) {
    Copy-Seed -Source (Join-Path $CTestDir $name) -Name "ctest-$name"
}

foreach ($name in @(
    'arith.c', 'cast.c', 'control.c', 'decl.c', 'enum.c', 'function.c',
    'initializer.c', 'struct.c', 'typedef.c', 'union.c'
)) {
    Copy-Seed -Source (Join-Path $ChibiccDir $name) -Name "chibicc-$name"
}

$Count = (Get-ChildItem -LiteralPath $DstDir -File).Count
Write-Host "seeded $Count files into $DstDir"
