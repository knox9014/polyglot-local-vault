# 04. Vault and Data Model

## Vault

Vault는 사용자의 일반 로컬 폴더다. 특수한 데이터베이스 포맷으로 프로젝트 전체를 감싸지 않는다.

시스템 데이터는 성격에 따라 두 디렉터리로 나뉜다. (→ `03_SYSTEM_ARCHITECTURE.md`)

| | `.vault/` | `.vault-ai/` |
|---|---|---|
| 성격 | source of truth | 순수 파생물 |
| 재생성 | 불가 | 재스캔으로 완전 복구 |
| git | 커밋 대상 | ignore 대상 |
| 형식 | JSONL / TOML (사람이 읽음) | 바이너리 인덱스 |
| 내용 | manual link, alias, 승인 이력, 설정 | 검색 인덱스, 파서 캐시, 유사도 스케치, 미승인 제안 |

## 객체의 두 가지 지위

v0.1은 Node 목록을 "저장되는 것"의 목록처럼 기술했다. 이것은 규모에 따라 성립하지 않는다. 100만 행 CSV 하나가 노드 100만 개를 만들면 관계 저장소가 즉시 붕괴한다.

따라서 **주소 지정 가능(addressable)** 과 **저장됨(materialized)** 을 분리한다.

### 주소 지정 가능한 객체

파일 내용에서 결정론적으로 계산되므로 저장 없이 언제나 주소로 지칭할 수 있다.

```text
File            Directory
DocumentSection CodeClass       CodeFunction    CodeMethod
CodeVariable    JSONObject      JSONProperty    YamlNode
Table           Column          Row
Notebook        NotebookCell
GitCommit
```

### 저장되는 객체 (materialize-on-link)

위 객체 중 **실제로 링크가 걸린 것만** 관계 저장소에 노드로 실체화한다.

```text
사용자가 vault://data/x.csv#row:1042 에 링크를 건다
  → 그 순간 노드 1개 생성
링크가 없는 나머지 999,999 행
  → 노드 없음. 주소는 여전히 유효하고 검색도 됨
```

기본 파싱으로 만드는 노드는 스키마 수준까지다. CSV는 `Table` / `Column` / 행 수 / 추론 타입, Notebook은 셀 목록, JSON은 최상위 구조. 개별 행·셀·배열 원소는 링크가 걸릴 때 실체화한다.

## 객체 주소

각 객체는 사람이 읽을 수 있는 논리 주소를 가진다.

```text
vault://src/router.py
vault://src/router.py#TeacherRouter
vault://src/router.py#TeacherRouter.select_teacher
vault://config/model.json#/router/threshold
vault://docs/architecture.md#teacher-router
vault://data/train.csv#col:label
vault://data/train.csv#row:1042
vault://experiments/run.ipynb#cell:12
```

**주소는 저장 시점에 해석하지 않는다.** 조회 시점에 계단식으로 해석한다(아래).

## Node

```text
NODE
id:      python:src/router.py#TeacherRouter
type:    CodeClass
name:    TeacherRouter
source:  src/router.py
range:   [42, 118]
```

`range`는 필수다. 유사도 스케치 계산과 뷰어 스크롤에 쓰인다.

## Edge

```text
EDGE
from:       vault://src/router.py#TeacherRouter
rel:        defined_in
to:         vault://src/router.py
origin:     parser
confidence: certain
```

### origin

관계가 어디서 왔는지.

```text
manual      사용자가 직접 생성
parser      정적 분석으로 도출
git         로컬 git 히스토리에서 도출
suggested   제안 엔진이 만든 후보 (미승인 상태. .vault-ai/ 에만 존재)
imported    외부 도구에서 가져옴
```

### confidence

v0.1은 `origin=parser` 하나로 `defined_in`(100% 확실)과 `calls`(오탐 다수)를 같은 취급했다. 이는 신뢰 수준 판단을 무의미하게 만든다. 별도 축으로 분리한다.

```text
certain     defined_in, contains, parent_of, imports(정적 문자열), json_pointer
            → 기본 UI에 표시
probable    calls(bare/self 형태), inherits(vault 내 정의)
            → 필터를 켜야 표시
heuristic   co_change, 승인 전 제안
            → 제안 검토 화면에서만 표시
```

**측정 근거**: 호출부 360,893건 분석 결과 `foo()` 유일 해석률 73.0%, `self.foo()` 75.3%, `obj.foo()` 30.3%. 앞의 둘만 `probable`로 생성하고 `obj.foo()`는 edge를 만들지 않는다. (→ `06_POLYGLOT_PARSERS.md`)

## 계단식 주소 해석

경로만으로 객체를 식별하면 rename/move 시 관계가 끊어진다. 그러나 **모든 노드에 안정적 ID가 필요한 것은 아니다.**

파서가 만든 노드는 파일에서 재생성되는 파생물이므로 안정성이 필요 없다. 안정성이 실제로 필요한 것은 **manual link의 양 끝점뿐**이며, 그 수는 사용자가 손으로 건 링크 수(수백~수천)이지 vault 전체 객체 수가 아니다.

따라서 content hash나 inode 기반 ID를 도입하지 않는다. 논리 주소를 저장하고 조회 시점에 해석한다.

### 파일 주소 — 3단

```text
L1  경로 그대로 존재                → HIT (비용 0)
L2  aliases.jsonl (git rename)      → HIT
L3  BROKEN — 링크를 보존하고 UI에 표시
```

**측정 근거** (실제 저장소 4개, 약 10년 히스토리, 대상이 생존한 링크 4,961건 기준):

| 단계 | 비율 |
|---|---:|
| L1 경로 유지 | 88.2% |
| L2 git alias | 7.5% |
| **자동 복구 계** | **95.7%** |
| 단서 없음 | 3.7% |

basename 유일 매칭 단계도 측정했으나 기여도가 **0.3%** 였다. 모호 후보 UI와 확정 흐름을 만들 가치가 없어 **채택하지 않는다.**

### 심볼 주소 — 5단

파일과 달리 심볼은 이름 매칭과 유사도 매칭이 실질적으로 기여한다.

```text
S1  같은 경로에 같은 qualname               → HIT (비용 0)
S2  aliases.jsonl 경로에 같은 qualname       → HIT
S3  qualname이 vault 전역에서 유일           → 후보 제시 (1클릭 확정)
S4  본문 유사도 Jaccard ≥ 0.40               → 후보 제시 (1클릭 확정)
S5  BROKEN — 링크를 보존하고 UI에 표시
```

**측정 근거** (대상이 생존한 심볼 링크 20,837건 기준):

| 단계 | 비율 |
|---|---:|
| S1 같은 경로 동일 심볼 | 70.4% |
| S2 git alias 경로 | 9.6% |
| **자동 복구 계** | **80.0%** |
| S3 qualname 유일 | 9.1% |
| S4 본문 유사도 | 4.5% |
| **+1클릭 계** | **93.7%** |

S3+S4가 13.7%p를 채운다. **파일 주소에서는 뺐지만 심볼 주소에서는 반드시 넣는다.** 계단의 깊이가 주소 종류마다 다르다는 것이 핵심이다.

언어별 편차도 측정했다. 자동 복구 80.2~94.5%, +1클릭 최종 91.6~97.5%. Python이 자동 복구 최하위이며 S3+S4 의존도가 가장 높다(13.5%p). TypeScript는 3.0%p만 의존한다. **최종 수치는 언어를 가리지 않고 수렴한다.**

### BROKEN 처리 원칙

깨진 링크를 조용히 삭제하지 않는다. 사용자가 고칠 수 있는 정보를 시스템이 버려서는 안 된다.

대규모 구조 개편은 한 번에 수십~수백 개 링크를 깨뜨린다. 측정에서 `requests/` → `src/requests/` 이전 시 S1이 **1건**까지 떨어진 사례가 있었다(git alias가 205건을 복구). 따라서 개별 수정이 아니라 **접두사 규칙 기반 일괄 재지정**을 제공한다. (→ `09_DESKTOP_UX.md`)

## alias 테이블

`aliases.jsonl` 은 두 소스에서 자동 축적된다.

1. **Watcher rename 이벤트** — 신뢰도는 높지만 항상 도착하지 않는다
2. **Git rename detection** (`--diff-filter=R -M`) — 유사도 기반 추적. 실질적 주력

체인은 압축해서 저장한다. `a → b → c` 는 `a → c` 로.

## 심볼 유사도 인덱스

S4를 위한 인덱스다. 파서가 이미 심볼 본문 범위(`range`)를 알고 있으므로 추가 파싱 없이 인덱싱 시점에 계산한다.

```text
토큰화        영숫자 + '_' 단위
shingle       토큰 3-gram 의 crc32 집합
스케치        가장 작은 해시 32개 (minhash)
역인덱스      스케치 값 → 심볼 ID
매칭          스케치 투표 → 상위 40 후보 → 정확 Jaccard
임계값        0.40
```

**파라미터 근거** (45개 조합 스윕):

- 정밀도는 파라미터에 거의 무관했다 (91.6~93.6%). 유사도 분포가 이봉형이기 때문이다 — 같은 함수는 매우 유사하고 다른 함수는 매우 다르다.
- 따라서 임계값을 낮추는 것이 순이득이다. 0.60 → 0.40 으로 복구 809 → 1,515건(**+87%**), 정밀도 92.3% → 92.9%.
- 스케치 96 → 32 로 줄여도 결과가 소수점까지 동일했다. 인덱스 항목 152만 → 95만(**-38%**).

```text
확정: shingle k=3, sketch=32, threshold=0.40
```

## Vault 이동

Vault 폴더 전체를 다른 경로로 옮겨도 링크는 깨지지 않는다. 모든 주소가 vault 루트 기준 상대 경로이기 때문이다. `.vault/` 를 함께 옮기기만 하면 된다.
