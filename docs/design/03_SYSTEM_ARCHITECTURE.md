# 03. System Architecture

## 전체 구조

```text
                          USER COMPUTER
                               │
                               ▼
                          LOCAL VAULT
                               │
        ┌──────────────┬───────┴───────┬──────────────┐
        │              │               │              │
    Documents        Code            Data          History
        │              │               │              │
   .md .rst .txt      .py            .json         local git
                      .go            .yaml
                      .ts            .csv
                      .rs            .ipynb
        │              │               │              │
        └──────────────┴─── Parser Adapters ──────────┘
                               │
                               ▼
                       Universal Graph IR
                               │
        ┌──────────────┬───────┴───────┬──────────────┐
        │              │               │              │
   Fast Search    Graph Engine   Suggestion Engine  Metadata
        │              │               │              │
        └──────────────┴───────┬───────┴──────────────┘
                               │
                               ▼
                       Desktop Workspace
                               │
                               ▼
                          MCP Server
                               │
        ┌──────────────┬───────┴───────┬──────────────┐
        ▼              ▼               ▼              ▼
     ChatGPT        Codex          Claude        Other MCP AI
```

v0.1 대비 변경점 두 가지.

1. **Git이 최상위 입력원으로 승격**했다. 링크 복구의 핵심 근거이며 Phase 0 구성요소다.
2. **Suggestion Engine이 독립 모듈로 추가**되었다. 콜드스타트를 해소하는 결정론적 후보 생성기다. (→ `16_SUGGESTION_ENGINE.md`)

## 저장소 2계층 분리

v0.1에서 모든 시스템 데이터를 `.vault-ai/` 하나에 두었으나, 이는 자체 모순이었다.
`.vault-ai/`는 "삭제해도 재스캔으로 복구 가능한 캐시"로 정의되었는데, **manual link는 재스캔으로 복구할 수 없는 유일한 데이터**이면서 그 안에 있었다.

따라서 저장소를 성격에 따라 둘로 나눈다.

```text
MyProject/
├── README.md
├── docs/
├── src/
├── data/
├── experiments/
│
├── .vault/                  ← SOURCE OF TRUTH. git 커밋 대상. 사람이 읽을 수 있음
│   ├── links.jsonl          manual edges (append-only)
│   ├── aliases.jsonl        경로/심볼 이력 (rename/move 추적)
│   ├── decisions.jsonl      제안 승인/거절 이력
│   ├── sketches.jsonl       링크된 심볼의 유사도 스케치 (S4 재해석용, 재생성 불가)
│   ├── pending.jsonl        MCP AI 미승인 제안 (승인 모드일 때)
│   └── vault.toml           ignore 규칙, 설정
│
└── .vault-ai/               ← 순수 파생물. .gitignore 대상. 언제든 삭제 가능
    ├── index/               검색 인덱스
    ├── parsed/              파서 결과 캐시
    ├── similarity/          심볼 본문 minhash 스케치
    ├── suggestions/         미승인 제안 후보
    └── state/               watcher 커서, 스키마 버전
```

이 분리로 얻는 것:

- `.vault-ai/` 삭제 = 재인덱싱만 하면 완전 복구 (`11_SECURITY_PRIVACY_RELIABILITY.md`의 요구가 실제로 성립)
- manual link가 git에 남음 → 히스토리, diff, 되돌리기, 팀 공유가 부수적으로 따라옴
- JSONL이므로 merge conflict가 라인 단위 → 협업 확장 시 해결 가능

`links.jsonl` 한 줄:

```json
{"id":"l_01JBQZ8K3M","op":"add","from":"vault://docs/architecture.md#h:teacher-router","rel":"describes","to":"vault://src/router.py#TeacherRouter","origin":"manual","confidence":"certain","ts":"2026-08-16T09:00:00Z"}
```

append-only이므로 삭제도 tombstone 레코드(`op:"del"`)로 기록한다. Undo가 공짜로 따라온다. 전체 필드 정의는 `18_DATA_FORMATS.md` §4.1 참조.

## 핵심 모듈

### Vault Manager

- Vault 생성/열기
- 파일 경로 관리
- ignore 규칙 (`.gitignore` 문법 재사용)
- `.vault/` 와 `.vault-ai/` 관리
- 다중 인스턴스 락

### File Watcher + Reconciler

Watcher는 **저지연 힌트**로만 사용한다. 정합성은 별도로 보장한다.

OS 이벤트는 유실된다. inotify 큐 오버플로, FSEvents의 디렉터리 단위 coalescing, `ReadDirectoryChangesW` 버퍼 오버플로, 그리고 `git checkout` / 패키지 설치 시의 이벤트 폭풍이 모두 실재한다. rename은 특히 신뢰할 수 없어서 delete + create로 도착하는 경우가 잦다.

```text
정상   watcher 이벤트 → 100~300ms debounce → 변경분 재파싱
보정   앱 시작 시 + 유휴 N분마다 → (path, mtime_ns, size) 얕은 스캔
       → 인덱스와 diff → 누락분 처리
폭풍   단위 시간당 이벤트 > 임계 → watcher 일시 중단
       → 해당 서브트리 전체 재스캔으로 전환
```

정합성 스캔 비용은 측정되었다. 10만 파일 기준 스캔 281ms + diff 60ms ≈ **0.35초**. 유휴 시 주기 실행에 부담이 없다.

**단일 스레드로 구현한다.** 디스크 메타데이터 스캔은 I/O 바운드라 병렬화 이득이 없다. (측정: 2워커가 오히려 2.3배 느렸다.)

### Git Reader

Phase 0 구성요소다. 링크 복구율의 핵심 근거다.

- rename/move 체인 수집 (`--diff-filter=R -M`) → `aliases.jsonl`
- commit / diff / history / blame
- 동시 변경 관계 (제안 엔진 입력)

측정상 git alias 없이는 파일 링크 복구율이 95.7% → 88.2%로, 심볼 링크는 80.0% → 70.4%로 떨어진다. 대규모 구조 개편 시에는 격차가 훨씬 크다 — 전체 파일이 이동한 5개 사례에서 git alias만으로 64~98%가 복구되었다.

### Parser Layer

Tree-sitter 단일 백엔드. 파일 형식별 Adapter를 제공한다. (→ `06_POLYGLOT_PARSERS.md`)

### Universal Graph IR

모든 Parser 결과를 공통 Node/Edge 구조로 변환한다.
Edge는 `origin`과 `confidence`를 함께 가진다. (→ `04_VAULT_AND_DATA_MODEL.md`)

### Search Engine

3층 구조. (→ `05_FAST_LOCAL_SEARCH.md`)

```text
1층  In-memory path table   파일명 / 경로 / 확장자 / 퍼지
2층  역인덱스               본문 + 심볼 + 메타데이터
3층  임베디드 관계 저장소    노드 / 엣지 / alias / 링크
```

### Graph Engine

- Manual links (`.vault/links.jsonl`)
- Deterministic structural edges
- Git relations
- Backlinks
- Dependency traversal
- 계단식 주소 해석 (→ `04`)

### Suggestion Engine

결정론적 규칙으로 관계 후보를 생성한다. AI를 쓰지 않는다.
승인이 필요한 후보(R1·R3·R4)는 `.vault-ai/suggestions/` 에 저장되며, 사용자가 승인해야 `.vault/links.jsonl` 로 이동한다. R2·R6 는 승인 없이 반영된다 — 사람이 문서에 명시적으로 쓴 참조를 옮기는 것이라 추측이 아니다. MCP 경유 AI 제안은 이 경로가 아니다(→ `18_DATA_FORMATS.md` §4.5). (→ `16_SUGGESTION_ENGINE.md`)

### Desktop Workspace

- File tree
- Editor/viewer
- Search
- Related panel
- Graph view (focus node 1-hop)
- 제안 검토 UI
- BROKEN 링크 일괄 재지정 UI

### MCP Server

외부 AI가 Vault 기능을 사용하도록 4개 툴을 제공한다. (→ `08_MCP_AND_AI.md`)

## 데이터 흐름 요약

```text
파일 변경
  │
  ├─ watcher 이벤트 (저지연) ──┐
  └─ 주기 정합성 스캔 (보정) ──┤
                              ▼
                        변경 파일 목록
                              │
                              ▼
                     Tree-sitter 파싱
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
        검색 인덱스 갱신   Graph IR 갱신   유사도 스케치 갱신
              │               │               │
              └───────────────┼───────────────┘
                              ▼
                   링크 주소 재해석 (계단식)
                              │
                              ▼
                    Related / Graph UI 갱신
```

파싱이 부분 실패(편집 중 문법 오류)해도 이 흐름은 중단되지 않는다. Tree-sitter가 부분 트리를 반환하므로 살아남은 심볼로 인덱스를 갱신한다.
