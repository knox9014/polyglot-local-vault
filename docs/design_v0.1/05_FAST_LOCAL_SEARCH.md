# 05. Fast Local Search Engine

## 중요도

파일 검색기는 부가기능이 아니다.

**Knowledge Graph와 동급의 핵심 제품 기능**이다.

목표:

> 사용자가 파일을 찾을 때 AI에게 물어볼 필요가 없을 정도로 기본 검색이 빨라야 한다.

## 검색 대상

최소 다음을 지원한다.

- 파일명
- 전체/상대 경로
- 확장자
- 파일 내용
- 코드 심볼
- Markdown heading
- JSON/YAML key
- Notebook cell
- 메타데이터
- Graph 관계

## 검색 문법 예시

```text
router
*.py
ext:json
path:src
class:TeacherRouter
modified:7d
threshold
```

정확한 문법은 구현 단계에서 확정한다.

## 핵심 인덱스

```text
Filename Index
Path Index
Extension Index
Full-text Index
Symbol Index
Metadata Index
Graph Index
```

## 초기 인덱싱

```text
Vault
 ↓
Full Scan
 ↓
Parse
 ↓
Index Build
```

## 이후 처리

매 검색마다 디스크 전체를 훑지 않는다.

```text
파일 변경
 ↓
File Watcher
 ↓
변경 파일만 재파싱
 ↓
관련 Index만 갱신
```

## 검색 단계

체감 속도를 위해 저비용 검색부터 즉시 반환한다.

예:

```text
1. Filename / prefix
2. Path
3. Symbol
4. Exact content
5. Full-text / BM25
6. Graph related
```

UI는 전체 검색이 끝날 때까지 기다리지 않고 빠른 결과부터 표시한다.

## 검색 알고리즘 후보

Core 검색:

- Exact Match
- Prefix Search
- Substring Search
- Full-text Search
- BM25
- TF-IDF
- Symbol Search
- Metadata filtering
- Graph traversal

## Vector Search

초기 Core에서는 필수가 아니다.

Embedding은 모델 의존성을 만들 수 있으므로 선택적 플러그인으로 둔다.

```text
CORE
├─ Exact
├─ Prefix
├─ BM25
├─ TF-IDF
├─ Symbol
└─ Graph

OPTIONAL
└─ Embedding / Vector Search
```

## 성능 원칙

- Incremental indexing
- Memory-mapped index 검토
- Prefix/FST 구조 검토
- 파일 변경 debounce
- 병렬 parsing
- 큰 파일 별도 정책
- binary file 제외
- ignore rules
- 쿼리 cancellation
- 부분 결과 streaming

## 향후 벤치마크

최소 다음 규모를 측정한다.

- 1,000 files
- 10,000 files
- 100,000 files

측정 항목:

- cold indexing
- warm search
- keystroke-to-first-result
- file update-to-index refresh
- memory usage
- disk index size
