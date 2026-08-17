#!/usr/bin/env python3
"""
M1 — 언어별 심볼 레벨 Stable ID 복구율

실험 3의 한계였던 "Python 생태계만 측정" 을 해소한다.
Go / TypeScript / Rust 저장소를 전체 히스토리로 클론해 동일한 계단을 적용한다.

가설:
  Go   — 패키지=디렉터리 구조가 강제되어 파일 이동이 드물다 → S1 비율이 높을 것
  TS   — 리팩터링/번들 구조 변경이 잦다 → S1이 낮고 S2/S4 의존이 클 것
  Rust — 모듈 시스템이 파일 경로와 결합 → Go에 가까울 것

계단은 실험 3(symbol_bench2)과 동일:
  S1 동일 경로·동일 qualname → S2 git alias → S3 qualname 전역 유일
  → S4 본문 유사도(Jaccard>=0.60) → S5 실패 / GONE(오라클<0.30, 분모 제외)
"""

import subprocess, os, sys, io, tarfile, zlib, heapq, json
from collections import defaultdict, Counter
from tree_sitter import Language, Parser
import tree_sitter_python as tspy
import tree_sitter_go as tsgo
import tree_sitter_typescript as tsts
import tree_sitter_rust as tsrs

SHINGLE_K, SKETCH_K = 4, 96
MATCH_SIM, ORACLE_SIM = 0.60, 0.30
TOP_CANDIDATES = 40

# 언어별 파서 + 심볼 노드 타입 + 확장자
LANGS = {
    "python": dict(parser=Parser(Language(tspy.language())), ext=(".py",),
                   defs=("class_definition", "function_definition"),
                   wrap=("decorated_definition",),
                   recurse=("block", "if_statement", "try_statement", "with_statement", "module")),
    "go": dict(parser=Parser(Language(tsgo.language())), ext=(".go",),
               defs=("function_declaration", "method_declaration", "type_declaration"),
               wrap=(), recurse=("source_file", "block", "declaration_list")),
    "typescript": dict(parser=Parser(Language(tsts.language_typescript())), ext=(".ts",),
                       defs=("function_declaration", "class_declaration", "method_definition",
                             "interface_declaration", "type_alias_declaration"),
                       wrap=("export_statement", "ambient_declaration"),
                       recurse=("program", "statement_block", "class_body", "if_statement")),
    "rust": dict(parser=Parser(Language(tsrs.language())), ext=(".rs",),
                 defs=("function_item", "struct_item", "enum_item", "trait_item", "impl_item"),
                 wrap=(), recurse=("source_file", "declaration_list", "block", "mod_item")),
}


def git(repo, *a, binary=False, timeout=2400):
    r = subprocess.run(["git", "-C", repo, *a], capture_output=True, timeout=timeout,
                       **({} if binary else {"text": True, "errors": "replace"}))
    return r.stdout


def extract(src: bytes, cfg):
    tree = cfg["parser"].parse(src)
    out = []

    def label(node):
        n = node.child_by_field_name("name")
        if n is None:
            n = node.child_by_field_name("type")     # rust impl_item 등
        if n is None:
            # go type_declaration: type_spec 안에 이름
            for ch in node.named_children:
                if ch.type in ("type_spec", "variable_declarator"):
                    n = ch.child_by_field_name("name")
                    break
        if n is None:
            return None
        return src[n.start_byte:n.end_byte].decode("utf8", "replace")

    def walk(node, prefix):
        for ch in node.named_children:
            if ch.type in cfg["defs"]:
                nm = label(ch)
                if nm:
                    q = f"{prefix}.{nm}" if prefix else nm
                    out.append((q, src[ch.start_byte:ch.end_byte]))
                    walk(ch, q)
                    continue
            if ch.type in cfg["wrap"] or ch.type in cfg["recurse"]:
                walk(ch, prefix)
    walk(tree.root_node, "")
    return out


def tokenize(b: bytes):
    toks, cur = [], []
    for x in b:
        c = chr(x)
        if c.isalnum() or c == "_":
            cur.append(c)
        else:
            if cur: toks.append("".join(cur)); cur = []
    if cur: toks.append("".join(cur))
    return toks


def shingles(b: bytes):
    t = tokenize(b)
    if len(t) < SHINGLE_K:
        return {zlib.crc32("\x00".join(t).encode())} if t else set()
    return {zlib.crc32("\x00".join(t[i:i+SHINGLE_K]).encode()) for i in range(len(t)-SHINGLE_K+1)}


def sketch(hs):
    return heapq.nsmallest(SKETCH_K, hs) if len(hs) > SKETCH_K else sorted(hs)


def jac(a, b):
    if not a or not b: return 0.0
    i = len(a & b)
    return i / (len(a) + len(b) - i)


def snapshot(repo, rev, cfg):
    pats = [f"*{e}" for e in cfg["ext"]]
    raw = git(repo, "archive", rev, *pats, binary=True)
    syms = {}
    try:
        tf = tarfile.open(fileobj=io.BytesIO(raw))
    except Exception:
        return {}
    for m in tf.getmembers():
        if not m.isfile() or not m.name.endswith(cfg["ext"]): continue
        try: data = tf.extractfile(m).read()
        except Exception: continue
        if len(data) > 800_000: continue
        try: ss = extract(data, cfg)
        except Exception: continue
        if ss: syms[m.name] = [(q, shingles(b)) for q, b in ss]
    return syms


def build_alias(repo):
    out = git(repo, "log", "--all", "--diff-filter=R", "--name-status", "-M", "--format=")
    direct = {}
    for line in out.splitlines():
        p = line.split("\t")
        if len(p) == 3 and p[0].startswith("R"): direct[p[1]] = p[2]
    memo = {}
    def follow(p, d=0):
        if p in memo: return memo[p]
        if d > 64 or p not in direct: return p
        r = follow(direct[p], d+1); memo[p] = r; return r
    return {k: follow(k) for k in direct}


def analyze(repo, name, lang, frac):
    cfg = LANGS[lang]
    total = int(git(repo, "rev-list", "--count", "HEAD").strip())
    rev = git(repo, "rev-list", "HEAD", f"--skip={int(total*frac)}", "--max-count=1").strip()
    date = git(repo, "log", "-1", "--format=%ad", "--date=short", rev).strip()
    old = snapshot(repo, rev, cfg)
    head = snapshot(repo, "HEAD", cfg)
    if not old or not head: return None
    alias = build_alias(repo)

    head_by_path = {p: {q for q, _ in v} for p, v in head.items()}
    head_by_qual = defaultdict(list)
    items = []
    for p, v in head.items():
        for q, hs in v:
            head_by_qual[q].append(p); items.append((p, q, hs))
    inv = defaultdict(list)
    for i, (_, _, hs) in enumerate(items):
        for h in sketch(hs): inv[h].append(i)

    def best(hs):
        votes = Counter()
        for h in sketch(hs):
            post = inv.get(h)
            if post and len(post) <= 200:
                for i in post: votes[i] += 1
        bi, bs = None, 0.0
        for i, _ in votes.most_common(TOP_CANDIDATES):
            j = jac(hs, items[i][2])
            if j > bs: bs, bi = j, items[i]
        return bi, bs

    c = Counter(); s4nk = 0
    for p, v in old.items():
        np_ = alias.get(p, p)
        for q, hs in v:
            if q in head_by_path.get(p, ()): c["S1"] += 1; continue
            if np_ != p and q in head_by_path.get(np_, ()): c["S2"] += 1; continue
            cand = head_by_qual.get(q, [])
            if len(cand) == 1: c["S3u"] += 1; continue
            if len(cand) > 1: c["S3a"] += 1; continue
            bi, bs = best(hs)
            if bs >= MATCH_SIM:
                c["S4"] += 1
                if bi[1].split(".")[-1] == q.split(".")[-1]: s4nk += 1
                continue
            if bs >= ORACLE_SIM: c["S5e"] += 1
            else: c["GONE"] += 1
    c["n"] = sum(len(v) for v in old.values())
    c["head_n"] = len(items)
    return dict(repo=name, lang=lang, frac=frac, date=date, c=dict(c), s4nk=s4nk,
                commits=total)


TARGETS = [
    ("/tmp/lang/hugo", "hugo", "go"), ("/tmp/lang/cobra", "cobra", "go"),
    ("/tmp/lang/gin", "gin", "go"),
    ("/tmp/lang/core", "vue-core", "typescript"), ("/tmp/lang/prettier", "prettier", "typescript"),
    ("/tmp/lang/date-fns", "date-fns", "typescript"),
    ("/tmp/lang/tokio", "tokio", "rust"), ("/tmp/lang/clap", "clap", "rust"),
    ("/tmp/lang/serde", "serde", "rust"),
    ("/tmp/rn/django", "django", "python"), ("/tmp/rn/scikit-learn", "scikit-learn", "python"),
]

if __name__ == "__main__":
    results = []
    print(f"{'repo':<14}{'lang':<12}{'경과':<6}{'기준일':<12}{'심볼':>7}{'S1':>7}{'S2':>6}"
          f"{'S3u':>6}{'S4':>5}{'GONE':>6}  {'자동':>7}{'+1클릭':>8}")
    for repo, name, lang in TARGETS:
        if not os.path.isdir(repo): continue
        for frac in (0.25, 0.50, 0.75):
            try:
                r = analyze(repo, name, lang, frac)
            except Exception as e:
                print(f"  ! {name} {frac} 실패: {e}", file=sys.stderr); continue
            if not r: continue
            c = r["c"]; n = c.get("n", 0)
            if n < 50: continue
            live = n - c.get("GONE", 0)
            auto = (c.get("S1",0)+c.get("S2",0))/live*100 if live else 0
            plus = (c.get("S1",0)+c.get("S2",0)+c.get("S3u",0)+c.get("S4",0))/live*100 if live else 0
            r["auto"], r["plus"], r["live"] = auto, plus, live
            results.append(r)
            print(f"{name:<14}{lang:<12}{int(frac*100):>3}%  {r['date']:<12}{n:>7}"
                  f"{c.get('S1',0):>7}{c.get('S2',0):>6}{c.get('S3u',0):>6}{c.get('S4',0):>5}"
                  f"{c.get('GONE',0):>6}  {auto:>6.1f}%{plus:>7.1f}%")
        sys.stdout.flush()

    json.dump(results, open("/tmp/rn/m1_results.json", "w"), indent=1)
    print()
    print("### 언어별 종합 (50% 경과)")
    print(f"{'lang':<12}{'저장소':>7}{'심볼':>8}{'live':>8}{'S1%':>7}{'S2%':>7}{'S3u%':>7}{'S4%':>7}"
          f"  {'자동':>7}{'+1클릭':>8}")
    bylang = defaultdict(Counter)
    for r in results:
        if r["frac"] != 0.50: continue
        b = bylang[r["lang"]]
        for k in ("S1","S2","S3u","S3a","S4","S5e","GONE","n"): b[k] += r["c"].get(k, 0)
        b["repos"] += 1
    for lang, b in bylang.items():
        live = b["n"] - b["GONE"]
        if not live: continue
        auto = (b["S1"]+b["S2"])/live*100
        plus = (b["S1"]+b["S2"]+b["S3u"]+b["S4"])/live*100
        print(f"{lang:<12}{b['repos']:>7}{b['n']:>8}{live:>8}{b['S1']/live*100:>6.1f}%"
              f"{b['S2']/live*100:>6.1f}%{b['S3u']/live*100:>6.1f}%{b['S4']/live*100:>6.1f}%"
              f"  {auto:>6.1f}%{plus:>7.1f}%")
