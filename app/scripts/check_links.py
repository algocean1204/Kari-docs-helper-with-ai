#!/usr/bin/env python3
"""공개 글(README.md, Docs/, .github/)의 상대 링크가 실제 파일을 가리키는지 본다. 표준 라이브러리만 쓴다.

    python3 app/scripts/check_links.py     # 깨진 링크가 있으면 목록을 찍고 1로 끝난다
"""
import re
import subprocess
import sys
from pathlib import Path

LINK = re.compile(r"\]\(([^)\s#]+)(?:#[^)]*)?\)")


def broken(root):
    files = [root / "README.md", *sorted((root / "Docs").rglob("*.md")), *sorted((root / ".github").rglob("*.md"))]
    for f in files:
        if not f.is_file():
            continue
        text = re.sub(r"```.*?```", "", f.read_text(encoding="utf-8"), flags=re.S)  # 코드 블록 안은 글이다
        for target in LINK.findall(text):
            if re.match(r"^[a-z]+:", target):  # https:, mailto: 등 바깥 주소
                continue
            if not (f.parent / target).exists():
                yield f.relative_to(root), target


def main():
    root = Path(subprocess.run(["git", "rev-parse", "--show-toplevel"], cwd=Path(__file__).parent,
                               check=True, capture_output=True, text=True).stdout.strip())
    bad = list(broken(root))
    for f, target in bad:
        print(f"{f}: {target}")
    print(f"깨진 링크 {len(bad)}개")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
