# Assembles dist\YADM, dist\YADM-<version>-windows-x64.zip and, when Inno Setup
# is installed, dist\YADM-<version>-windows-x64-setup.exe: yadm.exe with the
# aria2c.exe sidecar and the video tools (yt-dlp, Deno, ffmpeg, ffprobe) next
# to it, plus the browser extension. Uses the official aria2, yt-dlp and Deno
# Windows builds and BtbN's LGPL ffmpeg build (checksums verified), cached in vendor\.
#
#   powershell -ExecutionPolicy Bypass -File scripts\bundle-windows.ps1
param(
    [string]$Aria2c = "",
    [string]$Aria2Version = "1.37.0",
    [string]$YtdlpVersion = "2026.10.26",
    [string]$DenoVersion = "2.9.8",
    # BtbN's release branch build (patch updates roll in; the checksum is still verified).
    [string]$FfmpegBranch = "9.0"
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

$Vendor = Join-Path $Root "vendor"
New-Item -ItemType Directory -Force $Vendor | Out-Null
$Tmp = [IO.Path]::GetTempPath()

function Get-Verified([string]$Url, [string]$Out, [string]$Expected) {
    Write-Host "Downloading $Url"
    Invoke-WebRequest -Uri $Url -OutFile $Out
    $Got = (Get-FileHash -Algorithm SHA256 $Out).Hash.ToLower()
    if (-not $Expected -or $Got -ne $Expected.ToLower()) { throw "checksum mismatch for $Url" }
}

function Get-Sum([string]$SumsUrl, [string]$Name) {
    $Text = (Invoke-WebRequest -Uri $SumsUrl -UseBasicParsing).Content
    if ($Text -is [byte[]]) { $Text = [Text.Encoding]::UTF8.GetString($Text) }
    foreach ($Line in $Text -split "`n") {
        $Parts = $Line.Trim() -split '\s+'
        if ($Parts.Count -ge 2 -and $Parts[-1].TrimStart('*') -eq $Name) { return $Parts[0] }
    }
    if ($Text -match '([0-9a-fA-F]{64})') { return $Matches[1] }
    return $null
}

$Ytdlp = Join-Path $Vendor "yt-dlp.exe"
if (-not (Test-Path $Ytdlp)) {
    $Base = "https://github.com/yt-dlp/yt-dlp/releases/download/$YtdlpVersion"
    Get-Verified "$Base/yt-dlp.exe" $Ytdlp (Get-Sum "$Base/SHA2-256SUMS" "yt-dlp.exe")
}

$Deno = Join-Path $Vendor "deno.exe"
if (-not (Test-Path $Deno)) {
    $Name = "deno-x86_64-pc-windows-msvc.zip"
    $Url = "https://github.com/denoland/deno/releases/download/v$DenoVersion/$Name"
    $Zip = Join-Path $Tmp $Name
    Get-Verified $Url $Zip (Get-Sum "$Url.sha256sum" $Name)
    $Unpacked = Join-Path $Tmp "yadm-deno"
    if (Test-Path $Unpacked) { Remove-Item -Recurse -Force $Unpacked }
    Expand-Archive -Path $Zip -DestinationPath $Unpacked
    Copy-Item (Join-Path $Unpacked "deno.exe") $Deno
}

$Ffmpeg = Join-Path $Vendor "ffmpeg.exe"
$Ffprobe = Join-Path $Vendor "ffprobe.exe"
if (-not ((Test-Path $Ffmpeg) -and (Test-Path $Ffprobe))) {
    $Name = "ffmpeg-n$FfmpegBranch-latest-win64-lgpl-$FfmpegBranch.zip"
    $Base = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest"
    $Zip = Join-Path $Tmp $Name
    Get-Verified "$Base/$Name" $Zip (Get-Sum "$Base/checksums.sha256" $Name)
    $Unpacked = Join-Path $Tmp "yadm-ffmpeg"
    if (Test-Path $Unpacked) { Remove-Item -Recurse -Force $Unpacked }
    Expand-Archive -Path $Zip -DestinationPath $Unpacked
    $Bin = Get-ChildItem -Recurse -Directory $Unpacked -Filter bin | Select-Object -First 1
    Copy-Item (Join-Path $Bin.FullName "ffmpeg.exe") $Ffmpeg
    Copy-Item (Join-Path $Bin.FullName "ffprobe.exe") $Ffprobe
}

cargo build --release -p yadm-app
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$Dist = Join-Path $Root "dist"
$App = Join-Path $Dist "YADM"
if (Test-Path $App) { Remove-Item -Recurse -Force $App }
New-Item -ItemType Directory -Force $App | Out-Null
Copy-Item "target\release\yadm.exe" (Join-Path $App "YADM.exe")
Copy-Item $Aria2c (Join-Path $App "aria2c.exe")
foreach ($Tool in $Ytdlp, $Deno, $Ffmpeg, $Ffprobe) { Copy-Item $Tool $App }
Copy-Item -Recurse "extension" (Join-Path $App "extension")
Copy-Item -Recurse "licenses" (Join-Path $App "licenses")

$Archive = Join-Path $Dist "YADM-$Version-windows-x64.zip"
if (Test-Path $Archive) { Remove-Item -Force $Archive }
Compress-Archive -Path $App -DestinationPath $Archive
Write-Host "Built $App"
Write-Host "Packed $Archive"

# Installer (needs Inno Setup 6+: https://jrsoftware.org/isdl.php or `choco install innosetup`).
$Iscc = Get-Command ISCC.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if (-not $Iscc) {
    $Iscc = Get-ChildItem "${env:ProgramFiles(x86)}\Inno Setup*\ISCC.exe", "$env:ProgramFiles\Inno Setup*\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup*\ISCC.exe" -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty FullName
}
if ($Iscc) {
    & $Iscc /Q "/DAppVersion=$Version" (Join-Path $PSScriptRoot "windows\yadm.iss")
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
    Write-Host "Packed $(Join-Path $Dist "YADM-$Version-windows-x64-setup.exe")"
} else {
    Write-Warning "Inno Setup not found; skipped the installer"
}
