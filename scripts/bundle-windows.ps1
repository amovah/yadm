# Assembles dist\YADM (and dist\YADM-<version>-windows-x64.zip): yadm.exe with
# the aria2c.exe sidecar next to it and the browser extension.
# Uses the official aria2 Windows build unless -Aria2c points at one.
#
#   powershell -ExecutionPolicy Bypass -File scripts\bundle-windows.ps1
param(
    [string]$Aria2c = "",
    [string]$Aria2Version = "1.37.0"
)
$ErrorActionPreference = "Stop"

$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $Root
$Version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value

if (-not $Aria2c) {
    $Vendor = Join-Path $Root "vendor"
    $Aria2c = Join-Path $Vendor "aria2c.exe"
    if (-not (Test-Path $Aria2c)) {
        $Name = "aria2-$Aria2Version-win-64bit-build1"
        $Zip = Join-Path ([IO.Path]::GetTempPath()) "$Name.zip"
        $Url = "https://github.com/aria2/aria2/releases/download/release-$Aria2Version/$Name.zip"
        Write-Host "Downloading $Url"
        Invoke-WebRequest -Uri $Url -OutFile $Zip
        $Unpacked = Join-Path ([IO.Path]::GetTempPath()) "yadm-aria2"
        if (Test-Path $Unpacked) { Remove-Item -Recurse -Force $Unpacked }
        Expand-Archive -Path $Zip -DestinationPath $Unpacked
        New-Item -ItemType Directory -Force $Vendor | Out-Null
        Copy-Item (Join-Path $Unpacked "$Name\aria2c.exe") $Aria2c
    }
}
if (-not (Test-Path $Aria2c)) { throw "aria2c.exe not found at $Aria2c" }

cargo build --release -p yadm-app
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$Dist = Join-Path $Root "dist"
$App = Join-Path $Dist "YADM"
if (Test-Path $App) { Remove-Item -Recurse -Force $App }
New-Item -ItemType Directory -Force $App | Out-Null
Copy-Item "target\release\yadm.exe" (Join-Path $App "YADM.exe")
Copy-Item $Aria2c (Join-Path $App "aria2c.exe")
Copy-Item -Recurse "extension" (Join-Path $App "extension")

$Archive = Join-Path $Dist "YADM-$Version-windows-x64.zip"
if (Test-Path $Archive) { Remove-Item -Force $Archive }
Compress-Archive -Path $App -DestinationPath $Archive
Write-Host "Built $App"
Write-Host "Packed $Archive"
