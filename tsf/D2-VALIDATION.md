---
title: D2 관리자 설치는 성공했으나 앱 입력 승인은 보류
date: 2026-10-12
layout: a4
purpose: technical
disposition: local-reusable
status: blocked-admin-ui
---

# D2 관리자 설치는 성공했으나 앱 입력 승인은 보류

- D2는 관리자 1회 설치 후 일반 사용자 실행으로 결정했다.
- x64·x86 시스템 등록과 COM DLL 로딩은 성공했다.
- 전환 API 성공만으로 실제 앱 입력을 승인할 수 없다.
- 사용자가 입력기 메뉴에 항목이 없음을 확인했다.

## 범위와 구조

Windows TSF S0의 설치 권한·입력기 선택·6개 앱 입력을 시험한다. 후보창, 설정창, 학습 저장, 서명, macOS 실기기 시험과 공개 릴리즈 승인은 이번 설치 시험에 포함하지 않는다.

```mermaid
flowchart LR
  A[관리자 설치] --> B[Program Files의 x64·x86 DLL]
  A --> C[HKLM COM·TSF 프로필·카테고리]
  C --> D[일반 사용자의 enable-user]
  D --> E[입력기 메뉴에서 실제 선택]
  E --> F[앱 입력·조합·확정 확인]
```

기존 `register`는 관리자 실행에서도 COM 등록 위치가 HKCU였다. 다른 관리자 계정으로 인증하는 PC와 관리자 앱에서 사용할 시스템 등록 경로를 분리했다. `register-machine`·`unregister-machine`은 권한 검사 후 HKLM과 공식 TSF API를 사용한다. 일반 사용자의 `enable-user`·`disable-user`가 그 사용자의 목록을 다룬다. 설치 파일은 사용자 쓰기 가능한 빌드 폴더 대신 Program Files에 둔다.

## 관찰과 검증

| 항목 | 관찰 | 판정 범위 |
|---|---|---|
| 빌드·단위 검사 | 공통 엔진 52개, TSF 키 로직 12개, HRESULT 회귀 1개 통과 | 실제 앱 검증과 별도 |
| x64·x86 selftest | DLL 로드, COM exports, `dkssud + Space → 안녕 ` 통과 | 직접 DLL 로딩·엔진 시험 |
| 비관리자 거부 | 양쪽 setup의 설치·제거가 `0x80070005`, 레지스트리·언어 목록 변화 없음 | 권한 없는 부분 설치 방지 |
| 최초 관리자 설치 | 양쪽 HKLM COM·프로필·5개 카테고리 등록 성공 | 설치 receipt 확인 |
| 실제 사용자 등록 | `InstallLayoutOrTip`, `EnableLanguageProfile` 성공; 한국어 목록에 자체 TIP 추가 | 현재 사용자의 설정 존재 |
| 등록 COM 로딩 | `CoCreateInstance` 성공, `IsEnabledLanguageProfile=true` | 등록 경로로 DLL 적재 가능 |
| 최초 전환 | `ActivateProfile=0x80004005` | 실패 |
| 활성화 플래그 추가 | 원시 HRESULT `0x00000000` | API 성공; 화면·DLL 활성화는 별도 확인 필요 |
| 메모장 | 빈 시험 탭에서 합성 `d` 키가 영문 그대로 표시; TSF 런타임 활성화 로그 없음 | TSF 입력 성공 미확인 |
| 사용자 확인 | 작업 표시줄 입력기 메뉴에 한오토마타 없음 | 메뉴 노출 미충족 |

최초 설치는 기본 비활성 프로필을 만들었다. `HKLM Enable=0`, 현재 사용자 `Enable=1`, 프로필 열거 `flags=0`이 함께 관찰됐다. 원인 후보를 줄이기 위해 [Microsoft SampleIME](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/IME/cpp/SampleIME/Register.cpp)와 같은 기본 활성 등록으로 소스와 빌드를 수정했다. **메뉴 누락의 확정 원인이나 해결 완료를 뜻하지 않는다.** 수정본 적용 요청은 `consent.exe`에서 대기하다 Windows가 작업 취소로 반환했다. 사용자는 보안 창이 보이지 않는다고 답했다. 사용자가 직접 취소했다고 단정할 수 없으며, 수정본은 시스템에 적용되지 않았다.

`ActivateProfile`의 [공식 반환값](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofilemgr-activateprofile)은 `S_FALSE`를 프로필 비활성으로 정의한다. 기존 Rust 래퍼는 이를 `Ok(())`로 바꾸므로 원시 HRESULT가 정확히 0일 때만 성공으로 판단하도록 수정했다. 회귀 검사의 실패→통과를 확인했다. 이번 플래그 추가 후 실측값은 `S_OK`였으므로, `S_FALSE`가 이번 화면 실패의 원인이었다고 주장하지 않는다.

## 앱 승인표

| 대상 | 진행 상태 | 남은 증거 |
|---|---|---|
| 일반 메모장 | 새 탭에서 키 입력 관찰; TSF 활성 미확인 | 실제 TSF 조합·확정·단축키 |
| Chrome | 미실행 | TSF 입력·확정·전환 |
| Edge | 미실행 | TSF 입력·확정·전환 |
| Word | 시작 화면 준비 | 빈 문서의 TSF 입력·확정 |
| 시작 검색 | 미실행 | AppContainer 입력·확정 |
| 관리자 메모장 | 미실행 | 관리자 앱의 TSF 입력·확정 |

현재 통과로 승인한 앱은 **0/6**이다. 합성 키의 통과도 물리 키보드 검증으로 대체하지 않는다. S0-5 전환·보정 키, S0-6 관리자/AppContainer, S0-7 트레이 공존은 실제 입력기 선택 후 확인한다.

## 재개·복구 절차

1. 화면 앞의 관리자 PowerShell에서 준비된 수정 도구를 적용한다. 기존 설치 DLL을 유지한 setup 교체·재등록 절차는 로컬 재개 묶음의 `repair-tools.ps1`에 있다. 자동 UAC 재요청은 중단했다.
2. 실제 사용자의 일반 창에서 설치된 setup으로 `enable-user`, `selftest-registered`, `status`를 실행한다.
3. 입력기 메뉴에 나타나고 실제 선택되는지 확인한다. API 성공만으로 다음 단계를 승인하지 않는다.
4. 트레이 앱을 종료하고 새 시험 문서에서 6종 앱을 검증한다. 사용 중인 기존 문서를 시험 대상으로 쓰지 않는다.
5. 기존 입력기로 복원하고 트레이 앱을 다시 실행한다. S0 시험을 종료할 때 일반 창에서 `disable-user`, 관리자 창에서 `install-machine.ps1 -Action Unregister`로 제거한다.

설치 스크립트는 등록 중 실패 시 등록 해제를 시도하고 파일과 진단 기록을 보관한다. **관리자 제거·실패 자동 복구의 실제 실행 검증은 아직 남아 있다.** 이번 기록은 설치 성공을 제품 동작 승인이나 공개 릴리즈 승인으로 확대하지 않는다.

종료 시 기존 Microsoft 한국어 입력기로 복원하는 API가 `S_OK`를 반환했고, 자체 TIP를 사용자 언어 목록에서 제거했다. 한국어 키보드 2개와 영어(미국) 1개가 남음을 확인했다. 기존 Hanautomata 트레이 앱도 다시 실행했다. **최초 설치의 Program Files 파일·HKLM 등록 및 비활성 HKCU CTF 설정은 남아 있다.** 시스템 등록 제거에도 관리자 실행이 필요하므로 전체 제거 완료로 기록하지 않는다.

## 검증 방법 메모

`Get-WinUserLanguageList`는 이 호스트에서 열거되지 않는 List를 반환한다. 바로 `Select-Object LanguageTag,InputMethodTips`로 보내면 null 필드가 나왔다. 권한 거부 회귀 검사는 전체 List를 `ConvertTo-Json -Depth 5`로 직렬화하도록 수정하고 재실행했다. 최초 언어 목록 JSON의 null 값은 복구 기준으로 사용하지 않는다.

Windows PowerShell 5.1에서 Rust UTF-8 출력을 OEM 코드 페이지로 읽어 최초 설치 로그의 한글이 깨졌다. 후속 설치 스크립트는 UTF-8로 읽는다. HRESULT와 ASCII 등록 단계는 최초 기록에서도 판독 가능하다.
