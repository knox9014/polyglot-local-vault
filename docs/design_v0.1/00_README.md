# Polyglot Local Vault — 문서 패키지

## 프로젝트 한 줄 정의

**컴퓨터의 다양한 파일을 하나의 로컬 Vault에서 초고속으로 검색·탐색·연결하고, 파일 내부 구조까지 의미 있는 객체로 관리하며, 필요할 때 외부 AI가 MCP를 통해 이 지식 구조를 사용할 수 있게 하는 로컬 Workspace.**

이 프로젝트는 AI가 핵심 기능을 수행하는 앱이 아니다. 핵심 정리·검색·분석·관계 구성은 결정론적 알고리즘으로 수행하고, AI는 MCP를 통해 선택적으로 연결한다.

## 최상위 원칙

1. Local-first
2. Original-file-first
3. Algorithm-first
4. Human-link-first
5. Polyglot
6. Search-first
7. AI-optional
8. MCP-first AI integration

## 핵심 구성요소

- Fast Local Search Engine
- Polyglot Parser System
- Semantic Graph Engine
- Local Workspace UI
- MCP Server
- Optional External AI

## 문서 구조

- `01_PRODUCT_VISION.md` — 제품 정의, 목표, 차별점
- `02_CORE_PRINCIPLES.md` — 변경하지 않을 최상위 설계 원칙
- `03_SYSTEM_ARCHITECTURE.md` — 전체 시스템 아키텍처
- `04_VAULT_AND_DATA_MODEL.md` — Vault, Object, Graph IR, 주소 체계
- `05_FAST_LOCAL_SEARCH.md` — 초고속 파일 검색 엔진
- `06_POLYGLOT_PARSERS.md` — 파일 형식별 Parser 설계
- `07_SEMANTIC_WEB_GRAPH.md` — 거미줄형 연결 및 Graph 설계
- `08_MCP_AND_AI.md` — MCP와 외부 AI 연결 원칙
- `09_DESKTOP_UX.md` — 사용자 인터페이스와 사용 흐름
- `10_MVP_ROADMAP.md` — v0.1 범위 및 개발 단계
- `11_SECURITY_PRIVACY_RELIABILITY.md` — 보안·개인정보·신뢰성
- `12_ENGINEERING_DECISIONS_TODO.md` — 구현 단계에서 결정할 항목
- `13_FUTURE_EXTENSIONS.md` — 초기 범위 밖의 장기 확장
- `14_LOCKED_DECISIONS.md` — 현재까지 확정된 의사결정 목록
- `15_REVIEW_HISTORY.md` — 20회 검토에서 반영된 핵심 개선

## 현재 범위

초기 제품은 **컴퓨터 로컬 전용 데스크톱 앱**으로 제한한다.

초기 범위에서 제외:

- 클라우드 동기화
- 웹 앱
- 모바일 앱
- 팀 협업
- SaaS
- 계정/로그인
- 서버형 Graph DB
- 서버형 Vector DB
- AI 자동 정리
- AI 자동 Graph 변경

핵심 엔진은 인터넷과 AI 없이도 정상 작동해야 한다.
