<#
.SYNOPSIS
    Populate fuzz/corpus/lex/ from the vendored c-testsuite.

.DESCRIPTION
    Windows-side mirror of scripts/fuzz/seed-lex.sh. Both scripts copy
    the same curated set of small .c files so Linux CI and local
    Windows dev end up with identical corpora.

.NOTES
    Task: tasks/03-lex/12-fuzz-target.md.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot  = Resolve-Path (Join-Path $ScriptDir '..\..')

$SrcDir = Join-Path $RepoRoot 'third_party\testsuites\c-testsuite\tests\single-exec'
$DstDir = Join-Path $RepoRoot 'fuzz\corpus\lex'

if (-not (Test-Path -LiteralPath $SrcDir -PathType Container)) {
    Write-Error "c-testsuite not vendored at $SrcDir. Run 'cargo xtask fetch-testsuites --only c-testsuite'."
}

New-Item -ItemType Directory -Force -Path $DstDir | Out-Null

# Curated seeds (see seed-lex.sh for rationale). Keep in sync.
$Seeds = @(
    '00001',  # bare main returning 0
    '00002',  # constant return value
    '00003',  # simple declaration + return
    '00005',  # nested pointers, dereference chain
    '00011',  # chained assignment
    '00012',  # comma operator
    '00023',  # sizeof expression
    '00061',  # #define directive (preprocessor tokens)
    '00094',  # bitwise operators
    '00098',  # tiny expression
    '00112',  # string literal vs null pointer comparison
    '00114'   # forward declaration + function definition
)

foreach ($stem in $Seeds) {
    $src = Join-Path $SrcDir "$stem.c"
    $dst = Join-Path $DstDir "$stem.c"
    if (-not (Test-Path -LiteralPath $src -PathType Leaf)) {
        Write-Warning "seed $src not found, skipping"
        continue
    }
    Copy-Item -LiteralPath $src -Destination $dst -Force
}

$Count = (Get-ChildItem -LiteralPath $DstDir -Filter '*.c' -File).Count
Write-Host "seeded $Count files into $DstDir"
