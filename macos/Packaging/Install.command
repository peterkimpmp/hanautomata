#!/bin/sh
# User-local, verified replacement. An old bundle is moved intact to Backups, never deleted.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
app="$here/Hanautomata.app"
dest="$HOME/Library/Input Methods"
backups="$HOME/Library/Application Support/Hanautomata/Backups"
activate=yes
while [ "$#" -gt 0 ]; do
    case "$1" in
        --app) app=${2:?missing app}; shift 2 ;;
        --destination) dest=${2:?missing destination}; shift 2 ;;
        --backups) backups=${2:?missing backup directory}; shift 2 ;;
        --no-activate) activate=no; shift ;;
        *) echo 'usage: Install.command [--app APP] [--destination DIR] [--backups DIR] [--no-activate]' >&2; exit 2 ;;
    esac
done
installed="$dest/Hanautomata.app"
[ -d "$app" ] && [ ! -L "$app" ] || { echo 'A real Hanautomata.app bundle is required.' >&2; exit 1; }
[ ! -L "$dest" ] && [ ! -L "$installed" ] || { echo 'Refusing a symbolic-link install destination.' >&2; exit 1; }
[ "$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$app/Contents/Info.plist")" = \
  'io.github.peterkimpmp.inputmethod.Hanautomata' ] || { echo 'Wrong application identifier.' >&2; exit 1; }
/usr/bin/codesign --verify --deep --strict "$app"
payload="$app/Contents/Resources/proofreading"
if [ -d "$payload" ]; then
    for required in hanautomata-proofread MANIFEST.json LICENSE THIRD-PARTY-NOTICES.md \
        data/ko.aff data/ko.dic data/dictionary.lock.json data/dictionary-source.zip \
        data/LICENSE.md data/LICENSE.GPL-3 sources/RUST-COPYRIGHT-library.html; do
        [ -s "$payload/$required" ] || { echo "Incomplete proofreading payload: $required" >&2; exit 1; }
    done
    [ -x "$payload/hanautomata-proofread" ] || { echo 'The proofreading helper is not executable.' >&2; exit 1; }
fi
mkdir -p "$dest"
lock="$dest/.hanautomata-install.lock"
if ! mkdir "$lock" 2>/dev/null; then
    echo "Another installation holds $lock. Wait for it to finish." >&2
    exit 1
fi
stage=
backup=
published=no
committed=no
cleanup() {
    result=$?
    trap - EXIT HUP INT TERM
    if [ "$result" -ne 0 ] && [ "$committed" != yes ]; then
        if [ "$published" = yes ]; then mv "$installed" "$stage/failed.app"; fi
        if [ -n "$backup" ] && [ -d "$backup/Hanautomata.app" ] && [ ! -e "$installed" ]; then
            mv "$backup/Hanautomata.app" "$installed"
            echo 'Installation failed; the previous bundle was restored.' >&2
        fi
    fi
    [ -z "$stage" ] || rm -rf "$stage"
    rmdir "$lock"
    exit "$result"
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM
stage=$(mktemp -d "$(dirname "$dest")/.hanautomata-stage.XXXXXX")
if [ -d "$installed/Contents/Resources/proofreading" ] && [ ! -d "$app/Contents/Resources/proofreading" ]; then
    echo 'The installed proofreading feature would be lost. Use package.sh --with-proofreading.' >&2
    exit 1
fi
/usr/bin/ditto "$app" "$stage/Hanautomata.app"
/usr/bin/codesign --verify --deep --strict "$stage/Hanautomata.app"
/usr/bin/diff -qr "$app" "$stage/Hanautomata.app" >/dev/null
if [ "$activate" = yes ]; then
    # Stop only this user's installed executable, after a verified replacement exists.
    for pid in $(pgrep -u "$(id -u)" -x Hanautomata || true); do
        if [ "$(ps -p "$pid" -o comm=)" = "$installed/Contents/MacOS/Hanautomata" ]; then kill -TERM "$pid"; fi
    done
    i=0
    while pgrep -u "$(id -u)" -x Hanautomata >/dev/null && [ "$i" -lt 25 ]; do sleep 0.2; i=$((i + 1)); done
    if pgrep -u "$(id -u)" -x Hanautomata >/dev/null; then
        echo 'Dadeum is still running. Quit it, then run this installer again; installed files were not changed.' >&2
        exit 1
    fi
fi
if [ -e "$installed" ]; then
    mkdir -p "$backups"
    backup=$(mktemp -d "$backups/upgrade-$(date +%Y%m%d-%H%M%S).XXXXXX")
    mv "$installed" "$backup/Hanautomata.app"
fi
mv "$stage/Hanautomata.app" "$installed"
published=yes
/usr/bin/codesign --verify --deep --strict "$installed"
committed=yes
echo "Installed files: $installed"
[ -z "$backup" ] || echo "Previous bundle: $backup/Hanautomata.app"
if [ "$activate" = yes ]; then
    if ! "$installed/Contents/MacOS/Hanautomata" --register; then
        echo 'Add 다듬 in System Settings > Keyboard > Input Sources, or log out and back in.'
    fi
    open -g "$installed" || echo 'If macOS blocked launch, use Privacy & Security > Open Anyway for Dadeum.'
    "$installed/Contents/MacOS/Hanautomata" --check || true
fi
echo 'Switch to another input source and back to Dadeum in apps that used the previous version.'
echo 'Activation is not verified by this installer. Confirm the new version in Dadeum settings > About.'
echo 'Settings and personal learning files are preserved.'
