# 07. Semantic Web / Graph

## 개념

파일 트리는 물리적 구조를 보여준다. Semantic Web은 의미적 구조를 보여준다.

```text
File Tree        Semantic Web
---------        ------------
폴더 위치         관계
경로              맥락
정적 구조         의미적 연결
```

## 거미줄형 연결

```text
                     architecture.md
                           │
                       describes
                           │
                           ▼
config.json ─────── TeacherRouter ─────── router.py
                           │
                       tested_by
                           │
                           ▼
                  experiment.ipynb
```

## 관계의 두 축

v0.1은 관계를 `origin` 하나로만 구분했다. 그 결과 `defined_in`(100% 확실)과 `calls`(오탐 다수)가 같은 `origin=parser` 라벨을 달았고, `11_SECURITY_PRIVACY_RELIABILITY.md` 가 강조한 "관계의 신뢰 수준 판단"이 성립하지 않았다.

v0.2는 **출처(origin)** 와 **신뢰도(confidence)** 를 별개 축으로 둔다.

### origin — 어디서 왔는가

| origin | 설명 | 저장 위치 |
|---|---|---|
| `manual` | 사용자가 직접 생성 | `.vault/links.jsonl` |
| `parser` | 정적 분석 | `.vault-ai/` (재생성 가능) |
| `git` | 로컬 git 히스토리 | `.vault-ai/` (재생성 가능) |
| `suggested` | 제안 엔진 후보, 미승인 | `.vault-ai/suggestions/` |
| `imported` | 외부 도구에서 가져옴 | `.vault/links.jsonl` |

승인된 제안은 `origin=manual` 로 승격되어 `.vault/links.jsonl` 로 이동한다. 승인·거절 이력은 `.vault/decisions.jsonl` 에 남는다.

### confidence — 얼마나 확실한가

| confidence | 관계 | 기본 표시 |
|---|---|---|
| `certain` | `defined_in` `contains` `parent_of` `imports`(정적) `json_pointer` `changed_in` | O |
| `probable` | `calls`(bare/self) `inherits`(vault 내) | 필터 켤 때 |
| `heuristic` | `co_changed`, 미승인 제안 | 제안 검토 화면만 |

`manual` 링크는 항상 `certain` 이다.

## 관계 종류

### Manual

사용자가 직접 생성한 연결. 가장 중요한 의미 관계다.

`.vault/links.jsonl` 에 append-only로 저장되며 git 커밋 대상이다. 삭제도 tombstone 레코드로 남으므로 되돌리기가 가능하다.

### Deterministic (Parser)

```text
certain    defined_in  contains  parent_of  imports  json_pointer
probable   calls(bare) calls(self) inherits
생성 안 함  calls(attr)   ← 유일 해석률 30.3%. 오탐이 정탐의 2.3배
```

`calls(attr)` 제외 근거는 `06_POLYGLOT_PARSERS.md` 참조.

### Git

```text
certain    changed_in  introduced_in  renamed_from
heuristic  co_changed  (동일 커밋 N회 이상 함께 변경)
```

Git 히스토리 전체를 Graph에 넣으면 노드가 폭증한다. 다음만 반영한다.

- 현재 존재하는 파일의 최근 N개 커밋
- rename/move 체인 (→ `aliases.jsonl`, 링크 복구의 핵심)
- 동시 변경 관계 (제안 엔진 입력, `heuristic`)

### Suggested

제안 엔진이 만든 후보. **Core Graph에 자동 저장되지 않는다.** (→ `16_SUGGESTION_ENGINE.md`)

## AI와 Graph

AI는 Core Graph를 자동 변경하지 않는다. MCP를 통해 관계를 제안할 수는 있으나, 실제 저장은 사용자의 명시적 승인 후에만 수행된다.

제안 엔진(결정론적)과 AI 제안(MCP 경유)은 **같은 승인 파이프라인을 공유**한다. 사용자 입장에서 흐름이 하나이며, 시스템 입장에서 승인 전에는 둘 다 `.vault-ai/suggestions/` 에만 존재한다.

```text
결정론적 제안 ─┐
              ├→ .vault-ai/suggestions/ → 사용자 승인 → .vault/links.jsonl
AI 제안(MCP) ─┘                                          origin=manual
```

## Graph UI 원칙

전체 Graph를 한 번에 보여주지 않는다.

```text
기본     Current Node + 1-hop, confidence=certain 만
확장     1-hop → 2-hop → Project → Entire Vault
필터     confidence, relation type, layer
```

### Layer Filter

- Documents
- Code
- Data
- Manual Links
- Dependencies
- Git

### Hairball 방지

- focus node 중심
- relation type filtering
- confidence 필터 (기본 `certain`)
- depth 제한
- relevance sorting
- node collapsing / grouping

`confidence` 필터가 v0.2에서 추가된 방어선이다. `probable` 관계를 기본으로 켜면 `calls` 계열이 화면을 채운다.

## 링크 무결성

주소는 조회 시점에 계단식으로 해석된다. (→ `04_VAULT_AND_DATA_MODEL.md`)

```text
파일 링크   L1 경로 → L2 git alias → BROKEN            자동 복구 95.7%
심볼 링크   S1 → S2 → S3 이름 유일 → S4 유사도 → BROKEN  자동 80.0%, +1클릭 93.7%
```

BROKEN 링크는 삭제하지 않고 UI에 표시한다. 대규모 구조 개편은 한 번에 다수를 깨뜨리므로 일괄 재지정을 제공한다. (→ `09_DESKTOP_UX.md`)

## Hypergraph

여러 객체가 하나의 사건·실험에 참여하는 경우 일반 Edge보다 Hyperedge가 자연스러울 수 있다.

```text
Experiment #37
├─ router.py
├─ config.json
├─ dataset.csv
└─ metrics.json
```

초기에는 일반 Graph로 시작하고, 필요성이 확인되면 검토한다. `13_FUTURE_EXTENSIONS.md` 참조.
