#!/usr/bin/env python3
"""공개 저장소에 들어가면 안 되는 것이 추적되고 있지 않은지 본다. 표준 라이브러리만 쓴다.

    python3 app/scripts/public_check.py     # 추적되는 파일 전부를 보고, 걸리는 것이 있으면 1로 끝난다

보는 것
1. 맨 위에는 README.md, LICENSE, .gitignore, .github, app, Docs만 둔다.
2. 파일 이름: .env(.env.example 제외), 인증서와 키 파일, 로컬 DB, 개발 도구의 설정·지침 파일.
3. 글 안의 비밀 모양: API 키, 토큰, 개인 키 블록.
4. 개인 정보: 기기의 사용자 폴더 경로, 예약된 도메인(example.com 등)이 아닌 전자우편 주소.
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
TOP = {"README.md", "LICENSE", ".gitignore", ".github", "app", "Docs"}

# 경로로 거르는 것.
BAD_NAMES = [
    (r"(^|/)\.env($|\.(?!example$))", ".env 파일"),
    (r"\.(pem|key|p8|p12|pfx|cer|keystore|jks|mobileprovision|provisionprofile)$", "인증서나 키 파일"),
    (r"\.(sqlite3?|db|db-wal|db-shm)$", "로컬 DB"),
    (r"(^|/)\.(claude|codex|cursor|grok|agents|antigravity|gemini|windsurf)/", "개발 도구의 로컬 폴더"),
    (r"(?i)(^|/)(agents?|claude|claude\.local|gemini)\.md$", "개발 도구의 지침 파일"),
]

# 글 안에서 찾는 비밀 모양. 시험의 가짜 키는 이 길이에 못 미친다.
SECRETS = [
    (r"sk-ant-[A-Za-z0-9_-]{40,}", "Anthropic API 키 모양"),
    (r"sk-proj-[A-Za-z0-9_-]{40,}", "OpenAI API 키 모양"),
    (r"sk-[A-Za-z0-9]{40,}", "API 키 모양"),
    (r"AKIA[0-9A-Z]{16}", "AWS 키 모양"),
    (r"gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{40,}", "GitHub 토큰 모양"),
    (r"xox[baprs]-[A-Za-z0-9-]{20,}", "Slack 토큰 모양"),
    (r"AIza[0-9A-Za-z_-]{35}", "Google API 키 모양"),
    (r"-----BEGIN [A-Z ]*PRIVATE KEY-----", "개인 키 블록"),
]

# 시험 자료가 쓰는 꾸민 사용자 이름. 이 밖의 이름이 든 사용자 폴더 경로는 누군가의 기기 경로로 본다.
FAKE_USERS = {"a", "me", "you", "name", "user", "username", "runner", "Shared"}
USER_PATH = re.compile(r"(?:/Users/|/home/|[A-Za-z]:\\Users\\)([A-Za-z0-9._-]+)")

EMAIL = re.compile(r"[A-Za-z0-9._%+-]+@([A-Za-z0-9.-]+\.[A-Za-z]{2,})")
SAFE_DOMAIN = re.compile(r"(?i)(^|\.)example\.(com|org|net)$|\.(example|test|invalid)$")

# 남이 쓴 라이선스 글(저작자의 연락처가 들어 있다)은 글 검사에서 뺀다.
SKIP_TEXT = re.compile(r"^(app/scripts/licenses/|Docs/THIRD_PARTY_LICENSES\.md$|app/assets/fonts/.*LICENSE)")


def main() -> int:
    files = subprocess.run(
        ["git", "ls-files", "-z"], cwd=ROOT, capture_output=True, check=True
    ).stdout.decode().split("\0")
    files = [f for f in files if f]
    found: list[str] = []

    for top in sorted({f.split("/", 1)[0] for f in files} - TOP):
        found.append(f"{top}: 맨 위에 둘 수 없다(app/ 또는 Docs/ 아래로)")

    for f in files:
        for pat, why in BAD_NAMES:
            if re.search(pat, f):
                found.append(f"{f}: {why}")
        if SKIP_TEXT.search(f):
            continue
        p = ROOT / f
        if not p.is_file() or p.stat().st_size > 4_000_000:
            continue
        try:
            text = p.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue  # 그림과 글꼴
        for n, line in enumerate(text.splitlines(), 1):
            for pat, why in SECRETS:
                if re.search(pat, line):
                    found.append(f"{f}:{n}: {why}")
            for m in USER_PATH.finditer(line):
                if m.group(1) not in FAKE_USERS:
                    found.append(f"{f}:{n}: 기기의 사용자 폴더 경로")
            for m in EMAIL.finditer(line):
                if not SAFE_DOMAIN.search(m.group(1)):
                    found.append(f"{f}:{n}: 예약된 도메인이 아닌 전자우편 주소({m.group(1)})")

    for line in found:
        print(line)
    print(f"파일 {len(files)}개, 걸린 것 {len(found)}개")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
