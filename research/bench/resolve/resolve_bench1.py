#!/usr/bin/env python3
"""
가정 검증 #2 — Stable ID 계단식 해석(cascade)의 실제 복구율

리뷰 §2에서 제안한 전략:
    링크는 논리 주소(vault://path#symbol)로 저장하고, 조회 시점에 계단식 해석한다.
      L1. 경로 그대로 존재            → HIT (비용 0)
      L2. alias 테이블(git rename)    → HIT
      L3. basename 유일 매칭          → 후보 제시 (사용자 1클릭)
      L4. 전부 실패                   → BROKEN 표시 (링크는 보존)

이 전략이 실제로 몇 %를 복구하는지, 실제 저장소의 실제 히스토리로 측정한다.

방법:
  - 과거 시점 T0의 파일 목록을 스냅샷으로 잡는다 (= 그 때 사용자가 링크를 걸었다고 가정)
  - HEAD 시점에 각 경로가 어떻게 되었는지 위 계단으로 해석
  - L1/L2/L3/L4 비율을 집계

L2의 alias는 git의 rename detection(-M)으로 히스토리 전체에서 미리 구축한다.
이는 실제 구현이 최초 인덱싱 시 한 번 수행할 수 있는 작업이다.
"""

import subprocess
import sys
import os
from collections import defaultdict, Counter


def git(repo, *args, timeout=1200):
    r = subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True,
                       timeout=timeout, errors="replace")
    return r.stdout


def build_alias_map(repo):
    """히스토리 전체의 rename 체인을 수집해 old_path -> new_path 매핑을 만든다.

    git log는 최신→과거 순이므로, 각 rename R old new 를 만나면
    'old는 결국 new로 갔다'를 기록한다. 체인은 나중에 압축한다.
    """
    out = git(repo, "log", "--all", "--diff-filter=R", "--name-status",
              "-M50%", "--format=%x00%H", "--no-renames" if False else "-M")
    direct = {}
    n_renames = 0
    for line in out.splitlines():
        if not line or line.startswith("\x00"):
            continue
        parts = line.split("\t")
        if len(parts) == 3 and parts[0].startswith("R"):
            _, old, new = parts
            # 최신 커밋부터 보므로 old->new는 마지막(=가장 과거) 기록을 남긴다
            direct[old] = new
            n_renames += 1
    # 체인 압축: a->b->c 를 a->c 로
    resolved = {}

    def follow(p, depth=0):
        if p in resolved:
            return resolved[p]
        if depth > 64 or p not in direct:
            return p
        r = follow(direct[p], depth + 1)
        resolved[p] = r
        return r

    return {k: follow(k) for k in direct}, n_renames


def snapshot(repo, rev):
    out = git(repo, "ls-tree", "-r", "--name-only", rev)
    return [l for l in out.splitlines() if l]


def pick_old_rev(repo, frac):
    total = int(git(repo, "rev-list", "--count", "HEAD").strip())
    skip = int(total * frac)
    out = git(repo, "rev-list", "HEAD", f"--skip={skip}", "--max-count=1").strip()
    return out, total


def analyze(repo, name, frac):
    old_rev, total = pick_old_rev(repo, frac)
    if not old_rev:
        return None
    old_files = snapshot(repo, old_rev)
    head_files = snapshot(repo, "HEAD")
    head_set = set(head_files)

    basename_index = defaultdict(list)
    for p in head_files:
        basename_index[os.path.basename(p)].append(p)

    alias, n_ren = build_alias_map(repo)

    counts = Counter()
    l3_wrong = 0
    for p in old_files:
        if p in head_set:
            counts["L1_path"] += 1
            continue
        a = alias.get(p)
        if a and a in head_set:
            counts["L2_alias"] += 1
            continue
        cands = basename_index.get(os.path.basename(p), [])
        if len(cands) == 1:
            counts["L3_basename_unique"] += 1
            # 정답 검증: alias가 알려주는 정답과 다르면 오답 제안
            if a and a in head_set and cands[0] != a:
                l3_wrong += 1
            continue
        if len(cands) > 1:
            counts["L3_basename_ambiguous"] += 1
            continue
        counts["L4_broken"] += 1

    n = len(old_files)
    date = git(repo, "log", "-1", "--format=%ad", "--date=short", old_rev).strip()
    return {
        "repo": name, "old_rev": old_rev[:8], "old_date": date,
        "commits_total": total, "commits_elapsed": int(total * frac),
        "old_files": n, "head_files": len(head_files),
        "renames_detected": n_ren, "counts": counts, "l3_wrong": l3_wrong,
    }


def main():
    repos = [("/tmp/rn/django", "django"), ("/tmp/rn/scikit-learn", "scikit-learn"),
             ("/tmp/rn/flask", "flask"), ("/tmp/rn/requests", "requests")]
    fracs = [(0.25, "최근 25% 커밋 경과"), (0.50, "50% 경과"), (0.75, "75% 경과")]

    print("### Stable ID 계단식 해석 복구율 (실제 git 히스토리)\n")
    print("L1 = 경로 그대로 존재 / L2 = git rename alias로 복구 / "
          "L3u = basename 유일매칭(1클릭 확정) / L3a = basename 모호 / L4 = 완전 실패\n")

    agg = Counter()
    for repo, name in repos:
        if not os.path.isdir(repo):
            continue
        print(f"── {name}")
        print(f"   {'경과':<16} {'대상':>7} {'L1':>7} {'L2':>6} {'L3u':>6} {'L3a':>6} "
              f"{'L4':>6}   {'자동복구':>8} {'+1클릭':>8}")
        for frac, label in fracs:
            r = analyze(repo, name, frac)
            if not r:
                continue
            c = r["counts"]
            n = r["old_files"]
            l1, l2 = c["L1_path"], c["L2_alias"]
            l3u, l3a, l4 = c["L3_basename_unique"], c["L3_basename_ambiguous"], c["L4_broken"]
            auto = (l1 + l2) / n * 100
            plus = (l1 + l2 + l3u) / n * 100
            print(f"   {label:<16} {n:>7} {l1:>7} {l2:>6} {l3u:>6} {l3a:>6} {l4:>6}   "
                  f"{auto:>7.1f}% {plus:>7.1f}%")
            if frac == 0.50:
                agg["n"] += n
                agg["l1"] += l1
                agg["l2"] += l2
                agg["l3u"] += l3u
                agg["l3a"] += l3a
                agg["l4"] += l4
                agg["l3_wrong"] += r["l3_wrong"]
        r = analyze(repo, name, 0.50)
        print(f"   (기준점 {r['old_date']}, 총 {r['commits_total']} 커밋, "
              f"git이 탐지한 rename {r['renames_detected']}건)\n")

    n = agg["n"]
    print("### 종합 (50% 경과 기준)")
    print(f"   대상 링크           : {n}")
    print(f"   L1 경로 유지        : {agg['l1']:>6}  ({agg['l1']/n*100:.1f}%)")
    print(f"   L2 alias 자동복구   : {agg['l2']:>6}  ({agg['l2']/n*100:.1f}%)")
    print(f"   L3 유일 basename    : {agg['l3u']:>6}  ({agg['l3u']/n*100:.1f}%)  ← 1클릭 확정")
    print(f"   L3 모호             : {agg['l3a']:>6}  ({agg['l3a']/n*100:.1f}%)  ← 후보 목록 제시")
    print(f"   L4 복구 불가        : {agg['l4']:>6}  ({agg['l4']/n*100:.1f}%)  ← BROKEN 표시")
    print()
    print(f"   자동 복구율(L1+L2)      = {(agg['l1']+agg['l2'])/n*100:.1f}%")
    print(f"   1클릭 포함 복구율(+L3u) = {(agg['l1']+agg['l2']+agg['l3u'])/n*100:.1f}%")
    print(f"   L3 오답 제안            = {agg['l3_wrong']}건 "
          f"({agg['l3_wrong']/max(agg['l3u'],1)*100:.1f}% of L3u)")


if __name__ == "__main__":
    main()
