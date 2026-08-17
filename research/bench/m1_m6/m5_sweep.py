#!/usr/bin/env python3
"""
M5 — S4 유사도 파라미터 스윕

실험 3에서 shingle 4-gram / 스케치 96 / 임계 0.60 은 첫 시도값이었다.
튜닝 여지를 확인한다.

정밀도 측정 방법 (중요):
  S3u로 해결된 심볼들은 "qualname이 vault 전역에서 유일" 하므로 정답을 안다.
  이 집합을 정답지로 삼아, S4 매처가 같은 대상을 고르는지 본다.
     precision = S4가 S3u 정답과 일치한 비율
  이렇게 하면 사람 검수 없이 정밀도를 정량화할 수 있다.

동시에 재현율 대용으로 S4가 실제 복구한 건수(S3에서 못 잡은 것)를 센다.
"""

import subprocess, io, tarfile, zlib, heapq, sys, json, itertools
from collections import defaultdict, Counter
from tree_sitter import Language, Parser
import tree_sitter_python as tspy

PARSER = Parser(Language(tspy.language()))
REPO = "/tmp/rn/django"
FRAC = 0.50


def git(repo, *a, binary=False):
    r = subprocess.run(["git", "-C", repo, *a], capture_output=True, timeout=1800,
                       **({} if binary else {"text": True, "errors": "replace"}))
    return r.stdout


def extract(src):
    tree = PARSER.parse(src); out = []
    def nm(n):
        c = n.child_by_field_name("name")
        return src[c.start_byte:c.end_byte].decode("utf8", "replace") if c else None
    def walk(n, p):
        for ch in n.named_children:
            if ch.type in ("class_definition", "function_definition"):
                x = nm(ch)
                if x:
                    q = f"{p}.{x}" if p else x
                    out.append((q, src[ch.start_byte:ch.end_byte])); walk(ch, q); continue
            if ch.type == "decorated_definition": walk(ch, p); continue
            if ch.type in ("block","if_statement","try_statement","with_statement","module"): walk(ch, p)
    walk(tree.root_node, "")
    return out


def tokenize(b):
    t, cur = [], []
    for x in b:
        c = chr(x)
        if c.isalnum() or c == "_": cur.append(c)
        else:
            if cur: t.append("".join(cur)); cur = []
    if cur: t.append("".join(cur))
    return t


def shingles(toks, k):
    if len(toks) < k:
        return {zlib.crc32("\x00".join(toks).encode())} if toks else set()
    return {zlib.crc32("\x00".join(toks[i:i+k]).encode()) for i in range(len(toks)-k+1)}


def jac(a, b):
    if not a or not b: return 0.0
    i = len(a & b); return i / (len(a)+len(b)-i)


def snapshot(rev):
    raw = git(REPO, "archive", rev, "*.py", binary=True)
    out = {}
    tf = tarfile.open(fileobj=io.BytesIO(raw))
    for m in tf.getmembers():
        if not m.isfile() or not m.name.endswith(".py"): continue
        try: d = tf.extractfile(m).read()
        except Exception: continue
        ss = extract(d)
        if ss: out[m.name] = [(q, tokenize(b)) for q, b in ss]
    return out


def build_alias():
    o = git(REPO, "log", "--all", "--diff-filter=R", "--name-status", "-M", "--format=")
    d = {}
    for line in o.splitlines():
        p = line.split("\t")
        if len(p) == 3 and p[0].startswith("R"): d[p[1]] = p[2]
    memo = {}
    def f(p, k=0):
        if p in memo: return memo[p]
        if k > 64 or p not in d: return p
        r = f(d[p], k+1); memo[p] = r; return r
    return {k: f(k) for k in d}


total = int(git(REPO, "rev-list", "--count", "HEAD").strip())
rev = git(REPO, "rev-list", "HEAD", f"--skip={int(total*FRAC)}", "--max-count=1").strip()
print(f"### M5 — S4 유사도 파라미터 스윕  (django @ {git(REPO,'log','-1','--format=%ad','--date=short',rev).strip()})\n")

old = snapshot(rev); head = snapshot("HEAD"); alias = build_alias()
head_by_path = {p: {q for q, _ in v} for p, v in head.items()}
head_by_qual = defaultdict(list)
head_items_tok = []
for p, v in head.items():
    for q, tk in v:
        head_by_qual[q].append(p); head_items_tok.append((p, q, tk))

# S1/S2/S3로 분류해 두고, S4가 필요한 집합(unmatched)과 정답지(S3u)를 만든다
unmatched, oracle = [], []
for p, v in old.items():
    np_ = alias.get(p, p)
    for q, tk in v:
        if q in head_by_path.get(p, ()): continue
        if np_ != p and q in head_by_path.get(np_, ()): continue
        cand = head_by_qual.get(q, [])
        if len(cand) == 1:
            oracle.append((q, tk, cand[0]))      # 정답: 이 경로의 이 qualname
        elif len(cand) == 0:
            unmatched.append((q, tk))
print(f"S4 대상(qualname 소멸) = {len(unmatched)},  정답지(S3u) = {len(oracle)}\n")

print(f"{'k':>3}{'sketch':>8}{'thr':>7}{'index':>9}{'S4 복구':>9}{'정밀도':>9}{'정답지 재현':>12}")
rows = []
for k in (3, 4, 5):
    hs_head = [(p, q, shingles(tk, k)) for p, q, tk in head_items_tok]
    hs_un = [(q, shingles(tk, k)) for q, tk in unmatched]
    hs_or = [(q, shingles(tk, k), tgt) for q, tk, tgt in oracle]
    for sk in (32, 96, 256):
        def sketch(s): return heapq.nsmallest(sk, s) if len(s) > sk else sorted(s)
        inv = defaultdict(list)
        for i, (_, _, s) in enumerate(hs_head):
            for h in sketch(s): inv[h].append(i)
        idx_entries = sum(len(v) for v in inv.values())

        def best(s):
            votes = Counter()
            for h in sketch(s):
                post = inv.get(h)
                if post and len(post) <= 200:
                    for i in post: votes[i] += 1
            bi, bs = None, 0.0
            for i, _ in votes.most_common(40):
                j = jac(s, hs_head[i][2])
                if j > bs: bs, bi = j, i
            return bi, bs

        best_un = [best(s) for _, s in hs_un]
        best_or = [(best(s), tgt, q) for q, s, tgt in hs_or]

        for thr in (0.40, 0.50, 0.60, 0.70, 0.80):
            rec = sum(1 for bi, bs in best_un if bs >= thr)
            fired = [(bi, bs, tgt, q) for (bi, bs), tgt, q in best_or if bs >= thr]
            ok = sum(1 for bi, bs, tgt, q in fired
                     if bi is not None and hs_head[bi][0] == tgt and hs_head[bi][1] == q)
            prec = ok / len(fired) * 100 if fired else float("nan")
            orc = len(fired) / len(hs_or) * 100 if hs_or else 0
            rows.append(dict(k=k, sketch=sk, thr=thr, recovered=rec, precision=prec,
                             oracle_fire=orc, index=idx_entries))
            print(f"{k:>3}{sk:>8}{thr:>7.2f}{idx_entries:>9,}{rec:>9}"
                  f"{prec:>8.1f}%{orc:>11.1f}%")
        sys.stdout.flush()

json.dump(rows, open("/tmp/rn/m5_results.json", "w"), indent=1)

print("\n### 해석")
print("  S4 복구     = qualname이 사라진 심볼 중 유사도로 건진 수 (재현)")
print("  정밀도      = 정답을 아는 S3u 집합에서 S4가 같은 대상을 고른 비율")
print("  정답지 재현 = S3u 집합에서 S4가 임계값을 넘겨 발화한 비율")
best_row = max((r for r in rows if r["precision"] == r["precision"] and r["precision"] >= 95),
               key=lambda r: r["recovered"], default=None)
if best_row:
    print(f"\n  >>> 정밀도 95% 이상 중 복구 최대: k={best_row['k']}, sketch={best_row['sketch']}, "
          f"thr={best_row['thr']:.2f} → 복구 {best_row['recovered']}건, "
          f"정밀도 {best_row['precision']:.1f}%")
