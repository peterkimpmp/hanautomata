#!/bin/sh
# Release package (M-14): a universal bundle (Apple Silicon + Intel) zipped as build/dist/Hanautomata-<version>-macos.zip,
# its SHA-256, and the Homebrew cask filled in with both (build/dist/hanautomata.rb from Packaging/hanautomata.rb.in).
# With HANAUTOMATA_SIGN_ID (Developer ID) and HANAUTOMATA_NOTARY_PROFILE (a `notarytool store-credentials` profile) the
# zip is notarized and the ticket stapled (decision D15). Nothing is uploaded or published here.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
"$here/build.sh" --universal
app="$here/build/Hanautomata.app"
version=$(/usr/libexec/PlistBuddy -c "Print CFBundleShortVersionString" "$here/Info.plist")
dist="$here/build/dist"
zip="$dist/Hanautomata-$version-macos.zip"
mkdir -p "$dist"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
if [ -n "${HANAUTOMATA_SIGN_ID:-}" ] && [ -n "${HANAUTOMATA_NOTARY_PROFILE:-}" ]; then
    xcrun notarytool submit "$zip" --keychain-profile "$HANAUTOMATA_NOTARY_PROFILE" --wait
    xcrun stapler staple "$app"
    rm -f "$zip"
    ditto -c -k --keepParent "$app" "$zip"
    signed="Developer ID, notarized"
elif [ -n "${HANAUTOMATA_SIGN_ID:-}" ]; then
    signed="Developer ID, not notarized"
else
    signed="ad-hoc (Gatekeeper blocks it after download; see README)"
fi
sha=$(shasum -a 256 "$zip" | cut -d' ' -f1)
echo "$sha  $(basename "$zip")" > "$zip.sha256"
sed -e "s/@VERSION@/$version/g" -e "s/@SHA256@/$sha/g" "$here/Packaging/hanautomata.rb.in" > "$dist/hanautomata.rb"
echo "packaged $zip ($(lipo -archs "$app/Contents/MacOS/Hanautomata"), $signed)"
echo "sha256 $sha"
echo "cask   $dist/hanautomata.rb"
