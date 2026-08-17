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
정합성 스캔 < 500ms @ 100K 파일 (2코어) → `17`: 281ms (→ `12_ENGINEERING_DECISIONS.md` CI 게이트와 동일 값)
git rename 체인이 aliases.jsonl 로 수집됨
.vault-ai/ 삭제 후 재인덱싱으로 완전 복구
```

이전 버전은 "10K 파일 < 1s"였다. 실측이 100K에서 281ms라 10K 기준 1s는 약 35배 느슨해 아무것도 걸러내지 못했다. `12`가 이미 확정한 CI 게이트(100K, 2코어, < 500ms)를 그대로 가져온다.

## Phase 1 — Fast Search + 최소 UI

우선순위가 가장 높다.

- In-memory path table (1층)
- 파일명 스코프 기본 / `/` 포함 시 경로 확장
- 역인덱스 (2층) — 본문
- Incremental indexing
- 검색창 + 뷰어 (최소 UI)

**종료 조건**

```text
keystroke → 첫 결과 p95 < 16ms @ 100K 파일 (파일명 스코프, 2코어) → `17`: 7.4ms
cold 인덱싱 < 30s @ 100K 파일 (2코어, 단일 스레드) → `17`: 18.5s
인덱스 크기 / 원본 < 10% (위치 정보 제외) → `17`: 5.5%
```

세 항목 모두 하드웨어 조건(2코어, `17`의 측정 환경이자 CI 러너 사양)을 명시했다. 값 자체는 `12_ENGINEERING_DECISIONS.md`의 CI 게이트와 동일하다.

> **이 시점에서 이미 단독으로 출시 가능한 제품이 된다.** 빠른 로컬 파일 검색기만으로도 가치가 있다. 이후 Phase는 그 위에 쌓는다.

## Phase 2 — Parsers + Symbol Search

- Tree-sitter 백엔드
- `.md` `.rst` `.txt` `.py` `.go` `.ts` `.rs` `.json` `.yaml` `.toml` `.csv` `.ipynb`
- 심볼 인덱스 (2층)
- 심볼 본문 shingle 스케치 (S4용)
- 부분 파싱 결과 반영

**종료 조건**

```text
심볼 검색 p95 < 50ms @ 100K 파일 (2코어)
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
실제 프로젝트(flask, 236 파일 — `17` 고정 저장소 중 최소 규모) 첫 오픈 시 제안 후보 300건 이상
  → `16` "생성량": R2~R5 합계 438건, 여유 138건
심볼 링크 자동 복구율 > 78% (고정 저장소 회귀 테스트, 2코어 → `17`: 실측 80.0%, 여유 2.0%p)
```

이전 버전은 "실제 프로젝트"가 어느 저장소인지 지정하지 않았다. `17`의 링크 복구 코퍼스 13개 중 flask(236 파일)를 골랐다 — R1만으로는 TypeScript가 12건에 그치는 저장소도 있어(→ `16`) R1 단독 수치로 임계값을 잡으면 깨진다. R2~R5를 합친 flask의 438건(→ `16` "생성량")을 기준으로 여유를 뒀다.

1클릭 포함 심볼 링크 복구율(93.7%, → `17`)은 제품 품질 목표이며 CI 종료 조건이 아니다 — 후보를 사람이 확정해야 나오는 수치라 무인 CI로 잴 수 없다. → `14_LOCKED_DECISIONS.md` "성능 목표".

**승인율 게이트 (실사용, CI 아님).** Phase 3 실사용에서 첫 100건의 제안 중 **30건 이상 승인**되어야 Phase 3를 종료된 것으로 본다. 이 값은 자동으로 측정할 수 없다 — 판정은 사람이 직접 한다. `17` "측정의 한계" §3이 밝히듯 자동 정밀도 지표 두 개가 실패했고 정밀도·승인율 모두 미측정이다. 따라서 여기서도 자동 정밀도 지표를 새로 만들지 않는다. **30건/100건(30%)은 실측 근거가 없는 초기값이다** — `16_SUGGESTION_ENGINE.md`가 초기 노출 상한 50건을 근거 없는 초기값으로 명시한 것과 같은 방식이다. Phase 3 실사용 데이터가 쌓이면 이 값을 조정한다. `01`이 R1을 "이 하나만 잘 돌아가도 제품이 성립한다"고 하는 핵심 주장에 대해, 후보의 **양**만 재던 이전 종료 조건에는 이 게이트가 없었다.

## Phase 4 — MCP

- 4개 툴 (`search` / `read` / `neighbors` / `link`)
- 모든 응답에 `vault://` 주소 포함 (주소 왕복성)
- 제안 불변 ID + 승인 흐름

**종료 조건**

```text
모든 툴 응답에 vault:// 주소 + neighbors_hint 포함 (구조 검증 — 결정론적, 모델 불필요)
read-only 기본, write는 명시적 승인 후에만
```

이전 버전의 "3회 호출 이내로 해결"은 평균인지 전건인지, 대상 모델이 무엇인지 불명이었다. 확인해보니 애초에 종료 조건 자격이 없다 — `08_MCP_AND_AI.md` "미검증 항목"이 이미 "툴 개수와 스키마가 모델의 호출 효율에 미치는 영향은 실제 모델 하네스 없이는 잴 수 없다"고 밝히고 있다. 측정 불가능한 것을 종료 조건에 두면 Phase가 끝나지 않는다. 대신 결정론적으로 검증 가능한 것 — 모든 응답이 다음 호출을 만들 수 있는 `vault://` 주소와 `neighbors_hint`를 실제로 포함하는지 — 로 바꿨다. "X와 연결된 설정 찾아줘" 류 질의의 평균 호출 횟수(목표 3회 이내)는 `08_MCP_AND_AI.md`가 이미 적어 둔 대로 Phase 4 실사용 측정 항목으로 남긴다.

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
