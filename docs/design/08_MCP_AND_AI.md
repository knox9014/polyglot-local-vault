# 08. MCP and Optional AI

## 핵심 철학

AI는 Vault를 정리하는 엔진이 아니다. AI는 Vault를 사용하는 외부 클라이언트다.

```text
Vault
 ↓
MCP Server
 ↓
AI
```

## AI 없이 가능한 기능

- 파일 검색
- 파일 열기
- 구조 Parsing
- Symbol 검색
- Graph / Backlink / Dependency 탐색
- Metadata 검색
- 링크 주소 해석 및 복구
- 관계 후보 제안 (결정론적)

**핵심 가치가 전부 AI 없이 성립한다.** 특히 제안 엔진이 결정론적이므로 콜드스타트 해소에도 AI가 필요 없다.

## MCP Tools — 4개

v0.1은 11개를 제안했다. 그러나 `12_ENGINEERING_DECISIONS.md` 는 동시에 "Tool 수를 너무 많이 늘리지 않아야 한다"고 적어 자기모순이었다.

툴이 많을수록 모델의 선택 정확도가 떨어진다. `symbols` / `references` / `backlinks` / `dependencies` / `graph` 는 모델 입장에서 구분이 모호하며, 잘못 고르고 다시 고르는 왕복이 늘어난다. 실제로 이들은 전부 하나의 관계 조회로 표현된다.

```text
vault.search(query, kind?="auto"|"file"|"symbol"|"text", filters?, limit?)
vault.read(uri, mode?="full"|"outline"|"range")
vault.neighbors(uri, rel?[], depth?=1, direction?="both")
vault.link(action="propose"|"remove", from, to, rel, suggestion_id?)
```

흡수 관계:

| v0.1 툴 | v0.2 |
|---|---|
| `vault.symbols` | `search(kind="symbol")` |
| `vault.list` | `search(kind="file")` |
| `vault.references` | `neighbors(rel=["references"])` |
| `vault.backlinks` | `neighbors(direction="in")` |
| `vault.dependencies` | `neighbors(rel=["imports","calls"])` |
| `vault.graph` | `neighbors(depth=N)` |
| `vault.history` | `neighbors(rel=["changed_in"])` |
| `vault.create_link` / `remove_link` | `link(action=...)` |

## 주소 왕복성 (Addressability Round-trip)

**모든 툴 응답에 `vault://` 주소가 포함되어야 한다.** 이것이 없으면 AI는 `search` 결과에서 다음 `neighbors` 호출을 만들 수 없고, 결국 파일 전체를 읽으려 든다 — 이 설계가 막으려는 바로 그 행동이다.

```json
{
  "uri": "vault://src/router.py#TeacherRouter",
  "type": "CodeClass",
  "range": {"start": 42, "end": 118},
  "preview": "class TeacherRouter:\n    \"\"\"라우팅 정책...\"\"\"",
  "confidence": "certain",
  "neighbors_hint": {"imports": 3, "described_by": 1, "tested_by": 1}
}
```

`neighbors_hint` 는 **다음 호출의 가치를 미리 알려주는 필드**다. AI가 무의미한 탐색을 반복하지 않고 필요한 곳으로 바로 간다.

## 사용 예

사용자:

> TeacherRouter와 연결된 설정을 찾아줘.

```text
1. vault.search("TeacherRouter", kind="symbol")
   → uri: vault://src/router.py#TeacherRouter
     neighbors_hint: { configured_by: 1, described_by: 1 }

2. vault.neighbors("vault://src/router.py#TeacherRouter", rel=["configured_by"])
   → uri: vault://config/model.json#/router
```

2회 호출로 끝난다. AI가 프로젝트 전체를 읽지 않고 Vault의 인덱스와 Graph를 이용한다.

## Link Write Safety

AI가 관계를 만들고 싶다면:

```text
1. AI가 vault.link(action="propose", ...) 호출
2. 서버가 불변 suggestion_id 발급, .vault-ai/suggestions/ 에 저장
3. 사용자가 UI에서 승인
4. .vault/links.jsonl 에 origin=manual 로 기록
```

**불변 ID가 필수다.** v0.1의 "제안 → 승인 → write" 흐름에는 제안과 승인 사이에 시간 차가 있는데, 그 사이 AI가 다른 제안을 밀어 넣으면 사용자가 무엇을 승인했는지 불명확해진다. 승인은 특정 `suggestion_id` 에 대해서만 수행된다.

AI 제안은 결정론적 제안 엔진과 **같은 파이프라인**을 쓴다. 사용자 입장에서 흐름이 하나다. (→ `16_SUGGESTION_ENGINE.md`)

## MCP 보안

- **read-only 기본.** write는 별도 툴이며 명시적 승인 필요
- ignored 파일은 노출하지 않음 (`.env`, credentials, 인증서 등)
- secret 민감 경로 기본 차단
- 감사 로그 (어떤 툴이 무엇을 읽었는지)
- `mode="outline"` 으로 본문 없이 구조만 반환 가능 — 최소 권한 원칙

자세한 정책은 `11_SECURITY_PRIVACY_RELIABILITY.md` 참조.

## 모델 독립성

MCP를 사용하므로 특정 모델·회사에 종속되지 않는다.

- ChatGPT
- Codex
- Claude
- Gemini
- Local LLM
- 미래의 MCP 호환 AI

## 미검증 항목

**MCP 툴 설계는 측정하지 못했다.** 툴 개수와 스키마가 모델의 호출 효율에 미치는 영향은 실제 모델 하네스 없이는 잴 수 없다. 위 4툴 압축은 설계 판단이며 실측 근거가 아니다.

Phase 4에서 다음을 측정해야 한다.

```text
"X와 연결된 설정 찾아줘" 류 질의 20개
  → 평균 호출 횟수 (목표: 3회 이내)
  → 잘못된 툴 선택 비율
  → 전체 파일 읽기로 도피한 비율 (목표: 0%)
```
