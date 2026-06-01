# Write SPI NOR image via xfel_spi_nor (device in FEL mode, USB connected).
#
# Usage:
#   .\xfel_spinor_recovery.ps1 flash_dump.bin

param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Image
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not (Test-Path -LiteralPath $Image -PathType Leaf)) {
    Write-Error "not a file: $Image"
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

$bytes = (Get-Item -LiteralPath $Image).Length

Write-Host "==> $($script:Xfel) version"
Invoke-Xfel version
Write-Host "==> write $bytes bytes @ 0 <- $Image"

$ans = Read-Host 'This will ERASE and overwrite SPI NOR. Continue? [y/N]'
if ($ans -notmatch '^[yY]([eE][sS])?$') { exit 1 }

Invoke-Xfel write 0 $Image
Write-Host '==> done'
