# 12. Engineering Decisions — Implementation Stage

제품 방향은 확정되었지만 아래는 구현 단계에서 벤치마크 후 선택해야 한다.

## Core Language

후보:

- Rust
- C++
- Go

평가 기준:

- 검색 속도
- 파일 I/O
- concurrency
- Tree-sitter 연동
- desktop packaging
- memory usage
- MCP 구현 편의성

## Desktop Framework

후보 예:

- Tauri
- Qt
- 기타 native framework

## Search Index

검토 대상:

- SQLite FTS
- Tantivy/Lucene 계열
- FST
- custom inverted index
- memory-mapped index

검색 요구사항별로 단일 기술 대신 복합 인덱스를 사용할 수 있다.

## Graph Storage

초기에는 대형 Graph DB를 바로 도입하지 않는 방향이 유력하다.

후보:

- SQLite adjacency tables
- embedded graph representation
- custom local store

## Parser Infrastructure

Tree-sitter 사용 범위 검토:

- Python AST는 native AST 가능
- 다중 언어 확대 시 Tree-sitter가 유리

## File Watcher

OS별 API를 직접 사용하거나 cross-platform library를 사용한다.

평가 기준:

- latency
- event duplication
- rename detection
- large directory behavior

## Stable ID

rename/move에도 링크가 최대한 유지되도록 전략을 결정해야 한다.

## Search Ranking

초기 ranking 후보:

```text
filename exact
> filename prefix
> symbol exact
> path match
> content exact
> BM25
> graph relation
```

실제 사용자 테스트로 조정한다.

## MCP Schema

Tool 수를 너무 많이 늘리지 않고, AI가 적은 호출로 필요한 맥락을 가져갈 수 있게 설계해야 한다.

## Benchmark

기술 선택 전에 최소 샘플 Vault를 만들어 비교한다.

- 1K files
- 10K files
- 100K files
