#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

target=x86_64-pc-windows-gnu
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target}"
out_dir="${CYMESH_DIST_DIR:-$root/dist-windows}"
profile="${CYMESH_BUILDER_PROFILE:-}"
if [[ -n "$profile" && ! "$profile" =~ ^relay-[a-f0-9]{12}$ ]]; then
    echo "Invalid CyMesh relay profile" >&2
    exit 1
fi
profile_arg=""
if [[ -n "$profile" ]]; then
    profile_arg=" -Profile $profile"
fi

cargo test --locked -p aegis-core
npm --prefix ui ci
npm --prefix ui run build
cargo build --locked --release -p aegis-mesh-app --target "$target" --features custom-protocol

build_dir="$CARGO_TARGET_DIR/$target/release"
mkdir -p "$out_dir"
install -m 755 "$build_dir/aegis-mesh-app.exe" "$out_dir/Cymesh.exe"
install -m 644 scripts/windows-install.ps1 "$out_dir/install-shortcuts.ps1"
install -m 644 LICENSE "$out_dir/LICENSE"
if [[ -f "$build_dir/WebView2Loader.dll" ]]; then
    install -m 644 "$build_dir/WebView2Loader.dll" "$out_dir/WebView2Loader.dll"
fi

printf '@echo off\r\npowershell -NoProfile -ExecutionPolicy Bypass -File "%%~dp0install-shortcuts.ps1"%s\r\nexit /b %%ERRORLEVEL%%\r\n' \
    "$profile_arg" > "$out_dir/Install-Cymesh.cmd"

files=(Cymesh.exe Install-Cymesh.cmd install-shortcuts.ps1 LICENSE)
if [[ -f "$build_dir/WebView2Loader.dll" ]]; then
    files+=(WebView2Loader.dll)
fi
(
    cd "$out_dir"
    archive="$(mktemp -d .cymesh-package.XXXXXX)"
    zip -q "$archive/portable.zip" "${files[@]}"
    mv "$archive/portable.zip" Cymesh-Windows-x64-Portable.zip
    rmdir "$archive"
)
printf 'Portable package: %s/Cymesh-Windows-x64-Portable.zip\n' "$out_dir"
