# 13. Future Extensions

이 문서의 기능은 초기 MVP에 포함하지 않는다. **측정으로 v0.1에서 제외하기로 한 항목에는 근거와 재검토 조건을 명시한다.**

## 측정에 근거해 유예한 것

### 구문(phrase) 검색

위치 인덱스가 필요하다. 측정상 인덱스 크기가 원본 대비 **5.5% → 21.8%** 로 확대된다.

```text
재검토 조건
  - 사용자가 정확 문자열 검색을 실제로 자주 쓰는지 확인
  - 후보 문서를 좁힌 뒤 원본 확인하는 현재 방식의 지연이 문제가 되는지
```

### 타입 추론 기반 call edge

`obj.foo()` 형태는 vault 내 유일 해석률이 **30.3%** 다(전체 호출부의 51.5%를 차지). 이름만으로는 오탐이 정탐의 2.3배다.

```text
재검토 조건
  - 언어별 타입 추론 백엔드(Pyright, gopls, rust-analyzer 등) 연동 비용 평가
  - 연동 후 유일 해석률 재측정. 70% 이상이면 probable로 생성
```

`bare` 와 `self` 형태는 v0.1에서 이미 생성한다(73.0% / 75.3%).

### Hypergraph

여러 객체가 하나의 사건·실험에 참여하는 구조.

```text
Experiment #37
├─ router.py
├─ config.json
├─ dataset.csv
└─ metrics.json
```

일반 Graph로 시작하고, 사용자가 실제로 이런 묶음을 만들려 하는지 확인한 뒤 검토한다.

## 추가 파일 형식

```text
.js  .tsx  .cpp  .h  .java  .sql  .xml
```

Tree-sitter 문법 파일 교체로 대응되므로 비용이 낮다. 다만 **형식 추가보다 파서 품질이 우선**이다.

장기:

```text
PDF / DOCX / PPTX / XLSX / Images / Audio / Video
```

이들은 Tree-sitter로 처리되지 않으므로 별도 Adapter가 필요하다.

### 미측정 언어

Java, C++, JavaScript는 심볼 링크 복구율을 측정하지 않았다. 추가 전에 같은 측정을 수행하는 것을 권한다. 측정 코드는 `vault-bench` 의 `m1_lang_symbols.py` 를 그대로 쓸 수 있다(파서 설정만 추가).

## Optional Embeddings

기본 검색과 별개로 선택적으로 embedding 검색을 추가할 수 있다.

```text
원칙
  - Core dependency가 되지 않는다
  - local embedding plugin 가능
  - 외부 provider 가능
```

측정 근거상 Core에서 뺀 이유는 유지된다 — 숫자·정확한 값 검색에 부적합하고, 파일명 퍼지 매칭(`tscfgjson` → `tsconfig.json`)은 embedding으로 대체되지 않는다.

## Plugin SDK

외부 개발자가 다음을 추가할 수 있게 한다.

- Parser (Adapter 인터페이스는 이미 정의되어 있다)
- Viewer
- Search provider
- Metadata provider
- **Suggestion rule** — 제안 규칙도 플러그인 대상이다. 도메인별로 유효한 규칙이 다를 수 있다

## 제안 엔진 확장

v0.1은 5개 규칙(R1~R5)으로 시작한다. 승인율 데이터가 쌓이면 다음을 검토한다.

```text
- 규칙별 승인율에 따른 랭킹 학습 (로컬, 사용자별)
- 테스트 파일 ↔ 대상 파일 (명명 규칙 기반)
- 인접 커밋 메시지에 등장하는 심볼명
- import 그래프 상 거리 기반 가중
```

**단, 학습을 도입하더라도 결정론적 규칙을 대체하지 않고 랭킹에만 쓴다.** Algorithm-first 원칙을 유지한다.

## Git 확장

초기 local Git 이후:

- GitHub / GitLab 연동
- issue / PR 연결
- 브랜치별 Graph 스냅샷

## 협업

로컬 엔진이 충분히 안정된 이후 별도 제품 단계로 검토한다.

- shared vault
- conflict resolution
- permissions
- team graph

**저장소 2계층 분리가 이 경로를 미리 열어둔다.** `.vault/links.jsonl` 이 git 커밋 대상이고 JSONL이므로 merge conflict가 라인 단위로 해결된다. 협업을 위해 별도 설계가 필요한 부분이 줄어든다.

## 모바일 / 웹

초기에는 제외한다. 로컬 데스크톱 엔진이 핵심이며 다른 플랫폼은 이후에 판단한다.

## 성능 확장

측정에서 드러난 여유와 한계.

| 항목 | 현재 | 확장 여지 |
|---|---|---|
| 경로 전체 퍼지 검색 | 2코어 50K까지 | 4코어+ 에서 100K 이상. SIMD 폭 확대 여지 |
| cold 인덱싱 | 단일 스레드 18.5s @ 100K | 병렬 파싱으로 코어 수만큼 단축 가능 |
| 인덱스 크기 | 5.5% (doc ID만) | 고빈도 term 컷으로 posting 45% 절감 가능 |

병렬 파싱은 CPU 바운드이므로 이득이 있다. 반면 메타데이터 스캔은 I/O 바운드라 병렬화하면 오히려 느려진다(측정: 2.3배).
