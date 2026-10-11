#!/bin/sh
# Signs the input-method bundle (M-14).
#   ./sign.sh build/Hanautomata.app    ad-hoc, for local builds (default)
# With a Developer ID (decision D15), set HANAUTOMATA_SIGN_ID="Developer ID Application: <name> (<team>)": the bundle is
# signed with the hardened runtime and a secure timestamp, as notarization requires (package.sh then notarizes it).
set -eu
app=${1:?usage: sign.sh <path to Hanautomata.app>}
if [ -n "${HANAUTOMATA_SIGN_ID:-}" ]; then
    codesign --force --options runtime --timestamp --sign "$HANAUTOMATA_SIGN_ID" "$app"
    codesign --verify --strict --verbose=1 "$app"
else
    codesign --force --sign - --timestamp=none "$app" 2>/dev/null
fi
