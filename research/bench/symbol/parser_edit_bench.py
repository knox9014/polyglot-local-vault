#!/usr/bin/env python3
"""
가정 검증 #4 — 편집 중 파일에서의 파서 복원력 (리뷰 §7 재검증)

앞선 측정에서 커밋된 코드에 대한 ast SyntaxError는 0.1~0.2%에 불과했다.
즉 "과거 코드라서 ast가 실패한다"는 내 §7 주장은 지지되지 않았다.

그런데 그 측정은 §7이 실제로 걱정한 상황을 재현하지 않았다.
커밋된 코드는 정의상 문법이 유효하다. §7의 논지는 이것이었다.

  "사용자는 편집 중인 파일을 저장한다. 편집 중인 파일은 절반은 문법 오류 상태다.
   CPython ast는 그 순간 심볼을 전부 잃는다 → Related 패널이 깜빡인다."

이 실험은 그 상황을 직접 재현한다.
실제 파일에 실제 편집 중 상태를 주입하고, 두 파서가 심볼을 몇 개나 건지는지 센다.

주입하는 편집 중 상태 (모두 실제 타이핑 중 흔히 발생):
  T1  파일 끝에서 새 함수를 타이핑 중       "def process_" 까지만
  T2  괄호를 열고 인자를 타이핑 중          "foo(a, b" 에서 멈춤
  T3  문자열을 열고 타이핑 중               따옴표 미종료
  T4  블록 헤더만 치고 본문 미작성          "if x:" 다음이 비어 있음
  T5  중간 줄 삭제 (잘라내기 직후)          임의의 줄 하나 제거
  T6  들여쓰기 붕괴 (블록 이동 중)          임의 줄의 들여쓰기 제거
"""

import subprocess, io, tarfile, ast, random, sys
from collections import Counter
from tree_sitter import Language, Parser
import tree_sitter_python as tspy

PARSER = Parser(Language(tspy.language()))
random.seed(20260816)


def git(repo, *a, binary=False):
    r = subprocess.run(["git", "-C", repo, *a], capture_output=True, timeout=1200,
                       **({} if binary else {"text": True, "errors": "replace"}))
    return r.stdout


def ts_symbols(src: bytes):
    tree = PARSER.parse(src)
    out = []
    def name_of(n):
        c = n.child_by_field_name("name")
        return src[c.start_byte:c.end_byte].decode("utf8", "replace") if c else None
    def walk(node, prefix):
        for ch in node.named_children:
            if ch.type in ("class_definition", "function_definition"):
                nm = name_of(ch)
                if nm:
                    q = f"{prefix}.{nm}" if prefix else nm
                    out.append(q); walk(ch, q); continue
            if ch.type == "decorated_definition":
                walk(ch, prefix); continue
            if ch.type in ("block", "if_statement", "try_statement", "with_statement", "module"):
                walk(ch, prefix)
    walk(tree.root_node, "")
    return set(out)


def ast_symbols(src: bytes):
    """CPython ast — 파싱 실패 시 심볼 0개 (이것이 핵심)"""
    try:
        tree = ast.parse(src)
    except Exception:
        return None          # None = 파싱 자체 실패
    out = set()
    def walk(node, prefix):
        for ch in ast.iter_child_nodes(node):
            if isinstance(ch, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
                q = f"{prefix}.{ch.name}" if prefix else ch.name
                out.add(q); walk(ch, q)
            else:
                walk(ch, prefix)
    walk(tree, "")
    return out


# ---------------------------------------------------------------------------
# 편집 중 상태 주입
# ---------------------------------------------------------------------------
def mutate(src: bytes, kind: str):
    lines = src.split(b"\n")
    if len(lines) < 8:
        return None
    if kind == "T1":   # 파일 끝에서 새 함수 타이핑 중
        return src + b"\n\ndef process_"
    if kind == "T2":   # 괄호 열고 인자 타이핑 중
        return src + b"\n\ndef handle(request, context"
    if kind == "T3":   # 문자열 미종료
        return src + '\n\nMESSAGE = "처리 중인 항목: '.encode("utf8")
    if kind == "T4":   # 블록 헤더만
        return src + b"\n\nif settings.DEBUG:\n"
    if kind == "T5":   # 중간 줄 삭제
        i = random.randrange(len(lines) // 4, max(len(lines) // 4 + 1, len(lines) - 2))
        return b"\n".join(lines[:i] + lines[i + 1:])
    if kind == "T6":   # 들여쓰기 붕괴
        idxs = [i for i, l in enumerate(lines) if l.startswith(b"    ") and l.strip()]
        if not idxs: return None
        i = random.choice(idxs)
        lines[i] = lines[i].lstrip()
        return b"\n".join(lines)
    return None


KINDS = [("T1", "함수 타이핑 중"), ("T2", "괄호 미종료"), ("T3", "문자열 미종료"),
         ("T4", "빈 블록"), ("T5", "줄 삭제"), ("T6", "들여쓰기 붕괴")]

REPOS = [("/tmp/rn/django", "django"), ("/tmp/rn/scikit-learn", "scikit-learn"),
         ("/tmp/rn/flask", "flask")]

SAMPLE = 400

files = []
for repo, name in REPOS:
    raw = git(repo, "archive", "HEAD", "*.py", binary=True)
    tf = tarfile.open(fileobj=io.BytesIO(raw))
    got = 0
    for m in tf.getmembers():
        if not m.isfile() or not m.name.endswith(".py"): continue
        d = tf.extractfile(m).read()
        if len(d) < 400 or len(d) > 60000: continue
        base = ast_symbols(d)
        if base is None or len(base) < 3: continue
        files.append((name, m.name, d, base, ts_symbols(d)))
        got += 1
        if got >= SAMPLE // len(REPOS): break

print(f"### 편집 중 파일에서의 심볼 복원력\n")
print(f"대상: 실제 저장소 HEAD의 .py {len(files)}개 "
      f"(원본 기준 ast 심볼 {sum(len(f[3]) for f in files)}개)\n")
print(f"{'편집 상태':<16}{'ast 파싱성공':>13}{'ast 심볼보존':>13}"
      f"{'ts 심볼보존':>13}{'ts 우위':>10}")

tot = Counter()
for kind, label in KINDS:
    a_ok = a_sym = t_sym = base_sym = n = 0
    for _, path, data, base_a, base_t in files:
        m = mutate(data, kind)
        if m is None: continue
        n += 1
        base_sym += len(base_a)
        sa = ast_symbols(m)
        if sa is not None:
            a_ok += 1
            a_sym += len(sa & base_a)
        st = ts_symbols(m)
        t_sym += len(st & base_a)
    if not n: continue
    ap = a_sym / base_sym * 100
    tp = t_sym / base_sym * 100
    print(f"{kind+' '+label:<16}{a_ok/n*100:>12.1f}%{ap:>12.1f}%{tp:>12.1f}%{tp-ap:>9.1f}%p")
    tot["a_ok"] += a_ok; tot["n"] += n; tot["a"] += a_sym; tot["t"] += t_sym; tot["b"] += base_sym

print()
print(f"{'전체 평균':<16}{tot['a_ok']/tot['n']*100:>12.1f}%"
      f"{tot['a']/tot['b']*100:>12.1f}%{tot['t']/tot['b']*100:>12.1f}%"
      f"{(tot['t']-tot['a'])/tot['b']*100:>9.1f}%p")
print()
print("해석:")
print("  'ast 파싱성공' = 편집 중 상태에서 ast.parse가 예외 없이 끝난 비율")
print("  '심볼보존'     = 원본 파일의 심볼 중 그 상태에서도 찾아낸 비율")
print("  ast는 파싱에 실패하면 심볼이 0개가 된다 (부분 결과 없음)")
