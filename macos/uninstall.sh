#!/bin/sh
# Disables and removes the input method. The learning file in ~/Library/Application Support/Hanautomata is kept.
set -eu
app="$HOME/Library/Input Methods/Hanautomata.app"
[ -d "$app" ] && "$app/Contents/MacOS/Hanautomata" --unregister || true
pkill -x Hanautomata 2>/dev/null || true
rm -rf "$app"
echo "removed $app (learning file kept)"
