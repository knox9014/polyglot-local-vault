# Polyglot Local Vault v0.1 — 설계 리뷰

리뷰 대상: `Polyglot_Local_Vault_Design_v0.1` (16개 문서, 1,564줄)
관점: 아키텍처 / 데이터 모델 / 성능 / 구현 리스크

---

## 0. 총평

**제품 방향은 견고하다.** 특히 다음 세 가지 판단은 이 종류의 프로젝트가 보통 실패하는 지점을 정확히 피해갔다.

- AI를 Core에서 제거하고 MCP 클라이언트로 격하 → 재현성 확보, 모델 종속 회피
- 원본 파일 + sidecar 인덱스 → lock-in 없음, 삭제해도 사용자 손실 없음
- Edge에 `origin` 부여 → 관계의 신뢰 수준을 시스템이 구분 가능

**하지만 문서 전체가 "무엇을 만들 것인가"에서 멈춰 있고, "이게 왜 어려운가"에 대한 답이 없다.**
설계서라기보다 아직 잘 정리된 요구사항 명세에 가깝다. 진짜 위험한 결정 4개(Stable ID, 저장 계층 분리, 파서 인프라, 콜드스타트)가 전부 `12_ENGINEERING_DECISIONS_TODO.md`로 미뤄져 있는데, 이것들은 **구현 중에 정하는 게 아니라 데이터 모델을 결정하는 항목**이다. 순서가 뒤집혀 있다.

아래는 심각도 순.

---

## 1. 치명적 — Manual Link가 캐시 디렉터리에 들어 있다

문서 11은 이렇게 말한다.

> `.vault-ai/`는 중요한 캐시/메타데이터이지만 원본 파일보다 우위의 source of truth가 되어서는 안 된다.
> `.vault-ai/`가 삭제되어도 원본 파일을 다시 스캔해 기본 인덱스와 Graph를 재구축할 수 있어야 한다.

그런데 문서 03의 저장 구조를 보면 `graph/`가 `.vault-ai/` 안에 있다. 그리고 문서 07·14는 **manual link가 제품의 가장 중요한 자산**이라고 못박는다.

**모순이다.** Manual link는 재스캔으로 복구가 **불가능한 유일한 데이터**다. 파서 결과는 파일에서 다시 만들면 되고, git edge도 다시 만들면 된다. 오직 사용자가 손으로 건 링크만 원본에 존재하지 않는다. 그걸 "삭제해도 되는 캐시" 폴더에 넣어놨다.

### 권고: 저장소를 2계층으로 명시 분리

```
프로젝트/
├── .vault/                  ← SOURCE OF TRUTH. git에 커밋. 사람이 읽을 수 있음
│   ├── links.jsonl          ← manual edges (append-only)
│   ├── aliases.jsonl        ← ID 이력 (rename/move 추적)
│   └── vault.toml           ← ignore 규칙, 설정
└── .vault-ai/               ← 순수 파생물. .gitignore. 언제든 삭제 가능
    ├── index/               ← 검색 인덱스
    ├── parsed/              ← 파서 결과 캐시
    └── state/               ← watcher 커서, 스키마 버전
```

이렇게 하면 얻는 것:

- `.vault-ai/` 삭제 = 재인덱싱만 하면 완전 복구 (문서 11의 요구가 실제로 성립)
- manual link가 git에 남음 → 히스토리, diff, 팀 공유가 공짜로 따라옴 (문서 13의 협업 확장 경로가 자동으로 열림)
- JSONL이므로 merge conflict가 라인 단위 → 협업 시 해결 가능

`links.jsonl` 한 줄 예시:

```json
{"from":"vault://docs/architecture.md#teacher-router","rel":"describes","to":"vault://src/router.py#TeacherRouter","created":"2026-08-16T09:00:00Z"}
```

---

## 2. 치명적 — Stable ID 문제를 실제보다 10배 크게 잡고 있다

문서 04·12·15가 반복해서 걱정하는 항목인데, 답 없이 "구현 단계에서 검토"로 남아 있다. 후보로 나열된 것(경로 / file identity / symbol signature / content hash / alias table)은 각각 다음 이유로 단독으로는 전부 실패한다.

| 방식 | 깨지는 상황 |
|---|---|
| relative path | move / rename |
| inode·FileID | 복사, 재클론, 일부 에디터의 atomic save(write+rename), Windows/Unix 상이 |
| content hash | 파일을 한 글자만 고쳐도 |
| symbol signature | 함수 시그니처 수정 |

**핵심 통찰: 모든 노드에 Stable ID가 필요한 게 아니다.**

파서가 만든 노드(`CodeClass`, `JSONProperty`, `NotebookCell` …)는 파일에서 결정론적으로 재생성되는 파생물이다. 재생성되면 그만이므로 안정성이 필요 없다 — 경로+심볼 경로로 그때그때 계산하면 된다.

**Stable ID가 실제로 필요한 대상은 "manual link의 양 끝점"뿐이다.** 그리고 그 개수는 사용자가 손으로 건 링크 수, 즉 수백~수천 개 수준이다. 10만 파일 전체에 대한 문제가 아니라 수천 개에 대한 문제다.

### 권고: 논리 주소 + 지연 해석(lazy resolution)

링크는 사람이 읽을 수 있는 논리 주소로 저장하고, 해석은 조회 시점에 계단식으로 시도한다.

```
저장:  vault://src/router.py#TeacherRouter.select_teacher

해석 순서:
1. 경로 그대로 존재하고 심볼도 존재  → HIT (99% 케이스, 비용 0)
2. 경로 없음 → aliases.jsonl 조회      → HIT
3. 경로 없음 → 같은 심볼명 유일 매칭   → 후보 제시 (사용자 1클릭 확정)
4. 전부 실패 → BROKEN 상태로 링크 보존, UI에 표시
```

여기서 중요한 건 **4번**이다. 깨진 링크를 조용히 삭제하지 않고 명시적으로 보여줘야 한다. 사용자가 고칠 수 있는 정보를 시스템이 버리면 안 된다.

alias는 두 소스에서 자동 축적한다.

- **Watcher rename 이벤트**: OS가 rename을 알려줄 때 (신뢰도 높지만 항상 오지는 않음, §5 참조)
- **`git log --follow` / rename detection**: git이 이미 유사도 기반 rename 추적을 해준다. 이걸 안 쓰는 건 낭비다. 문서 10의 Phase 6(Git)을 Phase 0으로 당길 근거가 여기 있다.

---

## 3. 치명적 — Semantic Web의 콜드스타트 문제

이게 제품 자체를 죽일 수 있는 지점인데 문서 어디에도 언급이 없다.

- 문서 02·07·14: **manual link가 가장 중요한 의미 관계다**
- 문서 02·08: **AI는 Graph를 자동 수정하지 않는다**

두 원칙을 합치면: **사용자가 vault를 처음 열었을 때 semantic web은 완전히 비어 있다.**

보이는 건 파서가 만든 구조 관계(`defined_in`, `imports`, `contains`)뿐인데, 그건 IDE가 이미 다 해주는 것들이다. 사용자 입장에서 "Graph 탭"을 열면 자기가 아는 파일 구조가 동그라미로 그려져 있을 뿐이다. **차별점이라고 선언한 기능이 Day 1에 가치가 0이다.**

그리고 링크를 걸어야 가치가 생기는데, 링크를 거는 건 노동이고, 노동의 대가가 아직 안 보이는 상태에서 사용자는 링크를 걸지 않는다. 전형적인 콜드스타트 데드락이다. Obsidian이 이걸 `[[wikilink]]`라는 **글 쓰는 김에 자연스럽게 링크가 생기는** 문법으로 풀었다는 점을 참고할 만하다.

### 권고: "제안(Suggestion)" 계층을 Core에 추가

원칙("AI가 Graph를 자동 수정하지 않는다")을 깨지 말고, **결정론적 휴리스틱으로 후보를 생성**해서 사용자에게 1클릭 승인을 요구한다. AI가 아니므로 원칙 위반이 아니다.

승인 전에는 `.vault/links.jsonl`에 쓰지 않고 `.vault-ai/suggestions`에만 둔다 → 캐시이므로 안전하다.

v0.1에 넣을 만한 결정론적 제안기:

| 제안기 | 규칙 | 예상 정확도 |
|---|---|---|
| Docstring ↔ 심볼 | 파이썬 docstring에 등장하는 다른 심볼명 | 높음 |
| Markdown 코드 참조 | 문서 본문의 `` `TeacherRouter` `` 백틱 토큰이 vault 내 유일 심볼과 일치 | 높음 |
| 경로 문자열 | JSON/YAML 값이 vault 내 실존 경로 | 매우 높음 |
| 설정 키 ↔ 코드 | `config["router"]["threshold"]` 형태 접근 → JSON Pointer 매칭 | 중간 |
| 동시 변경 | 같은 커밋에 N회 이상 함께 수정된 파일 쌍 | 중간 |
| 노트북 ↔ import | `.ipynb`가 import 하는 로컬 모듈 | 매우 높음 |

마지막 두 개는 git과 파서에서 공짜로 나온다. 첫 사용 시 "48개의 관계 후보를 찾았습니다" 화면을 보여줄 수 있으면 콜드스타트가 사라진다.

`origin` enum도 이에 맞춰 확장해야 한다. 현재 `manual|parser|git|imported`인데 `suggested`와 **confidence 필드**가 필요하다 (§4 참조).

---

## 4. 중대 — `origin=parser` 안에 확실한 것과 불확실한 것이 섞여 있다

문서 06은 Python 파서 추출 대상에 `calls`, `inheritance`를 포함시켰다. 그런데 이 둘은 정적 분석으로 확정할 수 없다.

```python
self.router.select()      # self.router의 타입을 모름 → 어느 select()인지 모름
handler = HANDLERS[key]   # 동적 디스패치 → 추적 불가
class Foo(Base):          # Base가 외부 패키지면 해석 불가
obj.run()                 # 이름만 같은 run()이 vault에 12개 있을 수 있음
```

타입 추론 없이 이름 매칭으로 call edge를 만들면 **오탐이 정탐보다 많아진다.** 그런데 이게 `defined_in`(100% 확실)과 같은 `origin=parser` 라벨을 달고 저장되면, 문서 11이 강조한 "관계의 신뢰 수준 판단"이 무의미해진다. 사용자는 Related 패널에 뜬 쓰레기를 보고 기능 전체를 불신하게 된다.

### 권고

**A. Edge에 confidence를 명시한다.**

```
certain    : defined_in, contains, parent_of, imports(정적 문자열), json_pointer
probable   : calls(이름 유일 매칭), inherits(vault 내 정의)
heuristic  : calls(이름 중복), co-change
```

UI 기본값은 `certain`만 표시. `probable` 이하는 필터로 켠다.

**B. v0.1 파서 범위를 줄인다.**

문서 06의 Python 추출 목록에서 `calls`를 빼는 것을 권한다. `imports`만으로도 의존 그래프는 충분히 유용하고, 정확도 100%다. `calls`는 v0.2에서 타입 추론(예: Jedi/Pyright 연동)과 함께 넣는 게 맞다.

> 문서 06에 이미 "초기에는 정적 분석으로 확실히 알 수 있는 관계만 저장한다"고 써 있다. 그런데 바로 위 목록에 `calls`가 들어 있다. 문서 내부에서 스스로 모순된다.

---

## 5. 중대 — File Watcher를 이벤트 스트림으로만 신뢰하고 있다

문서 03·05는 "File Watcher → 변경 파일만 재파싱"을 전제로 삼는다. 문서 12는 rename detection을 평가 기준으로 언급하지만, **이벤트가 유실된다는 사실 자체**를 다루지 않는다.

현실:

- **inotify(Linux)**: 디렉터리당 watch 필요, 기본 상한 `max_user_watches` = 8192. 대형 모노레포에서 조용히 초과 → 일부 디렉터리 감시 안 됨. 큐 오버플로 시 `IN_Q_OVERFLOW` 후 이벤트 유실.
- **FSEvents(macOS)**: 디렉터리 단위 coalescing, 파일 단위 정확도 낮음. 슬립/복귀 시 유실.
- **ReadDirectoryChangesW(Windows)**: 버퍼 오버플로 시 통째로 유실.
- **rename**: inotify는 `IN_MOVED_FROM`/`IN_MOVED_TO`를 cookie로 짝지어야 하는데, 짝의 한쪽이 감시 범위 밖이면 영원히 안 옴 → delete + create로 보임 → **링크 끊김**.
- **이벤트 폭풍**: `git checkout`, `npm install`, 빌드 산출물 → 수만 건이 순식간에.

### 권고: Watcher + 주기적 Reconciliation

Watcher는 **저지연 힌트**로만 쓰고, **정합성은 별도로 보장**한다.

```
정상:    watcher 이벤트 → 100~300ms debounce → 변경분 재파싱
보정:    앱 시작 시 + 유휴 시(N분) → mtime/size 기반 얕은 스캔
         → 인덱스와 diff → 누락분 처리
폭풍:    단위 시간당 이벤트 > 임계값 → watcher 일시 중단
         → 해당 서브트리 전체 재스캔으로 전환
```

`(path, mtime_ns, size)` 튜플만 비교하는 스캔은 10만 파일에서 1초 미만이다. 이 안전망이 없으면 "인덱스가 가끔 틀린 검색기"가 되는데, 그건 검색기로서 사망이다.

---

## 6. 중대 — 검색 스택이 중복 설계되어 있다

문서 05는 7개 인덱스(Filename / Path / Extension / Full-text / Symbol / Metadata / Graph)를 나열하고, 문서 12는 후보 기술로 SQLite FTS / Tantivy / FST / custom inverted / mmap을 병렬로 놓았다. 이대로 가면 인덱스 5종을 각각 관리하면서 **원자적 갱신**을 맞춰야 한다 — 유지보수 지옥이다.

**사실 관계 정리:**

- Tantivy의 term dictionary는 **이미 FST**다. 별도 FST를 만드는 건 중복이다.
- Filename / Path / Extension은 별도 인덱스가 아니라 **같은 문서의 필드**다.
- Metadata 필터(`modified:7d`)는 Tantivy fast field로 해결된다.
- Graph만 성격이 다르다(재귀 순회) → SQLite.

**그리고 가장 중요한 것:** 파일명 검색에는 인덱스가 필요 없다.

10만 파일의 경로 문자열 전체는 메모리로 대략 10MB다. 통째로 올려놓고 선형 스캔하면 퍼지 매칭 포함 **수 ms**에 끝난다. VS Code의 Ctrl+P, fzf, Everything이 전부 이 방식이다. 인덱스를 타면 오히려 느리고, 무엇보다 **퍼지 매칭이 안 된다**(`trrt` → `TeacherRouter` 같은 부분 문자 매칭은 역인덱스로 표현 불가).

### 권고: 3층 구조로 압축

```
1. In-memory path table   → 파일명/경로/확장자/퍼지 (전체 스캔, ~5ms)
2. Tantivy 단일 인덱스    → 본문 + 심볼 + 메타데이터 (필드 분리)
3. SQLite                 → 노드/엣지/alias/링크 (재귀 CTE로 순회)
```

문서 05의 "저비용부터 즉시 반환" 아이디어는 훌륭한데, 이 3층과 정확히 대응시키면 구현이 단순해진다. 1층 결과를 첫 프레임에 그리고, 2·3층은 async로 채운다.

---

## 7. 중대 — Python AST vs Tree-sitter 판단이 뒤집혀 있다

문서 12: "Python AST는 native AST 가능, 다중 언어 확대 시 Tree-sitter가 유리"

이건 **v0.1에서 파서를 두 벌 만들고 v0.2에서 갈아엎겠다**는 뜻이다. 그리고 native AST 선택에는 문서가 언급하지 않은 비용이 있다.

| | CPython `ast` | Tree-sitter |
|---|---|---|
| 문법 오류 파일 | **파싱 실패 (전부 잃음)** | 부분 트리 복구 |
| 코어가 Rust/Go일 때 | **Python 인터프리터 임베드 필요** | 단일 바이너리, C 라이브러리 |
| 증분 파싱 | 불가 (전체 재파싱) | 편집 구간만 |
| 언어 추가 | 언어마다 새 파서 | 문법 파일 교체 |
| 정밀도 | 높음 (데코레이터/스코프 해석) | 구문 수준 |

**문법 오류 항목이 결정적이다.** 사용자는 편집 중인 파일을 저장한다. 편집 중인 파일은 절반은 문법 오류 상태다. CPython `ast`는 그 순간 심볼을 **전부** 잃는다 → Related 패널이 깜빡이며 비었다 채워진다. 에디터 통합 제품에서 이건 치명적인 체감 품질 저하다.

또한 코어 언어 후보가 Rust/C++/Go(문서 12)인데 여기에 CPython을 임베드하면 배포 크기·플랫폼별 빌드·GIL 문제가 전부 따라온다. Tauri 앱에 Python 런타임을 번들하는 순간 이식성이 무너진다.

### 권고

**Tree-sitter로 통일하고 시작한다.** v0.1 추출 목표(module/class/function/method/import/변수)는 tree-sitter 쿼리로 전부 커버된다. 정밀 해석이 필요해지면 그때 언어별 백엔드를 **추가**하면 되고, 파서 어댑터 인터페이스(문서 06)가 이미 그걸 허용한다.

---

## 8. 중요 — CSV Row 정책 (문서가 질문만 하고 답이 없음)

문서 06: "모든 row를 Node로 만드는 것은 비효율적일 수 있으므로 정책이 필요하다."

100만 행 CSV 하나가 노드 100만 개를 만들면 그래프 저장소가 즉사한다. 그런데 특정 행에 링크를 걸고 싶은 요구는 실재한다.

### 권고: Materialize-on-link (가상 주소 + 지연 실체화)

- **주소는 항상 존재한다**: `vault://data/x.csv#row:1042` 는 언제나 유효한 주소다. 저장소를 조회하지 않고 파일을 직접 seek 해서 해석한다.
- **노드는 링크가 걸릴 때만 생긴다**: 사용자가 그 행에 실제로 링크를 걸면 그때 노드 1개를 DB에 만든다.
- **검색은 별개다**: 행 내용은 Tantivy에 넣되 노드가 아닌 "파일 내 위치"로만 인덱싱한다.
- **기본 노드는 스키마까지만**: `Table`, `Column`, 행 수, 추론 타입.

같은 원칙이 **Notebook cell, JSON 배열 원소, Markdown 리스트 항목**에도 적용된다. 즉 문서 04의 Node 목록은 "저장되는 것"이 아니라 **"주소 지정 가능한 것"** 의 목록으로 재정의되어야 한다. 이 구분이 문서에 없다.

---

## 9. 중요 — MCP 툴이 너무 많고, 주소 왕복이 설계되어 있지 않다

문서 08의 툴 11개(`vault.search`, `read`, `list`, `symbols`, `graph`, `references`, `backlinks`, `dependencies`, `history`, `create_link`, `remove_link`).

문제:
- **툴이 많을수록 모델의 선택 정확도가 떨어진다.** `symbols` / `references` / `backlinks` / `dependencies` / `graph`는 모델 입장에서 구분이 모호하다. 잘못 고르고, 다시 고르고, 왕복이 늘어난다.
- 문서 12도 "툴 수를 늘리지 않아야 한다"고 적어놨는데 문서 08은 11개를 제안한다. **문서 간 불일치.**

### 권고: 4개로 압축

```
vault.search(query, kind?="auto"|"file"|"symbol"|"text", filters?, limit?)
vault.read(uri, mode?="full"|"outline"|"range")
vault.neighbors(uri, rel?[], depth?=1, direction?="both")
vault.link(action="propose"|"remove", from, to, rel)   # 승인 필요
```

`references` / `backlinks` / `dependencies`는 전부 `neighbors`의 `rel` 필터다. `symbols`는 `search(kind="symbol")`이다. `history`는 `neighbors(rel=["changed_in"])`이다.

### 그리고 반드시 지켜야 할 것: 주소 왕복성

**모든 툴의 응답에 `vault://` URI가 포함되어야 한다.** 이게 없으면 AI는 `search` 결과에서 다음 `neighbors` 호출을 만들 수 없고, 결국 파일 전체를 읽으려 든다 — 문서 08이 막으려던 바로 그 행동이다.

```json
{
  "uri": "vault://src/router.py#TeacherRouter",
  "type": "CodeClass",
  "range": {"start": 42, "end": 118},
  "preview": "class TeacherRouter:\n    \"\"\"라우팅 정책...\"\"\"",
  "neighbors_hint": {"imports": 3, "described_by": 1, "tested_by": 1}
}
```

`neighbors_hint`처럼 **다음 호출의 가치를 미리 알려주는 필드**를 넣으면 AI의 탐색 효율이 크게 올라간다. 이건 MCP 서버 설계에서 실제로 효과가 큰 패턴이다.

**Write safety 구멍**: 문서 08의 승인 흐름(제안→승인→write)에서 "AI가 제안"과 "사용자가 승인" 사이에 시간 차가 있다. 그 사이 AI가 다른 제안을 밀어 넣으면 사용자가 무엇을 승인했는지 불명확해진다. → 제안에 **불변 ID를 발급**하고, 승인은 그 ID에 대해서만 수행되어야 한다.

---

## 10. 중요 — Phase 순서가 뒤집혀 있다

문서 10:

```
Phase 3 — Semantic Graph (manual links 포함)
Phase 4 — Workspace (File tree, Editor, Related panel)
```

**Manual link를 만들려면 링크를 걸 UI가 필요하다.** Phase 3에서 manual link를 구현해도 Phase 4가 끝나기 전엔 아무도 링크를 걸 수 없다. 즉 Phase 3이 끝난 시점에 검증할 수 있는 게 없다.

또 문서 15는 "20회 검토"를 언급하지만, 각 phase의 **종료 조건(exit criteria)** 이 없다. "Phase 1 완료"를 무엇으로 판정하는지 정의되지 않았다.

### 권고: 재배열 + 수치화된 종료 조건

```
P0  Vault + Watcher + Reconciliation + 링크 저장 포맷 확정
    exit: 10K 파일 vault에서 정합성 스캔 < 1s

P1  검색 (in-memory path + Tantivy) + 최소 UI (검색창 + 뷰어)
    exit: keystroke → 첫 결과 p95 < 16ms @ 10K files
    ★ 이 시점에서 이미 단독으로 쓸 만한 도구가 된다 ← 가장 중요

P2  Tree-sitter 파서 (md/py/json/yaml/csv/ipynb) + 심볼 검색
    exit: 심볼 검색 p95 < 50ms, 문법 오류 파일에서도 심볼 유지

P3  Workspace + Related 패널 + 링크 생성 UI + Graph 1-hop
    exit: 링크 생성 3클릭 이내

P4  Git + 제안 엔진 (콜드스타트 해소)
    exit: 실제 프로젝트 첫 오픈 시 제안 20개 이상, 승인율 50% 이상

P5  MCP (4 tools)
    exit: 외부 AI가 "X와 연결된 설정 찾아줘"를 3회 호출 이내로 해결
```

핵심은 **P1 종료 시점에 이미 출시 가능한 제품**이 되도록 자르는 것이다. 지금 로드맵은 P4까지 가야 뭔가 보인다.

---

## 11. 성능 목표가 정성적이다

문서 05는 측정 항목을 잘 나열했지만(cold indexing, warm search, keystroke-to-first-result, memory, disk size) **목표 수치가 없다.** "빨라야 한다"는 KPI가 아니다. 문서 02가 검색 속도를 "핵심 KPI"라고 선언한 것과 배치된다.

### 권고: 아래를 문서 05에 못박는다

| 지표 | 목표 (10K files) | 목표 (100K files) |
|---|---|---|
| Keystroke → 첫 결과 (p95) | < 16ms (1 frame) | < 30ms |
| Cold full index | < 15s | < 3min |
| 파일 저장 → 인덱스 반영 | < 200ms | < 500ms |
| 정합성 스캔 | < 1s | < 5s |
| RSS (idle) | < 150MB | < 400MB |
| 인덱스 크기 / 원본 크기 | < 25% | < 25% |
| 검색 취소 반응 | < 5ms | < 5ms |

**16ms의 근거**: 60fps에서 한 프레임. 이 안에 첫 결과가 나오면 사용자는 "즉각적"으로 느끼고, 넘어가면 "타이핑이 밀린다"고 느낀다. 문서 09의 `r → ro → rou → rout` UX가 성립하려면 이 수치가 필수다.

---

## 12. 문서에서 빠진 항목 (전부 실제 구현 시 부딪힘)

| 항목 | 왜 문제가 되는가 |
|---|---|
| **인덱스 스키마 버전** | 앱 업데이트 시 인덱스 포맷이 바뀌면? 전체 재인덱싱 강제 vs 마이그레이션. 버전 필드 없으면 크래시 |
| **다중 인스턴스** | 앱 2개가 같은 vault를 열면 인덱스 동시 쓰기 → 손상. 락 파일 필요 |
| **심볼릭 링크** | 순환 링크 → 무한 스캔. vault 밖을 가리키는 링크 처리 |
| **인코딩** | UTF-8 아닌 파일(CP949 등), BOM, 잘못된 바이트. 한국어 사용자면 CP949는 반드시 만난다 |
| **Windows 경로** | 대소문자 무시 파일시스템, 260자 제한, `\` vs `/` — Stable ID 정규화에 직결 |
| **삭제된 파일의 링크** | tombstone으로 남길지, BROKEN 표시할지 (§2의 4번 케이스) |
| **대용량 파일 임계값** | 문서 05가 "별도 정책"이라 했지만 수치 없음. 권장: 본문 인덱싱 1MB, 파서 5MB 상한 |
| **Undo** | 링크 삭제 되돌리기. append-only JSONL이면 공짜로 얻음 (§1의 부수 효과) |
| **바이너리 판별** | "binary file 제외"라 했지만 판별 기준 없음. 권장: 첫 8KB 내 NUL 바이트 |
| **ignore 규칙 문법** | `.gitignore` 문법 재사용 여부. 재사용을 강력히 권장 (학습 비용 0, `node_modules`/`.venv` 자동 제외) |

---

## 13. 제품 포지셔닝에 대한 한 가지 우려

문서 01은 차별점을 "Polyglot first-class semantic objects"로 고정했다. 개념적으로는 명확한데, **사용자가 이걸 왜 원하는지에 대한 시나리오가 `TeacherRouter` 예시 하나뿐**이다.

인접 도구들이 이미 각 축을 차지하고 있다.

- 파일 검색: fzf, Everything, VS Code Ctrl+P (매우 빠르고 무료)
- 코드 심볼: LSP, ctags, Sourcegraph (이미 정확도가 높음)
- 문서 링크: Obsidian (링크 UX가 성숙)
- 통합 검색: Glean 등 (엔터프라이즈)

**진짜 빈 곳은 "코드와 문서 사이의 링크"** 한 군데다. 이건 실제로 아무도 잘 못 하고 있고, 개발자가 실제로 아파하는 지점이다("이 설계 문서가 어느 코드 얘기인지 모르겠다", "이 config 값이 어디서 쓰이는지 모르겠다").

그렇다면 **v0.1의 마케팅 메시지는 "polyglot vault"가 아니라 "문서와 코드를 잇는다"** 여야 한다. 그리고 §3의 제안 엔진 중 **Markdown 백틱 토큰 → 심볼 매칭**이 그 가치를 즉시 증명하는 단일 기능이다. 이 하나만 잘 돌아가도 제품이 성립한다.

기술 문서 없이 "무엇이든 연결하는 도구"로 포지셔닝하면, 사용자는 뭘 연결해야 할지 모른 채 빈 그래프를 보게 된다.

---

## 14. 우선 처리 순서

| 순위 | 항목 | 이유 |
|---|---|---|
| 1 | `.vault/` vs `.vault-ai/` 분리 (§1) | 데이터 유실 방지. 나중에 바꾸면 마이그레이션 필요 |
| 2 | 논리 주소 + lazy resolution 확정 (§2) | 모든 저장 포맷이 여기에 의존 |
| 3 | 제안 엔진을 v0.1 범위에 포함 (§3) | 없으면 핵심 가치가 Day 1에 0 |
| 4 | Tree-sitter 단일 채택 (§7) | 늦게 바꾸면 파서 전량 재작성 |
| 5 | 검색 3층 구조 확정 (§6) | 인덱스 5종 관리 지옥 회피 |
| 6 | Watcher reconciliation (§5) | 정확성 문제. 조용히 틀리는 게 최악 |
| 7 | 성능 목표 수치화 (§11) | 기술 선택의 판단 기준이 됨 |
| 8 | Phase 재배열 (§10) | P1에 출시 가능한 제품이 나오게 |
| 9 | MCP 4툴 압축 (§9) | 상대적으로 늦게 정해도 됨 |
| 10 | 누락 항목 결정 (§12) | 구현 중 순차 처리 가능 |

---

## 15. 다음 단계 제안

문서로 더 논의하는 것보다, **가장 위험한 가정 2개를 코드로 30분 안에 검증**하는 걸 권한다.

**검증 1 — 검색 속도 가정**
10만 개 경로를 생성해서 in-memory 퍼지 매칭 벤치. 16ms 목표가 현실적인지, Tantivy가 필요한 구간이 어디부터인지 실측. → §6, §11의 근거 확보

**검증 2 — Stable ID 회복률**
실제 git 저장소의 히스토리를 재생해서, rename/move 발생 시 §2의 계단식 해석이 몇 %를 복구하는지 측정. → §2 전략의 타당성 검증

두 실험 모두 실제 앱 없이 독립 스크립트로 가능하며, 결과에 따라 §14의 1·2·4·5번 결정이 확정된다.
