#!/bin/sh
# One product version for both platforms: VERSION is the source, and every file that ships a version must match it
# (macOS Info.plist, Windows AssemblyInfo.cs and app.manifest, the four Rust crates). macos/build.sh and CI run this.
#   ./check-version.sh [repository root]    prints each mismatch and exits 1
set -eu
root=${1:-$(cd "$(dirname "$0")" && pwd)}
want=$(tr -d ' \r\n' < "$root/VERSION")
fail=0
check() {
    if [ "$2" != "$3" ]; then
        echo "version mismatch: $1 has '$2', VERSION says '$3'" >&2
        fail=1
    fi
}
check macos/Info.plist \
    "$(sed -n '/CFBundleShortVersionString/{n;s/.*<string>\(.*\)<\/string>.*/\1/p;}' "$root/macos/Info.plist")" "$want"
for key in AssemblyVersion AssemblyFileVersion; do
    check "src/AssemblyInfo.cs $key" "$(sed -n "s/.*\[assembly: $key(\"\(.*\)\")\].*/\1/p" "$root/src/AssemblyInfo.cs")" "$want.0"
done
check app.manifest "$(sed -n 's/.*<assemblyIdentity version="\([^"]*\)".*/\1/p' "$root/app.manifest")" "$want.0"
for crate in core ffi tsf core/tools/proofread; do
    check "$crate/Cargo.toml" "$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/$crate/Cargo.toml" | head -n 1)" "$want"
done
[ "$fail" -eq 0 ] && echo "version $want: Info.plist, AssemblyInfo.cs, app.manifest and 4 Cargo.toml match"
exit "$fail"
