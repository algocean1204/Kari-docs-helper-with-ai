#!/usr/bin/env python3
"""앱에 들어가는 제3자 코드의 라이선스 고지(THIRD_PARTY_LICENSES.md)를 만든다. 표준 라이브러리만 쓴다.

    python3 app/scripts/third_party.py            # Docs/THIRD_PARTY_LICENSES.md를 다시 쓴다
    python3 app/scripts/third_party.py --check    # 다시 만든 결과가 파일과 다르면 1로 끝난다

읽는 것
- Rust: app/engine/에서 `cargo metadata --locked --offline`. kari-ffi에서 일반 의존성만 따라간다(시험·예제·빌드
  스크립트용 제외). 라이선스 글은 각 크레이트 폴더(~/.cargo/registry/src/...)의 LICENSE*, COPYING*, NOTICE*.
- Dart: app/.dart_tool/package_graph.json과 package_config.json(`flutter pub get`이 만든다). kari의
  dependencies만 따라간다(dev_dependencies 제외). 라이선스 글은 각 패키지 폴더의 LICENSE*.
- Skia와 Skia가 품은 라이브러리: skia-bindings 크레이트 안의 skia/ 폴더.
- 글꼴: app/assets/fonts/Pretendard-LICENSE.txt.
- 지식 팩의 출처: app/engine/interview/pack/의 source 항목.
"""

import argparse
import difflib
import hashlib
import json
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
ENGINE, FONTS = HERE.parent / "engine", HERE.parent / "assets/fonts"  # 스크립트 폴더의 옆
ROOT = HERE.parent.parent
OUT = ROOT / "Docs/THIRD_PARTY_LICENSES.md"
SELF = f"python3 {(HERE / 'third_party.py').relative_to(ROOT)}"
TARGET = "aarch64-apple-darwin"

# MIT와 함께 바이너리로 배포하는 데 걸림이 없는 SPDX 식별자. 이 밖의 것은 "확인이 필요한 것"으로 모은다.
PERMISSIVE = {
    "MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause",
    "ISC", "Zlib", "Unicode-3.0", "Unicode-DFS-2016", "BSL-1.0", "0BSD", "CC0-1.0", "Unlicense",
    "CDLA-Permissive-2.0", "OFL-1.1", "libpng-2.0", "IJG",
}
COPYLEFT = re.compile(r"Mozilla Public License|\bMPL\b|GNU (Affero |Lesser |Library )?General Public License|\b(A|L)?GPL\b|\bSSPL\b")
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE|COPYRIGHT)", re.I)

# Skia 미리 빌드본(binary-cache, 기능 jpegd-jpege-pdf)에서 엔진 라이브러리에 실제로 링크된 것.
# 근거: 엔진 라이브러리의 문자열(libpng 1.6.58, libjpeg-turbo 3.1.0, wuffs, zlib의 오류 문구).
# HarfBuzz, FreeType, expat, ICU, libwebp의 기호는 없었다. Skia 기능을 바꾸면 다시 확인한다.
SKIA_PARTS = [
    ("Skia", "BSD-3-Clause", "https://skia.org", ["LICENSE"]),
    ("skcms(Skia 안)", "BSD-3-Clause", "https://skia.googlesource.com/skcms", ["modules/skcms/LICENSE"]),
    ("libpng", "libpng-2.0", "http://www.libpng.org/pub/png/libpng.html",
     ["third_party/externals/libpng/LICENSE"]),
    ("libjpeg-turbo", "IJG AND BSD-3-Clause AND Zlib", "https://libjpeg-turbo.org",
     ["third_party/externals/libjpeg-turbo/LICENSE.md", "third_party/externals/libjpeg-turbo/README.ijg"]),
    ("zlib(Chromium 판)", "Zlib", "https://chromium.googlesource.com/chromium/src/third_party/zlib",
     ["third_party/externals/zlib/LICENSE"]),
    ("Wuffs", "Apache-2.0", "https://github.com/google/wuffs", ["third_party/externals/wuffs/LICENSE"]),
]


def split_top(expr, op):
    """괄호 밖의 ` op `로 나눈다."""
    parts, depth, start, i = [], 0, 0, 0
    while i < len(expr):
        c = expr[i]
        depth += c == "("
        depth -= c == ")"
        if depth == 0 and expr.startswith(f" {op} ", i):
            parts.append(expr[start:i])
            i += len(op) + 2
            start = i
            continue
        i += 1
    return [p.strip() for p in parts + [expr[start:]]]


def spdx_ok(expr):
    """SPDX 표현에서 OR는 하나만, AND는 모두가 PERMISSIVE면 참."""
    expr = " ".join((expr or "").replace("/", " OR ").split())
    if not expr:
        return False
    alts = split_top(expr, "OR")
    if len(alts) > 1:
        return any(spdx_ok(a) for a in alts)
    parts = split_top(expr, "AND")
    if len(parts) > 1:
        return all(spdx_ok(p) for p in parts)
    if expr.startswith("(") and expr.endswith(")"):  # 위에서 나뉘지 않았으니 바깥 괄호 한 쌍이다
        return spdx_ok(expr[1:-1])
    return expr in PERMISSIVE


def guess(text):
    """라이선스 글에서 종류를 어림한다(Dart 패키지는 SPDX 표기가 없다)."""
    t = " ".join(text.split())
    if re.search(r"GNU (AFFERO|LESSER|LIBRARY)? ?GENERAL PUBLIC", t, re.I):
        return "GPL 계열(확인 필요)"
    if "Mozilla Public License" in t:
        return "MPL(확인 필요)"
    if "Apache License" in t and "Version 2.0" in t:
        return "Apache-2.0"
    if "Permission is hereby granted, free of charge" in t:
        return "MIT"
    if "Redistribution and use in source and binary forms" in t:
        return "BSD-3-Clause" if "Neither the name" in t or "names of its" in t else "BSD-2-Clause"
    if "SIL OPEN FONT LICENSE" in t.upper():
        return "OFL-1.1"
    return ""


def license_files(folder):
    folder = Path(folder)
    if not folder.is_dir():
        return []
    found = [p for p in sorted(folder.iterdir()) if p.is_file() and LICENSE_FILE.match(p.name)]
    for sub in ("LICENSES", "licenses"):
        if (folder / sub).is_dir():
            found += sorted(p for p in (folder / sub).iterdir() if p.is_file())
    return found


def read(p):
    return Path(p).read_text(encoding="utf-8", errors="replace").strip()


def rust():
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--offline", "--filter-platform", TARGET],
        cwd=ENGINE, check=True, capture_output=True, text=True).stdout)
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = next(i for i, p in pkgs.items() if p["name"] == "kari-ffi" and p["source"] is None)
    seen, stack = set(), [root]
    while stack:
        i = stack.pop()
        if i in seen:
            continue
        seen.add(i)
        stack += [d["pkg"] for d in nodes[i]["deps"] if any(k["kind"] is None for k in d["dep_kinds"])]
    rows = []
    for i in seen:
        p = pkgs[i]
        if p["source"] is None:  # 이 저장소의 크레이트(MIT)
            continue
        folder = Path(p["manifest_path"]).parent
        files = license_files(folder)
        if p.get("license_file"):
            lf = folder / p["license_file"]
            if lf.is_file() and lf not in files:
                files.append(lf)
        note = ""
        if not files:
            # 크레이트에 라이선스 파일이 없으면 상류 저장소에서 그 판의 것을 받아 둔 곳을 본다(받은 주소는 SOURCE.txt).
            local = HERE / "licenses" / f"{p['name']}-{p['version']}"
            files = license_files(local)
            if files:
                folder = local
                src = (local / "SOURCE.txt").read_text().split() if (local / "SOURCE.txt").is_file() else []
                note = "크레이트에 라이선스 파일이 없어 상류 저장소에서 받은 글: " + " ".join(src)
        url = p.get("repository") or p.get("homepage") or f"https://crates.io/crates/{p['name']}"
        why = ""
        local = HERE / "licenses" / f"{p['name']}-{p['version']}"
        if (local / "MPL-2.0.txt").is_file() and any("MPL" in read(f) for f in files):
            # 고지 글이 MPL 코드를 품었다고 밝히면, MPL-2.0 전문을 싣고 그 판의 소스 주소를 적어 조건(3.2)을 채운다.
            # MPL-PARTS.txt: 어느 코드가 MPL인지(고지 글과 크레이트 소스에서 확인해 적은 한 줄).
            files.append(local / "MPL-2.0.txt")
            parts = read(local / "MPL-PARTS.txt") if (local / "MPL-PARTS.txt").is_file() else "고지 글 참고"
            vcs = Path(p["manifest_path"]).parent / ".cargo_vcs_info.json"
            sha = json.loads(vcs.read_text())["git"]["sha1"] if vcs.is_file() else ""
            src = f"{url.rstrip('/')}/tree/{sha}" if sha and "github.com" in url else url
            note = f"일부 코드({parts})는 MPL-2.0. 소스: {src}" + (f". {note}" if note else "")
            why = "MPL-2.0 부분이 있음. 소스 받는 곳을 적어 조건을 채움"
        rows.append({"name": p["name"], "version": p["version"], "license": p["license"] or "",
                     "url": url, "files": files, "note": note, "folder": folder, "why": why})
    return sorted(rows, key=lambda r: (r["name"], r["version"]))


def dart():
    tool = ROOT / "app" / ".dart_tool"
    if not (tool / "package_graph.json").is_file():
        sys.exit("app/.dart_tool/package_graph.json이 없습니다. app/에서 `flutter pub get`을 먼저 돌리세요.")
    graph = {p["name"]: p for p in json.loads((tool / "package_graph.json").read_text())["packages"]}
    config = {p["name"]: p for p in json.loads((tool / "package_config.json").read_text())["packages"]}

    def reach(starts):
        seen, stack = set(), list(starts)
        while stack:
            n = stack.pop()
            if n not in seen:
                seen.add(n)
                stack += graph[n].get("dependencies", [])
        return seen

    direct = graph["kari"]["dependencies"]
    # hooks, code_assets는 빌드 훅(app/hook/build.dart)만 쓴다. 앱의 Dart 코드에는 들어가지 않는다.
    build_only = {"hooks", "code_assets"}
    shipped = reach([d for d in direct if d not in build_only])
    rows = []
    for n in sorted(reach(direct)):
        uri = config[n]["rootUri"]
        folder = Path(uri.removeprefix("file://")) if uri.startswith("file://") else (tool / uri).resolve()
        files = license_files(folder)
        note = "" if n in shipped else "빌드 때만 씀(앱에 들어가지 않음)"
        if n == "sky_engine":
            # Flutter 엔진(Skia, ICU, HarfBuzz, FreeType 등 포함)의 고지. 수 MB라 전문은 싣지 않는다.
            note = ("Flutter 엔진과 그 안의 Skia 등. 전문: Flutter SDK의 bin/cache/pkg/sky_engine/LICENSE, "
                    "앱 안의 Kari.app/Contents/Frameworks/App.framework/Resources/flutter_assets/NOTICES.Z")
            files = []
        text = "\n\n".join(read(f) for f in files)
        version = graph[n]["version"]
        rows.append({"name": n, "version": "Flutter SDK" if version == "0.0.0" else version,
                     "license": guess(text) if files else ("BSD-3-Clause 외 여럿" if n == "sky_engine" else ""),
                     "url": f"https://pub.dev/packages/{n}" if "pub-cache" in str(folder) else "https://github.com/flutter/flutter",
                     "files": files, "note": note, "folder": folder})
    return rows


def others():
    rows = [{"name": "Pretendard", "version": "글꼴", "license": "OFL-1.1",
             "url": "https://github.com/orioncactus/pretendard",
             "files": [FONTS / "Pretendard-LICENSE.txt"], "note": "앱에 넣은 글꼴", "folder": None}]
    reg = Path.home() / ".cargo/registry/src"
    skia = sorted(reg.glob("*/skia-bindings-*/skia"))
    for name, lic, url, rel in SKIA_PARTS:
        files = [skia[-1] / r for r in rel if skia and (skia[-1] / r).is_file()]
        rows.append({"name": name, "version": "skia-bindings에 포함", "license": lic, "url": url,
                     "files": files, "note": "엔진 라이브러리에 링크됨", "folder": None})
    return rows


def pack_sources():
    found = {}

    def walk(o):
        if isinstance(o, dict):
            s = o.get("source")
            if isinstance(s, dict) and s.get("url"):
                found[s["url"], s.get("title", "")] = True
            for v in o.values():
                walk(v)
        elif isinstance(o, list):
            for v in o:
                walk(v)

    for f in sorted((ENGINE / "interview/pack").rglob("*.json")):
        walk(json.loads(f.read_text()))
    return sorted(found, key=lambda k: (k[1].lower(), k[0]))


def home(p):
    """공개 파일에 사용자 이름이 든 절대 경로가 남지 않게 한다."""
    return str(p).replace(str(Path.home()), "~")


def cell(s):
    return str(s).replace("|", "\\|").replace("\n", " ")


def build():
    groups = [("Rust 크레이트(엔진 라이브러리 kari)", rust()), ("Dart 패키지(앱)", dart()),
              ("글꼴, Skia와 그 안의 라이브러리", others())]
    every = [r for _, rows in groups for r in rows]
    for r in every:
        if r.get("why"):  # 앞에서 이유를 정한 것(MPL 부분을 고지로 채운 것)
            pass
        elif r["name"] == "sky_engine":
            r["why"] = ""
        elif not spdx_ok(r["license"]):
            r["why"] = "라이선스 표현"
        else:
            # 표현은 허용형이어도 고지 글(NOTICE 등)이 카피레프트 코드를 품었다고 밝히는 경우
            found = sorted({m.group(0) for f in r["files"] for m in COPYLEFT.finditer(read(f))})
            r["why"] = "고지 글에 " + ", ".join(found) + " 언급" if found else ""
    trouble = [r for r in every if r["why"]]
    missing = [r for r in every if not r["files"] and r["name"] != "sky_engine"]

    out = ["# 제3자 라이선스 고지", "",
           "Kari는 MIT 라이선스로 배포됩니다. 앱에는 아래의 제3자 코드와 글꼴이 들어 있습니다.",
           f"이 파일은 `{SELF}`가 만듭니다. 손으로 고치지 마세요.", "",
           "## 확인이 필요한 것", "",
           "MIT와 함께 배포하기 어렵거나(GPL, AGPL, LGPL, MPL, SSPL 등) 라이선스를 알 수 없는 것:", ""]
    if trouble:
        out += ["| 이름 | 판 | 라이선스 | 걸린 이유 | 출처 |", "|---|---|---|---|---|"]
        out += [f"| {cell(r['name'])} | {cell(r['version'])} | {cell(r['license'] or '알 수 없음')} | {cell(r['why'])} | {r['url']} |"
                for r in trouble]
    else:
        out.append("- 없음")
    out += ["", "라이선스 글을 찾지 못한 것(아래 전문에 없음):", ""]
    out += [f"- {r['name']} {r['version']} ({r['license'] or '알 수 없음'}). 찾은 곳: `{home(r['folder'])}`"
            for r in missing] or ["- 없음"]

    kinds = Counter(r["license"] or "알 수 없음" for r in every)
    out += ["", "## 라이선스 종류별 개수", "", "| 라이선스 표현 | 개수 |", "|---|---|"]
    out += [f"| {cell(k)} | {v} |" for k, v in sorted(kinds.items(), key=lambda kv: (-kv[1], kv[0]))]

    texts, order = {}, []  # 같은 글은 한 번만 싣는다
    for title, rows in groups:
        out += ["", f"## {title}", "", "| 이름 | 판 | 라이선스 | 출처 | 비고 | 전문 |", "|---|---|---|---|---|---|"]
        for r in rows:
            refs = []
            for f in r["files"]:
                body = read(f)
                h = hashlib.sha256(" ".join(body.split()).encode()).hexdigest()[:10]  # 줄바꿈만 다른 글은 같게
                if h not in texts:
                    texts[h] = (body, [])
                    order.append(h)
                texts[h][1].append(f"{r['name']} {r['version']} ({f.name})")
                refs.append(f"[{h}](#t-{h})")
            out.append(f"| {cell(r['name'])} | {cell(r['version'])} | {cell(r['license'] or '알 수 없음')} | "
                       f"{r['url']} | {cell(r['note'])} | {' '.join(refs)} |")

    out += ["", "## 면접 지식 팩의 출처", "",
            f"면접 지식 팩(`{(ENGINE / 'interview/pack').relative_to(ROOT)}/`)의 개념 요약, 체크포인트, 오개념은 이 프로젝트가 한국어로 새로 쓴 글입니다.",
            "출처의 문장을 옮겨 싣지 않고, 체크포인트와 오개념마다 출처(제목, 주소, 절)를 붙여 두며 리포트에도 함께 보여 줍니다.",
            f"팩이 가리키는 출처는 {len(pack_sources())}곳입니다.", ""]
    out += [f"- {cell(t)}: {u}" for u, t in pack_sources()]

    out += ["", "## 라이선스 전문"]
    for h in order:
        body, users = texts[h]
        out += ["", f'<a id="t-{h}"></a>', "", f"### {h}", "", "쓰는 곳: " + ", ".join(users), "", "```text",
                body.replace("```", "'''"), "```"]
    return "\n".join(out) + "\n", trouble, missing, kinds


def main():
    ap = argparse.ArgumentParser(description=(__doc__ or "").strip().split("\n")[0] or None)  # -OO면 __doc__가 None
    ap.add_argument("--check", action="store_true", help="파일이 최신인지만 본다")
    args = ap.parse_args()
    text, trouble, missing, kinds = build()
    if args.check:
        old = OUT.read_text() if OUT.is_file() else ""
        if old == text:
            print("최신")
            return 0
        print(f"다시 만들어야 함: {SELF}")
        # 무엇이 다른지 앞의 몇 줄을 보여 준다(판이 바뀐 의존성, 찾지 못한 라이선스 글).
        diff = difflib.unified_diff(old.splitlines(), text.splitlines(), "지금 파일", "다시 만든 것", lineterm="", n=0)
        print(*[line[:200] for _, line in zip(range(60), diff)], sep="\n")
        return 1
    OUT.write_text(text)
    print(f"{OUT.relative_to(ROOT)}: 항목 {sum(kinds.values())}개")
    for k, v in sorted(kinds.items(), key=lambda kv: -kv[1]):
        print(f"  {v:4d}  {k}")
    print(f"확인이 필요한 것: {len(trouble)}개", *[f"  {r['name']} {r['version']}: {r['license'] or '알 수 없음'}"
                                                for r in trouble], sep="\n")
    print(f"라이선스 글을 못 찾은 것: {len(missing)}개", *[f"  {r['name']} {r['version']}" for r in missing], sep="\n")
    return 0


if __name__ == "__main__":
    assert spdx_ok("MIT OR Apache-2.0") and spdx_ok("(MIT OR Apache-2.0) AND Unicode-3.0")
    assert spdx_ok("MIT/Apache-2.0") and not spdx_ok("GPL-3.0") and not spdx_ok("MPL-2.0 AND MIT")
    assert spdx_ok("LGPL-2.1 OR MIT") and not spdx_ok("") and spdx_ok("Apache-2.0 WITH LLVM-exception OR MIT")
    assert not spdx_ok("(MIT OR Apache-2.0) AND GPL-2.0") and spdx_ok("(GPL-2.0 OR MIT) AND (Zlib)")
    sys.exit(main())
