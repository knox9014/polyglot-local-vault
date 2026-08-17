#!/usr/bin/env python3
"""문서 상호 참조 무결성 검사 — 파일 참조와 섹션 제목 참조를 실재 여부로 대조한다."""
import re, sys, glob, os

D = sys.argv[1] if len(sys.argv) > 1 else "."
files = {os.path.basename(f): f for f in glob.glob(os.path.join(D, "*.md"))}
num2name = {}
for n in files:
    m = re.match(r"(\d\d)_", n)
    if m: num2name[m.group(1)] = n

heads, text = {}, {}
for n, p in files.items():
    t = open(p, encoding="utf8").read()
    text[n] = t
    heads[n] = {h.strip() for h in re.findall(r"^#{2,4}\s+(.+?)\s*$", t, re.M)}

FILEREF = re.compile(r"`(\d\d_[A-Z0-9_]+\.md)`")
# `NN_FILE.md` "섹션"  또는  `NN` "섹션"
SECREF = re.compile(r"`(\d\d)(?:_[A-Z0-9_]+\.md)?`\s*(?:§\S+\s*)?[\"“]([^\"”\n]{2,60})[\"”]")

bad_file, bad_sec, ok = [], [], 0
for n in sorted(files):
    for i, line in enumerate(text[n].split("\n"), 1):
        for ref in FILEREF.findall(line):
            ok += 1
            if ref not in files: bad_file.append((n, i, ref))
        for num, sec in SECREF.findall(line):
            tgt = num2name.get(num)
            if tgt is None:
                bad_sec.append((n, i, num, sec, "대상 문서 없음")); continue
            ok += 1
            if sec in heads[tgt]: continue
            # 부분 일치 허용 (제목 안에 인용구가 들어간 경우)
            if any(sec in h for h in heads[tgt]): continue
            bad_sec.append((n, i, num, sec, f"→ {tgt} 에 해당 제목 없음"))

print(f"검사한 참조 {ok}건\n")
print(f"── 존재하지 않는 파일 참조: {len(bad_file)}건")
for n, i, r in bad_file: print(f"   {n}:{i}  `{r}`")
print(f"\n── 존재하지 않는 섹션 참조: {len(bad_sec)}건")
for n, i, num, sec, why in bad_sec: print(f"   {n}:{i}  `{num}` \"{sec}\"  {why}")
sys.exit(1 if bad_file or bad_sec else 0)
