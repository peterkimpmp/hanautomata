#!/bin/sh
# Builds macos/build/Hanautomata.app (InputMethodKit input method).
#   ./build.sh              this Mac's architecture, ad-hoc signed
#   ./build.sh --test       also run the Rust engine tests and the Swift key-map, engine-bridge and input-logic tests
#   ./build.sh --universal  Apple Silicon + Intel in one bundle (M-14; package.sh uses it)
# Signing: ad-hoc unless HANAUTOMATA_SIGN_ID names a Developer ID Application identity (decision D15; see sign.sh).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
out="$here/build"
app="$out/Hanautomata.app"
gen="$out/generated"
"$root/check-version.sh" >/dev/null
host=$(uname -m)
archs=$host
[ "${1:-}" = "--universal" ] && archs="arm64 x86_64"
export MACOSX_DEPLOYMENT_TARGET=14.0

rust_target() { [ "$1" = arm64 ] && echo aarch64-apple-darwin || echo x86_64-apple-darwin; }

if [ "${1:-}" = "--test" ]; then
    (cd "$root/core" && cargo test --release --locked --quiet)
    cargo test --release --locked --quiet --manifest-path "$root/core/tools/assist/Cargo.toml"
    mkdir -p "$out"
    swiftc -swift-version 5 -target "$host-apple-macos14.0" "$here/Sources/ProofreadingModel.swift" \
        "$here/Tests/Proofreading/main.swift" -o "$out/proofreading-tests"
    "$out/proofreading-tests"
    swiftc -swift-version 5 -target "$host-apple-macos14.0" "$here/Sources/KeyMap.swift" "$here/Tests/KeyMap/main.swift" \
        -o "$out/keymap-tests"
    "$out/keymap-tests"
fi

# The host library generates the Swift bindings and links the tests.
cargo build --release --locked --quiet --manifest-path "$root/ffi/Cargo.toml"
ffi="$root/ffi/target/release"
rm -rf "$gen" "$app"
mkdir -p "$gen" "$app/Contents/MacOS" "$app/Contents/Resources"
(cd "$root/ffi" && "$ffi/uniffi-bindgen" generate --library "$ffi/libhanautomata_ffi.dylib" --language swift --out-dir "$gen" >/dev/null)

# Link the static library by path: -l would pick the dylib next to it.
binaries=""
assist_binaries=""
for arch in $archs; do
    rt=$(rust_target "$arch")
    cargo build --release --locked --quiet --manifest-path "$root/core/tools/assist/Cargo.toml" --target "$rt"
    assist_binaries="$assist_binaries $root/core/tools/assist/target/$rt/release/hanautomata-assist"
    if [ "$archs" = "$host" ]; then
        lib="$ffi/libhanautomata_ffi.a"
    else
        rt=$(rust_target "$arch")
        cargo build --release --locked --quiet --manifest-path "$root/ffi/Cargo.toml" --lib --target "$rt"
        lib="$root/ffi/target/$rt/release/libhanautomata_ffi.a"
    fi
    swiftc -O -swift-version 5 -target "$arch-apple-macos14.0" -module-name Hanautomata \
        -Xcc -fmodule-map-file="$gen/hanautomata_ffiFFI.modulemap" -I "$gen" \
        "$gen/hanautomata_ffi.swift" "$here"/Sources/*.swift "$lib" \
        -framework Cocoa -framework InputMethodKit -framework Carbon -framework IOKit -framework SwiftUI \
        -o "$out/Hanautomata-$arch"
    binaries="$binaries $out/Hanautomata-$arch"
done
# shellcheck disable=SC2086
lipo -create $binaries -output "$app/Contents/MacOS/Hanautomata"
# shellcheck disable=SC2086
lipo -create $assist_binaries -output "$app/Contents/MacOS/hanautomata-assist"
if [ -n "${HANAUTOMATA_SIGN_ID:-}" ]; then
    codesign --force --options runtime --timestamp --sign "$HANAUTOMATA_SIGN_ID" "$app/Contents/MacOS/hanautomata-assist"
else
    codesign --force --sign - --timestamp=none "$app/Contents/MacOS/hanautomata-assist"
fi
# shellcheck disable=SC2086
rm -f $binaries

if [ "${1:-}" = "--test" ]; then
    swiftc -swift-version 5 "$here/Sources/PersonalAssistClient.swift" "$here/Tests/PersonalAssist/main.swift" -o "$out/assist-tests"
    "$out/assist-tests" "$app/Contents/MacOS/hanautomata-assist"
    swiftc -swift-version 5 "$here/Sources/PersonalAssistClient.swift" "$here/Sources/PersonalAssistPanel.swift" \
        "$here/Tests/PersonalAssistUI/main.swift" -framework Cocoa -o "$out/assist-ui-tests"
    "$out/assist-ui-tests" "$app/Contents/MacOS/hanautomata-assist" "$out/personal-assist-preview.png"
    swiftc -swift-version 5 -target "$host-apple-macos14.0" -module-name EngineTests \
        -Xcc -fmodule-map-file="$gen/hanautomata_ffiFFI.modulemap" -I "$gen" \
        "$gen/hanautomata_ffi.swift" "$here/Tests/Engine/main.swift" "$ffi/libhanautomata_ffi.a" -o "$out/engine-tests"
    "$out/engine-tests"
    swiftc -swift-version 5 -target "$host-apple-macos14.0" -module-name InputTests \
        -Xcc -fmodule-map-file="$gen/hanautomata_ffiFFI.modulemap" -I "$gen" \
        "$gen/hanautomata_ffi.swift" "$here/Sources/KeyMap.swift" "$here/Sources/Settings.swift" \
        "$here/Sources/InputCore.swift" "$here/Sources/Diagnostics.swift" "$here/Tests/Input/main.swift" \
        "$ffi/libhanautomata_ffi.a" -o "$out/input-tests"
    "$out/input-tests" "$root/tests/corpora"
    swiftc -swift-version 5 -target "$host-apple-macos14.0" -module-name AppKitTests \
        -Xcc -fmodule-map-file="$gen/hanautomata_ffiFFI.modulemap" -I "$gen" \
        "$gen/hanautomata_ffi.swift" "$here/Sources/KeyMap.swift" "$here/Sources/Settings.swift" \
        "$here/Sources/InputCore.swift" "$here/Sources/Diagnostics.swift" "$here/Tests/AppKit/main.swift" \
        "$ffi/libhanautomata_ffi.a" -framework Cocoa -o "$out/appkit-tests"
    "$out/appkit-tests"
    python3 "$here/Tests/test_install.py"
fi
cp "$here/Info.plist" "$app/Contents/Info.plist"
cp -R "$here/Resources/ko.lproj" "$here/Resources/en.lproj" "$app/Contents/Resources/"
res="$app/Contents/Resources"
swift "$here/Tools/make-icon.swift" "$res/icon.tiff" "한" >/dev/null
swift "$here/Tools/make-icon.swift" "$res/icon-auto.tiff" "가A" 10 >/dev/null
swift "$here/Tools/make-icon.swift" "$res/icon-korean.tiff" "가" >/dev/null
swift "$here/Tools/make-icon.swift" "$res/icon-english.tiff" "A" >/dev/null
"$here/sign.sh" "$app"
echo "built $app ($(lipo -archs "$app/Contents/MacOS/Hanautomata"))"
