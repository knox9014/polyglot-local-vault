# 02. Core Principles

## 1. Local-first

핵심 데이터와 인덱스는 사용자 PC에 저장한다.

기본 기능은 인터넷 연결 없이 작동해야 한다.

## 2. Original-file-first

원본 파일을 전용 포맷으로 강제 변환하지 않는다.

예:

```text
README.md
router.py
config.json
experiment.ipynb
```

는 그대로 유지한다.

시스템 메타데이터는 별도 숨김 디렉터리에 저장한다.

```text
.vault-ai/
```

앱을 삭제해도 원본 파일은 그대로 남아야 한다.

## 3. Algorithm-first

핵심 구조화에 AI를 사용하지 않는다.

예:

- Python → AST
- JSON → JSON parser
- Markdown → Markdown parser
- CSV → table parser
- Jupyter → notebook parser

동일 입력에는 가능한 한 동일 결과가 나오도록 설계한다.

## 4. Human-link-first

중요한 의미 관계는 사용자가 주도한다.

자동 생성이 허용되는 것은 프로그램이 결정론적으로 확정할 수 있는 관계다.

예:

- defines
- imports
- contains
- calls
- parent-child

AI의 의미 추론은 Vault Graph를 자동 수정하지 않는다.

## 5. Polyglot

Markdown을 특수한 중심 포맷으로 두지 않는다.

각 파일 형식은 해당 구조를 최대한 보존하며 공통 IR에 연결된다.

## 6. Search-first

파일 검색 성능은 부가기능이 아니라 핵심 KPI다.

## 7. AI-optional

AI가 없어도 프로그램 핵심 기능이 완전해야 한다.

## 8. MCP-first AI Integration

외부 AI는 MCP를 통해 Vault의 검색·읽기·Graph 기능을 사용한다.

## 9. Progressive Disclosure

내부 기능이 복잡하더라도 기본 UI는 단순해야 한다.

초기 사용자에게는:

- 파일
- 검색
- 에디터
- 관련 항목

정도만 보이고, Graph와 고급 기능은 필요할 때 열어야 한다.

## 10. Incremental by Default

파일 하나가 바뀌었다고 전체 Vault를 다시 처리하지 않는다.

변경된 파일과 관련 인덱스만 갱신한다.
