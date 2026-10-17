#!/bin/sh
# Installs the built input method for this user only (M-02: no administrator rights) and enables it.
# First install: if the input source is not found, log out and back in, then run this again.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
exec /bin/sh "$here/Packaging/Install.command" --app "$here/build/Hanautomata.app" "$@"
