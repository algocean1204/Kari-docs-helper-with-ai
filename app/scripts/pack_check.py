#!/usr/bin/env python3
"""면접 지식 팩 검사. 표준 라이브러리만 쓴다.

    python3 app/scripts/pack_check.py                      # app/engine/interview/pack/backend 검사
    python3 app/scripts/pack_check.py <팩 폴더> --partial  # 작성 중: 없는 영역 파일, 개념 목록 밖의 개념을 문제 삼지 않는다
    python3 app/scripts/pack_check.py --table              # 영역·개념 표(마크다운)를 출력한다
    python3 app/scripts/pack_check.py --doc Docs/architecture/knowledge-pack.md   # 문서의 표가 팩과 같은지 본다

규칙은 엔진의 검사기(app/engine/interview/src/pack.rs의 `check`)와 같다. 개념 목록과 핵심 여부는 그 파일의 `EXPECT`에서 읽는다.
"""
import json
import re
import sys
from pathlib import Path

ENGINE = Path(__file__).resolve().parent.parent / "engine"  # 스크립트 폴더의 옆
PACK = ENGINE / "interview/pack/backend"
KIND_FACES = {
    "mechanism": ["principle", "limit", "alternative"],
    "decision": ["rationale", "tradeoff", "alternative"],
    "operation": ["observe", "recover", "prevent"],
}
KIND_KO = {"mechanism": "메커니즘형", "decision": "결정형", "operation": "운영형"}
COMMON = ["estimate", "verify"]
ID = re.compile(r"^[a-z0-9]+(\.[a-z0-9]+)+$")
START, END = "<!-- pack-table:start -->", "<!-- pack-table:end -->"


def expected():
    """엔진 검사기의 개념 목록: {id: 핵심 여부}, 순서대로."""
    src = (ENGINE / "interview/src/pack.rs").read_text()
    block = src[src.index("const EXPECT"):]
    block = block[:block.index("];")]
    return {m[1]: m[2] == "true" for m in re.finditer(r'\("([a-z0-9.]+)",\s*(true|false)\)', block)}


def load(root):
    pack = json.loads((root / "pack.json").read_text())
    files = {a["id"]: json.loads((root / f"{a['id']}.json").read_text())
             for a in pack.get("areas", []) if (root / f"{a['id']}.json").exists()}
    return pack, files


def check(pack, files, expect, partial=False):
    errs = []
    e = errs.append

    def src(where, s):
        if not isinstance(s, dict):
            return e(f"{where}: source 없음")
        for k in ("title", "url", "where"):
            if not isinstance(s.get(k), str) or not s[k].strip():
                e(f"{where}: source.{k} 없음")
        if isinstance(s.get("url"), str) and not s["url"].startswith("https://"):
            e(f"{where}: source.url이 https가 아님")

    for k in ("id", "version", "lang", "role", "areas"):
        if k not in pack:
            e(f"pack.json: {k} 없음")
    concepts, ids = {}, set()
    for a in pack.get("areas", []):
        if a["id"] not in files:
            if not partial:
                e(f"{a['id']}.json 없음")
            continue
        d = files[a["id"]]
        if d.get("area") != a["id"]:
            e(f"{a['id']}.json: area가 {a['id']}가 아님")
        for c in d.get("concepts", []):
            cid = c.get("id", "?")
            if not ID.match(cid) or not cid.startswith(a["id"] + "."):
                e(f"{cid}: id 모양")
            if cid in concepts:
                e(f"{cid}: 겹침")
            concepts[cid] = c
            for k in ("name", "summary"):
                if not isinstance(c.get(k), str) or not c[k].strip():
                    e(f"{cid}: {k} 없음")
            kind = c.get("kind")
            if kind not in KIND_FACES:
                e(f"{cid}: kind")
                continue
            if not isinstance(c.get("core"), bool):
                e(f"{cid}: core")
            al = c.get("aliases", [])
            if not 2 <= len(al) <= 8 or not all(isinstance(x, str) and x.strip() for x in al):
                e(f"{cid}: aliases 2~8개")
            faces = c.get("faces", {})
            # 결정형과 운영형은 principle(어떻게 동작하는가)을 선택 면으로 가질 수 있다.
            allowed = KIND_FACES[kind] + COMMON + ([] if kind == "mechanism" else ["principle"])
            for f in faces:
                if f not in allowed:
                    e(f"{cid}: 허용하지 않는 면 {f}")
            if kind != "mechanism" and "principle" in faces and not 2 <= len(faces["principle"]) <= 5:
                e(f"{cid}.principle: 2~5개")
            for f in KIND_FACES[kind]:
                n = len(faces.get(f, []))
                if not 2 <= n <= 5:
                    e(f"{cid}.{f}: 체크포인트 {n}개(2~5)")
            for f in COMMON:
                if f in faces and not 1 <= len(faces[f]) <= 3:
                    e(f"{cid}.{f}: 1~3개")
            # 필수 체크포인트(must): 유형의 세 면과 선택 면 principle에는 2~3개, 공통 면에는 두지 않는다.
            for f, cps in faces.items():
                n_must = sum(1 for cp in cps if cp.get("must") is True)
                if any(not isinstance(cp.get("must"), bool) for cp in cps):
                    e(f"{cid}.{f}: must(참/거짓)가 없는 체크포인트")
                if f in COMMON:
                    if n_must:
                        e(f"{cid}.{f}: 공통 면에는 must를 두지 않음")
                elif not 2 <= n_must <= 3:
                    e(f"{cid}.{f}: 필수(must) {n_must}개(2~3)")
            for f, cps in faces.items():
                for i, cp in enumerate(cps, 1):
                    pid = cp.get("id", "?")
                    if pid != f"{cid}.{f}.{i}":
                        e(f"{pid}: id는 {cid}.{f}.{i}여야 함")
                    if pid in ids:
                        e(f"{pid}: 겹침")
                    ids.add(pid)
                    t = cp.get("text", "")
                    if not isinstance(t, str) or len(t.strip()) < 10:
                        e(f"{pid}: text")
                    src(pid, cp.get("source"))
            ms = c.get("misconceptions", [])
            if not 1 <= len(ms) <= 3:
                e(f"{cid}: misconceptions 1~3개")
            for i, m in enumerate(ms, 1):
                mid = m.get("id", "?")
                if mid != f"{cid}.m{i}":
                    e(f"{mid}: id는 {cid}.m{i}여야 함")
                if mid in ids:
                    e(f"{mid}: 겹침")
                ids.add(mid)
                for k in ("claim", "fix", "probe"):
                    if not isinstance(m.get(k), str) or not m[k].strip():
                        e(f"{mid}: {k} 없음")
                src(mid, m.get("source"))
            ss = c.get("scenarios", [])
            if not 1 <= len(ss) <= 3:
                e(f"{cid}: scenarios 1~3개")
            for i, s in enumerate(ss, 1):
                sid = s.get("id", "?")
                if sid != f"{cid}.s{i}":
                    e(f"{sid}: id는 {cid}.s{i}여야 함")
                if not isinstance(s.get("text"), str) or not s["text"].strip():
                    e(f"{sid}: text")
                fs = s.get("faces", [])
                if not fs or any(x not in faces for x in fs):
                    e(f"{sid}: faces가 그 개념의 면이 아님")
            ts = c.get("tradeoffs", [])
            if len(ts) > 2:
                e(f"{cid}: tradeoffs 0~2개")
            for t in ts:
                for k in ("axis", "a", "b"):
                    if not isinstance(t.get(k), str) or not t[k].strip():
                        e(f"{cid}: tradeoffs.{k}")
    for cid, c in concepts.items():
        for k in ("prereq", "alt"):
            for r in c.get(k, []):
                if r == cid:
                    e(f"{cid}: {k}가 자신을 가리킴")
                elif r not in concepts and not (partial and r in expect):
                    e(f"{cid}: {k}의 {r}가 팩에 없음")
        if cid in expect and c.get("core") != expect[cid]:
            e(f"{cid}: core는 {expect[cid]}여야 함(pack.rs의 EXPECT)")
        if cid not in expect and not partial:
            e(f"{cid}: pack.rs의 EXPECT에 없음")
    if not partial:
        for cid in expect:
            if cid not in concepts:
                e(f"{cid}: 없음")
    return concepts, errs


def table(pack, files):
    """문서에 싣는 영역·개념 표. 문서의 START와 END 사이가 이것과 같아야 한다."""
    areas = [(a, files.get(a["id"], {}).get("concepts", [])) for a in pack["areas"]]
    cs = [c for _, l in areas for c in l]
    cps = [cp for c in cs for v in c["faces"].values() for cp in v]
    out = [START, "",
           f"개념 {len(cs)}개(핵심 {sum(c['core'] for c in cs)}개), 체크포인트 {len(cps)}개"
           f"(필수 {sum(cp['must'] for cp in cps)}개), 오개념 {sum(len(c['misconceptions']) for c in cs)}개, "
           f"시나리오 {sum(len(c['scenarios']) for c in cs)}개, "
           f"트레이드오프 {sum(len(c.get('tradeoffs', [])) for c in cs)}개.", "",
           "| 영역 id | 이름 | 개념 수 |", "|---|---|---|"]
    out += [f"| `{a['id']}` | {a['name']} | {len(l)} |" for a, l in areas]
    out += ["", "| id | 이름 | 유형 | 핵심 |", "|---|---|---|---|"]
    out += [f"| `{c['id']}` | {c['name']} | {KIND_KO[c['kind']]} | {'○' if c['core'] else ''} |" for c in cs]
    return "\n".join(out + ["", END])


def main(argv):
    doc = argv[argv.index("--doc") + 1] if "--doc" in argv else None
    args = [a for a in argv if not a.startswith("--") and a != doc]
    pack, files = load(Path(args[0]) if args else PACK)
    if "--table" in argv:
        print(table(pack, files))
        return 0
    if doc:
        doc = Path(doc).read_text()
        got = doc[doc.index(START):doc.index(END) + len(END)] if START in doc and END in doc else ""
        same = got == table(pack, files)
        print("문서의 표가 팩과 같음" if same else "문서의 표가 팩과 다름: --table의 출력으로 바꾸세요")
        return 0 if same else 1
    concepts, errs = check(pack, files, expected(), "--partial" in argv)
    cps = sum(len(v) for c in concepts.values() for v in c.get("faces", {}).values())
    print(f"개념 {len(concepts)}개, 핵심 {sum(1 for c in concepts.values() if c.get('core'))}개, 체크포인트 {cps}개")
    for m in errs:
        print("오류:", m)
    print("통과" if not errs else f"실패 {len(errs)}건")
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
