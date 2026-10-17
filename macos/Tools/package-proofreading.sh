#!/bin/sh
# Preserve the optional offline spelling checker on both universal architectures, including exact sources/notices.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$here/.." && pwd)
proof="$root/core/tools/proofread"
app=${1:?usage: package-proofreading.sh APP}
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked --manifest-path "$proof/Cargo.toml" --target "$target"
done
python3 "$proof/prepare_dictionary.py" --with-source
universal="$here/build/proofreading-universal"
mkdir -p "$universal"
binary="$universal/hanautomata-proofread"
lipo -create "$proof/target/aarch64-apple-darwin/release/hanautomata-proofread" \
    "$proof/target/x86_64-apple-darwin/release/hanautomata-proofread" -output "$binary"
if [ -n "${HANAUTOMATA_SIGN_ID:-}" ]; then
    codesign --force --options runtime --timestamp --sign "$HANAUTOMATA_SIGN_ID" "$binary"
else
    codesign --force --sign - --timestamp=none "$binary"
fi
codesign --verify --strict "$binary"
python3 "$proof/package.py" --binary "$binary" --data "$root/build/proofreading-data" \
    --output "$app/Contents/Resources/proofreading" --target aarch64-apple-darwin --additional-target x86_64-apple-darwin
"$here/sign.sh" "$app"
