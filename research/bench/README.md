# vault-bench — Polyglot Local Vault 설계 가정 실측 코드

## search/ — 실험 1 (검색 속도)
- `bench1_baseline.rs`              최초 구현. 비트마스크 프리필터 + DP 스코어링
- `bench2_incremental_greedy.rs`    기각된 두 가설(incremental 축소 / greedy 스코어) 검증
- `bench3_final.rs`                 tight span + SIMD + 규모별 한계점 + 파일명 스코프 비교

실행:
    # 코퍼스 구성
    for r in microsoft/TypeScript rust-lang/rust nodejs/node \
             kubernetes/kubernetes python/cpython; do
      git clone --depth=1 --filter=blob:none https://github.com/$r.git $(basename $r)
    done
    for d in */; do (cd $d && git ls-files | sed "s|^|$d|"); done > all_paths.txt
    shuf --random-source=<(yes 42) all_paths.txt > shuffled.txt

    # 빌드 & 실행 (Cargo.toml의 [[bin]] 경로에 맞춰 src/ 배치 필요)
    cargo build --release
    ./target/release/bench3 shuffled.txt 20

## resolve/ — 실험 2 (Stable ID 복구율)
- `resolve_bench1.py`               1차 측정 (삭제된 파일이 분모에 섞인 버전)
- `resolve_bench2_corrected.py`     분모 정정판. 지표 A / 지표 B 분리

실행 (전체 히스토리 클론 필요, --depth 쓰면 안 됨):
    git clone https://github.com/django/django.git
    git clone https://github.com/scikit-learn/scikit-learn.git
    git clone https://github.com/pallets/flask.git
    git clone https://github.com/psf/requests.git
    python3 resolve_bench2_corrected.py

## symbol/ — 실험 3·4 (심볼 레벨 복구율 / 파서 복원력)
- `symbol_bench1.py`            1차 측정 (frozenset 순서 의존, 분모 미정정)
- `symbol_bench2_corrected.py`  minhash 스케치 + GONE 오라클로 정정한 본편
- `parser_edit_bench.py`        편집 중 파일에서 ast vs tree-sitter 심볼 보존율

의존성:
    pip install tree_sitter tree_sitter_python

실행 (실험 2와 같은 전체 히스토리 클론 필요):
    python3 symbol_bench2_corrected.py django scikit-learn flask requests
    python3 parser_edit_bench.py

## m1_m6/ — 3차 측정 (M1~M6)
- `m1_lang_symbols.py`   언어별(Go/TS/Rust/Python) 심볼 복구율
- `m2_suggestions.py`    제안 엔진 4규칙 생성량
- `m2b_precision.py`     지역성 필터 (실패)
- `m2c_prose_filter.py`  산문 빈도 필터 (실패)
- `m2d_dict_filter.py`   영어 사전 필터 (채택)  ※ pip install english-words

### M2 와 M2d 의 R1 은 같은 것이 아니다

`m2_suggestions.py` 의 R1 은 `.md` 만 훑는다. `m2d_dict_filter.py` 의 R1 은 `.md`/`.rst`/`.txt` 를
훑고 영어 사전 필터를 적용한다. `.rst` 를 쓰는 저장소(cpython·django·flask)에서 두 값은
자릿수가 다르다 — cpython 20 vs 6,180. **M2d 가 최종본이고 M2 의 R1 열은 폐기된 값이다.**
M2 의 R2·R3·R4 는 유효하다. 코퍼스 구성도 다르다(M2 에 flask, M2d 에 scikit-learn).

flask 규칙별 생성량(2026-08-17 추가 측정 → `raw_output/m2_flask.txt`)은 두 스크립트의
`TARGETS` 한 줄만 바꿔 각각 돌린 뒤 합친 것이다. R1 만 M2d, 나머지는 M2 를 쓴다.

    git clone https://github.com/pallets/flask.git      # --depth 금지 (R4 가 히스토리를 쓴다)

**Windows 에서 돌릴 때 두 가지.** `rule2_configs` 에 넘기는 root 는 `os.path.normpath` 형식이어야
한다 — `/` 구분자로 넘기면 `cand.startswith(root)` 가 항상 False 가 되어 R2 가 조용히 0 이 된다.
콘솔 인코딩은 `PYTHONIOENCODING=utf-8` 로 둔다(cp949 에서 출력이 죽는다).
- `m3_call_edges.py`     call edge 이름 해석 모호도
- `m4_reconcile.py`      Watcher 정합성 스캔 비용
- `m5_sweep.py`          유사도 파라미터 45조합 스윕
- `m6_index.rs`          역인덱스 규모/구축시간 (Rust)

## raw_output/ — 원본 측정 출력 + 통합 데이터셋
- `m*.txt`                     각 측정의 stdout 원문
- `measurements.json`          M1~M6 통합 데이터셋
- `m1_symbol_recovery.csv`     언어별 복구율 31행
- `m5_similarity_sweep.csv`    파라미터 스윕 45행

추가 의존성:
    pip install tree_sitter tree_sitter_python tree_sitter_go \
                tree_sitter_typescript tree_sitter_rust english-words
