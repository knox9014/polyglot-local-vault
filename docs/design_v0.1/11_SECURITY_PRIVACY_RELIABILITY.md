# 11. Security, Privacy, Reliability

## Local-first 보안

핵심 데이터는 로컬에 저장한다.

## 원본 파일 보호

앱 내부 데이터와 원본을 분리한다.

```text
project files
+
.vault-ai/
```

인덱싱 또는 Graph 처리 때문에 원본을 임의 수정하지 않는다.

## Secret 처리

개발 프로젝트에는 다음이 존재할 수 있다.

- `.env`
- API keys
- credentials
- private certificates

따라서:

- ignore rules
- secret-sensitive path defaults
- MCP exposure policy
- read permission controls

이 필요하다.

## MCP 보안

외부 AI가 모든 파일에 자동 접근하지 않도록:

- read-only 기본
- write tool 별도
- link write 명시적 승인
- ignored files 비공개
- audit log 검토

를 고려한다.

## Parser 신뢰성

AI 추론 대신 결정론적 Parser를 쓰는 핵심 이유 중 하나는 재현성이다.

같은 파일은 가능한 한 같은 구조로 분석되어야 한다.

## Graph 신뢰성

관계의 출처를 저장한다.

예:

```text
origin=manual
origin=parser
origin=git
```

그래야 사용자와 시스템이 관계의 신뢰 수준을 판단할 수 있다.

## 손상 대응

`.vault-ai/`가 삭제되거나 손상되어도 원본 파일을 다시 스캔해 기본 인덱스와 Graph를 재구축할 수 있어야 한다.

즉 `.vault-ai/`는 중요한 캐시/메타데이터이지만 원본 파일보다 우위의 source of truth가 되어서는 안 된다.
