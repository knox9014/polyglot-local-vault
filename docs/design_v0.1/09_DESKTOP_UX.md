# 09. Desktop UX

## 기본 목표

내부 구조는 복잡하더라도 사용자는 처음에 복잡함을 느끼지 않아야 한다.

## 기본 레이아웃

```text
┌──────────────────────────────────────────────┐
│ Search                                      │
├────────────┬─────────────────────┬───────────┤
│            │                     │           │
│   Files    │       Editor        │ Related   │
│            │                     │           │
│ README.md  │                     │ Links     │
│ src/       │                     │ Graph     │
│ data/      │                     │ Backlinks │
│            │                     │           │
└────────────┴─────────────────────┴───────────┘
```

## 기본 사용자 경험

```text
폴더 열기
 ↓
자동 인덱싱
 ↓
검색
 ↓
파일 열기
 ↓
필요하면 객체 연결
 ↓
Related / Graph 자동 갱신
```

## 초보 사용자

다음 기능만 알아도 사용할 수 있어야 한다.

- 파일 열기
- 검색
- 편집
- 링크

## 고급 사용자

필요한 경우:

- Symbol 탐색
- Graph
- Dependency
- Git
- MCP

를 사용한다.

## Search UX

검색창에 입력하는 즉시 결과를 표시한다.

예:

```text
r
ro
rou
rout
router
```

각 단계에서 빠른 파일명/심볼 결과를 즉시 갱신한다.

## Graph UX

기본적으로 전체 Graph를 보여주지 않는다.

Focus Node 기반으로 관련 객체만 표시한다.

## Related Panel

현재 객체의:

- backlinks
- manual links
- deterministic relations
- code dependencies
- git relations

을 간단히 보여준다.
