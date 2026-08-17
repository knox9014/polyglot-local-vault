# 07. Semantic Web / Graph

## 개념

파일 트리는 물리적 구조를 보여준다.

Semantic Web은 의미적 구조를 보여준다.

```text
File Tree        Semantic Web
---------        ------------
폴더 위치         관계
경로              맥락
정적 구조         의미적 연결
```

## 거미줄형 연결

예:

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

## 관계 종류

### Manual

사용자가 직접 생성한 연결.

가장 중요한 의미 관계다.

### Deterministic

Parser/정적 분석으로 확정 가능한 연결.

예:

- defined_in
- contains
- imports
- calls
- inherits
- parent_of

### Git

로컬 Git에서 확정 가능한 관계.

예:

- changed_in
- introduced_in

## AI 관계

AI는 Core Graph를 자동 변경하지 않는다.

MCP를 통해 AI가 관계를 제안할 수는 있지만, 실제 저장은 사용자의 명시적 승인 후에만 수행한다.

## Graph UI 원칙

전체 Graph를 한 번에 보여주지 않는다.

기본:

```text
Current Node
+
1-hop Relations
```

필요하면:

```text
1-hop
→ 2-hop
→ Project
→ Entire Vault
```

순으로 확장한다.

## Layer Filter

예:

- Documents
- Code
- Data
- Manual Links
- Dependencies
- Git

## Hairball 방지

노드가 많아질수록 전체 Graph는 시각적으로 무의미해질 수 있다.

따라서:

- focus node 중심
- relation type filtering
- depth 제한
- relevance sorting
- node collapsing
- grouping

을 사용한다.

## Hypergraph 가능성

여러 객체가 하나의 사건 또는 실험에 참여하는 경우 일반 Edge보다 Hyperedge가 자연스러울 수 있다.

예:

```text
Experiment #37
├─ router.py
├─ config.json
├─ dataset.csv
└─ metrics.json
```

초기 MVP에서는 일반 Graph로 시작하고 필요성이 확인되면 Hypergraph를 검토한다.
