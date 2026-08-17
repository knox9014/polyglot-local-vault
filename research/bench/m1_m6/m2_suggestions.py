#!/usr/bin/env python3
"""
M2 — 제안 엔진 실효성 (리뷰 §3 콜드스타트 검증)

리뷰 §3에서 제기한 문제:
  "manual link가 중심" + "AI 자동 수정 금지" → vault를 처음 열면 semantic web이 텅 빈다.
  차별점이라고 선언한 기능이 Day 1에 가치가 0이다.

그리고 제안한 해법:
  AI가 아니라 결정론적 휴리스틱으로 후보를 만들어 1클릭 승인을 받는다.
  "첫 사용 시 '48개의 관계 후보를 찾았습니다' 화면을 보여줄 수 있으면 콜드스타트가 사라진다."

이 실험은 그 48이 실제로 몇인지 잰다.

규칙 (전부 결정론적, AI 없음):
  R1  Markdown 백틱 토큰 ↔ 심볼 유일 매칭     doc → code (describes)
  R2  설정 파일 값 ↔ 실존 경로                config → file (references)
  R3  docstring 내 다른 심볼명 유일 매칭      code → code (mentions)
  R4  git 동시 변경 (동일 커밋 N회 이상)      file ↔ file (co_changed)
"""

import os, re, sys, json, subprocess, io, tarfile
from collections import defaultdict, Counter
from tree_sitter import Language, Parser
import tree_sitter_python as tspy
import tree_sitter_go as tsgo
import tree_sitter_typescript as tsts
import tree_sitter_rust as tsrs

P = {
    ".py": (Parser(Language(tspy.language())), ("class_definition", "function_definition")),
    ".go": (Parser(Language(tsgo.language())), ("function_declaration", "method_declaration", "type_declaration")),
    ".ts": (Parser(Language(tsts.language_typescript())),
            ("function_declaration", "class_declaration", "interface_declaration", "type_alias_declaration")),
    ".rs": (Parser(Language(tsrs.language())), ("function_item", "struct_item", "enum_item", "trait_item")),
}

# 식별자스러움 판정: 흔한 영단어를 걸러내기 위함
IDENT_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
def looks_like_identifier(tok: str) -> bool:
    if not IDENT_RE.match(tok): return False
    if len(tok) < 4: return False
    if "_" in tok: return True                       # snake_case
    if re.search(r"[a-z][A-Z]", tok): return True    # camelCase / PascalCase
    if tok[0].isupper() and len(tok) >= 5: return True
    return len(tok) >= 8                             # 긴 소문자 단어는 통과


def walk_files(root, exts, limit=None):
    out = []
    stack = [root]
    skip = {".git", "node_modules", ".venv", "__pycache__", "target", "dist", "build"}
    while stack:
        d = stack.pop()
        try:
            with os.scandir(d) as it:
                for e in it:
                    try:
                        if e.is_dir(follow_symlinks=False):
                            if e.name not in skip: stack.append(e.path)
                        elif e.is_file(follow_symlinks=False):
                            if any(e.name.endswith(x) for x in exts):
                                out.append(e.path)
                                if limit and len(out) >= limit: return out
                    except OSError: continue
        except OSError: continue
    return out


def build_symbol_index(root):
    """심볼 이름 -> [(파일경로, 이름)] . 유일성 판정을 위해 개수도 센다."""
    idx = defaultdict(list)
    exts = tuple(P.keys())
    files = walk_files(root, exts)
    parsed = 0
    for f in files:
        ext = os.path.splitext(f)[1]
        parser, defs = P[ext]
        try:
            src = open(f, "rb").read()
        except OSError: continue
        if len(src) > 800_000: continue
        try: root_node = parser.parse(src).root_node
        except Exception: continue
        parsed += 1
        stack = [root_node]
        while stack:
            n = stack.pop()
            if n.type in defs:
                nn = n.child_by_field_name("name")
                if nn is None:
                    for ch in n.named_children:
                        if ch.type in ("type_spec",):
                            nn = ch.child_by_field_name("name"); break
                if nn is not None:
                    nm = src[nn.start_byte:nn.end_byte].decode("utf8", "replace")
                    idx[nm].append(os.path.relpath(f, root))
            stack.extend(n.named_children)
    return idx, parsed, len(files)


BACKTICK = re.compile(r"`([^`\n]{2,60})`")
CODEFENCE = re.compile(r"```.*?```", re.S)

def rule1_markdown(root, symidx):
    """R1: .md 백틱 토큰 ↔ 유일 심볼"""
    mds = walk_files(root, (".md",))
    hits, raw_hits, samples = 0, 0, []
    seen = set()
    for f in mds:
        try: txt = open(f, encoding="utf8", errors="replace").read()
        except OSError: continue
        txt = CODEFENCE.sub(" ", txt)            # 코드블록 전체는 제외
        rel = os.path.relpath(f, root)
        for m in BACKTICK.finditer(txt):
            tok = m.group(1).strip()
            tok = tok.split("(")[0].strip()      # foo() → foo
            if "." in tok: tok = tok.split(".")[-1]
            if tok not in symidx: continue
            raw_hits += 1
            if not looks_like_identifier(tok): continue
            tgt = symidx[tok]
            if len(tgt) != 1: continue           # 유일한 것만
            key = (rel, tok)
            if key in seen: continue
            seen.add(key); hits += 1
            if len(samples) < 4: samples.append((rel, tok, tgt[0]))
    return dict(files=len(mds), raw=raw_hits, suggestions=hits, samples=samples)


PATHLIKE = re.compile(r'"([A-Za-z0-9_./\-]{4,120}\.[A-Za-z0-9]{1,6})"')

def rule2_configs(root):
    """R2: json/yaml/toml 값이 vault 안의 실존 경로"""
    cfgs = walk_files(root, (".json", ".yaml", ".yml", ".toml"))
    hits, samples = 0, []
    seen = set()
    for f in cfgs:
        try:
            txt = open(f, encoding="utf8", errors="replace").read(400_000)
        except OSError: continue
        base = os.path.dirname(f)
        rel = os.path.relpath(f, root)
        for m in PATHLIKE.finditer(txt):
            v = m.group(1)
            if v.startswith("http") or " " in v: continue
            for cand in (os.path.join(base, v), os.path.join(root, v)):
                cand = os.path.normpath(cand)
                if cand.startswith(root) and os.path.isfile(cand):
                    key = (rel, os.path.relpath(cand, root))
                    if key in seen: break
                    seen.add(key); hits += 1
                    if len(samples) < 4: samples.append(key)
                    break
    return dict(files=len(cfgs), suggestions=hits, samples=samples)


DOCSTR = re.compile(rb'"""(.*?)"""', re.S)

def rule3_docstrings(root, symidx):
    """R3: python docstring 안에 등장하는 다른 심볼명(유일)"""
    pys = walk_files(root, (".py",))
    hits, samples = 0, []
    seen = set()
    for f in pys:
        try: src = open(f, "rb").read()
        except OSError: continue
        if len(src) > 500_000: continue
        rel = os.path.relpath(f, root)
        for m in DOCSTR.finditer(src):
            body = m.group(1).decode("utf8", "replace")
            for tok in re.findall(r"[A-Za-z_][A-Za-z0-9_]*", body):
                if not looks_like_identifier(tok): continue
                t = symidx.get(tok)
                if not t or len(t) != 1: continue
                if t[0] == rel: continue          # 자기 파일은 제외
                key = (rel, tok)
                if key in seen: continue
                seen.add(key); hits += 1
                if len(samples) < 4: samples.append((rel, tok, t[0]))
    return dict(files=len(pys), suggestions=hits, samples=samples)


def rule4_cochange(repo, min_count=5, max_files_per_commit=12, max_commits=8000):
    """R4: 같은 커밋에서 min_count회 이상 함께 바뀐 파일 쌍"""
    out = subprocess.run(["git", "-C", repo, "log", f"-{max_commits}", "--name-only",
                          "--format=%x00", "--no-merges"],
                         capture_output=True, text=True, errors="replace", timeout=1200).stdout
    pairs = Counter()
    cur = []
    for line in out.splitlines():
        if line.startswith("\x00"):
            if 2 <= len(cur) <= max_files_per_commit:
                s = sorted(set(cur))
                for i in range(len(s)):
                    for j in range(i + 1, len(s)):
                        pairs[(s[i], s[j])] += 1
            cur = []
        elif line:
            cur.append(line)
    strong = {k: v for k, v in pairs.items() if v >= min_count}
    dist = Counter(v for v in strong.values())
    return dict(pairs_total=len(pairs), suggestions=len(strong),
                samples=sorted(strong.items(), key=lambda x: -x[1])[:4],
                dist=dict(sorted(dist.items())[:6]))


TARGETS = [
    ("/tmp/corpus/kubernetes", "kubernetes", None),
    ("/tmp/corpus/rust", "rust", None),
    ("/tmp/corpus/node", "node", None),
    ("/tmp/corpus/TypeScript", "TypeScript", None),
    ("/tmp/corpus/cpython", "cpython", None),
    ("/tmp/rn/django", "django", "/tmp/rn/django"),
    ("/tmp/rn/flask", "flask", "/tmp/rn/flask"),
    ("/tmp/lang/hugo", "hugo", "/tmp/lang/hugo"),
]

if __name__ == "__main__":
    results = []
    print("### M2 — 제안 엔진이 만드는 관계 후보 수 (결정론적 규칙만, AI 없음)\n")
    print(f"{'repo':<14}{'심볼':>9}{'R1 md↔심볼':>12}{'R2 설정↔경로':>13}"
          f"{'R3 docstr':>11}{'R4 동시변경':>12}{'합계':>10}")
    for root, name, gitrepo in TARGETS:
        if not os.path.isdir(root): continue
        symidx, parsed, nfiles = build_symbol_index(root)
        r1 = rule1_markdown(root, symidx)
        r2 = rule2_configs(root)
        r3 = rule3_docstrings(root, symidx) if os.path.isdir(root) else dict(suggestions=0, samples=[])
        r4 = rule4_cochange(gitrepo) if gitrepo else dict(suggestions=0, samples=[], dist={})
        tot = r1["suggestions"] + r2["suggestions"] + r3["suggestions"] + r4["suggestions"]
        print(f"{name:<14}{len(symidx):>9,}{r1['suggestions']:>12,}{r2['suggestions']:>13,}"
              f"{r3['suggestions']:>11,}{r4['suggestions']:>12,}{tot:>10,}")
        sys.stdout.flush()
        results.append(dict(repo=name, symbols=len(symidx), code_files=nfiles,
                            r1=r1, r2=r2, r3=r3, r4=r4, total=tot))
    json.dump(results, open("/tmp/rn/m2_results.json", "w"), indent=1, default=str)

    print("\n### 샘플 (규칙이 실제로 무엇을 찾는지)")
    for r in results:
        shown = False
        for key, label in (("r1", "R1 md→심볼"), ("r2", "R2 설정→경로"),
                           ("r3", "R3 docstring→심볼"), ("r4", "R4 동시변경")):
            s = r[key].get("samples") or []
            if not s: continue
            if not shown:
                print(f"\n── {r['repo']}"); shown = True
            print(f"   [{label}]")
            for x in s[:2]:
                print(f"      {x}")

    print("\n### R1 필터 효과 (흔한 영단어 제거)")
    print(f"{'repo':<14}{'백틱 매칭 총계':>14}{'식별자+유일 통과':>18}{'통과율':>9}")
    for r in results:
        raw = r["r1"].get("raw", 0); s = r["r1"]["suggestions"]
        if raw:
            print(f"{r['repo']:<14}{raw:>14,}{s:>18,}{s/raw*100:>8.1f}%")
