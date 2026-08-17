# Polyglot Local Vault — 문서 패키지 v0.2

## 프로젝트 한 줄 정의

**컴퓨터의 다양한 파일을 하나의 로컬 Vault에서 초고속으로 검색·탐색·연결하고, 파일 내부 구조까지 의미 있는 객체로 관리하며, 필요할 때 외부 AI가 MCP를 통해 이 지식 구조를 사용할 수 있게 하는 로컬 Workspace.**

이 프로젝트는 AI가 핵심 기능을 수행하는 앱이 아니다. 핵심 정리·검색·분석·관계 구성은 결정론적 알고리즘으로 수행하고, AI는 MCP를 통해 선택적으로 연결한다.

## v0.2에서 달라진 것

v0.1은 설계 검토 20회를 거쳐 방향을 정했으나, 되돌리기 비싼 결정 다수가 "구현 단계에서 벤치마크 후 선택"으로 남아 있었다.

**v0.2는 그 벤치마크를 수행한 결과다.** 실제 공개 저장소 18개, 커밋 약 13만 건, 파일 23만 개로 측정 10건을 수행했다. 합성 데이터를 쓰지 않았다.

주요 변경:

| | v0.1 | v0.2 |
|---|---|---|
| 파서 백엔드 | "Python은 native AST 가능" | **Tree-sitter 단일** (편집 중 심볼 보존 96.9% vs 16.7%) |
| 저장소 | `.vault-ai/` 단일 | **`.vault/` + `.vault-ai/` 2계층** |
| 링크 주소 | "Stable ID 전략 필요" | **논리 주소 + 계단식 해석** (파일 95.7% / 심볼 93.7%) |
| 검색 스코프 | 경로 전체 | **파일명 기본** (100K에서 18.8ms → 7.4ms) |
| Git | Phase 6 | **Phase 0** (복구율 7.5~9.6%p 담당) |
| 콜드스타트 | 미해결 | **Suggestion Engine 신설** |
| call edge | `calls` 전부 | **bare/self만** (attr 유일률 30.3%) |
| MCP 툴 | 11개 | **4개** + 주소 왕복성 |
| 성능 목표 | "빨라야 한다" | **하드웨어 조건 포함 수치 + CI 회귀** |
| 문서 형식 | `.md` | **`.md` `.rst` `.txt`** |

이 표의 수치는 전부 요약이며 조건이 생략돼 있다. **정본은 `17_MEASUREMENT_BASIS.md` 하나다** — 수치를 고칠 일이 생기면 `17`을 고치고 이 표는 참조로 따라간다.

## 최상위 원칙

1. Local-first
2. Original-file-first
3. **Source-of-truth 분리**
4. Algorithm-first (부분 결과 허용)
5. Human-link-first
6. **Suggest, don't decide**
7. Polyglot
8. Search-first
9. AI-optional
10. MCP-first AI integration
11. Progressive Disclosure
12. Incremental by Default
13. **Trust but verify**
14. **Addresses resolve, not point**
15. **Measure before deciding**

굵게 표시한 것이 v0.2에서 추가되었다.

## 핵심 구성요소

- Fast Local Search Engine (3층)
- Polyglot Parser System (Tree-sitter)
- Semantic Graph Engine (origin + confidence)
- **Suggestion Engine** (결정론적, 콜드스타트 해소)
- **Git Reader** (링크 복구의 핵심)
- Local Workspace UI
- MCP Server (4 tools)
- Optional External AI

## 문서 구조

| 파일 | 내용 |
|---|---|
| `01_PRODUCT_VISION.md` | 제품 정의, 목표, 차별점, 포지셔닝 |
| `02_CORE_PRINCIPLES.md` | 변경하지 않을 최상위 설계 원칙 15개 |
| `03_SYSTEM_ARCHITECTURE.md` | 전체 아키텍처, 저장소 2계층, 모듈 |
| `04_VAULT_AND_DATA_MODEL.md` | Vault, Object, Graph IR, 주소 체계, 계단식 해석 |
| `05_FAST_LOCAL_SEARCH.md` | 3층 검색 엔진, 성능 목표 |
| `06_POLYGLOT_PARSERS.md` | 파일 형식별 Parser, Tree-sitter, call edge 정책 |
| `07_SEMANTIC_WEB_GRAPH.md` | 거미줄형 연결, origin/confidence |
| `08_MCP_AND_AI.md` | MCP 4툴, 주소 왕복성, write safety |
| `09_DESKTOP_UX.md` | UI, 제안 검토, BROKEN 링크 재지정 |
| `10_MVP_ROADMAP.md` | Phase 0~5, 수치 종료 조건 |
| `11_SECURITY_PRIVACY_RELIABILITY.md` | 보안·개인정보·신뢰성 |
| `12_ENGINEERING_DECISIONS.md` | **아직 결정되지 않은 것만** |
| `13_FUTURE_EXTENSIONS.md` | 초기 범위 밖의 장기 확장 |
| `14_LOCKED_DECISIONS.md` | 확정 의사결정 + 측정 근거 |
| `15_REVIEW_HISTORY.md` | v0.1 20회 검토 + v0.2 실측 개정 |
| `16_SUGGESTION_ENGINE.md` | **신규** — 콜드스타트 해법 |
| `17_MEASUREMENT_BASIS.md` | **신규** — 확정 수치의 출처와 한계 |
| `18_DATA_FORMATS.md` | **신규** — `vault://` 주소, `.vault/` 파일 형식, `rel` 어휘 |

## 확정 수치 요약

전부 실측값이다. 조건과 한계는 `17_MEASUREMENT_BASIS.md` 참조.

### 검색 (2코어 Xeon 2.1GHz, p95)

```text
파일명 퍼지 검색       7.4 ms  @ 100K 파일
경로 전체 퍼지 검색    8.9 ms  @  50K 파일
경로 테이블 메모리      15 MB  @ 100K 파일
cold 인덱싱           18.5 s  @ 100K 파일 (단일 스레드)
전문 인덱스 크기        5.5 %  of 원본 (doc ID만)
정합성 스캔           281 ms  @ 100K 파일
```

### 링크 복구 (약 10년 경과, 대상 생존 링크 기준)

```text
파일 링크   자동 95.7 %
심볼 링크   자동 80.0 %  → +1클릭 93.7 %
            언어별 자동 80.2~94.5 %, 최종 91.6~97.5 %
```

### 파서 (편집 중 파일)

```text
Tree-sitter  심볼 보존 96.9 %
CPython ast  심볼 보존 16.7 %   (함수 타이핑 중에는 0.0 %)
```

### Graph 품질

```text
call edge 유일 해석률
  foo()        73.0 %   → 생성
  self.foo()   75.3 %   → 생성
  obj.foo()    30.3 %   → 생성하지 않음

제안 후보 생성량   저장소당 12~6,180 건
```

## 현재 범위

초기 제품은 **컴퓨터 로컬 전용 데스크톱 앱**으로 제한한다.

초기 범위에서 제외:

- 클라우드 동기화 / 웹 앱 / 모바일 앱
- 팀 협업 / SaaS / 계정·로그인
- 서버형 Graph DB / Vector DB
- AI 자동 정리 / AI 자동 Graph 변경
- 구문(phrase) 검색 — 인덱스가 5.5% → 21.8%로 확대 (→ `17`)
- `obj.foo()` call edge — 유일 해석률 30.3% (→ `17`)

핵심 엔진은 인터넷과 AI 없이도 정상 작동해야 한다.

## 아직 답하지 못한 것

기술 리스크는 측정으로 크게 줄었으나 **제품 리스크는 그대로다.**

1. 제안 승인율 — 후보를 많이 만드는 것은 확인했으나 사용자가 몇 %를 승인하는지 모른다
2. 제안 정밀도 — 자동 지표 두 개가 실패했다. 수치를 제시하지 않는 것이 정직한 상태다
3. Search ranking 가중치 — 정답이 사용자 의도에 달려 벤치마크로 정할 수 없다
4. MCP 툴 효율 — 실제 모델 하네스 없이는 측정 불가
5. **제품-시장 적합성** — 모든 측정이 "어떻게 만들 것인가"였다. "만들 가치가 있는가"는 측정하지 않았다

1·2·5번은 P3 완료 후 실사용으로만 확인된다.

## 다음 단계

`10_MVP_ROADMAP.md` 의 **Phase 0 → Phase 1** 수직 슬라이스. P1 종료 시점에 이미 단독으로 쓸 만한 로컬 파일 검색기가 되도록 잘라두었다.
