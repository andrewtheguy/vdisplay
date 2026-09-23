#!/usr/bin/env bash
# Build vdisplay.app for Apple Silicon and pack it into a disk image in dist/.
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

# The image holds the app and an Applications link to drag it onto.
dmg="dist/vdisplay-v$version-macos-arm64.dmg"
stage=dist/dmg
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname vdisplay -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg"
rm -rf "$stage"
echo "$dmg"
