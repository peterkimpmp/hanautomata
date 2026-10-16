#!/usr/bin/env python3
"""Generate host-build notices and retain exact runtime crate source archives."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess

HERE = Path(__file__).resolve().parent


def cargo(*args):
    return subprocess.check_output(["cargo", *args, "--manifest-path", str(HERE / "Cargo.toml"), "--locked", "--offline"], text=True, encoding="utf-8")


def runtime_keys(tree):
    return set(re.findall(r"^(\S+) v(\S+)(?: \(\*\))?$", tree, re.M))


def generate(output, target=None):
    host = target or next(line.split(": ", 1)[1] for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    tree = cargo("tree", "--target", host, "-e", "normal,no-proc-macro", "--prefix", "none", "--format", "{p}")
    keys = runtime_keys(tree)
    meta = json.loads(cargo("metadata", "--format-version", "1", "--filter-platform", host))
    packages = [p for p in meta["packages"] if (p["name"], p["version"]) in keys and p["source"] is not None]
    if len(packages) != len(keys):
        raise ValueError("runtime crate coverage mismatch")
    # The Cargo.lock, not the archive filename, is the provenance authority.
    locks = {}
    for block in (HERE / "Cargo.lock").read_text(encoding="utf-8").split("[[package]]"):
        parts = dict(re.findall(r'^(name|version|checksum) = "([^\"]+)"$', block, re.M))
        if "checksum" in parts:
            locks[(parts["name"], parts["version"])] = parts["checksum"]
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    sources = output / "sources"
    sources.mkdir(exist_ok=True)
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    rust_version = subprocess.check_output(["rustc", "--version"], text=True).strip()
    shutil.copy2(sysroot / "share/doc/rust/COPYRIGHT-library.html", sources / "RUST-COPYRIGHT-library.html")
    apache = (sysroot / "share/doc/rust/licenses/Apache-2.0.txt").read_text(encoding="utf-8")
    mit = (sysroot / "share/doc/rust/licenses/MIT.txt").read_text(encoding="utf-8").replace("Copyright (c) <year> <copyright holders>", "Copyright (c) The Rust Project Contributors")
    rows, texts = [], {}
    def add(label, body):
        texts.setdefault(hashlib.sha256(body.encode()).hexdigest(), [[], body])[0].append(label)
    add("Rust standard library (MIT option)", mit)
    add("hunspell-rs 0.4.0: Copyright 2016 Lipka Boldizsár; 2019 Alberto Piai; 2020 Bernhard Schuster", apache)
    for p in sorted(packages, key=lambda p: p["name"]):
        folder = Path(p["manifest_path"]).parent
        label = f"{p['name']} {p['version']}"
        archive = folder.parents[2] / "cache" / folder.parent.name / f"{p['name']}-{p['version']}.crate"
        if hashlib.sha256(archive.read_bytes()).hexdigest() != locks[(p["name"], p["version"])]:
            raise ValueError("crate source checksum mismatch")
        shutil.copy2(archive, sources / archive.name)
        rows.append(f"| {label} | {p['license']} | [source](sources/{archive.name}) |")
        files = [f for f in folder.iterdir() if f.is_file() and re.match(r"LICENSE|LICENCE|COPYING|UNLICENSE|NOTICE", f.name, re.I)]
        if not files and p["name"] != "hunspell-rs":
            raise ValueError(f"missing license text: {label}")
        for f in sorted(files):
            add(label + " / " + f.name, f.read_text(encoding="utf-8"))
        if p["name"] == "hunspell-sys":
            for name in ["COPYING.MPL", "license.hunspell", "license.myspell"]:
                add("bundled Hunspell 1.7.1 / " + name, (folder / "vendor" / name).read_text(encoding="utf-8"))
    out = ["# Proofreading CLI third-party notices", "",
        f"Generated from Cargo.lock for `{host}` using `{rust_version}`. Hanautomata-authored code is MIT.", "",
        "Native Hunspell 1.7.1 is contained in hunspell-sys's source archive. This prototype selects its MPL-1.1 option; upstream MySpell notices also apply. No native sources were modified.", "",
        "The Korean dictionary is a separate GPL-3.0-or-later component. Preserve dictionary/LICENSE.md, dictionary/LICENSE.GPL-3 and the pinned dictionary source archive when distributing it. The directory name may differ for a local build.", "",
        "Source archives for the linked Rust libraries are adjacent in sources/. Build-only crates are not linked. Rust compiler/runtime source and detailed component notices are available with the matching Rust toolchain (https://github.com/rust-lang/rust).", "",
        "| Component | Upstream license expression | Exact source |", "|---|---|---|", *rows, "", "## License texts"]
    for labels, body in texts.values():
        out.extend(["", "### " + ", ".join(labels), "", "```text", body.strip(), "```"])
    (output / "THIRD-PARTY-NOTICES.md").write_text("\n".join(out) + "\n", encoding="utf-8")
    shutil.copy2(HERE.parents[2] / "LICENSE", output / "LICENSE")
    print(f"{len(packages)} runtime crates: source hashes verified, notices written to {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=HERE.parents[2] / "build/proofreading")
    parser.add_argument("--target", help="Cargo target of the binary being packaged")
    args = parser.parse_args()
    generate(args.output, args.target)
