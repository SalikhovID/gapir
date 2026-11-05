#!/usr/bin/env bash
# Builds, signs and zips Gapir into Gapir-macos-arm64.zip in the repository root.
#
# MACOS_CERT_P12       base64 of the signing certificate
# MACOS_CERT_PASSWORD  its password
#
# A stable identity keeps Microphone and Accessibility permissions across updates.
# An ad-hoc signature would make macOS treat every release as a different app.
set -euo pipefail

: "${MACOS_CERT_P12:?base64 of the signing certificate is required}"
: "${MACOS_CERT_PASSWORD:?certificate password is required}"

cd "$(dirname "$0")/.."
root="$PWD"
bundle_dir="target/aarch64-apple-darwin/release/bundle/macos"

python3 scripts/third-party-licenses.py > THIRD-PARTY-LICENSES.txt
npx -y @tauri-apps/cli@2.12.4 build --target aarch64-apple-darwin --bundles app

work="$(mktemp -d)"
keychain="$work/signing.keychain-db"
trap 'security delete-keychain "$keychain" 2>/dev/null || true; rm -rf "$work"' EXIT

keychain_password="$(uuidgen)"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
echo "$MACOS_CERT_P12" | base64 --decode > "$work/cert.p12"
security import "$work/cert.p12" -k "$keychain" -P "$MACOS_CERT_PASSWORD" -T /usr/bin/codesign -f pkcs12
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" >/dev/null

# The certificate is self-signed, so it is addressed by hash: it counts as "not trusted",
# which only excludes it from name lookups.
identity="$(security find-identity -p codesigning "$keychain" | awk '/\)/ {print $2; exit}')"
[ -n "$identity" ]

codesign --force --options runtime --timestamp=none \
    --entitlements src-tauri/Entitlements.plist --keychain "$keychain" --sign "$identity" \
    "$bundle_dir/Gapir.app"
codesign --verify --strict "$bundle_dir/Gapir.app"

cd "$bundle_dir"
ditto -c -k --keepParent Gapir.app "$root/Gapir-macos-arm64.zip"
cd "$root"
shasum -a 256 Gapir-macos-arm64.zip > Gapir-macos-arm64.zip.sha256
cat Gapir-macos-arm64.zip.sha256
