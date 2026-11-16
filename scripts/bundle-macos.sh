#!/bin/sh
# Builds Rustcraft.app: the release binary, the assets, and an app icon made
# from assets/rustcraft-app-icon-v1.svg. The app keeps its saves and options
# in ~/Library/Application Support/Rustcraft.
#
#   scripts/bundle-macos.sh            # → target/release/bundle/Rustcraft.app
#   scripts/bundle-macos.sh <out dir>  # → <out dir>/Rustcraft.app
#   scripts/bundle-macos.sh --lite     # → target/release/bundle-lite/Rustcraft.app
#
# --lite (M74) leaves out assets/vanilla and assets/downloads and everything
# from them (Mojang's files, the packs, the mod jars): the app downloads them on
# first launch. It needs neither folder to exist, and is checked with
# scripts/check-bundle.sh --lite.
#
# Refuses to build when assets/vanilla lacks sounds, the font or textures
# (scripts/check-vanilla.sh), warns when the packs or mod jars in
# assets/downloads are missing (scripts/check-downloads.sh), and checks the
# finished app against its sources (scripts/check-bundle.sh).
#
# Needs only the Rust toolchain, python3 and tools every Mac has (qlmanage,
# sips, iconutil, codesign).
set -eu

cd "$(dirname "$0")/.."
LITE=0
if [ "${1:-}" = "--lite" ]; then LITE=1; shift; fi
ICON_SVG=assets/rustcraft-app-icon-v1.svg
if [ $LITE = 1 ]; then OUT=${1:-target/release/bundle-lite}; else OUT=${1:-target/release/bundle}; fi
APP="$OUT/Rustcraft.app"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

# A silent app is worse than no app: stop here if the vanilla layer is
# incomplete.
if [ $LITE = 0 ]; then
    if ! scripts/check-vanilla.sh assets; then
        echo "not building: complete assets/vanilla with scripts/fetch-vanilla-assets.sh first" >&2
        exit 1
    fi
    if ! scripts/check-downloads.sh assets/downloads; then
        echo "warning: building without the downloads listed above (Kenzo's packs and mod jars)" >&2
    fi
fi

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
# The assets, without the icon's source and the vanilla checkout's git data
# (assets/vanilla is the local fallback layer from scripts/fetch-vanilla-assets.sh;
# it goes into this local build only and is never committed).
if [ $LITE = 1 ]; then
    rsync -a --exclude '.DS_Store' --exclude '*.svg' --exclude '/vanilla' --exclude '/downloads' assets/ "$APP/Contents/Resources/assets/"
else
    rsync -a --exclude '.DS_Store' --exclude '*.svg' --exclude '/vanilla/.git' assets/ "$APP/Contents/Resources/assets/"
fi
# The bundled mods (Fullbright); players' own are in Application Support/Rustcraft/mods.
rsync -a --exclude '.DS_Store' mods/ "$APP/Contents/Resources/mods/"

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
    <key>NSLocalNetworkUsageDescription</key>
    <string>Rustcraft finds and joins worlds opened to LAN on your local network.</string>
</dict>
</plist>
EOF

# Signed for this Mac only (no developer ID), so it opens without a
# "damaged" warning once built here.
codesign --force --sign - "$APP" >/dev/null 2>&1
echo "built $APP"
if [ $LITE = 1 ]; then
    scripts/check-bundle.sh --lite "$APP" assets
else
    scripts/check-bundle.sh "$APP" assets
fi
