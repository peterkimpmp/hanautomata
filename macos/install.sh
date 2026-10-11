#!/bin/sh
# Installs the built input method for this user only (M-02: no administrator rights) and enables it.
# First install: if the input source is not found, log out and back in, then run this again.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
app="$here/build/Hanautomata.app"
dest="$HOME/Library/Input Methods"
[ -d "$app" ] || { echo "build first: $here/build.sh" >&2; exit 1; }
mkdir -p "$dest"
pkill -x Hanautomata 2>/dev/null || true
rm -rf "$dest/Hanautomata.app"
cp -R "$app" "$dest/"
echo "installed $dest/Hanautomata.app"
if "$dest/Hanautomata.app/Contents/MacOS/Hanautomata" --register; then
    echo "Select 한오토마타 (한) in the input menu."
else
    echo "Add it once: System Settings > Keyboard > Input Sources > Edit… > + > Korean > 한오토마타"
    echo "(or log out and back in, then run install.sh again)."
fi
echo "Learning file: ~/Library/Application Support/Hanautomata/words.json"
