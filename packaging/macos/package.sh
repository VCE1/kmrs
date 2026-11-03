#!/usr/bin/env bash
# Assemble KMServer.app from a built binary and package it as a DMG.
#
# Usage: package.sh <binary> <version> <outdir> [arch]
#
# With no Apple env vars set the app is ad-hoc signed for local testing.
# A release requires all five values; missing notarization credentials fail the
# build rather than silently shipping an unnotarized Developer ID app:
#   APPLE_CERTIFICATE           base64-encoded .p12 developer ID certificate
#   APPLE_CERTIFICATE_PASSWORD  password for the .p12
#   APPLE_API_KEY               PEM-encoded App Store Connect private key
#   APPLE_API_KEY_ID            App Store Connect key id
#   APPLE_API_ISSUER            App Store Connect issuer UUID
set -euo pipefail

BINARY="$1"
VERSION="$2"
OUTDIR="$3"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ "$(uname -s)" = "Darwin" ] || { printf 'macOS packaging requires Darwin\n' >&2; exit 1; }
command -v rsvg-convert >/dev/null || { printf 'rsvg-convert is required\n' >&2; exit 1; }
mkdir -p "$OUTDIR"
OUTDIR="$(cd "$OUTDIR" && pwd)"
case "$REPO_ROOT/" in "$OUTDIR/"*) printf 'output directory must not contain the repository\n' >&2; exit 1;; esac
APP="$OUTDIR/KMServer.app"

# The DMG name stays version-less so /releases/latest/download/ links are stable.
ARCH="${4:-$(uname -m)}"
DMG="$OUTDIR/KMServer_${ARCH}.dmg"
if [ -e "$APP" ] || [ -e "$DMG" ]; then
	printf 'output already exists: %s\n' "$OUTDIR" >&2
	exit 1
fi

mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/kmrs"
sed "s/@VERSION@/$VERSION/g" "$SCRIPT_DIR/Info.plist" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"

WORK="$(mktemp -d)"
PDFIUM_DIR="$WORK/pdfium"
mkdir -p "$PDFIUM_DIR"
curl -fsSL "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8125/pdfium-mac-arm64.tgz" -o "$WORK/pdfium.tgz"
tar -xzf "$WORK/pdfium.tgz" -C "$PDFIUM_DIR"
cp "$PDFIUM_DIR/lib/libpdfium.dylib" "$APP/Contents/MacOS/"
cp -R "$PDFIUM_DIR/licenses" "$APP/Contents/Resources/PDFium"
cp "$PDFIUM_DIR/LICENSE" "$APP/Contents/Resources/PDFium/"
install_name_tool -id @executable_path/libpdfium.dylib "$APP/Contents/MacOS/libpdfium.dylib"


WORK="$(mktemp -d)"
ICONSET="$OUTDIR/KMServer.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
	rsvg-convert -w "$size" -h "$size" "$SCRIPT_DIR/icon.svg" -o "$ICONSET/icon_${size}x${size}.png"
	rsvg-convert -w $((size * 2)) -h "$size" "$SCRIPT_DIR/icon.svg" -o "$ICONSET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/icon.icns"
rm -rf "$ICONSET"

WORK="$(mktemp -d)"
cleanup() {
	security delete-keychain "$WORK/build.keychain-db" >/dev/null 2>&1 || true
	rm -rf "$WORK"
}
trap cleanup EXIT

if [ -n "${APPLE_CERTIFICATE:-}" ]; then
	: "${APPLE_CERTIFICATE_PASSWORD:?required when APPLE_CERTIFICATE is set}"
	: "${APPLE_API_KEY:?required when APPLE_CERTIFICATE is set}"
	: "${APPLE_API_KEY_ID:?required when APPLE_CERTIFICATE is set}"
	: "${APPLE_API_ISSUER:?required when APPLE_CERTIFICATE is set}"
	KEYCHAIN="$WORK/build.keychain-db"
	KEYCHAIN_PASSWORD="$(openssl rand -hex 16)"
	security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
	security set-keychain-settings -lut 3600 "$KEYCHAIN"
	security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
	printf '%s' "$APPLE_CERTIFICATE" | base64 --decode >"$WORK/cert.p12"
	security import "$WORK/cert.p12" -k "$KEYCHAIN" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
	security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
	IDENTITY="$(security find-identity -v -p codesigning "$KEYCHAIN" | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p')"
	[ -n "$IDENTITY" ] || { printf 'Developer ID Application identity not found\n' >&2; exit 1; }
	printf 'Signing identity: %s\n' "$IDENTITY"
else
	IDENTITY="-"
fi

# The app and its runtime libraries must share one Developer ID team.
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP/Contents/MacOS/libpdfium.dylib"
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP/Contents/MacOS/kmrs"
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

if [ "$IDENTITY" != "-" ]; then
	printf '%s\n' "$APPLE_API_KEY" >"$WORK/notary.p8"
	ditto -c -k --keepParent "$APP" "$WORK/KMServer.zip"
	SUBMIT_LOG="$WORK/notary-submit.log"
	if ! xcrun notarytool submit "$WORK/KMServer.zip" \
		--key "$WORK/notary.p8" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER" \
		--wait | tee "$SUBMIT_LOG"; then
		SUBMISSION_ID="$(sed -n 's/^ *id: //p' "$SUBMIT_LOG")"
		[ -n "$SUBMISSION_ID" ] && xcrun notarytool log "$SUBMISSION_ID" \
			--key "$WORK/notary.p8" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER" || true
		exit 1
	fi
	xcrun stapler staple "$APP"
fi

hdiutil create -volname KMServer -srcfolder "$APP" -format UDRW "$WORK/KMServer.dmg"
mkdir -p "$WORK/mount"
hdiutil attach "$WORK/KMServer.dmg" -nobrowse -mountpoint "$WORK/mount"
ln -s /Applications "$WORK/mount/Applications"
hdiutil detach "$WORK/mount"
hdiutil convert "$WORK/KMServer.dmg" -format UDZO -o "$DMG"
hdiutil attach "$DMG" -nobrowse -readonly -mountpoint "$WORK/mount"
[ -d "$WORK/mount/KMServer.app" ]
[ -L "$WORK/mount/Applications" ]
hdiutil detach "$WORK/mount"
if [ "$IDENTITY" != "-" ]; then
	codesign --force --sign "$IDENTITY" "$DMG"
fi
printf 'Created %s\n' "$DMG"
