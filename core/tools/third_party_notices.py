# Writes THIRD-PARTY-NOTICES.md: the third-party code inside the Rust editions (Windows input method, macOS input method)
# and the license text each one ships. The Windows tray package includes the Rust personal expressions helper.
# Crates come from `cargo tree` for every release target, without proc-macro and build-only crates (they run at compile
# time and are not in the shipped files). Where a crate offers several licenses, this distribution uses the first of
# PREFERENCE. Runs offline against the local Cargo cache (any build fetches it). Rerun after a Cargo.lock change:
#   python3 core/tools/third_party_notices.py
import hashlib, json, pathlib, re, subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2]
LICENSES = pathlib.Path(__file__).resolve().parent / "licenses"
EDITIONS = (
    ("Windows 입력기", "tsf/Cargo.toml", ("x86_64-pc-windows-gnu", "i686-pc-windows-gnu")),
    ("macOS", "ffi/Cargo.toml", ("aarch64-apple-darwin", "x86_64-apple-darwin")),
    ("내 표현 서비스 (Windows 트레이·macOS)", "core/tools/assist/Cargo.toml", ("x86_64-pc-windows-msvc", "aarch64-apple-darwin", "x86_64-apple-darwin")),
)
PREFERENCE = ("MIT", "Apache-2.0", "Zlib", "0BSD", "Unlicense", "MPL-2.0")
NAME_HINT = {"MIT": "MIT", "Apache-2.0": "APACHE", "Zlib": "ZLIB", "0BSD": "0BSD", "Unlicense": "UNLICENSE", "MPL-2.0": "MPL"}
TEXT_HINT = {"MIT": "Permission is hereby granted", "Apache-2.0": "Apache License", "Zlib": "provided 'as-is'",
             "0BSD": "Permission to use, copy, modify", "Unlicense": "free and unencumbered", "MPL-2.0": "Mozilla Public License"}
SHARED = {"MPL-2.0": LICENSES / "MPL-2.0.txt"}  # the uniffi crates publish no license file


def cargo(*args):
    return subprocess.run(("cargo", *args), cwd=ROOT, check=True, capture_output=True, text=True).stdout


def linked(manifest, target):
    out = cargo("tree", "--manifest-path", manifest, "--target", target, "-e", "normal,no-proc-macro",
                "--prefix", "none", "--format", "{p}", "--offline", "--locked")
    crates = set()
    for line in out.splitlines():
        m = re.match(r"(\S+) v(\S+)(?: \((/[^)]*)\))?", line.strip())
        if m and not m.group(3):  # a path means one of this repository's own crates
            crates.add((m.group(1), m.group(2)))
    return crates


def choose(expression):
    offered = [part.strip("() ") for part in re.split(r"\s+OR\s+|/", expression)]
    for license in PREFERENCE:
        if license in offered:
            return license
    raise SystemExit(f"no license this tool knows in {expression!r}")


def license_text(package, license):
    folder = pathlib.Path(package["manifest_path"]).parent
    files = sorted(f for f in folder.iterdir() if f.is_file() and re.match(r"(LICEN[CS]E|COPYING|UNLICENSE)", f.name, re.I))
    named = [f for f in files if NAME_HINT[license] in f.name.upper()]
    generic = [f for f in files if f not in named and TEXT_HINT[license] in f.read_text(errors="replace")]
    for f in named + generic:
        return f.read_text(errors="replace").strip()
    if license in SHARED:
        return SHARED[license].read_text().strip()
    raise SystemExit(f"{package['name']} {package['version']} ships no {license} text")


def rust_std_text():
    sysroot = subprocess.run(("rustc", "--print", "sysroot"), check=True, capture_output=True, text=True).stdout.strip()
    template = (pathlib.Path(sysroot) / "share/doc/rust/licenses/MIT.txt").read_text().strip()
    text = template.replace("Copyright (c) <year> <copyright holders>", "Copyright (c) The Rust Project Contributors")
    if text == template:
        raise SystemExit("the toolchain's MIT template changed; fill in the Rust copyright line by hand")
    return text


def main():
    editions, packages = {}, {}
    for edition, manifest, targets in EDITIONS:
        meta = json.loads(cargo("metadata", "--manifest-path", manifest, "--format-version", "1", "--offline", "--locked"))
        by_key = {(p["name"], p["version"]): p for p in meta["packages"]}
        for target in targets:
            for key in linked(manifest, target):
                editions.setdefault(key, set()).add(edition)
                packages[key] = by_key[key]
    rustc = subprocess.run(("rustc", "--version"), check=True, capture_output=True, text=True).stdout.split()[1]
    texts = {}  # whitespace-insensitive digest -> [label, license, text, users]
    def add(license, text, user):
        digest = hashlib.sha256(" ".join(text.split()).encode()).hexdigest()
        entry = texts.setdefault(digest, [f"L{len(texts) + 1}", license, text, []])
        entry[3].append(user)
        return entry[0]
    std_label = add("MIT", rust_std_text(), f"Rust 표준 라이브러리 {rustc}")
    mingw_label = add("MinGW-w64 runtime", (LICENSES / "COPYING.MinGW-w64-runtime.txt").read_text().strip(), "MinGW-w64 런타임")
    rows = []
    for key in sorted(packages, key=lambda k: (k[0].lower(), k[1])):
        package = packages[key]
        license = choose(package["license"] or "")
        label = add(license, license_text(package, license), f"{key[0]} {key[1]}")
        source = package.get("repository") or f"https://crates.io/crates/{key[0]}/{key[1]}"
        rows.append(f"| {key[0]} | {key[1]} | {' · '.join(sorted(editions[key]))} | {package['license']} | {license} | {source} | {label} |")
    out = [
        "# 제3자 구성 요소 고지",
        "",
        "Hanautomata의 Windows 입력기(TSF)·macOS 입력기·내 표현 서비스에 들어가는 제3자 코드와 그 라이선스입니다. Windows 트레이 패키지도 "
        "Rust 내 표현 서비스를 포함합니다. 데이터 파일의 출처와 조건은 [NOTICE.md](NOTICE.md)에 있습니다.",
        "",
        "`core/tools/third_party_notices.py`가 각 구성의 Cargo.lock에서 만듭니다. 직접 고치지 않습니다. 라이선스를 고를 수 있는 "
        "크레이트는 '적용' 열의 라이선스로 배포합니다. macOS 목록에는 바인딩 생성용 uniffi `cli` 기능의 의존성도 들어 있습니다. "
        "실행 파일에 남지 않는 코드일 수 있지만 빠짐없이 적었습니다.",
        "",
        "## 툴체인 런타임",
        "",
        "| 구성 요소 | 포함 구성 | 라이선스 | 본문 |",
        "|---|---|---|---|",
        f"| Rust 표준 라이브러리 {rustc} | Windows 입력기 · macOS · 내 표현 서비스 | MIT OR Apache-2.0 (적용 MIT) · 구성 요소별 고지는 Rust 툴체인의 "
        f"`share/doc/rust/COPYRIGHT-library.html` | {std_label} |",
        f"| MinGW-w64 런타임 (x86_64·i686-pc-windows-gnu 툴체인이 정적으로 링크) | Windows 입력기 | 본문의 조항별 조건 | {mingw_label} |",
        "",
        f"## 크레이트 ({len(rows)}개)",
        "",
        "MPL-2.0 크레이트(uniffi)를 포함한 모든 크레이트의 소스 코드는 '출처'의 저장소와 crates.io의 같은 판에서 받을 수 있습니다.",
        "",
        "| 크레이트 | 판 | 포함 구성 | 선언 라이선스 | 적용 | 출처 | 본문 |",
        "|---|---|---|---|---|---|---|",
        *rows,
        "",
        "## 라이선스 본문",
    ]
    for label, license, text, users in texts.values():
        out += ["", f"### {label} · {license}", "", "적용 대상: " + ", ".join(users), "", "```text", text, "```"]
    (ROOT / "THIRD-PARTY-NOTICES.md").write_text("\n".join(out) + "\n", encoding="utf-8")
    print(f"THIRD-PARTY-NOTICES.md: {len(rows)} crates, {len(texts)} license texts")


main()
