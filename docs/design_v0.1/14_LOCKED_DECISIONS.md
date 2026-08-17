# 14. Locked Decisions

현재까지 대화에서 확정한 의사결정이다.

## 제품 범위

- 초기 버전은 컴퓨터 로컬 전용이다.
- 웹/모바일/클라우드부터 시작하지 않는다.

## AI

- AI는 핵심 정리 엔진이 아니다.
- AI 없이 Core가 완전히 작동해야 한다.
- AI는 MCP로 외부에서 연결한다.
- AI가 Graph를 자동으로 수정하지 않는다.

## 파일

- 원본 파일은 그대로 유지한다.
- 자체 포맷으로 강제 변환하지 않는다.
- `.vault-ai/`에 인덱스와 메타데이터를 저장한다.

## Parsing

- 결정론적 알고리즘 사용
- Python → AST
- JSON/YAML → structural parser
- Markdown → Markdown parser
- CSV → table parser
- Notebook → notebook parser

## Graph

- 사용자의 명시적 링크가 중심이다.
- Parser가 확정할 수 있는 구조 관계는 자동 생성할 수 있다.
- 거미줄형 Semantic Web을 사용한다.
- 기본 UI에서는 1-hop 중심으로 보여준다.

## Search

- 파일 검색기는 핵심 제품 기능이다.
- 검색 속도를 최우선 KPI 중 하나로 둔다.
- 매 검색마다 디스크 전체를 스캔하지 않는다.
- 초기 전체 인덱싱 후 incremental update를 사용한다.
- Exact/BM25/TF-IDF/Symbol/Graph 검색을 Core로 한다.
- Vector/Embedding은 필수 Core가 아니다.

## 초기 파일 형식

- `.md`
- `.py`
- `.json`
- `.yaml`
- `.csv`
- `.ipynb`

## 상호운용

- MCP를 핵심 외부 인터페이스로 사용한다.
- 특정 AI 모델에 종속되지 않는다.
