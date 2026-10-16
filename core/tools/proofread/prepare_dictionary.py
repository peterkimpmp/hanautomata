#!/usr/bin/env python3
"""Fetch a pinned public dictionary; never called by the runtime. Python 3.9+."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import tempfile
import urllib.request
import zipfile

HERE = Path(__file__).resolve().parent
LOCK = json.loads((HERE / "dictionary.lock.json").read_text(encoding="utf-8"))


def verify(data, spec):
    if len(data) != spec["size"] or hashlib.sha256(data).hexdigest() != spec["sha256"]:
        raise ValueError("dictionary size/SHA-256 mismatch")


def prepare(output, archive=None, with_source=False):
    spec = LOCK["archive"]
    if archive is None:
        with urllib.request.urlopen(spec["url"], timeout=45) as response:
            raw = response.read(spec["size"] + 1)
    else:
        with Path(archive).open("rb") as stream:
            raw = stream.read(spec["size"] + 1)
    verify(raw, spec)
    files = {}
    with zipfile.ZipFile(io.BytesIO(raw)) as package:
        for item in LOCK["files"]:
            name = spec["prefix"] + item["name"]
            if package.getinfo(name).file_size != item["size"]:
                raise ValueError("dictionary entry size mismatch")
            with package.open(name) as stream:
                data = stream.read(item["size"] + 1)
            verify(data, item)
            files[item["name"]] = data
    # Verify everything before touching output; no archive paths are extracted.
    output = Path(output)
    files["dictionary.lock.json"] = (HERE / "dictionary.lock.json").read_bytes()
    files["ko-aff-dic-0.7.94.zip"] = raw
    if with_source:
        spec = LOCK["source_archive"]
        cached = output / "hunspell-ko-source.zip"
        if cached.exists():
            with cached.open("rb") as stream:
                source = stream.read(spec["size"] + 1)
        else:
            with urllib.request.urlopen(spec["url"], timeout=45) as response:
                source = response.read(spec["size"] + 1)
        verify(source, spec)
        files["hunspell-ko-source.zip"] = source
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".prepare-", dir=output) as stage:
        for name, data in files.items():
            p = Path(stage) / name
            p.write_bytes(data)
        for name in files:
            os.replace(Path(stage) / name, output / name)
    return LOCK["id"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=HERE.parents[2] / "build/proofreading-data")
    parser.add_argument("--archive", type=Path, help="use an already downloaded archive without network")
    parser.add_argument("--with-source", action="store_true", help="also obtain and verify the matching dictionary source archive")
    args = parser.parse_args()
    try:
        print(prepare(args.output, args.archive, args.with_source))
    except (OSError, ValueError, zipfile.BadZipFile, KeyError):
        raise SystemExit("dictionary preparation failed; original output was not validated for use")


if __name__ == "__main__":
    main()
