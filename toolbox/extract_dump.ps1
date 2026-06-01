# Extract SPI NOR dump into ./workspace/ using dump_tool.
#
# Usage:
#   .\extract_dump.ps1 <dump.bin>

param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Dump
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not (Test-Path -LiteralPath $Dump -PathType Leaf)) {
    Write-Error "not a file: $Dump"
}

$scriptDir = Split-Path -Parent $PSCommandPath
$dumpTool = Join-Path $scriptDir 'bin\dump_tool.exe'
$workspace = Join-Path $scriptDir 'workspace'

if (-not (Test-Path -LiteralPath $dumpTool)) {
    Write-Error "dump_tool not found: $dumpTool"
}

if (Test-Path -LiteralPath $workspace) {
    $ans = Read-Host 'workspace exists. Replace? [y/N]'
    if ($ans -notmatch '^[yY]([eE][sS])?$') { exit 1 }
    Remove-Item -LiteralPath $workspace -Recurse -Force
}

Write-Host "==> extract $Dump -> $workspace"
& $dumpTool extract $Dump $workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host '==> done'
