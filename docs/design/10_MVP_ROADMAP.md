# 10. MVP Roadmap

## v0.1 목표

**Local Polyglot Vault + Fast Search + Structural Graph + MCP foundation**

## Phase 재배열의 이유

v0.1 로드맵은 두 가지 문제가 있었다.

1. **Phase 3(Semantic Graph)이 Phase 4(Workspace)보다 앞이었다.** manual link를 구현해도 링크를 걸 UI가 없으면 아무도 링크를 걸 수 없다. Phase 3 종료 시점에 검증할 수 있는 것이 없다.
2. **Phase 6(Git)이 마지막이었다.** 측정 결과 git rename detection이 파일 링크 복구의 7.5%p, 심볼 링크 복구의 9.6%p를 담당한다. 대규모 구조 개편 시에는 사실상 전부를 담당한다(전체 파일 이동 5개 사례에서 git alias만으로 64~98% 복구). Phase 0으로 올려야 한다.
3. **종료 조건이 없었다.** "Phase 1 완료"를 무엇으로 판정하는지 정의되지 않았다.

또한 **P1 종료 시점에 이미 단독으로 쓸 만한 도구**가 되도록 잘랐다. v0.1 로드맵은 P4까지 가야 무언가 보였다.

## Phase 0 — Foundations

- Vault open / create
- `.vault/` 와 `.vault-ai/` 2계층 저장소
- ignore 규칙 (`.gitignore` 문법 재사용, vendored 기본 패턴)
- File watcher + **주기 정합성 스캔**
- **Git reader** — rename 체인 수집 → `aliases.jsonl`
- 논리 주소 체계 + 계단식 해석 골격
- Parser Adapter 인터페이스
- 인덱스 스키마 버전 / 다중 인스턴스 락

**종료 조건**

```text
10K 파일 vault에서 정합성 스캔 < 1s
git rename 체인이 aliases.jsonl 로 수집됨
.vault-ai/ 삭제 후 재인덱싱으로 완전 복구
```

## Phase 1 — Fast Search + 최소 UI

우선순위가 가장 높다.

- In-memory path table (1층)
- 파일명 스코프 기본 / `/` 포함 시 경로 확장
- 역인덱스 (2층) — 본문
- Incremental indexing
- 검색창 + 뷰어 (최소 UI)

**종료 조건**

```text
keystroke → 첫 결과 p95 < 16ms @ 100K 파일 (파일명 스코프)
cold 인덱싱 < 30s @ 100K 파일
인덱스 크기 / 원본 < 10%
```

> **이 시점에서 이미 단독으로 출시 가능한 제품이 된다.** 빠른 로컬 파일 검색기만으로도 가치가 있다. 이후 Phase는 그 위에 쌓는다.

## Phase 2 — Parsers + Symbol Search

- Tree-sitter 백엔드
- `.md` `.rst` `.txt` `.py` `.go` `.ts` `.rs` `.json` `.yaml` `.csv` `.ipynb`
- 심볼 인덱스 (2층)
- 심볼 본문 shingle 스케치 (S4용)
- 부분 파싱 결과 반영

**종료 조건**

```text
심볼 검색 p95 < 50ms @ 100K 파일
문법 오류 파일에서 심볼 보존율 > 95%
```

## Phase 3 — Workspace + Graph + Suggestions

Graph와 UI와 제안 엔진을 **함께** 낸다. 셋 중 하나라도 빠지면 나머지의 가치가 검증되지 않는다.

- File tree / Editor / Related panel
- 링크 생성 UI
- Graph view (focus node 1-hop, confidence 필터)
- 계단식 해석 완성 (파일 3단 / 심볼 5단)
- BROKEN 링크 표시 + 일괄 재지정
- **Suggestion Engine** (R1~R5) + 승인/거절 UI

**종료 조건**

```text
링크 생성 3클릭 이내
실제 프로젝트 첫 오픈 시 제안 후보 100건 이상
심볼 링크 복구율 > 93% (고정 저장소 히스토리 회귀 테스트)
```

**미측정 검증 항목**: 제안 승인율. 이 시점에서 실사용으로 확인해야 한다. 승인율이 낮으면 후보가 많아도 콜드스타트는 풀리지 않는다. (→ `16_SUGGESTION_ENGINE.md`)

## Phase 4 — MCP

- 4개 툴 (`search` / `read` / `neighbors` / `link`)
- 모든 응답에 `vault://` 주소 포함 (주소 왕복성)
- 제안 불변 ID + 승인 흐름

**종료 조건**

```text
외부 AI가 "X와 연결된 설정 찾아줘"를 3회 호출 이내로 해결
read-only 기본, write는 명시적 승인 후에만
```

## Phase 5 — Git 확장

Phase 0에서 rename 체인만 읽었다면, 여기서 나머지를 붙인다.

- commit / diff / history / blame
- `changed_in` / `introduced_in` 관계
- 동시 변경 제안 (R4) 고도화

## v0.1에서 하지 않는 것

- Cloud sync
- Web app / Mobile
- Team collaboration
- Account / login / SaaS
- AI automatic organization
- AI automatic graph mutation
- mandatory embedding
- server-side databases
- **구문(phrase) 검색** — 위치 인덱스가 크기를 5.5% → 21.8%로 확대
- **`obj.foo()` call edge** — 유일 해석률 30.3%
- **타입 추론 백엔드** — v0.2에서 재검토

## 초기 성공 조건

사용자가 일반 프로젝트 폴더를 열고:

1. 빠르게 파일을 찾을 수 있다. *(P1)*
2. 파일 내부 symbol까지 찾을 수 있다. *(P2)*
3. 첫 화면에서 관계 후보를 제시받고 1클릭으로 승인할 수 있다. *(P3)*
4. 관련 객체를 직접 연결할 수 있다. *(P3)*
5. 관계를 Graph에서 탐색할 수 있다. *(P3)*
6. 파일을 옮기거나 이름을 바꿔도 링크가 유지된다. *(P0+P3)*
7. AI 없이 위 전부가 작동한다. *(P0~P3)*
8. MCP를 통해 외부 AI가 Vault를 읽을 수 있다. *(P4)*

6번이 v0.2에서 추가되었다. 측정으로 달성 가능함이 확인된 항목이며(파일 95.7%, 심볼 93.7%), 이것이 없으면 나머지가 시간이 지나면서 무너진다.
