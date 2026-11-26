# Native Windows build: compile the payload + injector and run the injector.
# The injector loads jc3vr_payload.dll from its own directory.
cargo build --target x86_64-pc-windows-msvc -p jc3vr_payload
if (-not $?) {
    Write-Error "Failed to build jc3vr_payload"
    exit 1
}

cargo build --target x86_64-pc-windows-msvc -p jc3vr_injector
if (-not $?) {
    Write-Error "Failed to build jc3vr_injector"
    exit 1
}

# The injector stays running as the capture host for in-process recordings, encoding them with the
# ffmpeg.exe beside it; scripts/fetch_ffmpeg.ps1 stages one.
if (-not (Test-Path "./target/x86_64-pc-windows-msvc/debug/ffmpeg.exe")) {
    Write-Warning "No ffmpeg.exe beside the injector, so recordings will have no video; run scripts/fetch_ffmpeg.ps1"
}

# The payload reads its tuning from this checkout's crates rather than the copies the build staged
# beside it, so editing a crate's tuning/*.toml applies live; see docs/mod/tuning.md.
& "./target/x86_64-pc-windows-msvc/debug/jc3vr_injector.exe" --source-tuning @args
