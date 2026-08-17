# 16. Suggestion Engine

## 해결하는 문제 — 콜드스타트

v0.1 설계에는 구조적 데드락이 있었다.

- `02_CORE_PRINCIPLES.md`: **manual link가 가장 중요한 의미 관계다**
- `02` / `08`: **AI는 Graph를 자동 수정하지 않는다**

두 원칙을 합치면 이렇게 된다.

> 사용자가 vault를 처음 열었을 때 Semantic Web은 **완전히 비어 있다.**

보이는 것은 파서가 만든 구조 관계(`defined_in`, `imports`, `contains`)뿐인데, 그건 IDE가 이미 제공한다. Graph 탭을 열면 자기가 아는 파일 구조가 동그라미로 그려져 있을 뿐이다. **차별점이라고 선언한 기능이 Day 1에 가치가 0이다.**

그리고 링크를 걸어야 가치가 생기는데, 링크를 거는 것은 노동이고, 노동의 대가가 아직 보이지 않는 상태에서 사용자는 링크를 걸지 않는다.

## 해법 — 결정론적 후보 생성 + 1클릭 승인

AI를 쓰지 않는다. **결정론적 규칙**으로 후보를 만들어 사용자에게 승인을 요청한다. AI가 아니므로 "AI가 Graph를 자동 수정하지 않는다"는 원칙을 위반하지 않는다.

```text
R1 (추측)                → .vault-ai/suggestions/ → 사용자 1클릭 승인 → .vault/links.jsonl
R6 / R2 (경로·문법 확정)  → .vault-ai/             → 즉시 Graph 표시    (재생성되면 파생물)
사용자가 지우거나 고친 것                          → .vault/            (그 판단은 재생성 불가)
```

규칙마다 승인 정책이 다르다 — 상세는 아래 "승인 정책". 2계층 저장소 원칙(`.vault/` 는 재생성 불가한 것만 담는다 → `04_VAULT_AND_DATA_MODEL.md`)은 깨지 않는다. R6/R2 링크는 문서를 다시 읽으면 재생성되므로 `.vault-ai/` 에 있어도 파생물이지만, 사용자가 삭제하거나 고치는 순간 그 판단은 재생성으로 복원할 수 없으므로 `.vault/` 로 옮긴다.

승인·거절 이력은 `.vault/decisions.jsonl` 에 남긴다. 거절한 후보를 다시 제안하지 않기 위해서다.

## 규칙

### R1 — 문서 인라인 코드 토큰 ↔ 심볼

문서(`.md` / `.rst` / `.txt`)의 백틱 토큰이 vault 내 심볼과 유일하게 일치하면 `describes` 후보를 만든다.

```text
docs/ref/migration-operations.txt  `SeparateDatabaseAndState`
    → django/db/migrations/operations/special.py

sklearn/datasets/descr/lfw.rst     `fetch_lfw_people`
    → sklearn/datasets/_lfw.py

src/services/formatting/README.md  `formatSpan`
    → src/services/formatting/formatting.ts
```

문서와 코드를 잇는 것이 이 제품이 노리는 빈 곳이다. (→ `01_PRODUCT_VISION.md`)

코드 펜스(``` 블록) 안은 제외한다. 예제 코드의 토큰은 그 문서가 설명하는 대상이 아니다.

**문서 쪽 끝점은 파일이다 — 소제목(heading)이 아니다.** 위 예시가 `docs/ref/migration-operations.txt`로 끝나고 `#h:...`가 붙지 않는 이유다. R3·R5·R6도 같다. 자동 생성 규칙이 만드는 링크의 문서 쪽 끝점은 전부 **파일 단위**로 통일한다.

```text
R1 이 만드는 것   vault://docs/ref/migration-operations.txt
                 → vault://django/db/migrations/operations/special.py

사용자가 만드는 것 vault://docs/architecture.md#h:teacher-router
                 → vault://src/router.py#TeacherRouter        (origin: manual)
```

세 가지 이유다.

```text
측정이 파일 단위다      M2d 는 (문서 파일, 토큰, 대상 파일) 삼중항을 셌다.
                      소제목 단위 생성량·정밀도는 측정한 적이 없다 (→ `17`)
끝점이 더 잘 깨진다     소제목은 문서 편집 중 가장 자주 바뀌는 부분이고
                      heading 계단에는 유사도 단계가 없다 (→ `18_DATA_FORMATS.md` §3)
승인 노동이 늘어난다     같은 토큰이 여러 소제목 아래 나오면 후보가 갈라진다.
                      파일 단위면 한 번 판정으로 끝난다
```

**사용자가 직접 만드는 링크는 소제목 끝점을 그대로 지원한다.** `03_SYSTEM_ARCHITECTURE.md`의 `#h:teacher-router` 예시가 그것이며, 그 링크는 `origin: manual`이라 이 규칙의 적용 대상이 아니다. 사용자가 소제목을 명시적으로 고른 것은 추측이 아니므로 자동 생성과 판단 근거가 다르다.

소제목 단위 자동 생성은 v0.1에서 하지 않는다. 승격하려면 소제목 단위 생성량과 정밀도를 먼저 측정해야 한다.

**R1의 위상**: R6(아래)가 생긴 이후 R1은 더 이상 "가장 중요한 규칙"이 아니다. Sphinx 문서가 있는 저장소에서는 R6가 더 직접적이고 사용자 판정 노동이 0이다. 그러나 R1은 여전히 **Sphinx가 없는 생태계를 위한 범용 폴백**이다 — 언어와 문서 포맷을 가리지 않는 유일한 규칙이라는 점이 오히려 중요해졌다. Rust/TypeScript/Go 저장소는 R6 문법이 사실상 없거나(→ `17_MEASUREMENT_BASIS.md` "생태계별 문서→코드 참조 밀도") 밀도가 낮으므로 R1이 여전히 주력이다.

### R2 — 설정 값 ↔ 실존 경로

JSON / YAML / TOML 값이 vault 안의 실존 파일 경로면 `references` 후보를 만든다.

```text
tsconfig.json  "typings/globals.d.ts"  → typings/globals.d.ts
pyproject.toml "tools/cpplint.py"      → tools/cpplint.py
```

경로 존재 여부로 판정하므로 정밀도가 사실상 100%다.

### R3 — docstring ↔ 심볼

docstring 안에 등장하는 다른 심볼 이름이 vault 내에서 유일하면 `mentions` 후보를 만든다. 자기 파일 내 심볼은 제외한다.

### R4 — Git 동시 변경

같은 커밋에서 N회 이상 함께 변경된 파일 쌍에 `co_changed` 후보를 만든다.

```text
기본값  N = 5, 커밋당 파일 수 ≤ 12 (대량 리팩터링 커밋 제외)
```

`confidence = heuristic` 이며 승인 전에는 Graph에 나타나지 않는다.

### R5 — Notebook ↔ 로컬 모듈

`.ipynb` 가 import 하는 vault 내 모듈에 `uses` 후보를 만든다. 정적 import 문에서만 도출하므로 정밀도가 높다.

### R6 — 문서 코드 참조 문법 (Sphinx role)

문서 파일에 명시적으로 쓰인 코드 참조 문법을 추출해 `describes` 후보를 만든다. R1이 "우연히 백틱 안에 있고 이름이 겹치는" 토큰을 대상으로 하는 것과 달리, R6는 사용자가 코드 심볼을 가리키려고 **의도적으로 쓴** 문법을 대상으로 한다.

v0.1은 Sphinx role(`:class:` `:func:` `:meth:` `:attr:` `:mod:` `:exc:` `:data:` `:obj:`)만 지원한다. 측정상 밀도가 의미 있는 수준인 유일한 문법이기 때문이다(→ `17_MEASUREMENT_BASIS.md` "생태계별 문서→코드 참조 밀도"). mkdocstrings(`:::`)·Doxygen 태그는 문법 존재만 확인됐고 밀도가 낮으므로 **향후 확장 후보**로만 남긴다.

```text
docs/ref/migrations.txt  :class:`~django.db.migrations.operations.SeparateDatabaseAndState`
    → django/db/migrations/operations/special.py
```

**측정 근거**: django 675개 문서에서 코드 참조 10,203건 중 81.8%(7,867건)가 저장소 안 심볼로 정확히 해석됐다(경로 확인 기준). 이 값은 하한이다 — 미해석 4.9%는 측정 도구가 상속을 따라가지 않아 생긴 한계이지 문서 문제가 아니다. 근거 전체는 `17_MEASUREMENT_BASIS.md` "문서→코드 참조 해석률".

**정밀도 특성**: 완전 수식 경로(`:class:`django.db.migrations....SeparateDatabaseAndState``)는 경로 자체가 곧 정답이므로 R2(경로 존재 판정)와 같은 성격이다 — 추측이 아니라 확인이다. 반면 짧은 이름(`:class:`QuerySet`` 처럼 경로 없이 쓴 것, 전체의 31% = 2,979건)은 Sphinx가 `currentmodule` 문맥으로 해석하는데, 이 제품이 그 문맥을 재현하지 않으면 **R1과 동일한 이름 대조 모호성 문제로 떨어진다.**

**구현 비용**: (1) Sphinx role 파서, (2) 완전 수식 경로 우선 처리(정밀도 사실상 100%), (3) 짧은 이름 2,979건에 대한 `currentmodule` 문맥 재현 또는 이름 유일성 대체 판정 — 이 부분만 R1급 작업량과 모호성을 가진다.

## 노이즈 제어

### 필수 — vendored 경로 제외

이것은 성능이 아니라 **정밀도** 문제다.

| repo | vendor 포함 | vendor 제외 |
|---|---:|---:|
| node | 764건 | 155건 |
| kubernetes | 767건 | 398건 |

제외된 609건(node)은 대부분 `deps/v8/third_party/` 같은 무관한 외부 코드를 가리켰다. `SECURITY.md` 의 `configure` 가 `abseil-cpp/conanfile.py` 로 연결되는 식이다.

따라서 **ignore 규칙은 Phase 0 필수 항목이다.** `.gitignore` 문법을 재사용하고, `vendor/` `third_party/` `deps/` `node_modules/` 등의 기본 패턴을 제공한다.

### 영어 단어 필터

진짜 오탐의 공통점은 위치도 빈도도 아니고 **그 토큰이 영어 단어라는 것**이다.

```text
오탐  join  phase  policy  buffer  error  pipe  module  stream  cache
      Local  Global  Configuration  Running  Parallel  image
정탐  ModelAdmin  LoadBalancerSourceRanges  lru_cache  TweedieRegressor
      PostCSS  clone_on_ref_ptr  assertEndsWith
```

규칙:

```text
토큰이 복합 식별자면        → 통과 (밑줄 / 내부 대문자 / 숫자 포함)
토큰이 평범한 단일 단어면    → 사전에 있으면 제외
```

사전은 정적 데이터(약 23만 단어)로 앱에 동봉 가능하며 네트워크가 필요 없다.

**알려진 한계**: `Ridge` `Lasso` `Birch` `Normalizer`(scikit-learn), `Signer`(django) 같은 실존 클래스명이 영어 단어라 걸린다. 원리적으로 피할 수 없다. 이 때문에 아래 원칙이 나온다.

## 설계 원칙 — 필터링하지 말고 랭킹하라

자동 정밀도 판정을 두 가지 방식으로 시도했고 **둘 다 실패했다.** 실패의 내용이 설계 결론을 만들었다.

### 실패 ① 지역성 필터

"문서와 대상 심볼이 같은 최상위 서브트리에 있으면 정답일 개연성이 높다"고 가정했다. 측정 결과 지역성이 django 0.0%, hugo 0.0%, cpython 0.3%였다.

이유:

```text
django   docs/faq/admin.txt       `ModelAdmin`         → django/contrib/admin/options.py
hugo     docs/.../postcss.md      `PostCSS`            → tpl/css/css.go
cpython  Misc/NEWS.d/3.9.0b1.rst  `get_source_segment` → Lib/ast.py
```

전부 정답인데 트리를 가로지른다. **표준 레이아웃은 `docs/` 와 `src/` 를 분리한다.** 즉 이 제품이 노리는 문서↔코드 관계는 본질적으로 교차 관계다. 지역성으로 거르면 가장 가치 있는 제안을 정확히 버린다.

### 실패 ② 산문 빈도 필터

"토큰이 문서 산문(백틱 밖)에 자주 등장하면 영단어다"라고 가정했다. 외부 사전 없이 vault 자체로 계산되는 규칙이었으나, 제거 목록이 이랬다.

```text
✗ LoadBalancerSourceRanges (산문 8회)
✗ ModelAdmin               (산문 250회)
✗ lru_cache                (산문 37회)
✗ fetch_20newsgroups       (산문 8회)
✗ PostCSS                  (산문 28회)
```

**좋은 문서는 API 이름을 산문에서도 반복한다.** 산문 빈도는 "영단어임"이 아니라 "그 문서의 주제임"의 신호였다. 정확히 거꾸로 읽은 것이다.

### 그래서

세 필터 중 둘이 실패했고, 성공한 것(영어 사전)도 실존 심볼을 버린다. **자동 필터로 완벽을 노리는 접근 자체가 틀렸다.**

```text
하지 않는다   임계값으로 걸러내서 "확실한 것만" 보여주기
한다          전부 랭킹해서 보여주고 사용자가 1클릭으로 확정/거절
```

승인은 1클릭, 거절도 1클릭이어야 한다. 거절한 것은 `decisions.jsonl` 에 기록되어 다시 나오지 않는다.

## 승인 정책

규칙마다 판정의 성격이 다르므로 승인 정책도 다르다.

| 규칙 | 판정 근거 | 정책 |
|---|---|---|
| R6 Sphinx role | 사람이 명시 + 경로 정확 | 자동 반영, 틀리면 사후 삭제 |
| R2 설정값↔경로 | 경로 존재로 판정 | 자동 반영 |
| R1 이름 대조 | 추측 | 승인 필요 (상위 50건 노출) |
| R3 docstring↔심볼 | 추측 (이름 유일성) | 승인 필요 |
| R4 git 동시 변경 | 추측 (`confidence = heuristic`) | 승인 필요 |
| R5 Notebook↔모듈 | 정적 import — 확인에 가까우나 **미측정** | 승인 필요 (아래) |

**R5를 자동 반영으로 올리지 않았다.** 정적 import 문에서만 도출하므로 성격은 R2(확인)에 가깝다. 그러나 R5는 생성량조차 측정한 적이 없고, 노트북의 상대 경로·`sys.path` 조작처럼 "확인"이 성립하지 않는 경로가 있는지도 확인하지 않았다. 자동 반영은 사용자 승인 없이 링크를 만드는 되돌리기 비싼 결정이므로, **판정 근거가 확인임이 실제로 측정된 R2·R6에만 준다**(→ `02_CORE_PRINCIPLES.md` "15. Measure before deciding"). R5 생성량을 측정한 뒤 재검토한다.

이 표 이전 버전은 R6·R2·R1 셋만 있었고 R3·R4·R5의 정책이 비어 있었다. 아래 산문이 "추측성 후보(R1, R3, R4)"라고 언급할 뿐이라 R5는 어디에도 분류되지 않았다.

R6/R2를 자동 반영하는 것은 위 "필터링하지 말고 랭킹하라" 원칙과 충돌하지 않는다. 그 원칙이 겨냥한 것은 **추측성 후보**(R1, R3, R4)다 — 자동 정밀도 판정 필터가 실패했으므로 필터링 대신 랭킹 후 사용자 판정에 맡기라는 결론이었다. R6는 애초에 추측이 아니라 사람이 문서에 이미 명시적으로 써 놓은 참조이고, 완전 수식 경로가 지배적이라 경로 확인만으로 확정된다. 판정할 것이 없으므로 랭킹 대상도 아니다.

7,867건(R6, django 기준)을 승인 큐에 넣으면 B7(제안 검토 UI 규모 문제, → `09_DESKTOP_UX.md`)이 그대로 재발한다. R6/R2를 자동 반영으로 분리하는 것이 그 회피책이다. 자동 반영된 링크가 틀렸을 경우 사용자는 삭제할 수 있고, 그 삭제는 `.vault/` 에 기록되어 재생성으로 되살아나지 않는다(→ 위 "저장 위치" 다이어그램).

## 생성량 — 콜드스타트는 해결되는가

실제 저장소 8개에서 R1 기준 측정(vendored 제외, 영어 사전 필터 적용 후).

| repo | 심볼 | 필터 전 | 필터 후 |
|---|---:|---:|---:|
| cpython | 56,153 | 7,674 | 6,180 |
| django | 29,022 | 5,709 | 4,721 |
| scikit-learn | 9,333 | 3,528 | 3,341 |
| rust | 98,983 | 1,058 | 870 |
| kubernetes | 64,038 | 478 | 358 |
| hugo | 6,930 | 382 | 161 |
| node | 2,511 | 436 | 120 |
| TypeScript | 20,430 | 39 | 12 |
| **합계** | | **19,304** | **15,763** |

가장 작은 저장소(flask, 236 파일)는 규칙별로 따로 측정했다 — Phase 3 종료 조건과 승인율 게이트가 이 저장소를 대상으로 하기 때문이다(→ `10_MVP_ROADMAP.md`).

| 규칙 | flask | 정책 |
|---|---:|---|
| R1 이름 대조 | **156** | 승인 필요 (필터 전 238, 제거율 34.5%) |
| R2 설정값↔경로 | 9 | 자동 반영 |
| R3 docstring↔심볼 | 83 | 승인 필요 |
| R4 git 동시 변경 | 346 | 승인 필요 |
| R5 Notebook↔모듈 | 미측정 | 승인 필요 (flask에 `.ipynb` 없음) |
| R6 Sphinx role | 미측정 | 자동 반영 |
| **합계 (R1~R4)** | **594** | |
| **검토 큐 (R1+R3+R4)** | **585** | 사용자가 판정할 대상 |

조건: flask @ `d318b683` (2026-08-16), 심볼 1,082, 전체 히스토리 클론. R4는 `--no-merges` 최근 8,000커밋 · 동시 변경 5회 이상 · 커밋당 파일 12개 이하. 원문은 `research/bench/raw_output/m2_flask.txt`.

**이전 버전은 이 값을 `438`로 적었고, 그것은 세대가 다른 두 측정을 섞은 값이었다.** 438의 R1 항은 `0`인데, 그 R1은 `.md` 파일만 훑던 초기 구현(M2)의 결과다. flask 문서는 `docs/*.rst`이므로 전량이 누락됐다. 위 표의 R1 156은 이 문서가 다른 수치(cpython 6,180 등)에 쓰는 것과 같은 최종 규칙(M2d — `.md`/`.rst`/`.txt` + 영어 사전 필터)으로 다시 잰 값이다. 같은 이유로 M2 세대의 cpython R1 `20` · django R1 `0`도 인용해서는 안 된다.

**측정을 인용할 때 세대를 확인할 것.** `raw_output/m2.txt`와 `m2d.txt`는 코퍼스 구성도 다르다(전자에 flask·후자에 scikit-learn). 규칙 구현이 바뀌면 이전 세대의 같은 이름 수치는 폐기된 것으로 다룬다.

**생성량이 많다는 것은 콜드스타트 해결의 필요조건이지 충분조건이 아니다.** 후보가 0건이면 애초에 승인할 것이 없으니 콜드스타트는 확실히 풀리지 않는다 — 그 실패는 배제됐다. 하지만 생성량이 곧 해결을 의미하지도 않는다. cpython처럼 6,180건을 만들어 놓고 그대로 목록에 쏟아내면, 콜드스타트가 "빈 Graph"에서 "6,180번의 판정 노동"으로 형태만 바뀐 것이다. 판정 노동이 사용자에게 전가되는 것 자체가 리스크이지 해법이 아니다. 무엇을 먼저 보여줄지 — 즉 랭킹 — 가 없으면 생성량은 오히려 위협이 된다.

단, TypeScript는 12건에 그쳤다. 문서가 적은 저장소에서는 R1의 생성량이 낮다. 이런 경우 R2(설정↔경로)와 R4(git 동시 변경)가 주력이 된다.

## 후보 우선순위

"필터링하지 말고 랭킹하라" 원칙(위)에 따라 후보를 버리지 않는다. 대신 **결정론적 신호로 순서를 매겨** 판정 노동을 앞부분에 집중시킨다. 임베딩·LLM·외부 모델은 쓰지 않는다 — 핵심 구조화에 AI를 쓰지 않는다는 Algorithm-first 원칙(→ `02_CORE_PRINCIPLES.md`)이 여기도 적용된다.

```text
규칙 종류          R2(설정값↔실존 경로)는 경로 존재로 판정하므로 사실상 정밀도 100% → 최상단
복합 식별자 여부    밑줄 / 내부 대문자 / 숫자 포함 → 영어 단어 오탐일 확률이 낮음
토큰 길이          짧을수록 흔한 단어일 확률이 높음
백틱 등장 횟수      같은 문서에서 반복 언급될수록 그 문서의 주제일 개연성
심볼 docstring 유무  문서화된 심볼이 문서에서 언급될 개연성
문서-심볼 이름 겹침  파일명/경로 토큰과 심볼명의 겹침
```

위 목록은 **순서와 방향**만 정의한다. 신호 간 상대 가중치는 확정하지 않는다 — 근거가 없다. `12_ENGINEERING_DECISIONS.md`의 "미결 ② Search Ranking 가중치"와 같은 성격이다: 벤치마크로 정할 수 있는 값이 아니라, Phase 3 실사용 로그(승인/거절 이력)로 조정해야 하는 값이다.

### 초기 노출 상한

첫 화면에는 랭킹 상위 **50건**만 보여준다. 이 숫자에는 실측 근거가 없다 — 초기값이며, Phase 3 실사용 로그(승인율, 사용자가 몇 건까지 실제로 판정하는지)로 조정한다. 근거 있는 척 쓰지 않는다.

**이것은 필터링이 아니라 랭킹 후 페이징이다.** 51번째 후보 이후도 `.vault-ai/suggestions/`에 그대로 남고, 사라지거나 자동 거절되지 않는다. 나머지에 접근하는 UI는 → `09_DESKTOP_UX.md` "제안 검토 UI".

## 아직 측정되지 않은 것

**승인율은 측정하지 않았다.** 생성량은 확인했지만 사용자가 실제로 몇 %를 승인할지는 사람이 관여해야 알 수 있다. 승인율이 낮으면 후보가 많아도 콜드스타트는 풀리지 않는다.

이는 P3 완료 시점의 검증 항목이다. (→ `10_MVP_ROADMAP.md`)

정밀도 수치를 이 문서에 명시하지 않은 것도 같은 이유다. 자동 지표 두 개가 실패했고 세 번째도 한계가 있는 상태에서 정밀도 숫자를 쓰면, 측정하지 못한 것을 측정한 것처럼 만든다.

**R6도 마찬가지다.** 해석률(경로가 저장소 안 심볼로 맞는지)은 django로 측정했지만(→ `17_MEASUREMENT_BASIS.md`), 승인율·오탐률(사용자가 자동 반영된 링크를 실제로 얼마나 삭제하는지)은 미측정이다. R6를 자동 반영으로 결정한 것은 "경로 확인만으로 확정된다"는 판정 근거 때문이지, 승인율이 높을 것으로 측정했기 때문이 아니다.

## AI 제안과의 관계

MCP를 통한 AI 제안도 **같은 승인 UX**를 쓴다. 그러나 저장 위치는 결정론적 제안과 다르다.

```text
결정론적 제안(R1/R3/R4) ─→ .vault-ai/suggestions/ → 승인 → .vault/links.jsonl (origin=manual)
AI 제안(MCP, 승인 모드)  ─→ .vault/pending.jsonl   → 승인 → .vault/links.jsonl (origin=manual)
AI 제안(MCP, 즉시 반영)  ─→ .vault/links.jsonl 직접 기록                     (origin=ai)
```

사용자 입장에서 흐름이 하나다. 그러나 시스템 입장에서는 다르다 — 결정론적 제안은 문서/코드를 재파싱하면 재생성되므로 `.vault-ai/` 에 둬도 되지만, AI 제안은 재생성 불가한 판단이므로 `.vault/` 에 둔다. `.vault-ai/` 를 지우면 결정론적 제안 후보만 사라지고(재스캔으로 복구), AI 제안은 영향받지 않는다. (→ `18_DATA_FORMATS.md` §4.5)

각 제안에는 **불변 ID**를 발급한다. AI가 제안하고 사용자가 승인하는 사이에 다른 제안이 끼어들어 무엇을 승인했는지 불명확해지는 것을 막는다. (→ `08_MCP_AND_AI.md`)
