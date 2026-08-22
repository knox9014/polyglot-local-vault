**한국어** | [English](README.md)

# Polyglot Local Vault

컴퓨터의 다양한 파일을 하나의 로컬 Vault에서 초고속으로 검색·탐색·연결하고,
파일 내부 구조(함수·제목·설정 키)까지 `vault://` 주소를 가진 객체로 관리하는 로컬 데스크톱 Workspace.

핵심 정리·검색·분석·관계 구성은 **결정론적 알고리즘**이 한다. AI는 MCP로 붙는 선택 요소다.

**현재 상태 (2026-08-21): Phase 0~4 완료.** MVP 경계였던 P0~P3을 넘어 P4(MCP)까지 끝났다.
코어 `polyglot-vault/` 6,850줄 + 데스크톱 `desktop/src-tauri/` 1,146줄, 테스트 142 + 21개 통과.
공개 배포는 아직 하지 않았다.

**[소개 페이지 보기 →](https://claude.ai/code/artifact/6908e7d8-a0ac-4273-88cd-92c73bf968a0)**

## 진행 상황

| Phase | 내용 | 상태 |
|---|---|---|
| P0 | 스캔 · `vault://` 주소 · 락 · 정합성 스캔 · watcher · git reader · Parser Adapter | 완료 (2026-08-18) |
| P1 | Fast Search — path table · 역인덱스 · 증분 인덱싱 · 검색창 + 뷰어 | 완료 (2026-08-19) |
| P2 | Parsers + Symbol Search — 12개 형식에서 심볼 추출 | 완료 (2026-08-19) |
| P3 | Workspace + Graph + Suggestions — R1 · R2 · import 해석, 승인 UI, D3 그래프 | 완료 (2026-08-21) |
| P4 | MCP — `search` / `read` / `neighbors` / `link` 4툴 stdio 서버 | 완료 (2026-08-21) |

데스크톱 프레임워크는 **Tauri로 확정**됐다 (2026-08-18, Windows release 빌드 keystroke p95 8.4ms vs PySide6 14.7ms).

다음은 미정 — 후보는 실사용 버그 수정, 승인율 게이트(P3 종료 조건) 측정, 배포 준비.

## 시작하기

Claude Code에서 이 디렉터리를 열면 `CLAUDE.md` 를 먼저 읽는다.
사람이 읽을 순서는 다음과 같다.

1. `CLAUDE.md` — 현재 상태, 확정 결정, 기각된 설계, 다음 할 일 (**정본**)
2. `docs/design/00_README.md` — 설계 문서 패키지 개요
3. `docs/design/18_DATA_FORMATS.md` — `vault://` 주소 문법과 `.vault/` 파일 형식
4. `docs/design/17_MEASUREMENT_BASIS.md` — 모든 수치의 출처와 한계

`TODO.md` 의 blocker 7 · major 7 · minor 3 은 **전부 닫혔다** (2026-08-17). 이력으로만 남아 있다.

### 빌드

```bash
cd polyglot-vault && cargo test     # 코어 테스트
cd desktop && npm install
npm run tauri dev                   # 데스크톱 앱 개발 실행
npm run tauri build                 # 릴리스 빌드
```

MCP 서버는 별도 stdio 바이너리다.

```bash
cd polyglot-vault && cargo run --bin vault-mcp
```

## 구조

```
polyglot-vault/     Rust 코어 — 인덱스 · 파서 · 제안 엔진 · MCP (src/mcp.rs, src/bin/vault-mcp.rs)
desktop/            Tauri 데스크톱 앱 (src-tauri/ Rust, src/ 프런트)
docs/design/        v0.2 설계 문서 19개
docs/design_v0.1/   원본 v0.1 (참고용, 수정 금지)
research/reports/   설계 리뷰 + 벤치마크 보고서 + P4 MCP 호출 효율 실측
research/data/      측정 원시 데이터
research/bench/     재현 가능한 측정 코드 (Rust 4 / Python 10)
```

## 지원 형식 12종

코드 `.py` `.go` `.rs` `.ts` (Tree-sitter로 class/struct/trait/interface/function/method) ·
문서 `.md` `.rst` `.txt` (heading) ·
데이터 `.json` `.yaml` `.toml` (중첩 키 → JSON Pointer) `.csv` (헤더 컬럼) ·
노트북 `.ipynb` (코드 셀을 Python 파서로 재분석)

## 핵심 실측값

전부 실제 공개 저장소 18개(커밋 13만, 파일 23만)에서 측정했다. 합성 데이터 없음.
조건은 `docs/design/17_MEASUREMENT_BASIS.md` 참조.

| 지표 | 값 |
|---|---|
| 파일명 퍼지 검색 p95 | 7.4 ms @ 100K 파일 (2코어) |
| cold 인덱싱 | 18.5 s @ 100K 파일 |
| 전문 인덱스 / 원본 | 5.5 % |
| 정합성 스캔 | 281 ms @ 100K 파일 |
| 파일 링크 자동 복구율 | 95.7 % |
| 심볼 링크 복구율 | 자동 80.0 % → 1클릭 포함 93.6 % |
| 편집 중 심볼 보존 | Tree-sitter 96.9 % vs CPython ast 16.7 % |
| MCP 호출 효율 (자연어 질의 20건) | 평균 1.05회, 정답 20/20, grep 대비 토큰 17.6배 절감 |

CI 게이트는 자동 복구율 기준이다 — 합산 심볼 > 78%, 파일 > 93%, 저장소별 심볼 > 63%, 파일 > 47%.
1클릭 포함 수치는 제품 목표이지 게이트가 아니다.
