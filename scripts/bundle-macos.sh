#!/bin/sh
# Builds Rustcraft.app: the release binary, the assets, and an app icon made
# from assets/rustcraft-app-icon-v1.svg. The app keeps its saves and options
# in ~/Library/Application Support/Rustcraft.
#
#   scripts/bundle-macos.sh            # → target/release/bundle/Rustcraft.app
#   scripts/bundle-macos.sh <out dir>  # → <out dir>/Rustcraft.app
#
# Needs only the Rust toolchain and tools every Mac has (qlmanage, sips,
# iconutil, codesign).
set -eu

cd "$(dirname "$0")/.."
ICON_SVG=assets/rustcraft-app-icon-v1.svg
OUT=${1:-target/release/bundle}
APP="$OUT/Rustcraft.app"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

cargo build --release

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# The icon: the SVG drawn at 1024 px by Quick Look, scaled to every size an
# .icns holds.
qlmanage -t -s 1024 -o "$WORK" "$ICON_SVG" >/dev/null
MASTER="$WORK/$(basename "$ICON_SVG").png"
ICONSET="$WORK/AppIcon.iconset"
mkdir "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z $size $size "$MASTER" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double "$MASTER" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/rustcraft "$APP/Contents/MacOS/rustcraft"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
# The assets, without the icon's source.
rsync -a --exclude '.DS_Store' --exclude '*.svg' assets/ "$APP/Contents/Resources/assets/"

cat >"$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Rustcraft</string>
    <key>CFBundleDisplayName</key>
    <string>Rustcraft</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.etertyers.rustcraft</string>
    <key>CFBundleExecutable</key>
    <string>rustcraft</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.games</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

# Signed for this Mac only (no developer ID), so it opens without a
# "damaged" warning once built here.
codesign --force --sign - "$APP" >/dev/null 2>&1
echo "built $APP"
