# Polyglot Local Vault

컴퓨터의 다양한 파일을 하나의 로컬 Vault에서 초고속으로 검색·탐색·연결하고,
파일 내부 구조까지 의미 있는 객체로 관리하는 로컬 데스크톱 Workspace.

**현재 상태: 설계 v0.2 완료 + 실측 10건 완료. 구현 코드 0줄.**

## 시작하기

Claude Code에서 이 디렉터리를 열면 `CLAUDE.md` 를 먼저 읽는다.
사람이 읽을 순서는 다음과 같다.

1. `CLAUDE.md` — 현재 상태, 확정 결정, 기각된 설계, 다음 할 일
2. `TODO.md` — 미해결 blocker 6 / major 7 / minor 3
3. `docs/design/00_README.md` — 설계 문서 패키지 개요
4. `docs/design/17_MEASUREMENT_BASIS.md` — 모든 수치의 출처와 한계

## 구조

```
docs/design/        v0.2 설계 문서 18개
docs/design_v0.1/   원본 v0.1 (참고용, 수정 금지)
research/reports/   설계 리뷰 + 벤치마크 보고서 3종 + v0.2 검토 결과
research/data/      측정 원시 데이터
research/bench/     재현 가능한 측정 코드 (Rust 4 / Python 10)
```

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
| 심볼 링크 복구율 | 자동 80.0 % → 1클릭 포함 93.7 % |
| 편집 중 심볼 보존 | Tree-sitter 96.9 % vs CPython ast 16.7 % |

## 다음 작업

`docs/design/18_DATA_FORMATS.md` 작성. 검토에서 나온 blocker 6건 중 5건이 여기로 수렴한다.
`.vault/` 는 git 커밋 대상이고 사용자 링크가 들어가는 곳이라 나중에 바꾸면 마이그레이션이 필요하다.

**이 문서를 쓰기 전에는 Phase 0을 시작하지 않는다.**
