#!/usr/bin/env python3
"""
M4 — Watcher Reconciliation 비용

리뷰 §5에서 이렇게 주장했다.

  "(path, mtime_ns, size) 튜플만 비교하는 스캔은 10만 파일에서 1초 미만이다.
   이 안전망이 없으면 '인덱스가 가끔 틀린 검색기'가 되는데, 그건 검색기로서 사망이다."

Watcher 이벤트는 유실된다(inotify 큐 오버플로, FSEvents coalescing, 대량 변경 폭풍).
따라서 주기적 정합성 스캔이 필수인데, 그 비용이 실용적인지 실측한다.

측정:
  1. 전체 트리 walk + stat (cold / warm 캐시)
  2. 인덱스 상태와의 diff 비용
  3. ignore 규칙 적용 효과 (.git, node_modules 등 제외)
  4. 병렬 스캔 효과
"""

import os, sys, time, json
from concurrent.futures import ThreadPoolExecutor

ROOT = sys.argv[1] if len(sys.argv) > 1 else "/tmp/corpus"
IGNORE_DIRS = {".git", "node_modules", ".venv", "venv", "__pycache__", "target",
               "build", "dist", ".mypy_cache", ".pytest_cache"}


def scan_serial(root, use_ignore=True):
    """os.scandir 기반 walk. DirEntry.stat()은 대부분의 OS에서 캐시된 값을 쓴다."""
    out = {}
    stack = [root]
    dirs = 0
    while stack:
        d = stack.pop()
        dirs += 1
        try:
            with os.scandir(d) as it:
                for e in it:
                    try:
                        if e.is_dir(follow_symlinks=False):
                            if use_ignore and e.name in IGNORE_DIRS:
                                continue
                            stack.append(e.path)
                        elif e.is_file(follow_symlinks=False):
                            st = e.stat(follow_symlinks=False)
                            out[e.path] = (st.st_mtime_ns, st.st_size)
                    except OSError:
                        continue
        except OSError:
            continue
    return out, dirs


def scan_parallel(root, workers, use_ignore=True):
    """최상위 서브디렉터리를 워커로 분배"""
    tops = []
    files = {}
    with os.scandir(root) as it:
        for e in it:
            if e.is_dir(follow_symlinks=False):
                if use_ignore and e.name in IGNORE_DIRS: continue
                tops.append(e.path)
            elif e.is_file(follow_symlinks=False):
                st = e.stat(follow_symlinks=False)
                files[e.path] = (st.st_mtime_ns, st.st_size)
    with ThreadPoolExecutor(max_workers=workers) as ex:
        for sub, _ in ex.map(lambda p: scan_serial(p, use_ignore), tops):
            files.update(sub)
    return files


def timed(fn, *a, **kw):
    t = time.perf_counter()
    r = fn(*a, **kw)
    return r, (time.perf_counter() - t) * 1000


print("### M4 — Watcher Reconciliation 비용\n")
print(f"대상: {ROOT}")

# --- 1. cold (페이지 캐시 비움은 컨테이너에서 불가하므로 첫 실행을 cold로 간주)
(res_cold, dirs), t_cold = timed(scan_serial, ROOT, True)
n = len(res_cold)
print(f"파일 {n:,}개, 디렉터리 {dirs:,}개 (ignore 규칙 적용)\n")

print(f"{'모드':<28}{'시간':>10}{'파일/ms':>10}")
print(f"{'1회차 (cold-ish)':<28}{t_cold:>9.0f}ms{n/t_cold:>10.1f}")

# --- 2. warm 반복
warm = []
for _ in range(3):
    (r, _), t = timed(scan_serial, ROOT, True)
    warm.append(t)
t_warm = min(warm)
print(f"{'2~4회차 최소 (warm)':<28}{t_warm:>9.0f}ms{n/t_warm:>10.1f}")

# --- 3. ignore 미적용
(r2, d2), t_noig = timed(scan_serial, ROOT, False)
print(f"{'ignore 미적용':<28}{t_noig:>9.0f}ms{len(r2)/t_noig:>10.1f}   (파일 {len(r2):,})")

# --- 4. 병렬
for w in (2, 4):
    r3, t_par = timed(scan_parallel, ROOT, w, True)
    print(f"{f'병렬 {w} 워커':<28}{t_par:>9.0f}ms{len(r3)/t_par:>10.1f}")

# --- 5. diff 비용 (인덱스 상태와 비교)
index = dict(res_cold)
def diff(cur, idx):
    added = cur.keys() - idx.keys()
    removed = idx.keys() - cur.keys()
    changed = [p for p in (cur.keys() & idx.keys()) if cur[p] != idx[p]]
    return added, removed, changed

_, t_diff_same = timed(diff, res_cold, index)
# 1% 변경 주입
import random
random.seed(7)
mut = dict(res_cold)
keys = list(mut)
for k in random.sample(keys, max(1, len(keys)//100)):
    mut[k] = (mut[k][0] + 1_000_000, mut[k][1] + 1)
(a, rm, ch), t_diff_mut = timed(diff, mut, index)
print()
print(f"{'diff (변경 없음)':<28}{t_diff_same:>9.1f}ms")
print(f"{'diff (1% 변경 주입)':<28}{t_diff_mut:>9.1f}ms   탐지 {len(ch):,}건")

# --- 6. 메모리
idx_bytes = sum(len(k.encode()) + 64 for k in index)   # 문자열 + 튜플 오버헤드 근사
print(f"\n인덱스 상태 메모리 근사: {idx_bytes/1048576:.1f} MB  ({n:,} 파일)")

# --- 7. 규모별 외삽
print(f"\n### 규모별 warm 스캔 시간 (실측 {n:,}개에서 선형 외삽)")
rate = n / t_warm
for size in (1_000, 10_000, 50_000, 100_000, 232_000):
    print(f"  {size:>7,} 파일 → {size/rate:>7.0f} ms")

json.dump({"files": n, "dirs": dirs, "t_cold_ms": t_cold, "t_warm_ms": t_warm,
           "t_noignore_ms": t_noig, "t_diff_ms": t_diff_mut,
           "index_mb": idx_bytes/1048576},
          open("/tmp/rn/m4_results.json", "w"), indent=1)
