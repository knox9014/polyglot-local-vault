# 06. Polyglot Parser System

## 핵심 원칙

파일을 단순 텍스트 blob으로만 취급하지 않는다.

각 파일 형식의 고유 구조를 알고리즘으로 추출한다.

## v0.1 지원

```text
.md
.py
.json
.yaml
.csv
.ipynb
```

## Markdown

추출 대상:

- heading
- links
- tags
- code blocks
- lists
- document hierarchy

## Python

AST를 사용한다.

추출 대상:

- module
- class
- function
- method
- imports
- variables
- calls
- inheritance
- decorators

초기에는 정적 분석으로 확실히 알 수 있는 관계만 저장한다.

## JSON

추출 대상:

- object
- array
- key
- value
- JSON Pointer path

예:

```text
config.json#/router/threshold
```

## YAML

추출 대상:

- mapping
- sequence
- scalar
- hierarchy
- path

## CSV

추출 대상:

- header
- schema
- column
- row count
- inferred basic types

모든 row를 Node로 만드는 것은 데이터 규모에 따라 비효율적일 수 있으므로 정책이 필요하다.

## Jupyter Notebook

추출 대상:

- notebook metadata
- markdown cell
- code cell
- output
- execution count
- cell ordering

Python code cell은 Python parser로 재분석할 수 있다.

## Parser Adapter Interface

장기적으로 각 Parser는 공통 인터페이스를 따른다.

```text
Input
→ file

Output
→ Nodes
→ Edges
→ Searchable Text
→ Metadata
→ Source Ranges
```

이를 통해 외부 개발자가 새 포맷을 추가할 수 있도록 한다.

## 향후 파일

```text
.js
.ts
.tsx
.cpp
.h
.java
.rs
.sql
.toml
.xml
```

장기적으로:

- PDF
- DOCX
- PPTX
- XLSX
- Images
- Audio
- Video

그러나 초기에는 지원 개수보다 Parser 품질을 우선한다.
