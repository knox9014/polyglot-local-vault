# 13. Future Extensions

이 문서의 기능은 초기 MVP에 포함하지 않는다.

## 추가 파일 형식

- JavaScript
- TypeScript
- C/C++
- Java
- Rust
- SQL
- TOML
- XML

장기적으로:

- PDF
- DOCX
- PPTX
- XLSX
- Images
- Audio
- Video

## Optional Embeddings

기본 검색과 별개로 선택적으로 embedding 검색을 추가할 수 있다.

원칙:

- Core dependency가 되지 않는다.
- local embedding plugin 가능
- 외부 provider 가능

## Hypergraph

실험·의사결정 등 여러 객체가 하나의 사건에 참여하는 구조에서 검토한다.

## Plugin SDK

외부 개발자가:

- Parser
- Viewer
- Search provider
- Metadata provider

를 추가할 수 있게 한다.

## Git 확장

초기 local Git 이후:

- GitHub
- GitLab
- issue/PR 연결

등을 검토할 수 있다.

## 협업

로컬 엔진이 충분히 안정된 이후 별도 제품 단계로 검토한다.

- shared vault
- conflict resolution
- permissions
- team graph

## 모바일/웹

초기에는 제외한다.

로컬 데스크톱 엔진이 핵심이며, 다른 플랫폼은 이후에 판단한다.
