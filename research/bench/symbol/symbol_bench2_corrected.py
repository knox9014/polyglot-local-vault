#!/usr/bin/env python3
"""
가정 검증 #3b — 심볼 레벨 Stable ID 복구율 (정정판)

1차판의 두 결함을 고쳤다.

  결함 1: 유사도 후보 검색이 `list(frozenset)[:80]` 에 의존 — 순서가 비결정적이고
          재현성이 없다. → minhash 스케치(가장 작은 crc32 k개)로 교체.
  결함 2: S5(복구 불가) 안에 "실제로 삭제된 심볼"이 섞여 있다.
          12년이 지나면 함수 상당수는 그냥 사라진다. 그건 링크가 깨지는 게 정상이다.
          → 느슨한 임계값(ORACLE_SIM)의 오라클로 "아직 존재하는 심볼"을 판정해
            분모를 정정한다.

계단:
  S1. 같은 경로에 같은 qualname                  → HIT
  S2. git alias로 옮긴 경로에 같은 qualname       → HIT
  S3. vault 전체에서 qualname 유일                → 1클릭
  S4. 본문 유사도 (Jaccard >= MATCH_SIM)          → 1클릭
  S5. 실패
  GONE. 오라클도 못 찾음 = 심볼이 실제로 사라짐   → BROKEN이 정상, 분모에서 제외
"""

import subprocess, os, sys, ast, io, tarfile, zlib, heapq
from collections import defaultdict, Counter
from tree_sitter import Language, Parser
import tree_sitter_python as tspy

PARSER = Parser(Language(tspy.language()))

SHINGLE_K = 4       # 토큰 n-gram 크기
SKETCH_K = 96       # minhash 스케치 크기
MATCH_SIM = 0.60    # S4 확정 임계값
ORACLE_SIM = 0.30   # "아직 존재한다"고 볼 느슨한 임계값
TOP_CANDIDATES = 40


def git(repo, *a, timeout=2400, binary=False):
    r = subprocess.run(["git", "-C", repo, *a], capture_output=True, timeout=timeout,
                       **({} if binary else {"text": True, "errors": "replace"}))
    return r.stdout


def extract_symbols(src: bytes):
    tree = PARSER.parse(src)
    out = []
    def name_of(node):
        n = node.child_by_field_name("name")
        return src[n.start_byte:n.end_byte].decode("utf8", "replace") if n else None
    def walk(node, prefix):
        for ch in node.named_children:
            t = ch.type
            if t in ("class_definition", "function_definition"):
                nm = name_of(ch)
                if nm:
                    q = f"{prefix}.{nm}" if prefix else nm
                    out.append((q, src[ch.start_byte:ch.end_byte]))
                    walk(ch, q)
                    continue
            if t == "decorated_definition":
                walk(ch, prefix); continue
            if t in ("block", "if_statement", "try_statement", "with_statement", "module"):
                walk(ch, prefix)
    walk(tree.root_node, "")
    return out


def tokenize(body: bytes):
    toks, cur = [], []
    for b in body:
        c = chr(b)
        if c.isalnum() or c == "_":
            cur.append(c)
        else:
            if cur: toks.append("".join(cur)); cur = []
    if cur: toks.append("".join(cur))
    return toks


def shingle_hashes(body: bytes):
    """토큰 n-gram의 crc32 집합"""
    t = tokenize(body)
    if len(t) < SHINGLE_K:
        return {zlib.crc32("\x00".join(t).encode())} if t else set()
    return {zlib.crc32("\x00".join(t[i:i + SHINGLE_K]).encode())
            for i in range(len(t) - SHINGLE_K + 1)}


def sketch(hs):
    """minhash 스케치 — 가장 작은 SKETCH_K개. 결정적이고 재현 가능."""
    return heapq.nsmallest(SKETCH_K, hs) if len(hs) > SKETCH_K else sorted(hs)


def jaccard(a, b):
    if not a or not b: return 0.0
    i = len(a & b)
    return i / (len(a) + len(b) - i)


def snapshot(repo, rev, parser_stats=False):
    raw = git(repo, "archive", rev, "*.py", binary=True)
    syms, stats = {}, Counter()
    try:
        tf = tarfile.open(fileobj=io.BytesIO(raw))
    except Exception:
        return {}, stats
    for m in tf.getmembers():
        if not m.isfile() or not m.name.endswith(".py"):
            continue
        try:
            data = tf.extractfile(m).read()
        except Exception:
            continue
        stats["files"] += 1
        if parser_stats:
            try:
                ast.parse(data); stats["ast_ok"] += 1
            except SyntaxError:
                stats["ast_syntaxerror"] += 1
            except Exception:
                stats["ast_other"] += 1
            if PARSER.parse(data).root_node.has_error:
                stats["ts_partial"] += 1
            else:
                stats["ts_ok"] += 1
        try:
            ss = extract_symbols(data)
        except Exception:
            stats["extract_fail"] += 1
            continue
        if ss:
            syms[m.name] = [(q, shingle_hashes(b)) for q, b in ss]
            stats["symbols"] += len(ss)
    return syms, stats


def build_alias(repo):
    out = git(repo, "log", "--all", "--diff-filter=R", "--name-status", "-M", "--format=")
    direct = {}
    for line in out.splitlines():
        p = line.split("\t")
        if len(p) == 3 and p[0].startswith("R"):
            direct[p[1]] = p[2]
    memo = {}
    def follow(p, d=0):
        if p in memo: return memo[p]
        if d > 64 or p not in direct: return p
        r = follow(direct[p], d + 1); memo[p] = r; return r
    return {k: follow(k) for k in direct}


def analyze(repo, name, frac, parser_stats=False):
    total = int(git(repo, "rev-list", "--count", "HEAD").strip())
    rev = git(repo, "rev-list", "HEAD", f"--skip={int(total*frac)}", "--max-count=1").strip()
    date = git(repo, "log", "-1", "--format=%ad", "--date=short", rev).strip()

    old, ostat = snapshot(repo, rev, parser_stats)
    head, hstat = snapshot(repo, "HEAD", parser_stats)
    alias = build_alias(repo)

    head_by_path = {p: {q for q, _ in v} for p, v in head.items()}
    head_by_qual = defaultdict(list)
    items = []
    for p, v in head.items():
        for q, hs in v:
            head_by_qual[q].append(p)
            items.append((p, q, hs))

    # minhash 역인덱스
    inv = defaultdict(list)
    for idx, (_, _, hs) in enumerate(items):
        for h in sketch(hs):
            inv[h].append(idx)

    def best_match(hs):
        votes = Counter()
        for h in sketch(hs):
            post = inv.get(h)
            if post and len(post) <= 200:      # 흔한 shingle은 건너뜀
                for i in post:
                    votes[i] += 1
        best, bs = None, 0.0
        for i, _ in votes.most_common(TOP_CANDIDATES):
            j = jaccard(hs, items[i][2])
            if j > bs: bs, best = j, items[i]
        return best, bs

    c = Counter()
    s4_name_kept = 0
    for p, v in old.items():
        newp = alias.get(p, p)
        for q, hs in v:
            if q in head_by_path.get(p, ()):
                c["S1"] += 1; continue
            if newp != p and q in head_by_path.get(newp, ()):
                c["S2"] += 1; continue
            cand = head_by_qual.get(q, [])
            if len(cand) == 1:
                c["S3u"] += 1; continue
            if len(cand) > 1:
                c["S3a"] += 1; continue
            best, bs = best_match(hs)
            if bs >= MATCH_SIM:
                c["S4"] += 1
                if best[1].split(".")[-1] == q.split(".")[-1]:
                    s4_name_kept += 1
                continue
            if bs >= ORACLE_SIM:
                c["S5_exists"] += 1      # 존재는 하는데 확정 못 함
            else:
                c["GONE"] += 1           # 실제로 사라짐 → 분모 제외
    c["old_symbols"] = sum(len(v) for v in old.values())
    c["head_symbols"] = len(items)
    return dict(name=name, date=date, c=c, s4_name_kept=s4_name_kept, ostat=ostat, hstat=hstat)


REPOS = [("/tmp/rn/django", "django"), ("/tmp/rn/scikit-learn", "scikit-learn"),
         ("/tmp/rn/flask", "flask"), ("/tmp/rn/requests", "requests")]

if __name__ == "__main__":
    sel = set(sys.argv[1:]) or None
    print("### 심볼 레벨 계단식 해석 복구율 (정정판)\n")
    print(f"S4 확정 임계 Jaccard>={MATCH_SIM} / 오라클 임계 >={ORACLE_SIM} / "
          f"shingle {SHINGLE_K}-gram / sketch {SKETCH_K}\n")
    print(f"{'repo':<14}{'경과':<6}{'기준일':<12}{'심볼':>7}{'S1':>7}{'S2':>5}{'S3u':>5}"
          f"{'S3a':>5}{'S4':>5}{'S5e':>5}{'GONE':>6}  {'지표A':>7}{'지표B':>8}")
    agg, pst = Counter(), Counter()
    for repo, name in REPOS:
        if sel and name not in sel: continue
        if not os.path.isdir(repo): continue
        for frac, lab in [(0.25, "25%"), (0.5, "50%"), (0.75, "75%")]:
            r = analyze(repo, name, frac, parser_stats=(frac == 0.5))
            c = r["c"]; n = c["old_symbols"]
            if not n: continue
            live = n - c["GONE"]
            hit = c["S1"] + c["S2"]
            plus = hit + c["S3u"] + c["S4"]
            A = plus / n * 100
            B = plus / live * 100 if live else 0
            print(f"{name:<14}{lab:<6}{r['date']:<12}{n:>7}{c['S1']:>7}{c['S2']:>5}{c['S3u']:>5}"
                  f"{c['S3a']:>5}{c['S4']:>5}{c['S5_exists']:>5}{c['GONE']:>6}  {A:>6.1f}%{B:>7.1f}%")
            if frac == 0.5:
                for k in ("S1","S2","S3u","S3a","S4","S5_exists","GONE","old_symbols"):
                    agg[k] += c[k]
                agg["s4nk"] += r["s4_name_kept"]
                for k, v in r["ostat"].items(): pst["old_" + k] += v
                for k, v in r["hstat"].items(): pst["head_" + k] += v
        print()

    n = agg["old_symbols"]
    if n:
        live = n - agg["GONE"]
        hit = agg["S1"] + agg["S2"]
        plus = hit + agg["S3u"] + agg["S4"]
        print("### 종합 (50% 경과 기준)")
        print(f"  과거 심볼 링크 총계        : {n}")
        print(f"  그중 실제로 사라짐(GONE)   : {agg['GONE']} ({agg['GONE']/n*100:.1f}%)  ← BROKEN이 정상")
        print(f"  대상이 살아있는 링크       : {live}\n")
        for k, lab in [("S1","S1 같은 경로 동일 심볼"),("S2","S2 git alias 경로"),
                       ("S3u","S3 qualname 유일"),("S3a","S3 qualname 모호"),
                       ("S4","S4 본문 유사도"),("S5_exists","S5 존재하나 확정 실패")]:
            print(f"  {lab:<26}: {agg[k]:>6} ({agg[k]/live*100:5.1f}% of live)")
        print()
        print(f"  >>> 자동 복구 (S1+S2)      = {hit/live*100:.1f}%")
        print(f"  >>> +1클릭 (S3u+S4)        = {plus/live*100:.1f}%")
        print(f"  >>> S4 단독 기여           = {agg['S4']/live*100:.1f}%p"
              f"  (그중 이름 유지 {agg['s4nk']}/{agg['S4']})")
        print(f"  >>> 지표A (전체 대비)      = {plus/n*100:.1f}%")
        print()
    if pst:
        print("### 파서 비교 — CPython ast vs tree-sitter (리뷰 §7 검증)")
        for era, lab in (("old", "과거 리비전(2013~2016)"), ("head", "HEAD(2026)")):
            f = pst[era + "_files"]
            if not f: continue
            print(f"  [{lab}] .py {f}개")
            print(f"     ast 성공          : {pst[era+'_ast_ok']:>6} ({pst[era+'_ast_ok']/f*100:5.1f}%)")
            print(f"     ast SyntaxError   : {pst[era+'_ast_syntaxerror']:>6} "
                  f"({pst[era+'_ast_syntaxerror']/f*100:5.1f}%)  ← 심볼 전량 유실")
            print(f"     tree-sitter 무오류: {pst[era+'_ts_ok']:>6} ({pst[era+'_ts_ok']/f*100:5.1f}%)")
            print(f"     tree-sitter 부분  : {pst[era+'_ts_partial']:>6} "
                  f"({pst[era+'_ts_partial']/f*100:5.1f}%)  ← 심볼 일부 보존")
            print(f"     추출 심볼         : {pst[era+'_symbols']:>6}")
