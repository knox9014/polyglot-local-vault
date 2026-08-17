# 04. Vault and Data Model

## Vault

Vault는 사용자의 일반 로컬 폴더다.

특수한 데이터베이스 포맷으로 프로젝트 전체를 감싸지 않는다.

## First-class Objects

예상 Node 유형:

```text
File
Directory
DocumentSection
CodeClass
CodeFunction
CodeMethod
CodeVariable
JSONObject
JSONProperty
YamlNode
Table
Column
Row
Notebook
NotebookCell
GitCommit
```

향후 필요에 따라 확장한다.

## 객체 주소

각 객체는 가능한 한 안정적인 주소를 가진다.

예:

```text
vault://src/router.py
vault://src/router.py#TeacherRouter
vault://src/router.py#TeacherRouter.select_teacher
vault://config/model.json#/router/threshold
vault://docs/architecture.md#teacher-router
```

## Node 예시

```text
NODE
id: python:src/router.py#TeacherRouter
type: PythonClass
name: TeacherRouter
source: src/router.py
```

## Edge 예시

```text
EDGE
from: python:src/router.py#TeacherRouter
relation: defined_in
to: file:src/router.py
origin: parser
```

## Edge Origin

최소 다음을 구분한다.

```text
manual
parser
git
imported
```

AI가 제안한 것은 Core Graph에 자동 저장하지 않는다.

## Stable ID

경로만으로 객체를 식별하면 rename/move 시 관계가 깨질 수 있다.

따라서 구현 단계에서 다음을 조합하는 방식을 검토한다.

- relative path
- file identity
- parser symbol signature
- content hash
- alias/history table

핵심 목표는 파일 이동 후에도 가능한 한 링크를 복구하는 것이다.
