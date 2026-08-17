# 03. System Architecture

## 전체 구조

```text
                 USER COMPUTER

                      │
                      ▼
                 LOCAL VAULT
                      │
       ┌──────────────┼──────────────┐
       │              │              │
    Documents        Code           Data
       │              │              │
      .md            .py          .json
                                    .yaml
                                    .csv
                                    .ipynb
       │              │              │
       └──────── Parser Adapters ─────┘
                      │
                      ▼
              Universal Graph IR
                      │
       ┌──────────────┼──────────────┐
       │              │              │
 Fast Search      Graph Engine    Metadata
       │              │              │
       └──────────────┼──────────────┘
                      │
                      ▼
                Desktop Workspace
                      │
                      ▼
                  MCP Server
                      │
       ┌──────────────┼──────────────┐
       ▼              ▼              ▼
    ChatGPT         Codex         Other AI
```

## 핵심 모듈

### Vault Manager

- Vault 생성/열기
- 파일 경로 관리
- ignore 규칙
- `.vault-ai/` 관리

### File Watcher

- 생성
- 수정
- 삭제
- rename

이벤트를 받아 incremental update를 수행한다.

### Parser Layer

파일 형식별 Parser Adapter를 제공한다.

### Universal Graph IR

모든 Parser 결과를 공통 Node/Edge 구조로 변환한다.

### Search Engine

- Filename
- Path
- Extension
- Symbol
- Full-text
- BM25
- Metadata
- Graph

검색을 통합한다.

### Graph Engine

- Manual links
- Deterministic structural edges
- Backlinks
- Dependency traversal

### Desktop Workspace

- File tree
- Editor/viewer
- Search
- Related panel
- Graph view

### MCP Server

외부 AI가 Vault 기능을 사용하도록 API를 제공한다.

## 저장 구조 예시

```text
MyProject/
├── README.md
├── docs/
├── src/
├── data/
├── experiments/
└── .vault-ai/
    ├── index/
    ├── graph/
    ├── metadata/
    ├── cache/
    └── state/
```
