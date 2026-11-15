# Fetch an LGPL ffmpeg build for Windows and stage it beside the built injector: the Windows
# counterpart of scripts/fetch_ffmpeg.sh, fetching the same pinned, checksum-verified BtbN build.
# See that script for why this build, and docs/mod/capture.md for what the injector does with it.
#
#   powershell -ExecutionPolicy Bypass -File scripts/fetch_ffmpeg.ps1
#
# A tester with a prebuilt injector can instead put ffmpeg.exe beside jc3vr_injector.exe by hand.
$ErrorActionPreference = "Stop"

# Bump all three together with scripts/fetch_ffmpeg.sh.
$Release = "autobuild-2026-10-31-12-59"
$Build = "ffmpeg-n8.1.3-27-g8dca1586f4-win64-lgpl-8.1"
$Sha256 = "d4c2a42673917b4a97104a3e9f082729a9730796b98664e37c0b7eba6c6c613d"
$Url = "https://github.com/BtbN/FFmpeg-Builds/releases/download/$Release/$Build.zip"

$Repo = Split-Path -Parent $PSScriptRoot
$Cache = Join-Path $Repo ".ffmpeg"
$Zip = Join-Path $Cache "$Build.zip"
New-Item -ItemType Directory -Force -Path $Cache | Out-Null

function Test-Hash($Path) {
    (Test-Path $Path) -and ((Get-FileHash -Algorithm SHA256 $Path).Hash.ToLower() -eq $Sha256)
}

if (-not (Test-Hash $Zip)) {
    Write-Host "fetch_ffmpeg: downloading $Build"
    Invoke-WebRequest -Uri $Url -OutFile "$Zip.part"
    if (-not (Test-Hash "$Zip.part")) {
        Remove-Item "$Zip.part"
        Write-Error "fetch_ffmpeg: checksum mismatch for $Zip (expected $Sha256)"
    }
    Move-Item -Force "$Zip.part" $Zip
}

$Extract = Join-Path $Cache "extract"
if (Test-Path $Extract) { Remove-Item -Recurse -Force $Extract }
Expand-Archive -Path $Zip -DestinationPath $Extract
$Exe = Join-Path $Extract "$Build\bin\ffmpeg.exe"
$License = Join-Path $Extract "$Build\LICENSE.txt"

$Staged = $false
foreach ($BuildProfile in @("debug", "release")) {
    $Dir = Join-Path $Repo "target\x86_64-pc-windows-msvc\$BuildProfile"
    if (-not (Test-Path $Dir)) { continue }
    Copy-Item -Force $Exe (Join-Path $Dir "ffmpeg.exe")
    Copy-Item -Force $License (Join-Path $Dir "ffmpeg-LICENSE.txt")
    Write-Host "fetch_ffmpeg: staged -> $Dir\ffmpeg.exe"
    $Staged = $true
}
if (-not $Staged) {
    Copy-Item -Force $Exe (Join-Path $Cache "ffmpeg.exe")
    Write-Host "fetch_ffmpeg: no built target dir yet; ffmpeg is cached at $(Join-Path $Cache 'ffmpeg.exe')"
}
