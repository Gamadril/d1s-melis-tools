# Repack ./workspace/ into a SPI NOR dump using dump_tool.
#
# Usage:
#   .\pack_dump.ps1 [output.bin]

param(
    [Parameter(Position = 0)]
    [string]$Out
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$scriptDir = Split-Path -Parent $PSCommandPath
$dumpTool = Join-Path $scriptDir 'bin\dump_tool.exe'
$workspace = Join-Path $scriptDir 'workspace'

if (-not $Out) {
    $Out = "nor_repacked_{0:yyyyMMdd}.bin" -f (Get-Date)
}

if (-not (Test-Path -LiteralPath $workspace -PathType Container)) {
    Write-Error "workspace not found: $workspace"
}
if (-not (Test-Path -LiteralPath $dumpTool)) {
    Write-Error "dump_tool not found: $dumpTool"
}

if (Test-Path -LiteralPath $Out) {
    $ans = Read-Host "$Out exists. Replace? [y/N]"
    if ($ans -notmatch '^[yY]([eE][sS])?$') { exit 1 }
    Remove-Item -LiteralPath $Out -Force
}

Write-Host "==> pack $workspace -> $Out"
& $dumpTool pack $workspace $Out
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host '==> done'
