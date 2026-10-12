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
# Wait for it to exit (up to 5 s) so the new copy can take over its connection name.
i=0; while pgrep -x Hanautomata >/dev/null && [ $i -lt 25 ]; do sleep 0.2; i=$((i + 1)); done
rm -rf "$dest/Hanautomata.app"
cp -R "$app" "$dest/"
echo "installed $dest/Hanautomata.app"
# Start the new copy now: after the old one is stopped, the system may not start it again by itself (2026-10-12), and
# apps then type without it. An app that was open through several restarts may still need to be reopened.
open -g "$dest/Hanautomata.app" 2>/dev/null || true
if "$dest/Hanautomata.app/Contents/MacOS/Hanautomata" --register; then
    echo "Select 한오토마타 (한) in the input menu."
    echo "An app that was typing with the old copy: switch to another input source and back (or reopen the app)."
else
    echo "Add it once: System Settings > Keyboard > Input Sources > Edit… > + > Korean > 한오토마타"
    echo "(or log out and back in, then run install.sh again)."
fi
"$dest/Hanautomata.app/Contents/MacOS/Hanautomata" --check || true
echo "Learning file: ~/Library/Application Support/Hanautomata/words.json"
