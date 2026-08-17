# 10. MVP Roadmap

## v0.1 목표

**Local Polyglot Vault + Fast Search + Structural Graph + MCP foundation**

## Phase 0 — Core Foundations

- Vault open/create
- `.vault-ai/`
- File watcher
- stable object ID 전략
- parser interface
- basic local persistence

## Phase 1 — Fast Search

우선순위를 매우 높게 둔다.

- Filename index
- Path index
- Extension filtering
- Full-text
- Symbol search
- Incremental indexing
- Search UI

## Phase 2 — Parsers

초기:

- Markdown
- Python
- JSON
- YAML
- CSV
- Jupyter Notebook

## Phase 3 — Semantic Graph

- Node/Edge model
- Manual links
- Deterministic relations
- Backlinks
- 1-hop Graph UI

## Phase 4 — Workspace

- File tree
- Editor/viewer
- Related panel
- Graph view
- Search integration

## Phase 5 — MCP

- read
- search
- symbol lookup
- graph traversal
- link write with user approval

## Phase 6 — Local Git

- commits
- diff
- history
- blame
- selected Graph relations

## v0.1에서 하지 않는 것

- Cloud sync
- Web app
- Mobile
- Team collaboration
- Account/login
- SaaS
- AI automatic organization
- AI automatic graph mutation
- mandatory embedding
- server-side databases

## 초기 성공 조건

사용자가 일반 프로젝트 폴더를 열고:

1. 빠르게 파일을 찾을 수 있다.
2. 파일 내부 symbol까지 찾을 수 있다.
3. 관련 객체를 연결할 수 있다.
4. 관계를 Graph에서 탐색할 수 있다.
5. AI 없이 핵심 기능이 전부 작동한다.
6. MCP를 통해 외부 AI가 Vault를 읽을 수 있다.
