#!/usr/bin/env python3
"""Assemble an experimental local payload; never installs, registers, publishes, or downloads."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
from write_notices import generate

HERE=Path(__file__).resolve().parent

def package(binary, data, output, target=None):
    if target is None:
        target=next(line.split(': ',1)[1] for line in subprocess.check_output(['rustc','-vV'],text=True).splitlines() if line.startswith('host: '))
    lock=json.loads((HERE/'dictionary.lock.json').read_text())
    output.mkdir(parents=True,exist_ok=True)
    destination=output/'data'; destination.mkdir(exist_ok=True)
    for metadata in lock['files']:
        name=metadata['name']
        original=data/name
        if hashlib.sha256(original.read_bytes()).hexdigest()!=metadata['sha256']: raise ValueError('dictionary checksum failed')
        shutil.copy2(original,destination/name)
    # Require the complete pinned source, not just a source URL, in the development payload.
    source=lock['source_archive']
    candidates=[p for p in data.iterdir() if p.is_file() and p.stat().st_size==source['size']]
    matched=next((p for p in candidates if hashlib.sha256(p.read_bytes()).hexdigest()==source['sha256']),None)
    if matched is None: raise ValueError('Run prepare_dictionary.py --with-source first')
    shutil.copy2(matched,destination/'dictionary-source.zip')
    shutil.copy2(HERE/'dictionary.lock.json',destination/'dictionary.lock.json')
    shutil.copy2(binary,output/binary.name)
    generate(output,target)
    manifest={p.relative_to(output).as_posix():dict(bytes=p.stat().st_size,sha256=hashlib.sha256(p.read_bytes()).hexdigest())
              for p in sorted(output.rglob('*')) if p.is_file() and p.name!='MANIFEST.json'}
    (output/'MANIFEST.json').write_text(json.dumps(dict(schema_version=1,experimental=True,target=target,
        rustc=subprocess.check_output(['rustc','--version'],text=True).strip(),files=manifest),indent=2)+'\n')
    print('Payload verified:',len(manifest),'files in',output)

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',required=True,type=Path)
    parser.add_argument('--data',required=True,type=Path)
    parser.add_argument('--output',required=True,type=Path)
    parser.add_argument('--target')
    args=parser.parse_args(); package(args.binary,args.data,args.output,args.target)
