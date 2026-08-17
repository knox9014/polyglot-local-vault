#!/usr/bin/env python3
"""
M3 — call edge 정확도 (리뷰 §4 검증)

리뷰 §4에서 이렇게 주장했다.

  "타입 추론 없이 이름 매칭으로 call edge를 만들면 오탐이 정탐보다 많아진다."
  "v0.1 파서 범위에서 calls를 빼는 것을 권한다."

이 주장을 실측한다. 실제 저장소의 모든 호출부를 추출하고,
심볼 이름만으로 유일하게 해석되는 비율을 센다.

분류:
  UNIQUE      vault 안에 그 이름의 정의가 정확히 1개  → edge 신뢰 가능
  AMBIGUOUS   2개 이상                                → 어느 것인지 알 수 없음
  EXTERNAL    0개 (표준 라이브러리/외부 패키지/빌트인) → edge 생성 불가

추가로 호출 형태를 나눈다.
  bare      foo(...)            — 모듈 스코프 함수 호출일 가능성
  attr      obj.method(...)     — 리시버 타입을 모르면 해석 불가 (핵심 케이스)
  self      self.method(...)    — 리시버가 확정적 → 클래스 스코프로 좁힐 수 있음
"""

import subprocess, io, tarfile, sys, json
from collections import defaultdict, Counter
from tree_sitter import Language, Parser
import tree_sitter_python as tspy
import tree_sitter_go as tsgo

PY = Parser(Language(tspy.language()))
GO = Parser(Language(tsgo.language()))


def git(repo, *a, binary=False):
    r = subprocess.run(["git", "-C", repo, *a], capture_output=True, timeout=1800,
                       **({} if binary else {"text": True, "errors": "replace"}))
    return r.stdout


def files_at_head(repo, ext):
    raw = git(repo, "archive", "HEAD", f"*{ext}", binary=True)
    try:
        tf = tarfile.open(fileobj=io.BytesIO(raw))
    except Exception:
        return []
    out = []
    for m in tf.getmembers():
        if m.isfile() and m.name.endswith(ext):
            try:
                d = tf.extractfile(m).read()
            except Exception:
                continue
            if len(d) < 2_000_000:
                out.append((m.name, d))
    return out


# ---------------------------------------------------------------------------
# 정의 수집: 이름 -> 정의 개수
# ---------------------------------------------------------------------------
def collect_defs_py(files):
    defs = Counter()
    cls_methods = defaultdict(set)   # 클래스명 -> 메서드명 집합
    for path, src in files:
        root = PY.parse(src).root_node
        def walk(n, cls):
            for ch in n.named_children:
                if ch.type in ("class_definition", "function_definition"):
                    nn = ch.child_by_field_name("name")
                    if nn:
                        nm = src[nn.start_byte:nn.end_byte].decode("utf8", "replace")
                        defs[nm] += 1
                        if ch.type == "class_definition":
                            walk(ch, nm)
                        else:
                            if cls: cls_methods[cls].add(nm)
                            walk(ch, cls)
                        continue
                walk(ch, cls)
        walk(root, None)
    return defs, cls_methods


def collect_defs_go(files):
    defs = Counter()
    for path, src in files:
        root = GO.parse(src).root_node
        def walk(n):
            for ch in n.named_children:
                if ch.type in ("function_declaration", "method_declaration"):
                    nn = ch.child_by_field_name("name")
                    if nn:
                        defs[src[nn.start_byte:nn.end_byte].decode("utf8", "replace")] += 1
                walk(ch)
        walk(root)
    return defs, {}


# ---------------------------------------------------------------------------
# 호출부 수집
# ---------------------------------------------------------------------------
def collect_calls_py(files):
    calls = []   # (kind, name)
    for path, src in files:
        root = PY.parse(src).root_node
        stack = [root]
        while stack:
            n = stack.pop()
            if n.type == "call":
                f = n.child_by_field_name("function")
                if f is not None:
                    if f.type == "identifier":
                        calls.append(("bare", src[f.start_byte:f.end_byte].decode("utf8", "replace")))
                    elif f.type == "attribute":
                        obj = f.child_by_field_name("object")
                        at = f.child_by_field_name("attribute")
                        if at is not None:
                            nm = src[at.start_byte:at.end_byte].decode("utf8", "replace")
                            is_self = (obj is not None and obj.type == "identifier"
                                       and src[obj.start_byte:obj.end_byte] == b"self")
                            calls.append(("self" if is_self else "attr", nm))
            stack.extend(n.named_children)
    return calls


def collect_calls_go(files):
    calls = []
    for path, src in files:
        root = GO.parse(src).root_node
        stack = [root]
        while stack:
            n = stack.pop()
            if n.type == "call_expression":
                f = n.child_by_field_name("function")
                if f is not None:
                    if f.type == "identifier":
                        calls.append(("bare", src[f.start_byte:f.end_byte].decode("utf8", "replace")))
                    elif f.type == "selector_expression":
                        at = f.child_by_field_name("field")
                        if at is not None:
                            calls.append(("attr", src[at.start_byte:at.end_byte].decode("utf8", "replace")))
            stack.extend(n.named_children)
    return calls


TARGETS = [
    ("/tmp/rn/django", "django", "python", ".py"),
    ("/tmp/rn/scikit-learn", "scikit-learn", "python", ".py"),
    ("/tmp/rn/flask", "flask", "python", ".py"),
    ("/tmp/lang/hugo", "hugo", "go", ".go"),
    ("/tmp/lang/gin", "gin", "go", ".go"),
]

print("### M3 — call edge 이름 해석 모호도\n")
print("UNIQUE = 그 이름의 정의가 vault에 정확히 1개 (edge 신뢰 가능)")
print("AMBIG  = 2개 이상 (어느 정의인지 알 수 없음)")
print("EXTERN = 0개 (외부 패키지/빌트인, edge 생성 불가)\n")
print(f"{'repo':<14}{'lang':<8}{'호출부':>9}{'UNIQUE':>9}{'AMBIG':>9}{'EXTERN':>9}"
      f"  {'정탐률':>8}{'오탐률':>8}")

allres = []
agg = Counter()
bykind_agg = defaultdict(Counter)
for repo, name, lang, ext in TARGETS:
    files = files_at_head(repo, ext)
    if not files: continue
    if lang == "python":
        defs, clsm = collect_defs_py(files); calls = collect_calls_py(files)
    else:
        defs, clsm = collect_defs_go(files); calls = collect_calls_go(files)

    c = Counter(); bykind = defaultdict(Counter)
    for kind, nm in calls:
        d = defs.get(nm, 0)
        cat = "UNIQUE" if d == 1 else ("AMBIG" if d > 1 else "EXTERN")
        c[cat] += 1
        bykind[kind][cat] += 1
        bykind_agg[kind][cat] += 1
    n = sum(c.values())
    if not n: continue
    resolvable = c["UNIQUE"] + c["AMBIG"]
    prec = c["UNIQUE"] / resolvable * 100 if resolvable else 0
    print(f"{name:<14}{lang:<8}{n:>9,}{c['UNIQUE']:>9,}{c['AMBIG']:>9,}{c['EXTERN']:>9,}"
          f"  {prec:>7.1f}%{100-prec:>7.1f}%")
    allres.append(dict(repo=name, lang=lang, defs=len(defs), calls=n, **dict(c)))
    for k in c: agg[k] += c[k]
    agg["defs"] += len(defs)

n = agg["UNIQUE"] + agg["AMBIG"] + agg["EXTERN"]
res = agg["UNIQUE"] + agg["AMBIG"]
print()
print("### 종합")
print(f"  전체 호출부              : {n:,}")
print(f"  UNIQUE (신뢰 가능)       : {agg['UNIQUE']:,} ({agg['UNIQUE']/n*100:.1f}%)")
print(f"  AMBIG  (모호)            : {agg['AMBIG']:,} ({agg['AMBIG']/n*100:.1f}%)")
print(f"  EXTERN (외부/빌트인)     : {agg['EXTERN']:,} ({agg['EXTERN']/n*100:.1f}%)")
print()
print(f"  >>> vault 내부로 해석되는 호출부 중 유일 매칭 = {agg['UNIQUE']/res*100:.1f}%")
print(f"  >>> 즉 모호(오탐 위험) 비율                   = {agg['AMBIG']/res*100:.1f}%")

print()
print("### 호출 형태별")
print(f"{'형태':<10}{'건수':>10}{'UNIQUE':>10}{'AMBIG':>10}{'EXTERN':>10}  {'유일률':>8}")
for kind in ("bare", "self", "attr"):
    b = bykind_agg.get(kind)
    if not b: continue
    t = sum(b.values()); r = b["UNIQUE"] + b["AMBIG"]
    print(f"{kind:<10}{t:>10,}{b['UNIQUE']:>10,}{b['AMBIG']:>10,}{b['EXTERN']:>10,}"
          f"  {(b['UNIQUE']/r*100 if r else 0):>7.1f}%")

json.dump(dict(per_repo=allres, agg=dict(agg),
               by_kind={k: dict(v) for k, v in bykind_agg.items()}),
          open("/tmp/rn/m3_results.json", "w"), indent=1)
