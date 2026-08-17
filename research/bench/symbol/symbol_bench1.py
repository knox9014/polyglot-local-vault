#!/usr/bin/env python3
"""
가정 검증 #3 — 심볼 레벨 Stable ID 복구율 + 파서 선택 실측

실험 2는 파일 레벨만 측정했다. 그런데 리뷰 §2의 주소는 `path#symbol` 이다.
파일이 안 움직여도 함수가 이름이 바뀌거나 다른 파일로 옮겨가면 링크는 깨진다.
따라서 파일 레벨 95.7%는 상한이고, 실제 값은 그보다 낮다.

측정 A — 심볼 레벨 계단식 해석
  주소:  vault://path#Qualified.Name   (예: django/db/models/query.py#QuerySet.filter)
  S1. 같은 경로에 같은 qualname 존재            → HIT (비용 0)
  S2. git alias로 옮긴 경로에 같은 qualname     → HIT
  S3. vault 전체에서 qualname이 유일하게 존재   → 1클릭 확정
  S4. 본문 유사도로 이동/개명 추적 (Jaccard)    → 1클릭 확정
  S5. 실패                                     → BROKEN

  S4가 핵심 질문이다. git이 파일 rename을 유사도로 잡아주듯,
  심볼도 유사도 매칭이 필요한가? 필요하다면 얼마나 기여하는가?

측정 B — 파서 선택 (리뷰 §7 검증)
  리뷰 §7에서 "CPython ast는 문법 오류/구버전 문법에서 심볼을 전부 잃는다,
  tree-sitter는 부분 복구한다"고 주장했다.
  과거 리비전의 실제 코드로 두 파서의 파싱 성공률을 직접 비교한다.
"""

import subprocess, os, sys, ast, tempfile, shutil, tarfile, io
from collections import defaultdict, Counter
from tree_sitter import Language, Parser
import tree_sitter_python as tspy

PARSER = Parser(Language(tspy.language()))


def git(repo, *a, timeout=1800, binary=False):
    r = subprocess.run(["git", "-C", repo, *a], capture_output=True,
                       timeout=timeout, **({} if binary else {"text": True, "errors": "replace"}))
    return r.stdout


# ---------------------------------------------------------------------------
# 심볼 추출 (tree-sitter)
# ---------------------------------------------------------------------------
def extract_symbols(src: bytes):
    """(qualname, body_bytes) 리스트를 반환. class/def를 중첩 경로로 수식한다."""
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
            if t in ("decorated_definition",):
                walk(ch, prefix); continue
            if t in ("block", "if_statement", "try_statement", "with_statement", "module"):
                walk(ch, prefix)
    walk(tree.root_node, "")
    return out


def ts_has_error(src: bytes) -> bool:
    return PARSER.parse(src).root_node.has_error


# ---------------------------------------------------------------------------
# 본문 유사도 (토큰 shingle Jaccard)
# ---------------------------------------------------------------------------
def shingles(body: bytes, k=4):
    toks = []
    cur = []
    for b in body:
        c = chr(b)
        if c.isalnum() or c == "_":
            cur.append(c)
        else:
            if cur: toks.append("".join(cur)); cur = []
    if cur: toks.append("".join(cur))
    if len(toks) < k:
        return frozenset(["\x00".join(toks)]) if toks else frozenset()
    return frozenset("\x00".join(toks[i:i + k]) for i in range(len(toks) - k + 1))


def jaccard(a, b):
    if not a or not b: return 0.0
    i = len(a & b)
    return i / (len(a) + len(b) - i)


# ---------------------------------------------------------------------------
# 리비전 스냅샷 → 심볼 테이블
# ---------------------------------------------------------------------------
def snapshot_symbols(repo, rev, want_parser_stats=False):
    """{path: [(qualname, shingles)]}, 파서 통계"""
    raw = git(repo, "archive", rev, "*.py", binary=True)
    syms = {}
    stats = Counter()
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
        if want_parser_stats:
            # CPython ast
            try:
                ast.parse(data)
                stats["ast_ok"] += 1
            except SyntaxError:
                stats["ast_syntaxerror"] += 1
            except Exception:
                stats["ast_other"] += 1
            # tree-sitter
            if ts_has_error(data):
                stats["ts_partial"] += 1
            else:
                stats["ts_ok"] += 1
        try:
            ss = extract_symbols(data)
        except Exception:
            stats["ts_extract_fail"] += 1
            continue
        if ss:
            syms[m.name] = [(q, shingles(b)) for q, b in ss]
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


SIM_THRESHOLD = 0.60


def analyze(repo, name, frac, parser_stats=False):
    total = int(git(repo, "rev-list", "--count", "HEAD").strip())
    rev = git(repo, "rev-list", "HEAD", f"--skip={int(total*frac)}", "--max-count=1").strip()
    date = git(repo, "log", "-1", "--format=%ad", "--date=short", rev).strip()

    old, ostat = snapshot_symbols(repo, rev, parser_stats)
    head, hstat = snapshot_symbols(repo, "HEAD", parser_stats)
    alias = build_alias(repo)

    # HEAD 인덱스
    head_by_path = {p: {q for q, _ in v} for p, v in head.items()}
    head_by_qual = defaultdict(list)      # qualname -> [path]
    head_by_leaf = defaultdict(list)      # 마지막 이름 -> [(path, qual)]
    shingle_index = defaultdict(list)     # shingle -> [(path, qual, sh)]
    head_items = []
    for p, v in head.items():
        for q, sh in v:
            head_by_qual[q].append(p)
            head_by_leaf[q.split(".")[-1]].append((p, q))
            head_items.append((p, q, sh))
    # 유사도 후보 인덱스는 크기를 제한한다 (흔한 shingle 제외)
    for idx, (p, q, sh) in enumerate(head_items):
        for s in list(sh)[:80]:
            shingle_index[s].append(idx)

    c = Counter()
    sim_hits_ok = 0
    for p, v in old.items():
        newp = alias.get(p, p)
        for q, sh in v:
            # S1
            if q in head_by_path.get(p, ()):
                c["S1"] += 1; continue
            # S2
            if newp != p and q in head_by_path.get(newp, ()):
                c["S2"] += 1; continue
            # S3: qualname이 vault 전체에서 유일
            cand = head_by_qual.get(q, [])
            if len(cand) == 1:
                c["S3_unique"] += 1; continue
            if len(cand) > 1:
                c["S3_ambiguous"] += 1; continue
            # S4: 본문 유사도
            votes = Counter()
            for s in list(sh)[:80]:
                for idx in shingle_index.get(s, ())[:40]:
                    votes[idx] += 1
            best, bests = None, 0.0
            for idx, _ in votes.most_common(30):
                hp, hq, hsh = head_items[idx]
                j = jaccard(sh, hsh)
                if j > bests:
                    bests, best = j, (hp, hq)
            if best and bests >= SIM_THRESHOLD:
                c["S4_similarity"] += 1
                # 이름이 유지된 채 파일만 이동한 경우인지 확인
                if best[1].split(".")[-1] == q.split(".")[-1]:
                    sim_hits_ok += 1
                continue
            c["S5_broken"] += 1

    c["old_symbols"] = sum(len(v) for v in old.values())
    c["head_symbols"] = len(head_items)
    return dict(name=name, date=date, rev=rev[:8], c=c, sim_name_kept=sim_hits_ok,
                ostat=ostat, hstat=hstat)


REPOS = [("/tmp/rn/django", "django"), ("/tmp/rn/scikit-learn", "scikit-learn"),
         ("/tmp/rn/flask", "flask"), ("/tmp/rn/requests", "requests")]

if __name__ == "__main__":
    only = sys.argv[1] if len(sys.argv) > 1 else None
    fracs = [(0.25, "25%"), (0.5, "50%"), (0.75, "75%")]

    print("### 심볼 레벨 계단식 해석 복구율\n")
    print("S1 같은경로 동일심볼 / S2 git alias 경로 / S3u qualname 유일 / S3a 모호")
    print("S4 본문 유사도(Jaccard>=%.2f) / S5 실패\n" % SIM_THRESHOLD)
    print(f"{'repo':<14}{'경과':<6}{'기준일':<12}{'심볼수':>8}{'S1':>8}{'S2':>6}{'S3u':>6}"
          f"{'S3a':>6}{'S4':>6}{'S5':>7}  {'S1+S2':>7} {'+S3u+S4':>8}")

    agg = Counter()
    pstat = Counter()
    for repo, name in REPOS:
        if only and only != name: continue
        if not os.path.isdir(repo): continue
        for frac, lab in fracs:
            r = analyze(repo, name, frac, parser_stats=(frac == 0.5))
            c = r["c"]
            n = c["old_symbols"]
            if n == 0: continue
            auto = (c["S1"] + c["S2"]) / n * 100
            plus = (c["S1"] + c["S2"] + c["S3_unique"] + c["S4_similarity"]) / n * 100
            print(f"{name:<14}{lab:<6}{r['date']:<12}{n:>8}{c['S1']:>8}{c['S2']:>6}"
                  f"{c['S3_unique']:>6}{c['S3_ambiguous']:>6}{c['S4_similarity']:>6}"
                  f"{c['S5_broken']:>7}  {auto:>6.1f}% {plus:>7.1f}%")
            if frac == 0.5:
                for k in ("S1","S2","S3_unique","S3_ambiguous","S4_similarity","S5_broken","old_symbols"):
                    agg[k] += c[k]
                agg["sim_name_kept"] += r["sim_name_kept"]
                for k, v in r["ostat"].items(): pstat["old_" + k] += v
                for k, v in r["hstat"].items(): pstat["head_" + k] += v
        print()

    n = agg["old_symbols"]
    if n:
        print("### 종합 (50% 경과 기준)")
        print(f"  과거 심볼 링크 총계 : {n}")
        for k, lab in [("S1","S1 같은 경로 동일 심볼"), ("S2","S2 git alias 경로"),
                       ("S3_unique","S3 qualname 유일"), ("S3_ambiguous","S3 qualname 모호"),
                       ("S4_similarity","S4 본문 유사도 매칭"), ("S5_broken","S5 복구 불가")]:
            print(f"  {lab:<24}: {agg[k]:>6} ({agg[k]/n*100:5.1f}%)")
        print()
        print(f"  자동 복구 (S1+S2)          = {(agg['S1']+agg['S2'])/n*100:.1f}%")
        print(f"  +1클릭 (S3u+S4 포함)       = "
              f"{(agg['S1']+agg['S2']+agg['S3_unique']+agg['S4_similarity'])/n*100:.1f}%")
        print(f"  S4 중 이름 유지(순수 이동) = {agg['sim_name_kept']}/{agg['S4_similarity']}")
        print()

    if pstat:
        print("### 파서 비교 — CPython ast vs tree-sitter (리뷰 §7 검증)")
        for era in ("old", "head"):
            f = pstat[era + "_files"]
            if not f: continue
            print(f"  [{'과거 리비전' if era=='old' else 'HEAD'}] .py 파일 {f}개")
            print(f"     ast 파싱 성공        : {pstat[era+'_ast_ok']:>6} ({pstat[era+'_ast_ok']/f*100:5.1f}%)")
            print(f"     ast SyntaxError      : {pstat[era+'_ast_syntaxerror']:>6} "
                  f"({pstat[era+'_ast_syntaxerror']/f*100:5.1f}%)  ← 심볼 전량 유실")
            print(f"     tree-sitter 무오류   : {pstat[era+'_ts_ok']:>6} ({pstat[era+'_ts_ok']/f*100:5.1f}%)")
            print(f"     tree-sitter 부분복구 : {pstat[era+'_ts_partial']:>6} "
                  f"({pstat[era+'_ts_partial']/f*100:5.1f}%)  ← 심볼 일부 보존")
            print(f"     추출된 심볼          : {pstat[era+'_symbols']:>6}")
