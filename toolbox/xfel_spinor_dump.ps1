# Dump full SPI NOR via xfel_spi_nor (device in FEL mode, USB connected).
#
# Prereq: board in FEL (e.g. sd_fel_switcher), WinUSB/libusb driver for 1f3a:efe8
#
# Usage:
#   .\xfel_spinor_dump.ps1 [output.bin]
#   .\xfel_spinor_dump.ps1 -Help

param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Rest,
    [switch]$Help
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Show-Usage {
    Get-Content $PSCommandPath | Select-Object -Skip 1 -First 7 | ForEach-Object {
        $_ -replace '^\# ?', ''
    }
    exit $(if ($Help) { 0 } else { 1 })
}

if ($Help) { Show-Usage }

$Out = $null
foreach ($arg in $Rest) {
    if ($arg -match '^-') {
        Write-Error "unknown option: $arg"
        Show-Usage
    }
    if ($Out) {
        Write-Error "extra argument: $arg"
        exit 1
    }
    $Out = $arg
}

if (-not $Out) {
    $Out = "nor_dump_{0:yyyyMMdd}.bin" -f (Get-Date)
}

$scriptDir = Split-Path -Parent $PSCommandPath
$script:Xfel = Join-Path $scriptDir 'bin\xfel_spi_nor.exe'
if (-not (Test-Path -LiteralPath $script:Xfel)) {
    Write-Error "xfel_spi_nor not found: $($script:Xfel)"
}

function Invoke-Xfel {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Args)
    & $script:Xfel @Args
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

function Parse-SpinorSize {
    param([string]$Text)
    if ($Text -match 'with\s+(\d+)\s+bytes') { return [uint64]$Matches[1] }
    if ($Text -match '^[^\s]+\s+(\d+)\s*$') { return [uint64]$Matches[1] }
    return $null
}

Write-Host "==> $($script:Xfel) version"
Invoke-Xfel version

Write-Host "==> $($script:Xfel) spinor"
$spinorOut = & $script:Xfel spinor 2>&1 | Out-String
if ($LASTEXITCODE -ne 0) { Write-Error $spinorOut.TrimEnd() }
Write-Host $spinorOut.TrimEnd()

$size = Parse-SpinorSize $spinorOut
if (-not $size) { Write-Error 'could not parse NOR size from device' }

Write-Host "==> read $size bytes @ 0 -> $Out"
Invoke-Xfel read 0 $size $Out

$bytes = (Get-Item -LiteralPath $Out).Length
Write-Host "==> done: $Out ($bytes bytes)"
if ([uint64]$bytes -ne $size) {
    Write-Warning 'file size != requested length'
    exit 1
}
