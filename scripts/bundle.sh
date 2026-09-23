#!/usr/bin/env bash
# Build vdisplay.app for Apple Silicon and zip it into dist/.
set -euo pipefail
cd "$(dirname "$0")/.."

target=aarch64-apple-darwin
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)

cargo build --release --locked --target "$target"

app=dist/vdisplay.app
rm -rf dist
mkdir -p "$app/Contents/MacOS"
cp "target/$target/release/vdisplay" "$app/Contents/MacOS/vdisplay"
sed "s/@VERSION@/$version/g" packaging/Info.plist > "$app/Contents/Info.plist"
# Ad-hoc: Apple Silicon refuses to run unsigned code, and there is no identity.
codesign --force --sign - "$app"
codesign --verify --strict "$app"

ditto -c -k --keepParent "$app" "dist/vdisplay-v$version-macos-arm64.zip"
echo "dist/vdisplay-v$version-macos-arm64.zip"
