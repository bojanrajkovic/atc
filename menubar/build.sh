#!/usr/bin/env bash
#
# build.sh — compile with SwiftPM, assemble a macOS .app bundle, codesign.
#
# Usage:  ./build.sh [debug|release]
#
# No Xcode project and no xcodebuild: `swift build` compiles, this script
# bundles and signs. Xcode is only the toolchain provider.
#
# Environment:
#   SIGN_IDENTITY  codesign identity. Defaults to ad-hoc ("-"). A real
#                  identity also gets the hardened runtime and a secure
#                  timestamp, which notarization requires.
#   VERSION        CFBundleShortVersionString to stamp (default 0.0.0).
#
set -euo pipefail

CONFIG="${1:-debug}"
APP_NAME="ATCMenubar"
APP="build/$APP_NAME.app"
SIGN_IDENTITY="${SIGN_IDENTITY:--}"
VERSION="${VERSION:-0.0.0}"

# SwiftPM stamps the binary with the deployment target as its SDK version.
# macOS reads that stamp to pick the system appearance, so pass the real SDK
# version or the app gets the previous release's look.
SDK_VERSION="$(xcrun --sdk macosx --show-sdk-version)"
LINK_FLAGS=(-Xlinker -platform_version -Xlinker macos -Xlinker 26.0 -Xlinker "$SDK_VERSION")

echo "→ swift build -c $CONFIG"
swift build -c "$CONFIG" "${LINK_FLAGS[@]}"
BIN_PATH="$(swift build -c "$CONFIG" --show-bin-path)"

echo "→ assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "$BIN_PATH/$APP_NAME" "$APP/Contents/MacOS/$APP_NAME"
sed -e "s/__SHORT_VERSION__/$VERSION/g" \
    -e "s/__BUILD_VERSION__/$(date +%Y%m%d%H%M)/g" \
    Resources/Info.plist > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

echo "→ codesign ($SIGN_IDENTITY)"
sign_args=(--force --sign "$SIGN_IDENTITY")
if [ "$SIGN_IDENTITY" != "-" ]; then
	sign_args+=(--options runtime --timestamp)
fi
codesign "${sign_args[@]}" "$APP"
codesign --verify --verbose=1 "$APP"
echo "✓ built $APP"
