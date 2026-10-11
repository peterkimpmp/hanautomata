#!/bin/sh
# Regenerates the C# golden suite (full in core/target/golden, committed lite sample in core/tests/golden-lite) and runs
# the Rust parity gate G1. Needs the .NET 9 SDK (DOTNET=/path/to/dotnet to override) and cargo.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
dotnet=${DOTNET:-dotnet}
cd "$root"
"$dotnet" build tests/dotnet/Hanautomata.Golden -c Release -v quiet
"$dotnet" tests/dotnet/Hanautomata.Golden/bin/Release/net9.0/Hanautomata.Golden.dll --out core/target/golden
"$dotnet" tests/dotnet/Hanautomata.Golden/bin/Release/net9.0/Hanautomata.Golden.dll --out core/tests/golden-lite --lite 40
cd core
cargo test --release
