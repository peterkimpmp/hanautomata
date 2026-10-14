---
title: 개발 회귀를 보강했으며 Windows 전체 동작 승인은 남아 있다
layout: a4
purpose: technical
status: in-progress
date: 2026-10-14
baseline: 36794fece157e16617249dbaad580b6e8b6bf982
disposition: local-reusable
---

# 개발 회귀를 보강했으며 Windows 전체 동작 승인은 남아 있다

- C# 단위 단언 26,391개가 현재 기준 소스에서 통과했다.
- 트레이 통합 시험은 TSF 동시 처리 제거 후 106/106이다.
- 보호 필드·선택 보존 실패 4건을 재현하고 수정했다.
- Word 언어 속성 후보의 실제 효과와 6종 앱 승인은 미검증이다.

## 판정과 검증 경로

[계획](WINDOWS-QUALITY-PLAN.md)의 95점·필수 게이트 전부 통과를 아직 충족하지 않는다. 개발 검사 수는 품질 점수가 아니다. 현재 종합 점수는 산정 보류이며 TSF 전체 앱 승인은 **0/6**이다. 설치 API 성공을 앱 승인으로 합산하지 않는다.

```mermaid
flowchart LR
  A[현재 소스 빌드] --> B[회귀 실패 재현]
  B --> C[보호·선택 보존 수정]
  C --> D[자동 시험]
  D --> E[후보 DLL 설치·실제 앱 시험 대기]
```

## 실행된 증거

로그 정본은 무시된 `build/windows-quality/`에 보존한다. 기준 소스는 `36794fe`, 변경 소스는 `codex/windows-quality`의 해당 커밋·diff와 연결한다. 바이너리 SHA256은 `baseline.json` 및 후보 묶음 manifest에 둔다.

| 항목 | 실제 관측·명령 | 판정과 범위 |
|---|---|---|
| C# 기준 단위 | `build.ps1 -Test`, exit 0, `PASS: 26391 assertions; 0 failures` | 통과. 엔진·저장소 검증이며 실제 앱 보증 아님 |
| C# 개발 앱 | `build.ps1`, exit 0, .NET Framework 4.8용 EXE 생성 | 0.5.0 개발 빌드 |
| 기준 패키지 | `tests/ReleaseTests.ps1`, exit 0, 19 assertions | 기준 문서 구성의 패키징·해시·설치 사전 검증. 실제 설치/로그인 아님 |
| 기준 TSF | `tsf/build.ps1 -Test -X86`, exit 0, 70 tests + 양쪽 selftest | 병합된 0.5.0 기준 소스의 새 증거 |
| 보호 결함 재현 | `cargo test --release protection_tests`, exit 101, 1 passed / 2 failed | 조회 오류와 PRIVATE/PIN을 안전한 일반 필드로 보던 정책 실패 |
| 선택 결함 재현 | `cargo test --release selection_tests`, exit 101, 0 passed / 2 failed | 4,097 UTF-16 단위 선택의 앞부분만 교체, 부분 읽기·분리 surrogate 손실 |
| 후보 TSF | `tsf/build.ps1 -Test -X86`, exit 0, 52 core + 14 TSF lib + 15 shell = **81 tests** | x64/x86 release 및 양쪽 DLL selftest 통과. 미설치 후보 |
| 트레이 통합 최초 | `build.ps1 -Integration`, exit 1, 104/106 | Caps Lock 활성 확인과 영문 대문자 출력 실패 |
| 트레이 통합 수정 후 | 같은 명령, exit 0, **106/106** | 시험용 WinForms의 후크·합성 키 경로. 물리 키/다른 앱 아님 |
| 중복 시험 거부 | 실행 중인 트레이를 둔 상태의 `--integration-test`, exit 2 | 새 후크/입력 전에 중복 소유자를 거부 |
| 권한 거부 경로 | `tsf/test-machine-registration.ps1`, exit 0 | 비관리자 등록/제거 거부, 관측한 등록 경로 존재 여부·사용자 언어 목록 불변 |

회귀를 만들면서 발생한 Rust 컴파일 오류는 수정 후 다시 실행했다. 위 결함 재현 행은 컴파일 실패가 아닌 실제 assertion 실패 로그다. 테스트를 통과시키려고 기대 문자열·필수 기준을 낮추지 않았다.

## 변경과 원인

### 보호 필드는 오류와 정상 조회를 구분한다

이전 구현은 InputScope 조회 실패를 `false`로 바꾸고 같은 컨텍스트에서 계속 재사용했다. 후보는 조회 오류·잘못된 VARIANT·암호/PIN/비공개 필드·비활성 컨텍스트에서 키를 가로채지 않는다. 실제 키 콜백마다 재확인하고, 통과 모드로 바뀔 때 이전 조합/재변환 상태를 폐기한다. 원래 문서에 이미 표시된 텍스트는 유지하며 새 필드에 쓰지 않는다. 늦게 온 이전 조합 종료 콜백도 새 세션에 적용하지 않는다.

`IS_PASSWORD`는 보안 경계 자체가 아니므로 앱의 입력 서비스 비활성화도 확인한다. 공식 정의: [InputScope](https://learn.microsoft.com/en-us/windows/win32/api/inputscope/ne-inputscope-inputscope), [TSF compartments](https://learn.microsoft.com/en-us/windows/win32/tsf/predefined-compartments). 실제 앱의 조회 호환성과 지연은 설치 후 검증해야 한다.

### 선택 변환은 전체를 읽었을 때만 바꾼다

이전 구현은 최대 4,096 단위만 읽고 원래 전체 선택을 덮어썼다. 후보는 원본 선택을 유지한 채 복제 범위를 나눠 읽고 한도 초과·조회 오류·잘못된 UTF-16이면 교체하지 않는다. 한도와 정확히 같은 길이, 분리된 surrogate, 작은 청크를 각각 회귀로 검증했다.

### 통합 시험도 실제 트레이의 소유권을 따른다

최초 실패 때 설치된 TSF 로그는 시험 프로세스에서 `tray=false`였다. 시험 진입점은 실제 앱의 mutex를 건너뛰고 있어 TSF도 Caps Lock을 처리했다. 동일 사용자 mutex를 보유하도록 바꾼 후 새 시험 프로세스의 로그는 `tray=true`, 결과는 106/106이었다. Caps Lock 기대값은 그대로 유지했다. 기존 트레이는 정상 종료·복원했고 사용자 문서를 닫지 않았다.

### Word F2의 언어 속성은 아직 후보다

기존 `TF_ST_CORRECTION`만으로는 Word 한/영 자동 고침 ON 충돌을 해결하지 못했다. 후보는 확정 단어를 교체한 같은 편집 세션에서 `GUID_PROP_LANGID`를 결과에 맞춰 기록한다(ASCII 영문 0409, 한글 0412). 혼합 언어·다른 문자·숫자만 있는 선택은 임의로 재분류하지 않는다. 형식 속성은 유지하고 Word 설정은 바꾸지 않는다.

이 API는 [텍스트의 언어 식별자](https://learn.microsoft.com/en-us/windows/win32/tsf/predefined-properties)를 나타낸다. Microsoft SampleIME도 [조합 언어 속성을 기록](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/IME/cpp/SampleIME/Composition.cpp)한다. **Word 자동 고침 억제를 보장하는 문서가 아니므로 실제 성공은 설치 후 화면 결과로 판정한다.**

## 남은 필수 시험

| 항목 | 현재 상태 | 다음 증거 |
|---|---|---|
| Word F2 | 설치된 이전 후보 실패, 새 후보 미설치 | 새 DLL·Auto 모드·자동 고침 ON에서 실제 왕복 |
| 6종 앱 | 전체 승인 0/6 | 앱별 조합/확정/삭제/초점/재변환 및 물리 입력 |
| 자동 시작 | 이 실행 시점 HKCU Run의 Hanautomata 값 없음 | ON/OFF 옵션과 실제 로그인 시험. 현재 값을 임의 변경하지 않음 |
| 설치·복구 | 비관리자 거부와 preflight만 자동화 가능 | 관리자 갱신·제거·실패 복구 실제 실행 |
| 개인정보·입력 보존 | 정책 회귀 통과 | 실제 보호 필드, 동일 컨텍스트 이동, 편집 거부, 취소/복귀 |
| TSF 기능 완성 | 후보/설정 UI·영속 학습 없음 | 구현 및 트레이와 기능별 교차 검증 |
| 성능·지속 사용 | 엔진 재생만 있으며 UI 지연·장기 시험 없음 | 10,000키 실제 경로·p95·30분 입력 |
| 배포 | 개발 후보·미서명 | 출시 게이트 별도, 현재 공개 릴리즈 없음 |

Chrome/Edge는 허용된 Aside 실행면, 시작 검색은 도구 정책, 관리자 메모장은 사용자 화면의 관리자 권한이 필요하다. 기존 제약을 우회하지 않는다. 독립 코드 검토와 설치 후보의 최종 사전 점검은 별도 원장에 남긴다.
