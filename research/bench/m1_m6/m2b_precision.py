#!/usr/bin/env python3
"""
M2b — 제안 엔진 정밀도 보강

M2 1차 결과의 문제:
  개수는 충분했지만(438~4,567) 샘플을 보니 정밀도가 규칙·저장소별로 크게 달랐다.

  좋은 예: kubernetes  README.md `LoadBalancerSourceRanges` → pkg/proxy/serviceport.go
           TypeScript  formatting/README.md `formatSpan`    → services/formatting/formatting.ts
  나쁜 예: node        SECURITY.md `configure`  → deps/v8/third_party/.../conanfile.py
           node        docstring   `Module`     → deps/crates/vendor/diplomat_core/.../modules.rs

나쁜 예의 공통점은 **vendored / third-party 디렉터리**다.
실제 vault라면 ignore 규칙으로 제외될 곳인데, 1차 측정은 포함했다.

이 실험은 두 가지를 추가한다.
  (1) vendor/third-party 제외 후 재측정
  (2) 자동 정밀도 지표 — "지역성(locality)"
      문서와 대상 심볼이 같은 최상위 서브트리에 있으면 관계일 개연성이 높다.
      트리를 가로지르는 매칭은 흔한 영단어 오탐일 가능성이 높다.
  (3) 문서 형식 확장 — django 등은 .rst/.txt를 쓴다 (1차에서 R1=0이 나온 이유)
"""

import os, re, sys, json
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

VENDOR = {"vendor", "third_party", "third-party", "deps", "node_modules", "external",
          "externals", "3rdparty", "Godeps", "_vendor"}
SKIP = {".git", "node_modules", "__pycache__", "target", "dist", "build", ".venv", "venv"}

IDENT_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
def ident_like(t):
    if not IDENT_RE.match(t) or len(t) < 4: return False
    if "_" in t: return True
    if re.search(r"[a-z][A-Z]", t): return True
    if t[0].isupper() and len(t) >= 5: return True
    return len(t) >= 8


def walk(root, exts, exclude_vendor):
    out, stack = [], [root]
    while stack:
        d = stack.pop()
        try:
            with os.scandir(d) as it:
                for e in it:
                    try:
                        if e.is_dir(follow_symlinks=False):
                            if e.name in SKIP: continue
                            if exclude_vendor and e.name in VENDOR: continue
                            stack.append(e.path)
                        elif e.is_file(follow_symlinks=False) and e.name.endswith(exts):
                            out.append(e.path)
                    except OSError: continue
        except OSError: continue
    return out


def symbol_index(root, exclude_vendor):
    idx = defaultdict(list)
    for f in walk(root, tuple(P.keys()), exclude_vendor):
        ext = os.path.splitext(f)[1]
        parser, defs = P[ext]
        try: src = open(f, "rb").read()
        except OSError: continue
        if len(src) > 800_000: continue
        try: node = parser.parse(src).root_node
        except Exception: continue
        rel = os.path.relpath(f, root)
        stack = [node]
        while stack:
            n = stack.pop()
            if n.type in defs:
                nn = n.child_by_field_name("name")
                if nn is None:
                    for ch in n.named_children:
                        if ch.type == "type_spec":
                            nn = ch.child_by_field_name("name"); break
                if nn is not None:
                    idx[src[nn.start_byte:nn.end_byte].decode("utf8","replace")].append(rel)
            stack.extend(n.named_children)
    return idx


BACKTICK = re.compile(r"[`:]{1,2}([A-Za-z_][A-Za-z0-9_.]{2,58})[`:]{0,2}`")
BT = re.compile(r"`([^`\n]{2,60})`")
FENCE = re.compile(r"```.*?```", re.S)
DOCSTR = re.compile(rb'"""(.*?)"""', re.S)

DOC_EXTS = (".md", ".rst", ".txt")


def top(path):
    p = path.split(os.sep)
    return p[0] if len(p) > 1 else ""


def rule_docs(root, symidx, exclude_vendor):
    docs = walk(root, DOC_EXTS, exclude_vendor)
    res = []
    seen = set()
    for f in docs:
        try: txt = open(f, encoding="utf8", errors="replace").read(500_000)
        except OSError: continue
        txt = FENCE.sub(" ", txt)
        rel = os.path.relpath(f, root)
        for m in BT.finditer(txt):
            tok = m.group(1).strip().split("(")[0].strip()
            if "." in tok: tok = tok.split(".")[-1]
            if not ident_like(tok): continue
            tgt = symidx.get(tok)
            if not tgt or len(tgt) != 1: continue
            k = (rel, tok)
            if k in seen: continue
            seen.add(k)
            res.append((rel, tok, tgt[0]))
    return res


def rule_docstring(root, symidx, exclude_vendor):
    res, seen = [], set()
    for f in walk(root, (".py",), exclude_vendor):
        try: src = open(f, "rb").read()
        except OSError: continue
        if len(src) > 500_000: continue
        rel = os.path.relpath(f, root)
        for m in DOCSTR.finditer(src):
            for tok in re.findall(r"[A-Za-z_][A-Za-z0-9_]*", m.group(1).decode("utf8","replace")):
                if not ident_like(tok): continue
                t = symidx.get(tok)
                if not t or len(t) != 1 or t[0] == rel: continue
                k = (rel, tok)
                if k in seen: continue
                seen.add(k)
                res.append((rel, tok, t[0]))
    return res


def locality(pairs):
    """같은 최상위 서브트리 비율 = 자동 정밀도 대용 지표"""
    if not pairs: return 0.0, 0
    same = sum(1 for a, _, b in pairs if top(a) == top(b))
    return same / len(pairs) * 100, same


TARGETS = [
    ("/tmp/corpus/kubernetes", "kubernetes"),
    ("/tmp/corpus/TypeScript", "TypeScript"),
    ("/tmp/corpus/node", "node"),
    ("/tmp/corpus/rust", "rust"),
    ("/tmp/corpus/cpython", "cpython"),
    ("/tmp/rn/django", "django"),
    ("/tmp/rn/scikit-learn", "scikit-learn"),
    ("/tmp/lang/hugo", "hugo"),
]

if __name__ == "__main__":
    print("### M2b — vendor 제외 + 문서형식 확장(.md/.rst/.txt) + 지역성 지표\n")
    print("지역성 = 문서와 대상 심볼이 같은 최상위 서브트리에 있는 비율")
    print("        (트리를 가로지르는 매칭은 흔한 영단어 오탐일 가능성이 높다)\n")
    print(f"{'repo':<14}{'심볼(전체)':>11}{'심볼(vendor제외)':>17}"
          f"{'문서제안(전체)':>15}{'문서제안(제외)':>15}{'지역성':>9}{'docstr':>8}{'지역성':>9}")
    out = []
    for root, name in TARGETS:
        if not os.path.isdir(root): continue
        idx_all = symbol_index(root, False)
        idx_cln = symbol_index(root, True)
        d_all = rule_docs(root, idx_all, False)
        d_cln = rule_docs(root, idx_cln, True)
        ds_cln = rule_docstring(root, idx_cln, True)
        loc_all, _ = locality(d_all)
        loc_cln, _ = locality(d_cln)
        loc_ds, _ = locality(ds_cln)
        print(f"{name:<14}{len(idx_all):>11,}{len(idx_cln):>17,}"
              f"{len(d_all):>15,}{len(d_cln):>15,}{loc_cln:>8.1f}%{len(ds_cln):>8,}{loc_ds:>8.1f}%")
        sys.stdout.flush()
        out.append(dict(repo=name, sym_all=len(idx_all), sym_clean=len(idx_cln),
                        docs_all=len(d_all), docs_clean=len(d_cln),
                        loc_all=loc_all, loc_clean=loc_cln,
                        docstr=len(ds_cln), loc_docstr=loc_ds,
                        samples=[list(x) for x in d_cln[:5]]))
    json.dump(out, open("/tmp/rn/m2b_results.json", "w"), indent=1)

    print("\n### vendor 제외 효과")
    print(f"{'repo':<14}{'제안 변화':>16}{'지역성 변화':>16}")
    for r in out:
        dv = r["docs_clean"] - r["docs_all"]
        lv = r["loc_clean"] - r["loc_all"]
        print(f"{r['repo']:<14}{r['docs_all']:>6,} → {r['docs_clean']:<6,}"
              f"{r['loc_all']:>7.1f}% → {r['loc_clean']:.1f}%")

    print("\n### 샘플 (vendor 제외 후)")
    for r in out:
        if not r["samples"]: continue
        print(f"\n── {r['repo']}")
        for s in r["samples"][:3]:
            mark = "✓" if top(s[0]) == top(s[2]) else "?"
            print(f"   {mark} {s[0]}  `{s[1]}`  →  {s[2]}")
