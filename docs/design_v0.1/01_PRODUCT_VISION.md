# 01. Product Vision

## 제품 정의

Polyglot Local Vault는 Markdown 중심의 지식 관리 도구를 넘어, 다양한 로컬 파일과 파일 내부 객체를 하나의 지식 공간으로 연결하는 데스크톱 Workspace다.

지원 대상은 단순 파일이 아니라 다음과 같은 내부 객체까지 포함한다.

- Markdown heading
- Python class / function / method
- JSON object / property
- YAML node
- CSV schema / column / row
- Jupyter notebook cell
- 향후 다른 언어의 symbol 및 문서 객체

## 해결하려는 문제

현재 개발자의 지식은 보통 다음과 같이 흩어진다.

- 문서: Obsidian / Notion
- 코드: VS Code / IDE
- 설정: JSON / YAML
- 데이터: CSV / DB
- 실험: Jupyter
- 변경 기록: Git
- AI 작업: ChatGPT / Codex / Claude 등

Polyglot Local Vault는 이들을 하나의 로컬 지식 공간으로 연결한다.

## 핵심 가치

### 1. 파일 검색

사용자가 파일을 찾기 위해 AI에게 질문할 필요가 없을 정도로 빠른 검색을 제공한다.

### 2. 구조 검색

단순히 `router.py`를 찾는 것이 아니라 다음을 찾을 수 있어야 한다.

- `TeacherRouter` 클래스
- `select_teacher()` 함수
- 특정 JSON 설정값
- 특정 Markdown heading
- 특정 Notebook cell

### 3. 관계 탐색

파일과 내부 객체를 거미줄처럼 연결한다.

예:

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

### 4. AI 독립성

AI는 제품 핵심이 아니다.

AI 없이도:

- 검색
- 파싱
- Graph
- Backlink
- Symbol 탐색
- 관계 추적

이 전부 작동해야 한다.

### 5. AI 상호운용성

AI를 사용하고 싶을 때는 MCP를 통해 연결한다.

따라서 특정 모델 또는 특정 회사에 종속되지 않는다.

## 차별점

이 제품을 단순히 "AI가 붙은 Obsidian"으로 정의하지 않는다.

핵심 차별점은:

> **Polyglot first-class semantic objects**

즉 `.md`, `.py`, `.json`, `.csv`, `.ipynb` 같은 원본 파일과 그 내부 객체가 같은 Vault의 1급 객체가 되는 것이다.
