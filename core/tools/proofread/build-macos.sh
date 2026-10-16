#!/bin/sh
# Local experimental bundle and an isolated AppKit demo. Does not install the IME.
set -eu
case "${1:-}" in ""|--test) ;; *) echo 'usage: build-macos.sh [--test]' >&2; exit 2 ;; esac
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
cd "$root"
cargo build --release --locked --manifest-path "$here/Cargo.toml"
python3 "$here/prepare_dictionary.py" --with-source
"$root/macos/build.sh" "${1:-}"
app="$root/macos/build/Hanautomata.app"
python3 "$here/package.py" --binary "$here/target/release/hanautomata-proofread" \
  --data "$root/build/proofreading-data" --output "$app/Contents/Resources/proofreading"
"$root/macos/sign.sh" "$app"
demo="$root/macos/build/Hanautomata-Proofreading-Demo.app"
python3 - "$app" "$demo" <<'PY'
import pathlib, plistlib, shutil, sys
source, target = map(pathlib.Path, sys.argv[1:])
if target.exists(): shutil.rmtree(target)
shutil.copytree(source, target)
info = target/'Contents/Info.plist'
with info.open('rb') as f: config = plistlib.load(f)
for key in list(config):
    if key.startswith(('InputMethod', 'ComponentInputMode', 'TIS', 'tsInput')): del config[key]
config.update(CFBundleIdentifier='io.github.peterkimpmp.Hanautomata.ProofreadingDemo',
              CFBundleName='Hanautomata Proofreading Demo', CFBundleDisplayName='Hanautomata Proofreading Demo',
              LSUIElement=False, HanautomataProofreadingDemo=True)
with info.open('wb') as f: plistlib.dump(config, f)
PY
"$root/macos/sign.sh" "$demo"
printf 'Experimental demo: %s\n' "$demo"
