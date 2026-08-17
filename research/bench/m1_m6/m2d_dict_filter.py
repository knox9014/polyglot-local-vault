#!/usr/bin/env python3
"""
M2d — 제안 정밀도: 영어 사전 필터 (최종)

두 번의 실패에서 배운 것.

  M2b 지역성 필터   → 실패. docs/ 와 src/ 는 원래 분리되므로
                      제품이 노리는 문서↔코드 관계를 정확히 벌준다.
  M2c 산문 빈도 필터 → 실패. 좋은 문서는 API 이름을 산문에서도 반복한다.
                      `ModelAdmin`(산문 250회), `LoadBalancerSourceRanges`(8회)를 버렸다.

실제 오탐의 공통점은 빈도도 위치도 아니고 **그 토큰이 영어 단어라는 것**이다.
  오탐: Local, warnings, Global, Configuration, priority, Worker, Resource, Script, Instant
  정탐: ModelAdmin, LoadBalancerSourceRanges, lru_cache, TweedieRegressor, PostCSS

규칙:
  토큰이 "평범한 단일 단어"(밑줄 없음 + 내부 대문자 없음)이고
  그 소문자형(또는 단수형)이 영어 사전에 있으면 제외한다.
  복합 식별자는 사전에 없으므로 전부 통과한다.

사전은 정적 데이터(약 234k 단어)로 앱에 동봉 가능하며 네트워크가 필요 없다.
"""
import os, re, sys, json, random
from collections import defaultdict, Counter
from english_words import get_english_words_set
from tree_sitter import Language, Parser
import tree_sitter_python as tspy, tree_sitter_go as tsgo
import tree_sitter_typescript as tsts, tree_sitter_rust as tsrs

WORDS = get_english_words_set(['web2'], lower=True)
P = {
 ".py": (Parser(Language(tspy.language())), ("class_definition","function_definition")),
 ".go": (Parser(Language(tsgo.language())), ("function_declaration","method_declaration","type_declaration")),
 ".ts": (Parser(Language(tsts.language_typescript())), ("function_declaration","class_declaration","interface_declaration","type_alias_declaration")),
 ".rs": (Parser(Language(tsrs.language())), ("function_item","struct_item","enum_item","trait_item")),
}
VENDOR={"vendor","third_party","third-party","deps","node_modules","external","3rdparty"}
SKIP={".git","node_modules","__pycache__","target","dist","build",".venv","venv"}
DOC=(".md",".rst",".txt"); FENCE=re.compile(r"```.*?```",re.S); BT=re.compile(r"`([^`\n]{2,60})`")
IDENT=re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")

def compound(t):
    return "_" in t or bool(re.search(r"[a-z][A-Z]", t)) or bool(re.search(r"[A-Z]{2,}[a-z]", t)) or any(c.isdigit() for c in t)

def english_word(t):
    l = t.lower()
    if l in WORDS: return True
    for suf in ("s","es","ed","ing"):
        if l.endswith(suf) and l[:-len(suf)] in WORDS: return True
    return False

def keep(t):
    if not IDENT.match(t) or len(t) < 4: return False
    if compound(t): return True          # 복합 식별자는 통과
    return not english_word(t)           # 단일 단어는 사전에 없을 때만 통과

def walk(root, exts):
    out,st=[],[root]
    while st:
        d=st.pop()
        try:
            with os.scandir(d) as it:
                for e in it:
                    try:
                        if e.is_dir(follow_symlinks=False):
                            if e.name in SKIP or e.name in VENDOR: continue
                            st.append(e.path)
                        elif e.is_file(follow_symlinks=False) and e.name.endswith(exts): out.append(e.path)
                    except OSError: continue
        except OSError: continue
    return out

def symidx(root):
    idx=defaultdict(list)
    for f in walk(root, tuple(P.keys())):
        parser,defs=P[os.path.splitext(f)[1]]
        try: src=open(f,"rb").read()
        except OSError: continue
        if len(src)>800_000: continue
        try: node=parser.parse(src).root_node
        except Exception: continue
        rel=os.path.relpath(f,root); stk=[node]
        while stk:
            n=stk.pop()
            if n.type in defs:
                nn=n.child_by_field_name("name")
                if nn is None:
                    for ch in n.named_children:
                        if ch.type=="type_spec": nn=ch.child_by_field_name("name"); break
                if nn is not None: idx[src[nn.start_byte:nn.end_byte].decode("utf8","replace")].append(rel)
            stk.extend(n.named_children)
    return idx

TARGETS=[("/tmp/corpus/kubernetes","kubernetes"),("/tmp/corpus/TypeScript","TypeScript"),
 ("/tmp/corpus/node","node"),("/tmp/corpus/rust","rust"),("/tmp/corpus/cpython","cpython"),
 ("/tmp/rn/django","django"),("/tmp/rn/scikit-learn","scikit-learn"),("/tmp/lang/hugo","hugo")]

print("### M2d — 영어 사전 필터 (최종)\n")
print(f"사전 {len(WORDS):,}단어. 복합 식별자는 무조건 통과, 단일 단어만 사전 검사.\n")
print(f"{'repo':<14}{'심볼':>9}{'문서':>8}{'필터 전':>9}{'필터 후':>9}{'제거':>8}{'제거율':>9}")
res=[]; random.seed(3)
for root,name in TARGETS:
    if not os.path.isdir(root): continue
    idx=symidx(root); raw=[]; seen=set()
    for f in walk(root,DOC):
        try: txt=open(f,encoding="utf8",errors="replace").read(500_000)
        except OSError: continue
        body=FENCE.sub(" ",txt); rel=os.path.relpath(f,root)
        for m in BT.finditer(body):
            tok=m.group(1).strip().split("(")[0].strip()
            if "." in tok: tok=tok.split(".")[-1]
            if not IDENT.match(tok) or len(tok)<4: continue
            t=idx.get(tok)
            if not t or len(t)!=1: continue
            k=(rel,tok)
            if k in seen: continue
            seen.add(k); raw.append((rel,tok,t[0]))
    kept=[x for x in raw if keep(x[1])]; drop=[x for x in raw if not keep(x[1])]
    print(f"{name:<14}{len(idx):>9,}{0:>8}{len(raw):>9,}{len(kept):>9,}{len(drop):>8,}"
          f"{(len(drop)/len(raw)*100 if raw else 0):>8.1f}%")
    sys.stdout.flush()
    res.append(dict(name=name,raw=len(raw),kept=len(kept),drop=len(drop),
                    kept_s=kept,drop_s=drop))
print("\n### 필터가 제거한 것 (영어 단어여야 함)")
for r in res:
    if not r["drop_s"]: continue
    print(f"── {r['name']}: " + ", ".join(f"`{t}`" for _,t,_ in random.sample(r["drop_s"],min(8,len(r["drop_s"])))))
print("\n### 필터가 남긴 것 (무작위 표본)")
for r in res:
    if not r["kept_s"]: continue
    print(f"\n── {r['name']} (총 {r['kept']:,})")
    for rel,tok,tgt in random.sample(r["kept_s"],min(3,len(r["kept_s"]))):
        print(f"   • `{tok}`  {rel} → {tgt}")
tr=sum(r["raw"] for r in res); tk=sum(r["kept"] for r in res)
print(f"\n### 종합\n  필터 전 {tr:,} → 필터 후 {tk:,} ({tk/tr*100:.1f}%), 저장소당 평균 {tk/len(res):,.0f}건")
json.dump([{k:v for k,v in r.items() if not k.endswith('_s')} for r in res],
          open("/tmp/rn/m2d_results.json","w"),indent=1)
