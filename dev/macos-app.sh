#!/usr/bin/env bash
# Wraps a built db9s-gui into db9s.app and zips it for a release:
#   dev/macos-app.sh <rust target> <version>
#   -> db9s-gui-v<version>-<target>.zip (built with: cargo build --release -p db9s-gui --target <target>)
# Signed ad-hoc only (arm64 refuses unsigned code); not notarized, see the cask's caveats.
set -euo pipefail
target=$1
version=${2#v}
bin=target/$target/release/db9s-gui
[[ -x $bin ]] || bin=target/release/db9s-gui  # local build without --target
app=db9s.app

rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "$bin" "$app/Contents/MacOS/db9s-gui"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>db9s</string>
  <key>CFBundleDisplayName</key><string>db9s</string>
  <key>CFBundleIdentifier</key><string>io.github.damien-mathieu1.db9s</string>
  <key>CFBundleExecutable</key><string>db9s-gui</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$app"
ditto -c -k --norsrc --keepParent "$app" "db9s-gui-v$version-$target.zip"
echo "db9s-gui-v$version-$target.zip"
