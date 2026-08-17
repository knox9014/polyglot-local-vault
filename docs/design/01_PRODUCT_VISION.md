# 01. Product Vision

## 제품 정의

Polyglot Local Vault는 Markdown 중심의 지식 관리 도구를 넘어, 다양한 로컬 파일과 파일 내부 객체를 하나의 지식 공간으로 연결하는 데스크톱 Workspace다.

지원 대상은 단순 파일이 아니라 내부 객체까지 포함한다.

- 문서 heading
- Python / Go / TypeScript / Rust 의 class, function, method
- JSON object / property
- YAML node
- CSV schema / column / row
- Jupyter notebook cell
- Git commit

## 해결하려는 문제

개발자의 지식은 보통 이렇게 흩어진다.

```text
문서       Obsidian / Notion / docs/
코드       VS Code / IDE
설정       JSON / YAML
데이터     CSV / DB
실험       Jupyter
변경 기록  Git
AI 작업    ChatGPT / Codex / Claude
```

Polyglot Local Vault는 이들을 하나의 로컬 지식 공간으로 연결한다.

## 핵심 가치

### 1. 파일 검색

사용자가 파일을 찾기 위해 AI에게 질문할 필요가 없을 정도로 빠른 검색을 제공한다.

측정된 목표: 100K 파일에서 keystroke → 첫 결과 p95 < 16ms. 부분 문자 퍼지 매칭 포함(`tscfgjson` → `tsconfig.json`).

### 2. 구조 검색

`router.py` 를 찾는 것이 아니라 다음을 찾는다.

- `TeacherRouter` 클래스
- `select_teacher()` 함수
- 특정 JSON 설정값
- 특정 문서 heading
- 특정 Notebook cell

### 3. 관계 탐색

파일과 내부 객체를 거미줄처럼 연결한다.

```text
architecture.md
      │ describes
      ▼
TeacherRouter
      │ implemented_by
      ▼
router.py
      │ configured_by
      ▼
config.json
      │ tested_by
      ▼
experiment.ipynb
```

### 4. 관계가 시간을 견딤

파일을 옮기거나 이름을 바꿔도 링크가 유지된다.

측정 결과, 실제 저장소에서 약 10년치 히스토리를 건너뛰어도 파일 링크의 **95.7%**, 심볼 링크의 **93.7%** 가 복구된다. 대상이 실제로 삭제된 경우는 링크가 끊어지는 것이 정상 동작이며, 그때도 조용히 삭제하지 않고 표시한다.

이 항목이 v0.2에서 핵심 가치로 승격되었다. 링크가 시간이 지나면서 무너지면 나머지 가치가 전부 무너지기 때문이다.

### 5. AI 독립성

AI는 제품 핵심이 아니다. AI 없이 다음이 전부 작동한다.

- 검색 / 파싱 / Graph / Backlink / Symbol 탐색 / 관계 추적
- **관계 후보 제안** — 결정론적 규칙으로 수행하므로 콜드스타트 해소에도 AI가 필요 없다

### 6. AI 상호운용성

AI를 쓰고 싶을 때는 MCP로 연결한다. 특정 모델이나 회사에 종속되지 않는다.

## 차별점

이 제품을 "AI가 붙은 Obsidian"으로 정의하지 않는다.

개념적 차별점은 **Polyglot first-class semantic objects** 다. `.md`, `.py`, `.json`, `.csv`, `.ipynb` 같은 원본 파일과 그 내부 객체가 같은 Vault의 1급 객체가 된다.

### 그러나 v0.1의 마케팅 메시지는 더 좁아야 한다

인접 도구들이 이미 각 축을 점유하고 있다.

| 축 | 기존 도구 |
|---|---|
| 파일 검색 | fzf, Everything, VS Code Ctrl+P |
| 코드 심볼 | LSP, ctags, Sourcegraph |
| 문서 링크 | Obsidian |
| 통합 검색 | Glean 등 엔터프라이즈 |

**진짜 빈 곳은 "코드와 문서 사이의 링크" 한 군데다.** 이건 실제로 아무도 잘 하지 못하고 있고, 개발자가 실제로 아파하는 지점이다.

> "이 설계 문서가 어느 코드 얘기인지 모르겠다"
> "이 config 값이 어디서 쓰이는지 모르겠다"
> "이 함수가 왜 이렇게 되어 있는지 설명한 문서가 어디 있더라"

따라서 v0.1의 메시지는 "polyglot vault"가 아니라 **"문서와 코드를 잇는다"** 여야 한다.

### 그 가치를 즉시 증명하는 단일 기능

제안 엔진의 R1 규칙 — 문서의 인라인 코드 토큰을 vault 내 심볼과 매칭 — 이 그것이다.

```text
docs/ref/migration-operations.txt  `SeparateDatabaseAndState`
    → django/db/migrations/operations/special.py

sklearn/datasets/descr/lfw.rst     `fetch_lfw_people`
    → sklearn/datasets/_lfw.py

src/services/formatting/README.md  `formatSpan`
    → src/services/formatting/formatting.ts
```

vault를 처음 열었을 때 이런 후보가 수백~수천 건 제시된다(측정: 저장소당 12~6,180건). 사용자는 1클릭으로 승인한다.

**이 하나만 잘 돌아가도 제품이 성립한다.** 기술 문서 없이 "무엇이든 연결하는 도구"로 포지셔닝하면 사용자는 무엇을 연결해야 할지 모른 채 빈 그래프를 보게 된다.

## 아직 검증되지 않은 것

기술 리스크는 측정으로 상당히 줄었으나 **제품 리스크는 그대로 남아 있다.**

- 제안 후보를 사용자가 실제로 몇 % 승인하는지 측정하지 않았다. 승인율이 낮으면 후보가 많아도 콜드스타트는 풀리지 않는다.
- 링크 복구율 93.7%는 사용자가 링크를 걸었다는 전제 위에 있다.
- 위 포지셔닝("문서와 코드를 잇는다")이 실제 지불 의사로 이어지는지는 측정할 수 없다.

이는 P3 완료 후 실사용으로 확인해야 한다. (→ `10_MVP_ROADMAP.md`)
