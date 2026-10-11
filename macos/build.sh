#!/bin/sh
# Builds macos/build/Hanautomata.app (InputMethodKit input method) for this Mac's architecture.
#   ./build.sh          build the bundle (Rust engine via UniFFI + Swift shell), ad-hoc signed
#   ./build.sh --test   also run the Rust engine tests and the Swift key-map tests
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
out="$here/build"
app="$out/Hanautomata.app"
gen="$out/generated"
arch=$(uname -m)
target="$arch-apple-macos14.0"
export MACOSX_DEPLOYMENT_TARGET=14.0

if [ "${1:-}" = "--test" ]; then
    (cd "$root/core" && cargo test --release --quiet)
    mkdir -p "$out"
    swiftc -swift-version 5 -target "$target" "$here/Sources/KeyMap.swift" "$here/Tests/KeyMap/main.swift" -o "$out/keymap-tests"
    "$out/keymap-tests"
fi

cargo build --release --quiet --manifest-path "$root/ffi/Cargo.toml"
ffi="$root/ffi/target/release"
rm -rf "$gen" "$app"
mkdir -p "$gen" "$app/Contents/MacOS" "$app/Contents/Resources"
(cd "$root/ffi" && "$ffi/uniffi-bindgen" generate --library "$ffi/libhanautomata_ffi.dylib" --language swift --out-dir "$gen" >/dev/null)

# Link the static library by path: -l would pick the dylib next to it.
swiftc -O -swift-version 5 -target "$target" -module-name Hanautomata \
    -Xcc -fmodule-map-file="$gen/hanautomata_ffiFFI.modulemap" -I "$gen" \
    "$gen/hanautomata_ffi.swift" "$here"/Sources/*.swift \
    "$ffi/libhanautomata_ffi.a" \
    -framework Cocoa -framework InputMethodKit -framework Carbon \
    -o "$app/Contents/MacOS/Hanautomata"
if [ "${1:-}" = "--test" ]; then
    swiftc -swift-version 5 -target "$target" -module-name EngineTests \
        -Xcc -fmodule-map-file="$gen/hanautomata_ffiFFI.modulemap" -I "$gen" \
        "$gen/hanautomata_ffi.swift" "$here/Tests/Engine/main.swift" "$ffi/libhanautomata_ffi.a" -o "$out/engine-tests"
    "$out/engine-tests"
fi
cp "$here/Info.plist" "$app/Contents/Info.plist"
cp -R "$here/Resources/ko.lproj" "$here/Resources/en.lproj" "$app/Contents/Resources/"
swift "$here/Tools/make-icon.swift" "$app/Contents/Resources/icon.tiff" >/dev/null
codesign --force --sign - --timestamp=none "$app" 2>/dev/null
echo "built $app ($arch, ad-hoc signed)"
