# 18. Data Formats

`.vault/` 에 저장되는 것의 형식을 못 박는다.

이 문서가 구현보다 먼저 오는 이유는 하나다. `.vault/` 는 **git 커밋 대상이고 사용자가 실제로 건 링크가 들어간다.** 출시 후에 형식을 바꾸면 사용자 데이터를 마이그레이션해야 하는데, 링크는 몇 년에 걸쳐 쌓이므로 사실상 되돌릴 수 없다.

v0.2 검토의 blocker 중 **B1 · B1-b · B2 · B3 · B4 · B6** 이 이 문서로 수렴한다. (→ `TODO.md`)

---

## 1. `vault://` 주소

### 1.1 문법

```abnf
address    = "vault://" path [ "#" fragment ]
path       = segment *( "/" segment )
segment    = 1*pchar                      ; "/" 와 "#" 는 반드시 인코딩
fragment   = symbol / typed
symbol     = qualname                     ; 접두사 없음 = 심볼
typed      = prefix ":" value / pointer
prefix     = "h" / "row" / "col" / "cell"
pointer    = "/" *( "/" token )           ; RFC 6901 JSON Pointer
qualname   = name *( "." name )
```

예시:

```text
vault://src/router.py                          파일
vault://src/router.py#TeacherRouter            클래스
vault://src/router.py#TeacherRouter.select     메서드
vault://docs/architecture.md#h:teacher-router  문서 소제목
vault://config/model.json#/router/threshold    JSON 값
vault://data/train.csv#col:label               열
vault://data/train.csv#row:1042                행
vault://experiments/run.ipynb#cell:12          노트북 셀
```

### 1.2 심볼은 접두사가 없다

fragment 타입 중 **심볼만 접두사를 갖지 않는다.** 나머지는 전부 접두사나 `/` 로 시작한다.

근거: 충돌하는 것은 심볼과 소제목 **한 쌍뿐**이다. `#/` `#row:` `#col:` `#cell:` 은 이미 구분된다. 그리고 심볼 주소가 압도적으로 많다 — R6 측정에서 django 문서 하나의 참조가 전부 심볼을 가리켰다(→ `17_MEASUREMENT_BASIS.md` "문서→코드 참조 해석률"). 가장 흔한 것을 가장 짧게 둔다.

**확장자로 타입을 추론하지 않는다.** `.md` 안의 코드 블록에도 심볼 주소를 걸 수 있고, 코드 파일의 docstring에도 소제목이 있을 수 있다. 추론은 나중에 반드시 막힌다.

새 fragment 타입을 추가할 때는 **반드시 접두사를 붙인다.** 접두사 없는 자리는 심볼이 영구히 점유한다.

### 1.3 CSV 셀 주소는 정의하지 않는다

`04_VAULT_AND_DATA_MODEL.md` 의 주소 지정 가능 객체 목록은 표에 대해 `Table` / `Column` / `Row` 까지만 둔다. 셀 단위 링크 요구가 측정된 바 없으므로 v0.1에서 정의하지 않는다.

따라서 `cell:` 은 **노트북 셀 전용**이며 `.csv` 와 `.ipynb` 사이에 충돌이 없다.

### 1.4 경로 정규화

```text
구분자        항상 "/" — Windows 의 "\" 는 저장 시 변환
기준          vault 루트 기준 상대 경로. 절대 경로와 "../" 는 거부한다
유니코드      저장은 NFC. 비교 전 양쪽을 NFC 로 정규화한다
대소문자      저장은 원본 그대로. 비교는 정확 일치가 1순위 (→ 3.1 계단 F1a)
```

**대소문자를 저장 시점에 접지 않는다.** 접으면 Linux vault 에서 서로 다른 두 파일이 같은 주소를 갖게 된다. 대소문자 차이는 해석 계단에서 흡수한다.

**Windows 260자 제한은 주소의 문제가 아니다.** 주소는 vault 루트 기준 상대 경로이므로 vault 를 어디에 두든 길이가 변하지 않는다. 실제 파일 접근 시 `\\?\` 접두사를 쓴다 — 구현 사항이며 형식과 무관하다.

### 1.5 인코딩과 이스케이핑

퍼센트 인코딩은 **필수인 것만** 한다. 주소는 사람이 읽는 것이 목적이므로 한글·CJK·기타 유니코드 문자는 인코딩하지 않는다.

| 문자 | 인코딩 | 이유 |
|---|---|---|
| `#` | `%23` | fragment 구분자 |
| `%` | `%25` | 인코딩 이스케이프 자체 |
| `/` (세그먼트 안) | `%2F` | 경로 구분자 |
| 제어문자 (U+0000–U+001F) | `%XX` | 파일에 그대로 쓸 수 없다 |
| 그 외 | 하지 않는다 | `vault://docs/설계.md#h:주소-체계` 는 이대로 유효하다 |

**심볼 이름 안의 `.` 은 `%2E` 로 이스케이프한다.** qualname 구분자와 충돌하기 때문이다.

```text
vault://src/lib.rs#Config.load          Config 안의 load
vault://src/lib.rs#Config%2Eload        "Config.load" 라는 이름의 심볼 하나
```

### 1.6 언어별 qualname 규칙

```text
Python      Module 은 경로가 담당. 클래스/함수는 파일 기준 qualname
              Cls.method  /  outer.inner
Rust        impl 블록의 메서드는 타입 기준
              Type.method
            trait impl 이 여럿이면 같은 qualname 이 여러 개 나온다.
            그 모호성은 해석 계단이 처리한다 (→ 3.2)
TypeScript  namespace / class 중첩을 그대로 이어붙인다
              NS.Cls.method
Go          메서드는 리시버 타입 기준
              Type.Method
```

---

## 2. Heading slug

R1 과 R6 가 만드는 링크의 **문서 쪽 끝점**이다. `01_PRODUCT_VISION.md` 가 제품이 성립하는 조건으로 든 기능이 여기 걸려 있으므로 규칙을 명시한다.

### 2.1 생성 규칙

순서대로 적용한다.

```text
1. NFC 정규화
2. 소문자화 (유니코드 단순 소문자. 로케일 의존 규칙을 쓰지 않는다)
3. 앞뒤 공백 제거
4. 공백 및 공백류를 "-" 로
5. 유니코드 문자(Letter)·숫자(Number)·"-"·"_" 만 남기고 제거
6. 연속된 "-" 를 하나로, 양끝의 "-" 제거
7. 결과가 비면 "section"
```

```text
"## Teacher Router"        → teacher-router
"## 주소 체계"              → 주소-체계
"## `vault://` 문법"        → vault-문법
"## 3. 해석 계단!"          → 3-해석-계단
"## ---"                   → section
```

**로케일 의존 소문자화를 금지하는 이유**: 터키어 로케일에서 `I` 는 `ı` 가 된다. 같은 문서가 사용자 로케일에 따라 다른 주소를 갖게 되면 링크가 환경마다 깨진다.

### 2.2 중복 heading

같은 slug 가 한 문서에 여러 번 나오면 **등장 순서대로** 접미사를 붙인다.

```text
첫 번째   주소-체계
두 번째   주소-체계-2
세 번째   주소-체계-3
```

**알려진 한계**: 문서 중간에 같은 이름의 소제목이 새로 삽입되면 그 뒤의 번호가 밀린다. `주소-체계-2` 가 가리키던 대상이 조용히 바뀐다.

이 한계는 해석 계단이 흡수한다 — 링크 생성 시 그 소제목의 텍스트를 `anchor_hint` 로 함께 저장하고(→ 4.1), 번호가 밀렸을 때 텍스트로 재탐색한다.

### 2.3 블록 ID 는 v0.1 에서 도입하지 않는다

Obsidian 은 문단에 `^a3f9k2` 같은 ID 를 심어 제목 변경에 영향받지 않게 한다. 기성 해법이지만 **사용자 문서를 수정해야 한다.**

이 제품의 vault 는 대개 남의 저장소이고, `[[...]]` 문법을 심지 않기로 한 것과 같은 이유로 채택하지 않는다. 사용자가 명시적으로 요청할 때만 심는 절충안은 **v0.2 에서 재검토**한다.

---

## 3. 주소 타입별 해석 계단

주소는 저장 시점에 해석하지 않고 조회 시점에 계단식으로 해석한다. (→ `04_VAULT_AND_DATA_MODEL.md`)

계단의 깊이는 주소 종류마다 다르다. 파일에서 이름 매칭의 기여가 0.3% 였던 반면 심볼에서 S3+S4 가 13.7%p 를 담당했다(→ `17_MEASUREMENT_BASIS.md` "링크 복구율"). **기여가 측정되지 않은 단계는 만들지 않는다.**

### 3.1 파일 — 3단 (기존)

```text
F1   경로 그대로 존재                      → HIT
F1a    정확 일치 실패 시 대소문자 무시 재시도  → HIT (플랫폼 무관하게 시도)
F2   aliases.jsonl (git rename)          → HIT
F3   BROKEN
```

`F1a` 를 v0.2 에서 추가한다. Windows·macOS 에서 만든 링크가 Linux 에서 깨지는 것을 막는다. 비용이 조회 한 번이고 모호성이 없다.

### 3.2 심볼 — 5단 (기존)

```text
S1   같은 경로에 같은 qualname             → HIT
S2   alias 경로에 같은 qualname            → HIT
S3   qualname 이 vault 전역에서 유일         → 후보 제시 (1클릭)
S4   본문 유사도 Jaccard ≥ 0.40            → 후보 제시 (1클릭)
S5   BROKEN
```

S3 에서 후보가 여럿이면(Rust trait impl 등) 전부 제시하고 사용자가 고른다. 자동 확정하지 않는다.

### 3.3 Heading — 3단 (신규)

```text
H1   같은 파일에 같은 slug                 → HIT
H2   같은 파일에서 anchor_hint 텍스트 일치   → HIT (번호 밀림·오타 수정 흡수)
H3   BROKEN — 그 파일의 소제목 목록 제시
```

**유사도 계단을 두지 않는다.** 심볼의 임계값 0.40 은 심볼 본문으로 스윕한 값이지 소제목 텍스트가 아니다(→ `17_MEASUREMENT_BASIS.md`). 근거 없는 파라미터를 옮기지 않는다.

대신 H3 의 비용이 낮다는 점에 기댄다 — 파일은 살아 있으므로 후보가 그 문서의 소제목 몇 개뿐이다. 심볼처럼 vault 전역을 뒤질 필요가 없다.

**측정 후 재검토 항목**: 실제 저장소에서 소제목이 얼마나 자주·어떻게 바뀌는지 측정하지 않았다. H2 로 부족하다는 근거가 나오면 그때 계단을 늘린다.

### 3.4 JSON Pointer — 3단 (신규)

```text
J1   포인터가 그대로 존재                   → HIT
J2   파일 alias 를 따라간 뒤 존재            → HIT
J3   BROKEN — 마지막 세그먼트 이름이 문서 안에서 유일하면 후보 제시
```

### 3.5 열 — 2단 (신규)

```text
C1   같은 이름의 열이 존재                  → HIT
C2   BROKEN — 그 파일의 열 목록 제시
```

열 이름은 안정적이고 개수가 적다. 계단을 더 둘 이유가 없다.

### 3.6 행 · 노트북 셀 — 2단 (신규)

행 번호와 셀 인덱스는 **삽입·삭제로 밀린다.** 안정적 식별자가 없으므로 `anchor_hint` 에 기댄다.

```text
R1   같은 번호의 행/셀이 있고 anchor_hint 와 일치     → HIT
R2   같은 파일에서 anchor_hint 로 재탐색             → HIT (밀림 흡수)
R3   BROKEN
```

`anchor_hint` 는 링크 생성 시점에 기록한다(→ 4.1).

```text
행       그 행의 첫 번째 열 값 + 행 내용의 짧은 해시
노트북 셀  셀 소스의 짧은 해시
```

---

## 4. `.vault/` 파일 형식

### 4.0 공통 규칙

```text
인코딩      UTF-8, BOM 없음
줄바꿈      "\n"
한 줄       JSON 객체 하나 (JSONL)
정렬        추가 순서. 재정렬하지 않는다
첫 줄       헤더 레코드
```

헤더 레코드:

```json
{"_type":"links","_v":1}
```

`_v` 가 구현이 아는 값보다 크면 **읽기 전용으로 열고 사용자에게 알린다.** 앱을 다운그레이드했을 때 사용자 데이터를 조용히 잘라내지 않기 위해서다.

**append-only 다.** 수정과 삭제도 새 레코드로 기록한다. 같은 `id` 의 마지막 레코드가 유효한 상태다. 이 규칙에서 undo 와 이력이 공짜로 따라오고, 줄 단위 merge 가 가능해진다(→ `03_SYSTEM_ARCHITECTURE.md`).

### 4.1 `links.jsonl`

manual link 와 사용자 개입의 기록. **재생성 불가능한 유일한 데이터다.**

```json
{"_type":"links","_v":1}
{"id":"l_01JBQZ8K3M","op":"add","from":"vault://docs/architecture.md#h:teacher-router","rel":"describes","to":"vault://src/router.py#TeacherRouter","origin":"manual","confidence":"certain","ts":"2026-08-17T09:00:00Z","to_hint":{"kind":"symbol","name":"TeacherRouter"}}
{"id":"l_01JBQZ8K3M","op":"retarget","to":"vault://src/core/router.py#TeacherRouter","by":"user","ts":"2026-09-02T11:20:00Z"}
{"id":"l_01JBR4T7XQ","op":"del","ts":"2026-09-03T08:10:00Z"}
```

| 필드 | 설명 |
|---|---|
| `id` | ULID. `l_` 접두사. 시간순 정렬되고 충돌이 없다 |
| `op` | `add` / `del` / `retarget` |
| `from` `to` | `vault://` 주소 |
| `rel` | → 5장 |
| `origin` | → 4.6 |
| `confidence` | `certain` / `probable` / `heuristic` (→ `07_SEMANTIC_WEB_GRAPH.md`) |
| `ts` | RFC 3339, UTC |
| `to_hint` `from_hint` | 해석 계단의 입력. → 아래 |
| `last_known` | 재지정 시 직전 주소. BROKEN UI 가 "마지막으로 알려진 주소"를 표시하는 근거 (→ `09_DESKTOP_UX.md`) |
| `by` | `user` / `auto` — 재지정 주체 |

`id` 가 있으므로 tombstone 과 일괄 재지정이 표현된다. **B3 해소.**

`*_hint` 는 주소 타입별로 필요한 것만 담는다.

```json
{"kind":"heading","text":"Teacher Router"}
{"kind":"row","key":"user_1042","hash":"9f2a1c3b"}
{"kind":"cell","hash":"7d4e0a91"}
{"kind":"symbol","name":"TeacherRouter"}
```

### 4.2 `aliases.jsonl`

경로·심볼의 이동 이력. 해석 계단 F2 / S2 의 입력이다.

```json
{"_type":"aliases","_v":1}
{"kind":"path","from":"requests/api.py","to":"src/requests/api.py","source":"git","confidence":"high","ts":"2026-08-17T09:00:00Z","commit":"a1b2c3d"}
{"kind":"symbol","from":"vault://src/router.py#Router","to":"vault://src/router.py#TeacherRouter","source":"user","confidence":"high","ts":"..."}
```

| 필드 | 값 |
|---|---|
| `kind` | `path` / `symbol` |
| `source` | `git` / `watcher` / `user` |
| `confidence` | `high` / `medium` |
| `commit` | `source=git` 일 때만 |

`source` 에 **`user` 를 포함한다.** S3/S4 후보를 사용자가 1클릭으로 확정한 결과가 여기 쌓인다. `04_VAULT_AND_DATA_MODEL.md` 는 alias 소스를 watcher/git 둘로만 적었는데, 그러면 사용자 확정 결과를 저장할 곳이 없어 같은 후보를 매번 다시 물어보게 된다.

체인은 압축한다. `a → b → c` 는 `a → c` 로 저장한다.

**`source` 별 신뢰도 서술을 통일한다** (M1): watcher rename 이벤트는 **도착하면** 신뢰도가 높지만 자주 유실된다. git 은 신뢰도가 높으나 **커밋 후에만** 작동한다. 커밋 전 구간은 watcher 가 유일한 단서이며, 유실되면 정합성 스캔이 delete + create 로 본다. 이 구간의 복구는 보장되지 않는다.

### 4.3 `decisions.jsonl`

제안에 대한 사용자 판정. 거절한 후보를 다시 제안하지 않기 위한 것이다.

```json
{"_type":"decisions","_v":1}
{"key":"d3f81a02c95b7e64","verdict":"reject","rule":"R1","from":"vault://docs/faq.md","to":"vault://src/util.py#buffer","ts":"..."}
```

`key` 는 **재생성된 후보와 매칭하는 내용 해시**다.

```text
key = sha256( rule + "\n" + from_addr + "\n" + to_addr )  의 앞 16자리 hex
```

후보는 재스캔 때마다 새로 만들어지므로 후보 ID 로는 매칭할 수 없다. 주소가 바뀌면 키도 바뀌어 다시 제안되는데, 이것은 의도된 동작이다 — 대상이 달라졌으면 판단도 다시 받아야 한다.

`verdict` 는 `accept` / `reject`. `accept` 는 `links.jsonl` 에 레코드를 남기므로 여기서는 중복 승인을 막는 용도로만 쓴다.

### 4.4 `sketches.jsonl`

S4(본문 유사도)를 위한 심볼 스케치. **B1 해소.**

```json
{"_type":"sketches","_v":1}
{"addr":"vault://src/router.py#TeacherRouter","link_id":"l_01JBQZ8K3M","k":3,"size":32,"sketch":[19283746,28374651,...],"reason":"created","ts":"..."}
{"addr":"vault://src/router.py#TeacherRouter","link_id":"l_01JBQZ8K3M","k":3,"size":32,"sketch":[...],"reason":"refreshed","ts":"..."}
```

**왜 `.vault-ai/` 가 아니라 여기인가**: S4 는 *사라진 옛 심볼의 본문*과 현재 심볼을 비교한다. 옛 심볼은 파일에서 이미 사라졌으므로 **재스캔으로 재생성할 수 없다.** 벤치마크는 git 으로 과거 커밋을 꺼낼 수 있었지만 제품은 git 없는 vault 도 허용한다.

기록 대상은 **링크가 걸린 심볼만**이다. vault 전체가 아니다. `04` 의 materialize-on-link 원칙과 같다. 레코드당 약 200 B 이므로 링크 수천 개 규모에서 수백 KB 다.

**갱신 정책 (B1-b 해소)**

```text
created    링크 생성 시점에 1회
refreshed  S1 또는 S2 로 해석에 성공할 때마다 갱신
```

생성 시점 스케치만 고정하면, 정상적으로 진화한 심볼이 오랜 시간 뒤 rename 될 때 비교 대상이 옛 스케치라 Jaccard 가 임계값 아래로 떨어져 **S4 가 조용히 죽는다.** 임계값 0.40 은 "직전 커밋 vs 현재"로 스윕한 값이지 "1년 전 vs 현재"가 아니다(→ `17_MEASUREMENT_BASIS.md` "유사도 파라미터").

**이 구간은 실측 근거가 없다.** 그러므로 새 파라미터(갱신 주기, 보관 개수 등)를 만들지 않는다. "해석 성공 시 갱신"만 규정하고, 갱신 빈도가 문제가 되면 그때 측정한다.

같은 `addr` 의 마지막 레코드가 유효하다. 이전 레코드는 이력으로 남는다.

### 4.5 `pending.jsonl`

MCP 를 통해 외부 AI 가 밀어넣은 미승인 제안. **M2 해소.**

```json
{"_type":"pending","_v":1}
{"id":"p_01JBQZC7M2","rule":"mcp","agent":"claude-code","from":"vault://docs/setup.md","rel":"configures","to":"vault://config/model.json","rationale":"setup 문서가 이 설정 파일을 설명함","ts":"...","status":"pending"}
```

**왜 `.vault-ai/` 가 아닌가**: AI 제안은 재스캔으로 재생성되지 않는다. `.vault-ai/` 는 "삭제해도 재인덱싱으로 완전 복구"가 정의인데(→ `11_SECURITY_PRIVACY_RELIABILITY.md`), AI 제안이 거기 있으면 그 정의가 거짓이 되고 Phase 0 종료 조건("`.vault-ai/` 삭제 후 완전 복구")도 판정 불가가 된다.

`id` 는 **불변**이다. AI 가 제안하고 사용자가 승인하는 사이에 다른 제안이 끼어들어도 무엇을 승인했는지 확정된다(→ `08_MCP_AND_AI.md`).

`status` 는 `pending` / `accepted` / `rejected`. 승인 시 `links.jsonl` 에 `origin="manual"` 로 레코드가 생긴다 — 최종 판단이 사람의 것이기 때문이다.

**즉시 반영 모드**(사용자가 명시적으로 켠 경우)에서는 `pending` 을 거치지 않고 `links.jsonl` 에 `origin="ai"` 로 직접 기록한다. 기본값은 승인 모드다.

### 4.6 `origin` 값

```text
manual      사용자가 직접 생성. 제안을 승인한 것도 포함
extracted   문서에 사람이 명시적으로 쓴 참조를 추출 (R6 / R2)
parser      정적 분석으로 도출 (defined_in, imports, calls)
git         git 히스토리에서 도출
suggested   제안 엔진 후보, 미승인
ai          MCP 를 통해 AI 가 직접 기록 (즉시 반영 모드)
imported    외부 도구에서 가져옴
```

**`extracted` 를 신설한다.** R6 로 뽑은 링크는 정적 분석의 산물이 아니고(`parser` 아님) 사용자가 이 앱에서 만든 것도 아니다(`manual` 아님). 사람이 문서에 써둔 것을 읽은 것이라 신뢰도가 `parser` 보다 높고 `manual` 보다 낮다.

저장 위치가 `origin` 에 따라 갈린다.

```text
extracted / parser / git   → .vault-ai/   (재파싱으로 재생성됨)
manual / ai                → .vault/      (재생성 불가)
suggested                  → .vault-ai/suggestions/
```

사용자가 `extracted` 링크를 삭제하거나 재지정하면, **그 판단은 재생성 불가이므로** `.vault/links.jsonl` 에 `op:"del"` / `op:"retarget"` 레코드로 기록된다. 재파싱해도 되살아나지 않는다. (→ `16_SUGGESTION_ENGINE.md` "승인 정책")

---

## 5. `rel` 어휘

**B4 해소.** 정의되지 않은 rel 을 예시에서 쓰던 문제를 정리한다.

### 5.1 정방향만 저장한다

역관계는 별도 rel 로 저장하지 않고 조회 시 `direction="in"` 으로 표현한다(→ `08_MCP_AND_AI.md`).

```text
저장    {from: docs/arch.md, rel: describes, to: src/router.py}
조회    neighbors("src/router.py", direction="in")  → docs/arch.md
```

기존 문서가 쓰던 역관계 이름은 다음과 같이 대응한다. **저장 형식에는 등장하지 않는다.**

| 기존 표기 | 정방향 | 방향 |
|---|---|---|
| `described_by` | `describes` | in |
| `configured_by` | `configures` | in |
| `tested_by` | `tests` | in |
| `implemented_by` | `implements` | in |

### 5.2 어휘 목록

구조 관계 — 파서가 만든다. 재생성 가능.

| rel | from → to | confidence |
|---|---|---|
| `defined_in` | 심볼 → 파일 | certain |
| `contains` | 파일/심볼 → 하위 객체 | certain |
| `parent_of` | 심볼 → 중첩 심볼 | certain |
| `imports` | 파일 → 파일/모듈 | certain |
| `json_pointer` | 파일 → JSON 노드 | certain |
| `inherits` | 심볼 → 심볼 | probable |
| `calls` | 심볼 → 심볼 | probable |

`obj.foo()` 형태는 edge 를 만들지 않는다 — 유일 해석률이 낮다(→ `17_MEASUREMENT_BASIS.md` "Graph 품질").

의미 관계 — 사람이 만들거나 문서에서 추출한다.

| rel | from → to | 출처 |
|---|---|---|
| `describes` | 문서 → 코드 | manual · R1 · R6 |
| `references` | 설정/문서 → 파일 | R2 |
| `mentions` | docstring → 심볼 | R3 |
| `configures` | 설정 → 코드 | manual |
| `tests` | 테스트 → 코드 | manual |
| `implements` | 코드 → 문서/스펙 | manual |
| `uses` | 노트북 → 모듈 | R5 |

git 관계.

| rel | from → to | confidence |
|---|---|---|
| `changed_in` | 파일/심볼 → 커밋 | certain |
| `co_changed` | 파일 → 파일 | heuristic |

**`defines` 를 쓰지 않는다.** `02_CORE_PRINCIPLES.md` 가 홀로 `defines` 를 쓰고 나머지 전 문서가 `defined_in` 을 쓴다. 방향이 심볼 → 파일이므로 `defined_in` 으로 통일한다.

### 5.3 확장

여기 없는 rel 은 저장하지 않는다. 새 rel 이 필요하면 이 문서에 추가하고 `_v` 를 올린다. **미정의 rel 을 만난 구현은 그 레코드를 버리지 않고 보존한 채 UI 에 원문으로 표시한다** — 상위 버전이 쓴 데이터를 하위 버전이 지우면 안 된다.

---

## 6. Sphinx role → `rel` 매핑

R6 의 입력 처리 규칙이다. (→ `16_SUGGESTION_ENGINE.md`)

### 6.1 지원 role

| role | 대상 | rel |
|---|---|---|
| `:class:` `:func:` `:meth:` `:attr:` `:data:` `:exc:` `:obj:` | 심볼 | `describes` |
| `:mod:` | 파일/모듈 | `describes` |

전부 `describes` 로 매핑한다. R1(백틱 이름 대조)과 같은 rel 이며 **`origin` 과 `confidence` 로 구분한다.** rel 을 나누면 조회하는 쪽이 둘 다 물어봐야 하는데 사용자에게는 같은 관계다.

### 6.2 지원하지 않는 role

`:setting:` `:djadmin:` 같은 것은 프로젝트가 Sphinx 확장으로 정의한 고유 role 이다. 대상이 코드 심볼이 아니거나 프로젝트마다 의미가 다르므로 **v0.1 에서 해석하지 않는다.**

django 측정에서 `:setting:` 2,026건 · `:djadmin:` 429건이 이에 해당한다(→ `17_MEASUREMENT_BASIS.md`). 적은 양이 아니지만 범용 규칙을 만들 수 없다.

### 6.3 대상 해석

role 본문에서 대상만 뽑는다.

```text
:class:`~django.db.models.ForeignKey`        → django.db.models.ForeignKey
:class:`레이블 <django.http.HttpResponse>`    → django.http.HttpResponse
:class:`.QuerySet`                           → QuerySet
:class:`QuerySet`                            → QuerySet
```

선행 `~` 는 표시용 축약이고 `.` 는 상대 참조다. 둘 다 대상 이름의 일부가 아니다.

### 6.4 짧은 이름 참조

경로 없이 쓴 참조가 django 측정에서 전체의 31% 였다(→ `17_MEASUREMENT_BASIS.md`). Sphinx 는 `currentmodule` / `module` 디렉티브 문맥으로 해석한다.

```text
1. 문서에 currentmodule/module 디렉티브 문맥이 있으면
     그것으로 수식해 완전 경로로 만든다        → confidence: certain
2. 문맥이 없고 그 이름이 vault 전역에서 유일하면
     링크를 만든다                            → confidence: probable
3. 유일하지 않으면 링크를 만들지 않는다
     R1 제안 큐로 보낸다                       → 사용자 승인 경로
```

**3번이 핵심이다.** 모호한 것을 자동 반영하지 않는다. R6 를 승인 없이 반영하기로 한 근거는 "완전 수식 경로는 추측이 아니라 확인"이라는 것인데, 짧은 이름은 그 근거가 성립하지 않는다. 근거가 없는 것은 승인 경로로 보낸다.

### 6.5 확장 후보

mkdocstrings(`::: module.Class`)와 Doxygen(`@ref`)은 문법이 실재하나 측정된 밀도가 낮다(→ `17_MEASUREMENT_BASIS.md` "생태계별 문서→코드 참조 밀도"). v0.1 에서 구현하지 않는다.

Doxygen 은 표본이 저장소 하나(문서 12개)뿐이므로 **"C++ 생태계는 쓰지 않는다"고 판단한 것이 아니다.** 대형 C++ 프로젝트는 측정하지 않았다.

---

## 7. `.vault/vault.toml`

```toml
[vault]
name = "MyProject"
schema_version = 1

[ignore]
use_gitignore = true
patterns = ["vendor/", "third_party/", "deps/", "node_modules/"]

[limits]
content_bytes = 1_048_576   # 본문 인덱싱 상한
parse_bytes   = 5_242_880   # 파싱 상한

[mcp]
write_mode = "approve"      # approve | immediate
```

`limits` 의 두 값은 실사용으로 조정할 대상이다(→ `12_ENGINEERING_DECISIONS.md` "미결 ⑤").

`write_mode` 기본값은 `approve` 다. 기본값을 정해두지 않으면 편의상 `immediate` 가 기본이 되고, AI 오탐이 그대로 git 에 커밋된다.

---

## 8. 미해결 · 측정하지 않은 것

이 문서가 정하지 **않은** 것을 명시한다.

- **소제목 변경 빈도** — H2(anchor_hint 텍스트 일치)로 충분한지 측정하지 않았다. 부족하다는 근거가 나오면 계단을 늘린다 (→ 3.3)
- **스케치 갱신 빈도** — "해석 성공 시 갱신"이 실제로 얼마나 자주 일어나고 파일이 얼마나 커지는지 모른다 (→ 4.4)
- **`extracted` 링크의 오탐률** — R6 가 만든 링크 중 사용자가 삭제하는 비율. 실사용 전에는 알 수 없다
- **CSV 셀 주소** — 요구가 측정되지 않아 정의하지 않았다 (→ 1.3)
- **블록 ID** — v0.2 재검토 (→ 2.3)
- **`:setting:` 계열 고유 role** — 범용 규칙을 만들 수 없다 (→ 6.2)
