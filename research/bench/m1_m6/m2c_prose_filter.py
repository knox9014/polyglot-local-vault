#!/usr/bin/env python3
"""
M2c — 제안 정밀도: 산문 빈도 필터

M2b의 지역성 지표는 실패했다. 샘플을 보면 이유가 명확하다.

  django  docs/faq/admin.txt  `ModelAdmin` → django/contrib/admin/options.py   ← 정답인데 "교차"
  hugo    docs/.../postcss.md `PostCSS`    → tpl/css/css.go                    ← 정답인데 "교차"
  cpython Misc/NEWS.d/...     `get_source_segment` → Lib/ast.py                ← 정답인데 "교차"

표준 레이아웃은 docs/ 와 src/ 를 분리한다. 즉 **제품이 노리는 문서↔코드 관계는 본질적으로
트리를 가로지른다.** 지역성으로 거르면 가장 가치 있는 제안을 정확히 버린다.

진짜 오탐은 다른 종류다.
  rust  RELEASES.md `warnings`      → clippy/lintcheck/recursive.rs
  node  .../LanguageSpecification.md `Global` → typings/globals.d.ts
  k8s   .../README.md `Local`       → cli-runtime/.../builder.go

공통점: **흔한 영단어**다. 그리고 이건 vault 자체에서 결정론적으로 판정할 수 있다.

  아이디어: 그 토큰이 문서 산문(백틱 밖 일반 텍스트)에 자주 등장하면 영단어다.
           `ModelAdmin`, `fetch_covtype`, `GetRemote` 는 산문에 안 나온다.
           `warnings`, `Local`, `Global`, `Configuration` 은 산문에 흔하다.

외부 사전이 필요 없고, vault 안에서 자족적으로 계산된다.
"""

import os, re, sys, json, random
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
VENDOR = {"vendor", "third_party", "third-party", "deps", "node_modules", "external", "3rdparty"}
SKIP = {".git", "node_modules", "__pycache__", "target", "dist", "build", ".venv", "venv"}
DOC_EXTS = (".md", ".rst", ".txt")
FENCE = re.compile(r"```.*?```", re.S)
BT = re.compile(r"`([^`\n]{2,60})`")
IDENT_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")

PROSE_MAX = 3   # 산문에 이 횟수를 초과해 등장하면 영단어로 간주


def ident_like(t):
    if not IDENT_RE.match(t) or len(t) < 4: return False
    if "_" in t: return True
    if re.search(r"[a-z][A-Z]", t): return True
    if t[0].isupper() and len(t) >= 5: return True
    return len(t) >= 8


def walk(root, exts):
    out, stack = [], [root]
    while stack:
        d = stack.pop()
        try:
            with os.scandir(d) as it:
                for e in it:
                    try:
                        if e.is_dir(follow_symlinks=False):
                            if e.name in SKIP or e.name in VENDOR: continue
                            stack.append(e.path)
                        elif e.is_file(follow_symlinks=False) and e.name.endswith(exts):
                            out.append(e.path)
                    except OSError: continue
        except OSError: continue
    return out


def symbol_index(root):
    idx = defaultdict(list)
    for f in walk(root, tuple(P.keys())):
        parser, defs = P[os.path.splitext(f)[1]]
        try: src = open(f, "rb").read()
        except OSError: continue
        if len(src) > 800_000: continue
        try: node = parser.parse(src).root_node
        except Exception: continue
        rel = os.path.relpath(f, root)
        st = [node]
        while st:
            n = st.pop()
            if n.type in defs:
                nn = n.child_by_field_name("name")
                if nn is None:
                    for ch in n.named_children:
                        if ch.type == "type_spec":
                            nn = ch.child_by_field_name("name"); break
                if nn is not None:
                    idx[src[nn.start_byte:nn.end_byte].decode("utf8","replace")].append(rel)
            st.extend(n.named_children)
    return idx


def analyze(root, name):
    symidx = symbol_index(root)
    docs = walk(root, DOC_EXTS)
    prose_freq = Counter()
    raw = []
    seen = set()
    for f in docs:
        try: txt = open(f, encoding="utf8", errors="replace").read(500_000)
        except OSError: continue
        body = FENCE.sub(" ", txt)
        rel = os.path.relpath(f, root)
        # 백틱 밖 산문 수집
        prose = BT.sub(" ", body)
        for w in re.findall(r"[A-Za-z_][A-Za-z0-9_]*", prose):
            prose_freq[w] += 1
        for m in BT.finditer(body):
            tok = m.group(1).strip().split("(")[0].strip()
            if "." in tok: tok = tok.split(".")[-1]
            if not ident_like(tok): continue
            t = symidx.get(tok)
            if not t or len(t) != 1: continue
            k = (rel, tok)
            if k in seen: continue
            seen.add(k)
            raw.append((rel, tok, t[0]))
    kept = [x for x in raw if prose_freq.get(x[1], 0) <= PROSE_MAX]
    dropped = [x for x in raw if prose_freq.get(x[1], 0) > PROSE_MAX]
    return dict(name=name, symbols=len(symidx), docs=len(docs),
                raw=len(raw), kept=len(kept), dropped=len(dropped),
                kept_s=kept, dropped_s=dropped, prose=prose_freq)


TARGETS = [("/tmp/corpus/kubernetes","kubernetes"), ("/tmp/corpus/TypeScript","TypeScript"),
           ("/tmp/corpus/node","node"), ("/tmp/corpus/rust","rust"),
           ("/tmp/corpus/cpython","cpython"), ("/tmp/rn/django","django"),
           ("/tmp/rn/scikit-learn","scikit-learn"), ("/tmp/lang/hugo","hugo")]

print("### M2c — 산문 빈도 필터 (외부 사전 없이 vault 자체로 판정)\n")
print(f"필터: 백틱 토큰이 문서 산문에 {PROSE_MAX}회 초과 등장하면 영단어로 보고 제외\n")
print(f"{'repo':<14}{'심볼':>9}{'문서':>8}{'필터 전':>9}{'필터 후':>9}{'제거':>8}{'제거율':>9}")
res = []
random.seed(11)
for root, name in TARGETS:
    if not os.path.isdir(root): continue
    r = analyze(root, name)
    print(f"{name:<14}{r['symbols']:>9,}{r['docs']:>8,}{r['raw']:>9,}{r['kept']:>9,}"
          f"{r['dropped']:>8,}{(r['dropped']/r['raw']*100 if r['raw'] else 0):>8.1f}%")
    sys.stdout.flush()
    res.append(r)

print("\n### 필터가 제거한 것 (오탐이어야 함)")
for r in res:
    if not r["dropped_s"]: continue
    print(f"\n── {r['name']}")
    for rel, tok, tgt in r["dropped_s"][:4]:
        print(f"   ✗ `{tok}` (산문 {r['prose'].get(tok,0)}회)  {rel} → {tgt}")

print("\n### 필터가 남긴 것 (무작위 표본 — 정탐이어야 함)")
sample_for_review = []
for r in res:
    if not r["kept_s"]: continue
    print(f"\n── {r['name']}  (총 {r['kept']:,}건 중 무작위 4건)")
    pick = random.sample(r["kept_s"], min(4, len(r["kept_s"])))
    for rel, tok, tgt in pick:
        print(f"   • `{tok}`  {rel} → {tgt}")
        sample_for_review.append((r["name"], rel, tok, tgt))

json.dump([{k: v for k, v in r.items() if k not in ("prose", "kept_s", "dropped_s")} for r in res],
          open("/tmp/rn/m2c_results.json", "w"), indent=1)
json.dump(sample_for_review, open("/tmp/rn/m2c_sample.json", "w"), indent=1)

tot_raw = sum(r["raw"] for r in res); tot_kept = sum(r["kept"] for r in res)
print(f"\n### 종합")
print(f"  필터 전 제안 : {tot_raw:,}")
print(f"  필터 후 제안 : {tot_kept:,}  ({tot_kept/tot_raw*100:.1f}%)")
print(f"  저장소당 평균: {tot_kept/len(res):,.0f}건")
