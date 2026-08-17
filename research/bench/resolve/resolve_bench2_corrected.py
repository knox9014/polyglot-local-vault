#!/usr/bin/env python3
"""
가정 검증 #2b — 분모 정정판

1차 측정의 결함: L4(복구 불가) 안에 "실제로 삭제된 파일"이 섞여 있었다.
삭제된 파일을 가리키던 링크가 깨지는 것은 전략의 실패가 아니라 정상 동작이다.
(리뷰 §2의 L4 정의 자체가 "링크를 보존하고 BROKEN으로 표시" 이므로 의도된 결과)

따라서 두 지표를 분리한다.

  지표 A (전체 링크 관점) : 과거에 건 링크 중 지금도 유효한 비율
  지표 B (전략 관점)      : "대상이 여전히 존재하는" 링크만을 분모로 한 복구율
                           ← 이것이 Stable ID 전략의 실제 성능이다

지표 B의 분모 = 과거 파일 중, git이 HEAD에 살아있는 무언가로 추적 가능한 것.
"""

import subprocess, os, sys
from collections import defaultdict, Counter


def git(repo, *a, timeout=1800):
    return subprocess.run(["git", "-C", repo, *a], capture_output=True, text=True,
                          timeout=timeout, errors="replace").stdout


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


def deleted_set(repo):
    """히스토리에서 삭제(D)된 적이 있고 HEAD에 없는 경로."""
    out = git(repo, "log", "--all", "--diff-filter=D", "--name-only", "--format=")
    return {l for l in out.splitlines() if l}


def analyze(repo, name, frac):
    total = int(git(repo, "rev-list", "--count", "HEAD").strip())
    rev = git(repo, "rev-list", "HEAD", f"--skip={int(total*frac)}", "--max-count=1").strip()
    old = [l for l in git(repo, "ls-tree", "-r", "--name-only", rev).splitlines() if l]
    head = [l for l in git(repo, "ls-tree", "-r", "--name-only", "HEAD").splitlines() if l]
    hset = set(head)
    bidx = defaultdict(list)
    for p in head: bidx[os.path.basename(p)].append(p)
    alias = build_alias(repo)
    dels = deleted_set(repo)

    c = Counter()
    for p in old:
        if p in hset:
            c["L1"] += 1; continue
        a = alias.get(p)
        if a and a in hset:
            c["L2"] += 1; continue
        # 대상이 실제로 삭제되었는가?
        if p in dels and not a:
            c["DELETED"] += 1; continue
        cand = bidx.get(os.path.basename(p), [])
        if len(cand) == 1: c["L3u"] += 1
        elif len(cand) > 1: c["L3a"] += 1
        else: c["L4"] += 1
    date = git(repo, "log", "-1", "--format=%ad", "--date=short", rev).strip()
    return name, date, len(old), c


REPOS = [("/tmp/rn/django", "django"), ("/tmp/rn/scikit-learn", "scikit-learn"),
         ("/tmp/rn/flask", "flask"), ("/tmp/rn/requests", "requests")]

print("### Stable ID 계단식 해석 — 분모 정정판\n")
print("DELETED = 대상 파일이 실제로 삭제됨 (링크가 깨지는 것이 정상 동작, 분모에서 제외)\n")
print(f"{'repo':<14}{'경과':<8}{'기준일':<12}{'과거파일':>8}{'L1':>7}{'L2':>6}{'L3u':>5}{'L3a':>5}{'L4':>5}"
      f"{'DEL':>6}  {'지표A':>7} {'지표B':>7}")

agg = Counter()
for repo, name in REPOS:
    if not os.path.isdir(repo): continue
    for frac, lab in [(0.25, "25%"), (0.5, "50%"), (0.75, "75%")]:
        nm, date, n, c = analyze(repo, name, frac)
        live = n - c["DELETED"]                      # 지표B 분모
        A = (c["L1"] + c["L2"]) / n * 100
        B = (c["L1"] + c["L2"]) / live * 100 if live else 0
        print(f"{nm:<14}{lab:<8}{date:<12}{n:>8}{c['L1']:>7}{c['L2']:>6}{c['L3u']:>5}"
              f"{c['L3a']:>5}{c['L4']:>5}{c['DELETED']:>6}  {A:>6.1f}% {B:>6.1f}%")
        if frac == 0.5:
            for k in ("L1", "L2", "L3u", "L3a", "L4", "DELETED"): agg[k] += c[k]
            agg["n"] += n
    print()

n, live = agg["n"], agg["n"] - agg["DELETED"]
hit = agg["L1"] + agg["L2"]
print("### 종합 (50% 경과 기준)")
print(f"  과거 링크 대상 총계     : {n}")
print(f"  그중 실제 삭제됨        : {agg['DELETED']} ({agg['DELETED']/n*100:.1f}%)  ← BROKEN이 정상")
print(f"  대상이 살아있는 링크    : {live}")
print()
print(f"  L1 경로 유지            : {agg['L1']:>5} ({agg['L1']/live*100:.1f}% of live)")
print(f"  L2 git alias 복구       : {agg['L2']:>5} ({agg['L2']/live*100:.1f}% of live)")
print(f"  L3u basename 유일       : {agg['L3u']:>5} ({agg['L3u']/live*100:.1f}%)")
print(f"  L3a basename 모호       : {agg['L3a']:>5} ({agg['L3a']/live*100:.1f}%)")
print(f"  L4 단서 없음            : {agg['L4']:>5} ({agg['L4']/live*100:.1f}%)")
print()
print(f"  >>> 지표A 전체 유효율   = {hit/n*100:.1f}%")
print(f"  >>> 지표B 전략 복구율   = {hit/live*100:.1f}%   (L3u 포함 시 {(hit+agg['L3u'])/live*100:.1f}%)")
