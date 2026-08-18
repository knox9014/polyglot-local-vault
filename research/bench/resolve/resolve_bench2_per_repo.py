#!/usr/bin/env python3
"""
가정 검증 #2 — 저장소별 파일 링크 복구율 하한

resolve_bench2_corrected.py 는 4개 저장소를 합산해 95.7%(게이트 93%)를 냈다.
심볼 쪽(N1, 17_MEASUREMENT_BASIS.md)에서 이미 드러났듯 합산은 심볼/파일 수가
많은 저장소에 끌려간다 — 저장소별로 갈라보면 다른 그림이 나올 수 있다.

측정 로직(git/build_alias/deleted_set/analyze)은 resolve_bench2_corrected.py
와 완전히 동일하다 — 다른 스크립트를 만드는 게 아니라 같은 걸 다르게 집계하는
것이므로, 알고리즘을 새로 짜지 않고 그대로 옮겼다. 다른 점은 저장소를 합산하지
않고 저장소별 hit/live 비율을 그대로 보여주는 것뿐이다.

지표 정의는 17 §"링크 복구율"과 동일: L1(경로 유지) + L2(git alias) 를
"자동 복구"로 센다. L3u(basename 유일)는 기각된 계단(→ CLAUDE.md "기각된
설계")이라 자동 복구에 포함하지 않는다.
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
        if p in dels and not a:
            c["DELETED"] += 1; continue
        cand = bidx.get(os.path.basename(p), [])
        if len(cand) == 1: c["L3u"] += 1
        elif len(cand) > 1: c["L3a"] += 1
        else: c["L4"] += 1
    date = git(repo, "log", "-1", "--format=%ad", "--date=short", rev).strip()
    return name, date, len(old), c


CORPUS_ROOT = sys.argv[1] if len(sys.argv) > 1 else r"C:\Users\seong\vault-corpus"
REPOS = [(os.path.join(CORPUS_ROOT, "django"), "django"),
         (os.path.join(CORPUS_ROOT, "scikit-learn"), "scikit-learn"),
         (os.path.join(CORPUS_ROOT, "flask"), "flask"),
         (os.path.join(CORPUS_ROOT, "requests"), "requests")]

print("### 파일 링크 자동 복구율 — 저장소별 (50% 경과 기준, PINNED_COMMITS.txt SHA)\n")
print(f"{'repo':<16}{'과거파일':>8}{'L1':>7}{'L2':>6}{'L3u':>5}{'L3a':>5}{'L4':>5}"
      f"{'DEL':>6}   {'live':>6}  {'자동복구율(L1+L2)/live':>22}")

floors = []
for repo, name in REPOS:
    if not os.path.isdir(repo):
        print(f"  {name}: 경로 없음 ({repo}) — 건너뜀", file=sys.stderr)
        continue
    nm, date, n, c = analyze(repo, name, 0.5)
    live = n - c["DELETED"]
    hit = c["L1"] + c["L2"]
    rate = hit / live * 100 if live else 0.0
    floors.append((name, rate))
    print(f"{nm:<16}{n:>8}{c['L1']:>7}{c['L2']:>6}{c['L3u']:>5}{c['L3a']:>5}{c['L4']:>5}"
          f"{c['DELETED']:>6}   {live:>6}  {rate:>20.1f}%")

print()
if floors:
    worst_name, worst_rate = min(floors, key=lambda t: t[1])
    print(f"### 저장소별 하한: {worst_name} {worst_rate:.1f}%")
    for margin in (2.0, 3.0):
        print(f"  게이트 후보(여유 {margin:.1f}%p): > {worst_rate - margin:.1f}%")
