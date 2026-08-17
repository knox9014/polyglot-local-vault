# 06. Polyglot Parser System

## 핵심 원칙

파일을 단순 텍스트 blob으로 취급하지 않는다. 각 파일 형식의 고유 구조를 결정론적 알고리즘으로 추출한다.

## 파서 백엔드 — Tree-sitter 단일

v0.1은 "Python은 native AST 가능, 다중 언어 확대 시 Tree-sitter가 유리"로 남겨두었다. 이는 v0.1에 파서를 두 벌 만들고 v0.2에서 갈아엎는 계획이며, **실시간 인덱싱이라는 사용 맥락을 빠뜨린 판단**이었다.

### 측정: 편집 중 파일에서의 심볼 보존율

실제 저장소의 `.py` 322개(원본 심볼 5,826개)에 실제 타이핑 중 상태를 주입해 측정했다.

| 편집 상태 | CPython `ast` | Tree-sitter |
|---|---:|---:|
| 파일 끝에서 함수 타이핑 중 (`def process_`) | **0.0%** | **99.9%** |
| 괄호 미종료 (`def handle(request, context`) | **0.0%** | **99.9%** |
| 문자열 미종료 | **0.0%** | **99.9%** |
| 블록 헤더만 (`if settings.DEBUG:`) | **0.0%** | **99.9%** |
| 중간 줄 삭제 (잘라내기 직후) | 68.7% | 92.4% |
| 들여쓰기 붕괴 (블록 이동 중) | 31.4% | 89.7% |
| **평균** | **16.7%** | **96.9%** |

앞의 네 가지는 예외 상황이 아니라 **새 함수를 작성할 때 반드시 지나가는 상태**다. `def process_` 를 타이핑하는 매 순간 AST 기반 인덱서는 그 파일의 심볼을 전부 잃는다. `09_DESKTOP_UX.md` 가 약속하는 "저장 즉시 Related 갱신"이 타이핑 중 깜빡이는 패널이 된다.

> 참고: 커밋된 코드에서는 `ast` 실패율이 0.2%에 불과했다(2013~2016년 코드 포함). "과거 문법 때문에 AST가 실패한다"는 것은 사실이 아니다. 문제는 오직 **편집 중 상태**다.

### 부수적 이점

| | CPython `ast` | Tree-sitter |
|---|---|---|
| 코어가 Rust/Go일 때 | Python 인터프리터 임베드 필요 | 단일 바이너리 |
| 증분 파싱 | 불가 (전체 재파싱) | 편집 구간만 |
| 언어 추가 | 언어마다 새 파서 | 문법 파일 교체 |

정밀 해석(데코레이터 평가, 타입 추론)이 필요해지면 언어별 백엔드를 **추가**한다. Parser Adapter 인터페이스가 이를 허용한다.

## v0.1 지원 형식

```text
문서   .md  .rst  .txt
코드   .py  .go  .ts  .rs
데이터 .json  .yaml  .csv
노트북 .ipynb
```

`.rst` / `.txt` 를 넣는 이유는 측정에서 드러났다. django는 문서를 `.txt` 로, cpython과 scikit-learn은 `.rst` 로 쓴다. `.md` 만 지원하면 이들 프로젝트에서 문서↔코드 관계가 **0건**이 된다.

`.go` / `.ts` / `.rs` 는 심볼 링크 복구율 측정에서 이미 검증되었다(자동 복구 85.6~94.5%).

## 형식별 추출 대상

### 문서 (.md / .rst / .txt)

- heading 및 계층
- 링크
- 태그
- 코드 블록 (인덱싱 대상이되 심볼 매칭에서는 제외)
- 인라인 코드 토큰 (제안 엔진 입력)
- 리스트

### 코드 — 공통

```text
module / package    class / struct / trait / interface
function / method   변수
imports             inheritance
decorators / attributes
```

### 코드 — call edge 정책

v0.1은 `calls` 를 추출 대상에 넣으면서 동시에 "확실히 알 수 있는 관계만 저장한다"고 적어 자기모순이었다. 측정으로 정리한다.

실제 저장소 5개의 **호출부 360,893건** 분석:

| 호출 형태 | 건수 | vault 내 유일 해석률 |
|---|---:|---:|
| `foo()` bare | 121,647 | **73.0%** |
| `self.foo()` | 53,238 | **75.3%** |
| `obj.foo()` attr | 186,008 | **30.3%** |
| 전체 | 360,893 | 48.5% |

전체로 보면 모호(51.5%)가 유일(48.5%)보다 많다. 그러나 **모호도는 `obj.foo()` 한 형태에 몰려 있다.**

```text
생성한다
  bare  foo()       → confidence = probable
  self  self.foo()  → confidence = probable
  (합계 174,885건 = 전체의 48.5%)

생성하지 않는다
  attr  obj.foo()   → 유일률 30.3%. 오탐이 정탐의 2.3배
  (타입 추론 백엔드 도입 후 v0.2에서 재검토)
```

`bare` 는 import 정보와 결합하면 유일률이 더 올라간다. 측정값 73.0%는 **하한**이다.

### JSON

- object / array / key / value
- JSON Pointer 경로 (`config.json#/router/threshold`)
- 값이 vault 내 실존 경로인 경우 → 제안 엔진 입력

### YAML

- mapping / sequence / scalar / 계층 / 경로

### CSV

- header / schema / column / 행 수 / 추론 타입
- **행은 노드로 만들지 않는다.** 주소(`#row:N`)는 항상 유효하고 검색도 되지만, 링크가 걸릴 때만 실체화한다. (→ `04_VAULT_AND_DATA_MODEL.md`)

### Jupyter Notebook

- notebook metadata / markdown cell / code cell / output / execution count / cell 순서
- Python code cell은 Python 파서로 재분석
- 셀도 CSV 행과 같은 materialize-on-link 정책

## Parser Adapter Interface

각 Parser는 공통 인터페이스를 따른다. 외부 개발자가 새 형식을 추가할 수 있다.

```text
Input
  → file bytes
  → 이전 파싱 결과 (증분 파싱용, 선택)

Output
  → Nodes           (id, type, name, source, range)
  → Edges           (from, rel, to, origin, confidence)
  → Searchable Text
  → Metadata
  → Source Ranges
  → Shingle Sketch  (심볼별 본문 minhash 스케치 32개)
  → Partial Flag    (문법 오류로 일부만 파싱되었는지)
```

**Shingle Sketch** 는 v0.2에서 추가되었다. 심볼 주소 해석의 S4 단계에 쓰인다. 파서가 이미 본문 범위를 알고 있으므로 추가 파싱 없이 계산된다. (파라미터: 토큰 3-gram, crc32, 최소 해시 32개)

**Partial Flag** 도 v0.2에서 추가되었다. 편집 중 파일에서 부분 결과임을 인덱서에 알린다. 인덱서는 부분 결과를 반영하되 기존 심볼을 통째로 지우지 않는다.

## 파싱 정책

```text
5MB 초과       파싱 제외 (파일 노드만 생성)
1MB 초과       본문 인덱싱 제외
바이너리        제외 (첫 8KB 내 NUL 바이트로 판정)
인코딩          UTF-8 우선, BOM 처리, 실패 시 로캘 인코딩 시도 후 바이너리 취급
vendored 경로   ignore 규칙으로 제외 (기본 패턴 제공)
```

vendored 경로 제외는 성능이 아니라 **정확도** 문제다. 측정에서 `deps/` `vendor/` `third_party/` 를 포함하면 제안 후보의 상당수가 무관한 외부 코드를 가리켰다(node: 764건 → 제외 시 155건). (→ `16_SUGGESTION_ENGINE.md`)

## 향후 형식

```text
.js  .tsx  .cpp  .h  .java  .sql  .toml  .xml
```

장기: PDF / DOCX / PPTX / XLSX / Images / Audio / Video

초기에는 지원 개수보다 Parser 품질을 우선한다.
