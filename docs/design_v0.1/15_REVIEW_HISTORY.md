# 15. Review History — 20회 검토 반영사항

이전 설계를 아키텍처·UX·성능·보안·확장성 관점에서 반복 검토하며 다음 문제를 수정했다.

| # | 발견한 문제 | 반영한 개선 |
|---:|---|---|
| 1 | 기능이 너무 많음 | MVP 단계화 |
| 2 | 자체 포맷 종속 위험 | 원본 파일 + sidecar index |
| 3 | AI가 Graph를 오염시킬 수 있음 | AI 자동 Graph 수정 금지 |
| 4 | 모든 관계를 동일 취급 | Manual / Parser / Git 출처 구분 |
| 5 | Graph가 hairball이 됨 | Focused 1-hop 기본 |
| 6 | Embedding 수 폭증 위험 | Embedding을 Core에서 분리 |
| 7 | 숫자·정확한 값 검색에 vector 부적합 | Exact/Structured 검색 유지 |
| 8 | Python을 텍스트로만 분석할 위험 | AST 기반 parsing |
| 9 | JSON 내부 링크가 불명확 | JSON Pointer식 주소 |
| 10 | Git history가 Graph를 폭발시킬 수 있음 | 중요 관계만 선택적으로 반영 |
| 11 | 파일 하나 수정 시 전체 재색인 | Incremental indexing |
| 12 | 파일 rename 시 관계 단절 | Stable ID 전략 필요 |
| 13 | 초기 Graph DB가 과도할 수 있음 | Embedded/local lightweight store 우선 |
| 14 | Vault 이동 시 링크 손상 | 상대 경로와 stable identity 검토 |
| 15 | secret 노출 위험 | ignore/security policy |
| 16 | 외부 AI가 과도한 context를 읽을 위험 | MCP를 통한 구조적 조회 |
| 17 | 모든 언어 parser 직접 구현 불가 | Parser Adapter/Plugin 구조 |
| 18 | UI 복잡성 | Progressive Disclosure |
| 19 | 앱 종속 위험 | 기존 파일/Git 생태계 유지 |
| 20 | 차별점 불명확 | Polyglot first-class semantic objects로 중심 가치 고정 |

## 이후 추가로 확정된 개선

20회 검토 이후 대화를 통해 다음 사항을 더 강하게 고정했다.

### AI 의존 제거

초기에는 AI가 의미적 정리를 담당하는 방향도 고려했으나 최종적으로 제거했다.

Core는 결정론적 알고리즘 기반이다.

### Search-first 강화

파일 검색기를 단순 기능이 아닌 핵심 제품 축으로 올렸다.

최종 핵심 자산:

```text
Fast Local Search Engine
+
Polyglot Parser System
+
Semantic Graph Engine
+
MCP Interface
```

이 네 가지를 중심으로 제품을 구현한다.
