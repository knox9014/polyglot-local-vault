# 08. MCP and Optional AI

## 핵심 철학

AI는 Vault를 정리하는 엔진이 아니다.

AI는 Vault를 사용하는 외부 클라이언트다.

```text
Vault
 ↓
MCP Server
 ↓
AI
```

## AI 없이 가능한 기능

- 파일 검색
- 파일 열기
- 구조 Parsing
- Symbol 검색
- Graph
- Backlink
- Dependency 탐색
- Metadata 검색

## 예상 MCP Tools

초기 후보:

```text
vault.search
vault.read
vault.list
vault.symbols
vault.graph
vault.references
vault.backlinks
vault.dependencies
vault.history
vault.create_link
vault.remove_link
```

정확한 Tool schema는 구현 단계에서 확정한다.

## 사용 예

사용자:

> TeacherRouter와 연결된 설정을 찾아줘.

외부 AI:

```text
vault.symbols("TeacherRouter")
```

결과:

```text
src/router.py#TeacherRouter
```

다음:

```text
vault.graph(node="TeacherRouter")
```

관련 설정 객체를 찾는다.

AI가 무작정 전체 프로젝트를 읽지 않고 Vault의 인덱스와 Graph를 이용한다.

## Link Write Safety

AI가 관계를 만들고 싶다면:

1. AI가 제안
2. 사용자가 승인
3. MCP write tool 실행
4. Graph에 저장

사용자 승인 없이 의미 관계를 자동 확정하지 않는다.

## 모델 독립성

MCP를 사용하면 다음과 같은 다양한 AI와 연결 가능하다.

- ChatGPT
- Codex
- Claude
- Gemini
- Local LLM
- 미래의 MCP 호환 AI

Vault는 특정 모델에 종속되지 않는다.
