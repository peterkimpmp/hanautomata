---
title: D2 관리자 설치는 성공했으나 앱 입력 승인은 보류
date: 2026-10-13
layout: a4
purpose: technical
disposition: local-reusable
status: word-autocorrect-conflict-confirmed-after-correction-flag
---

# D2 관리자 설치는 성공했으나 앱 입력 승인은 보류

- D2는 관리자 1회 설치 후 일반 사용자 실행으로 결정했다.
- x64·x86 시스템 등록과 COM DLL 로딩은 성공했다.
- 전환 API 성공만으로 실제 앱 입력을 승인할 수 없다.
- 사용자가 메뉴 표시·선택을 확인했고, 수정 DLL의 관리자 적용도 완료했다.
- 새 DLL에서 Word·메모장의 합성 키 기본 조합·확정을 관찰했다.

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
| 최초 사용자 확인 | 작업 표시줄 입력기 메뉴에 한오토마타 없음 | 당시 메뉴 노출 미충족; 재개 후 해결 확인 |

최초 설치는 기본 비활성 프로필을 만들었다. `HKLM Enable=0`, 현재 사용자 `Enable=1`, 프로필 열거 `flags=0`이 함께 관찰됐다. 원인 후보를 줄이기 위해 [Microsoft SampleIME](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/IME/cpp/SampleIME/Register.cpp)와 같은 기본 활성 등록으로 소스와 빌드를 수정했다. **메뉴 누락의 확정 원인이나 해결 완료를 뜻하지 않는다.** 첫 수정본 적용 요청은 `consent.exe`에서 대기하다 Windows가 작업 취소로 반환했다. 사용자는 보안 창이 보이지 않는다고 답했다. 사용자가 직접 취소했다고 단정할 수 없으며, 이 시점에는 수정본이 적용되지 않았다. 이후 사용자가 화면에서 직접 관리자 스크립트를 실행해 적용했다(아래 재개 기록).

`ActivateProfile`의 [공식 반환값](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofilemgr-activateprofile)은 `S_FALSE`를 프로필 비활성으로 정의한다. 기존 Rust 래퍼는 이를 `Ok(())`로 바꾸므로 원시 HRESULT가 정확히 0일 때만 성공으로 판단하도록 수정했다. 회귀 검사의 실패→통과를 확인했다. 이번 플래그 추가 후 실측값은 `S_OK`였으므로, `S_FALSE`가 이번 화면 실패의 원인이었다고 주장하지 않는다.

## 앱 승인표

| 대상 | 진행 상태 | 남은 증거 |
|---|---|---|
| 일반 메모장 | 새 DLL 적재 후 ⇧Space 전환, 합성 `dkssud → 안녕` 조합·Space 확정·F2 재변환 확인 | 실제 키보드 결과 확정·나머지 확정 키·전환 |
| Chrome | 미실행; 허용된 Aside 실행면에서 Chrome 시험을 수행하지 못함 | TSF 입력·확정·전환 |
| Edge | 미실행; 허용된 Aside 실행면에서 Edge 시험을 수행하지 못함 | TSF 입력·확정·전환 |
| Word | 교정 플래그 후보에서도 자동 고침 ON이면 F2 결과가 다시 한글로 바뀜. Word의 자동 고침 취소 후 영문 결과 확인 | Word 자동 고침 충돌 보강·나머지 전환 |
| 시작 검색 | SearchHost 실행이 화면 도구의 제품 정책으로 차단됨 | AppContainer 입력·확정 |
| 관리자 메모장 | 미실행 | 관리자 앱의 TSF 입력·확정 |

기본 조합·Space 확정을 관찰한 앱은 **2/6(Word·메모장)**, 모든 동작을 승인한 앱은 여전히 **0/6**이다. 합성 키의 통과를 물리 키보드 검증으로 대체하지 않는다. S0-5 전환·보정 키, S0-6 관리자/AppContainer, S0-7 트레이 공존은 아직 남아 있다.

## 2026-10-12 수동 관리자 적용 후 재개

사용자가 관리자 수정과 일반 사용자 명령을 실행한 뒤에도 메뉴에 보이지 않는다고 보고했다. `repair-tools.log`는 x64·x86 setup 교체 및 기존 DLL 경로의 기본 활성 재등록 성공으로 끝났고, HKLM 프로필 `Enable=1`을 확인했다. 관리자 설치를 다시 요청할 근거는 없었다.

다만 재개 직후 **현재 일반 사용자는 `IsEnabledLanguageProfile=false`, HKCU `Enable=0`, 한국어 언어 목록의 자체 TIP 없음**이었다. `GetProfile`/`EnumProfiles`의 `flags=0x2`와 현재 사용자의 활성 상태가 달랐다. 이전 사용자 명령이 이 상태를 남긴 이유는 확인하지 못했으며, 다른 계정에서 실행했다고 단정하지 않는다.

현재 계정의 비관리자 셸에서 설치된 x64 도구의 `enable-user`를 실행했다. 두 API가 성공했고, 별도 실행의 `selftest-registered`는 COM 적재 성공과 `IsEnabledLanguageProfile=true`, `status`와 `Get-WinUserLanguageList`는 한국어 목록에 자체 TIP가 있음을 확인했다. 이후 재조회에서도 유지됐다. 기존 한국어 입력기 2개와 영어 입력기 1개는 보존했다. 이후 사용자가 **“보이고 선택할 수 있음”**이라고 확인했다. 현재 메뉴 표시·선택 문제는 해결된 것으로 기록한다.

또한 기존 `selftest-registered`가 비활성 조회 결과를 `ok ... false`와 종료 코드 0으로 보고하던 결함을 보강했다. 사용자 활성화와 언어 목록 존재를 필수 조건으로 삼고, `enable-user`도 호출 후 같은 조건을 확인한다. 비활성 조회 성공·활성 상태지만 목록 누락의 두 회귀 검사가 기존 판단에서 실패함을 재현했다. 이 보강은 진단 결과의 정확성을 높이며, 메뉴 누락 자체의 해결 증거를 대신하지 않는다.

수정 후 공통 엔진 52개·TSF 키 로직 12개·등록 판단 3개, 총 **67개 검사 통과**. x64·x86 빌드와 DLL 직접 selftest, 설치된 COM을 사용하는 두 아키텍처의 `selftest-registered`가 통과했다. 새 x64 `enable-user`의 사후 조건 확인도 통과했다. Program Files의 도구·DLL을 이번 재개에서 다시 교체하지 않았으며, 새 진단 도구는 로컬 `build/tsf/x64`·`x86`에 보관한다.

## 메뉴 선택 후 실제 앱 시험과 조합 상태 결함

트레이 앱을 정상 종료하고 종료된 프로세스를 확인한 뒤, 기존 문서를 보존하면서 메모장의 새 탭과 Word의 새 문서를 시험했다. Word의 런타임 로그에는 같은 프로세스에서 두 차례 `activate ... tray=false`가 기록됐다. 처음 `d`를 입력했을 때 밑줄 조합이 보였지만 다음 키에서 첫 글자가 사라져, `dkssud + Space`의 결과가 `Kssud`로 남았다. 같은 새 문서의 다음 줄에서는 `안녕`의 조합 밑줄과 Space 확정 후 밑줄 해제를 확인했다. 첫 문장의 대문자화와 입력기 결함은 구분한다.

코드에서 프로세스 전체가 하나의 `Engine`을 공유하고, 서비스가 추가 활성화될 때 `Shell::reset()`이 그 엔진의 조합을 취소하는 결함을 발견했다. **첫 서비스가 `d`를 조합하는 동안 두 번째 서비스를 생성·초기화하고, 첫 서비스가 `kssud + Space`를 이어 입력하는 회귀 검사**가 `kssud `로 실패했다. 화면에서 관찰한 소실을 설명하는 구체적인 결함이며, 엔진을 서비스별로 분리한 뒤 `안녕 `으로 통과했다. S0의 학습 저장 미구현 조건은 그대로이며, 사전 파일은 읽기만 한다.

메모장은 DLL이 프로세스에 적재된 것을 확인했으나 전체 단어도 영문 그대로 확정됐다. 현재 로그에는 메모장의 활성 완료 기록이 없고, 이 현상의 원인은 아직 확정하지 않았다. [Microsoft의 IME 요구사항](https://learn.microsoft.com/en-us/windows/apps/develop/input/input-method-editor-requirements)은 Windows 앱 및 AppContainer의 별도 제약을 설명한다. 카테고리나 보안 설정을 추측으로 바꾸지 않았으며 새 DLL 적용 후 앱별 활성화 경로를 확인한다. 공통 조합 결함을 먼저 고치고 Chrome·Edge·시작 검색·관리자 메모장 시험을 이어 간다.

최신 빌드는 **68개 검사**(공통 엔진 52, 키 로직 12, 등록 3, 서비스 격리 1)와 x64·x86 DLL selftest를 통과했다. 이 단계에서는 설치 DLL 적용 전이었고, 이후 사용자가 아래 절차로 적용했다. 원격 main이나 공개 릴리즈에 반영했다고 주장하지 않는다. 이 최초 시험 후 Microsoft 한국어 입력기 복원 API의 `S_OK`와 기존 트레이 앱 재실행을 확인했다. 한오토마타의 사용자 목록 항목은 유지했다.

`update-machine.ps1`은 검증한 새 파일을 Program Files 아래 새 revision 폴더에 배치하고 두 비트수의 COM 경로를 등록한다. 이미 로드된 DLL 파일은 덮어쓰지 않고 이전 경로를 receipt에 남긴다. 일부 등록 실패 시 이전 경로 재등록을 시도하며 파일을 보관한다. 정상 payload 사전 검사, 비관리자 실행 거부, 잘못된 해시 거부 및 등록 경로·언어 목록 무변경을 확인했다. 관리자 적용 성공 경로는 아래에서 확인했으며, **등록 실패 시 복구 경로의 실제 시험은 남아 있다.** 자동 앱 종료·로그오프·재부팅은 없다.

## 새 런타임 적용 후 재시험

사용자가 `build/tsf/d2-session-fix-20261012/apply-admin.ps1`을 직접 실행했다. `revisions/d2-session-20261012/update-result.json`의 결과는 `updated-restart-apps-required`, 시각은 **2026-10-12 22:38:47 KST**다. x64·x86 selftest와 `register-machine`이 각각 종료 코드 0으로 끝났다. 설치 파일 4개의 SHA256, 두 HKLM COM 등록 경로, 일반 사용자의 `selftest-registered`를 확인했다. 이전 DLL은 보관했고 사용자 활성 상태와 언어 목록 항목도 유지됐다.

새 Word 프로세스와 메모장 프로세스에서 `revisions/d2-session-20261012/x64/hanautomata_tsf.dll` 적재를 확인했다. 트레이 앱은 실행되지 않은 상태였다. Word는 새 문서에서 ⇧Space 전환 후 `ㅇ → 아 → 안 → 안ㄴ → 안녀 → 안녕` 조합과 Space 확정을 관찰했다. 두 번째 서비스 활성화가 있었지만 이번 최초 단어에서는 첫 글자 소실이 나타나지 않았다. 이는 이번 합성 키 시도의 결과이며 모든 입력 상황의 결함 해소를 뜻하지 않는다.

메모장 첫 합성 시험은 `안ㄴ` 뒤에 도구가 사용자 입력 감지로 다음 키를 보내지 않았고, 재관찰 후 이어 입력했을 때 `안ㄴㅕ`가 됐다. 중간 포커스·입력이 섞인 시험이므로 연속 입력 결함의 재현으로 단정하지 않는다. 이후 물리 키보드 결과에 대해 사용자가 “글자가 빠지거나 자모가 분리됨”을 선택했으나, 뒤이어 실수로 눌렀다는 정정이 왔다. 정정 대상이 페이지 열기인지 시험 응답인지 확인 중이며 **물리 키보드 판정은 미확정**이다. 재관찰 시 준비했던 시험 탭은 비어 있었다.

다음 합성 시험은 그 빈 탭에서 첫 `d`가 영문으로 표시되는 것을 확인한 뒤 ⇧Space로 전환하고 `dkssud + Space`를 중단 없이 입력했다. 최종 화면은 앞선 영문 `d`와 확정된 `안녕 `이었으며, 한글 조합 중 밑줄과 Space 후 해제를 확인했다. 현재 재현만으로 추가 DLL 변경이나 관리자 재설치를 요청하지 않는다. 메모장 활성 완료 로그는 여전히 없어서 로그 부재만으로 TSF 미동작을 판단할 수도 없다.

화면 근거는 AIPM의 비공개 로컬 scratch `hanautomata-tsf-d2-20261012`에 `word-new-runtime-basic-commit.png`, `notepad-new-runtime-basic-commit.png`로 보관했다. 사용자 탭 제목 등이 포함되므로 공개 저장소에는 추가하지 않는다. 이후 실제 키보드 결과와 앱별 전체 검증을 이어 간다.

확인 응답을 기다리는 동안 Microsoft 한국어 입력기로 복원했다. 전환 API의 `S_OK`와 후속 `active`의 Microsoft TIP 토큰을 확인했고, 기존 트레이 실행 파일의 프로세스도 확인했다. 설치된 새 DLL·시스템 등록·한오토마타 사용자 목록 항목·시험 문서는 보존했다.

## 추가 확정 키 시험과 Word F2 후보 수정

계속 진행 요청 후 트레이가 실행되지 않고 한오토마타가 선택된 상태에서 시험을 재개했다. Word 새 시험 문서에서 조합 중 ⇧Space, Enter 확정 후 다음 줄 입력, Tab 확정 후 탭 뒤 입력, Backspace의 `아 → ㅇ` 조합 삭제를 확인했다. Enter 직후 화면만으로 줄바꿈이 불명확했으나 다음 문자가 실제 다음 줄에 들어가는 것으로 구분했다.

Word에서 자동 판별 `dkssud + Space → 안녕 ` 확정 직후 F2를 눌러도 변화가 없었다. 메모장에서는 같은 입력 뒤 F2가 `dkssud `로 바뀌었다. 이 앱별 차이는 합성 키로 관찰했으며 물리 키보드 결과를 대신하지 않는다. Word가 일반 키 이벤트에 앞서 F2를 처리하는 것이 원인 후보이고, 실제 콜백 전달 순서는 아직 추적하지 못했다.

기존 서비스는 TSF 보존 단축키를 등록하지 않았고 `OnPreservedKey`가 항상 미처리를 반환했다. [PreserveKey](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfkeystrokemgr-preservekey)와 [OnPreservedKey](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfkeyeventsink-onpreservedkey)를 통해 F2 재변환 경로를 추가한 **후보**를 준비했다. 등록 성공한 키만 서비스 종료 때 해제한다. 명령 경로는 현재 필드·트레이 상태를 다시 확인하고, 직전 Space 확정 단어가 없거나 비밀번호/트레이 공존·일시정지·보정 기능 꺼짐이면 처리하지 않는다. 등록 실패는 로그에 남기고 기존 일반 키 경로를 유지한다. Shift+F2·F12 등의 보존 단축키 등록은 이번 변경에 포함하지 않았다.

새 명령 경로의 단어 재변환·다시 되돌리기·새 조합 시 앱으로 전달, 그리고 공존·일시정지·기능 꺼짐의 두 검사를 추가했다. 미처리 명령 분기에서 재변환 검사가 실패했고, 연결 후 전체 **70개 검사**(공통 엔진 52, 키 로직 14, 등록 3, 서비스 격리 1)와 x64·x86 빌드·DLL selftest가 통과했다. 별도 Windows COM 시험 프로세스에서 서비스 활성화를 시도한 검사는 준비 단계가 `0x80070057`로 실패해 등록/해제 검증으로 사용할 수 없었다. 이 실패를 제품 회귀 검사 통과로 계산하지 않았고 해당 시험 코드는 후보에 포함하지 않았다. **실제 앱에서 보존 단축키 등록·콜백·해제를 확인하는 검증은 남아 있다.**

적용 묶음은 로컬 `build/tsf/d2-f2-fix-20261012`다. 설치할 파일 4개의 복사 전후 해시와 `sha256.json`, 기존 설치 경로를 확인했고, `-CheckOnly`는 `preflight-passed`, `changes=none`이었다. 당시 기존 등록은 `d2-session-20261012`였고 후보는 적용 전이었다. 비교 화면은 로컬 scratch의 `word-f2-no-change.png`, `notepad-f2-reconverted.png`다. 시험 후 Microsoft TIP 복원과 트레이 프로세스를 확인했다.

## 2026-10-13 F2 후보 적용과 미해결 확인

사용자가 화면의 관리자 PowerShell에서 F2 후보를 적용했다. `revisions/d2-f2-20261012/update-result.json`은 **2026-10-13 05:14:52 KST**, `updated-restart-apps-required`이며 양쪽 비트수의 selftest·시스템 등록이 종료 코드 0이다. 설치 파일 4개 해시 일치와 일반 사용자의 COM 적재·활성 상태·언어 목록 검사를 확인했다. 이전 `d2-session-20261012` 파일은 보존됐다.

트레이 앱을 정상 종료하고 한오토마타로 전환한 뒤, 기존 Word 문서를 닫지 않고 `/w`로 새 프로세스(PID 24832, 문서2)를 열었다. 이 프로세스에서 `d2-f2-20261012/x64/hanautomata_tsf.dll` 적재를 확인했다. 기존 Word 프로세스는 이전 DLL을 유지하므로 시험 대상으로 쓰지 않았다.

새 문서의 합성 키 `dkssud + Space`는 `안녕 `으로 조합·확정됐으나 F2 뒤에도 그대로였다. 로그에는 `preserved F2 handled=false`가 기록됐다. 사용자가 같은 문서에서 실제 키보드로 Enter → `dkssud` → Space → F2를 연속 입력한 결과도 **“안녕 그대로”**라고 확인했다. 이 응답은 이전 메모장 시험의 모호한 정정을 대신하지 않는다. 추가 합성 키 `hello + Space`는 `hello `로 확정됐지만 F2는 역시 미처리였다. 당시에는 모드·수신 서비스·재변환 상태를 구분할 로그가 없었다. **`hello` 결과만으로 Auto 모드를 추론한 해석은 후속 시험에서 철회했다. Word의 자동 고침도 출력에 영향을 준다.**

따라서 **F2 보존 콜백 전달은 확인됐지만 Word 재변환 문제는 미해결**이다. 일반 키 이벤트가 Word에 선점되는 것만이 원인이라는 가설은 해결 근거로 사용할 수 없다. 앱 승인 수는 0/6이다. 비교 화면 `word-preserved-f2-first.png`는 앞선 비공개 scratch에 보관했다.

추측으로 입력 동작을 다시 바꾸지 않고, 로컬 `build/tsf/d2-f2-trace-20261013`에 상태 추적용 후보를 준비한다. 일반 Word에서 `%LOCALAPPDATA%/Hanautomata/tsf-diagnostics.enabled` 파일이 있을 때만 서비스 ID·스레드 ID·모드·조합 여부·재변환 가능 여부·pass-through·일시정지·보존 키 등록 여부를 기록한다. Space/F2 처리 전후, 포커스·조합 종료·모드 알림을 비교한다. 입력 문자·키 코드·문서명·선택 내용은 기록하지 않으며 비밀번호 필드의 상태 추적은 생략한다. 서비스 재활성화 시 표식 존재 여부를 읽으므로 진단 종료 때 표식을 제거하고 시험 앱을 다시 연다. 이 후보는 원인 규명을 위한 진단판이며 Word 해결본으로 부르지 않는다.

## 2026-10-13 추적판 적용과 Word 자동 고침 A/B 시험

사용자가 적용한 `d2-f2-trace-20261013` receipt는 **2026-10-13 05:27:21 KST**, `updated-restart-apps-required`다. 설치 파일 4개의 SHA256이 묶음 manifest와 일치했고 일반 사용자의 등록 COM·활성화·언어 목록 검사가 통과했다. 기존 문서를 보존하며 `/w`로 연 새 Word(PID 25680, 문서3)가 이 revision의 x64 DLL을 적재했음을 확인했다. 기존 Word 프로세스 두 개는 이전 DLL을 유지했다.

| 구분 | 추적 및 화면 관찰 | 해석 범위 |
|---|---|---|
| 최초 실행 모드 | 활성화 직후 Word의 입력 모드 알림이 Auto를 English로 바꿈. ⇧Space 후 Korean, Space 확정 뒤 `fresh=false`, F2 `handled=false` | 이 시도의 미처리는 한글 고정 모드에서 재변환 단어를 만들지 않는 동작으로 설명됨. 앞선 사용자 시험의 모드까지 소급 확정하지 않음 |
| 열린 Word에서 입력기 재선택 | Microsoft IME로 전환한 뒤 한오토마타를 다시 선택하자 Auto 유지. Space 뒤 `fresh=true`, F2도 같은 서비스 1·스레드 17932에서 `handled=true` | 새 서비스가 확정 단어를 없앴다는 가설과 구분됨. 화면의 `안녕`은 그대로였음 |
| 한/영 자동 고침 OFF | 문장 중간의 `gksrmf + Space`가 `한글`, F2로 `gksrmf`, 다시 F2로 `한글` | 같은 설치 DLL에서 합성 키 왕복 성공 |
| 원래 ON으로 복원 | 같은 문장 중간 입력 뒤 F2가 처리됐지만 화면은 `한글` 그대로 | Word 자동 고침과 재변환의 충돌을 A/B/A로 재현. 최종 사용자 설정은 원래 ON |
| 문장 첫 글자 대문자화 | `Dkssud`로 바뀐 단어에서는 `reconversion skipped: text before the caret changed` | 호스트가 바꾼 텍스트를 덮어쓰지 않는 안전 검사 동작. 대소문자 차이를 무시하도록 완화하지 않음 |
| F12 | 일시정지 시 `paused=true`, 다시 누른 후 `paused=false`; 저장 대화상자 없음 | 이 Word 합성 키 시도에서 일시정지·복귀 관찰 |

[Word의 CorrectKeyboardSetting 설명](https://learn.microsoft.com/en-us/office/vba/api/word.autocorrect.correctkeyboardsetting)은 언어와 자판이 다른 단어를 해당 알파벳으로 자동 전환하는 설정을 설명한다. 실제 충돌 판단은 문서 설명만이 아니라 위 ON/OFF/ON 화면 대조에 근거한다. 시험 중 한/영 자동 고침만 일시 변경했고 원래 ON으로 복원했다. 사용자 문서를 닫거나 저장하지 않았다.

비공개 scratch에는 `word-autocorrect-before.png`, `word-f2-hangul-to-latin.png`, `word-f2-latin-to-hangul.png`, `word-f2-autocorrect-restored.png`와 PID 25680의 상태 로그를 보관한다. 입력기 로그 자체는 입력 문자를 기록하지 않는다. 화면에는 시험 문서와 사용자 창 제목이 있으므로 공개 저장소에 넣지 않는다. 현재 메모장은 이전 DLL을 적재하고 있어 추적판의 메모장 검증으로 계산하지 않는다.

### 교정 플래그 후보 — 빌드 통과, 설치·효과 미검증

F2와 Shift+F2가 확정된 텍스트를 바꿀 때 `ITfRange::SetText`에 `TF_ST_CORRECTION`을 전달하도록 두 호출을 수정했다. [공식 SetText 문서](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfrange-settext)에 따르면 이 플래그는 기존 내용의 교정임을 표시하며 원래 텍스트 속성을 보존한다. **Word 자동 고침을 억제한다고 문서가 보증하지 않으며, 이번 충돌에 효과가 있는지는 설치 후 재시험해야 한다.** Auto 초기화 문제와 문장 첫 글자 대문자화는 이 후보가 해결했다고 주장하지 않는다.

수정 전 실패 근거는 위 실제 Word A/B/A 시험이다. 수정 후 `build.ps1 -Test -X86`가 종료 코드 0으로 완료됐고 공통 엔진 52·키 로직 14·등록 3·서비스 격리 1, **70개 검사**, x64·x86 release 빌드와 DLL selftest가 통과했다. `git diff --check`도 통과했다. 로컬 적용 묶음은 `build/tsf/d2-word-correction-20261013`이며 기존 DLL을 보관하는 새 revision 방식이다. 4개 파일의 복사 전후 SHA256과 manifest가 일치했고 `-CheckOnly`는 `preflight-passed`, `changes=none`이었다. **설치된 것은 여전히 추적판이고 교정 후보는 미설치다.** Word 설정 변경을 제품 해결책이나 앱 승인으로 계산하지 않는다.

진단 종료 때 Microsoft 한국어 입력기 복원 API의 `S_OK`와 후속 활성 TIP를 확인했다. 기존 Hanautomata 트레이 실행 파일의 프로세스를 확인했고, 이번 진단에서 새로 만든 표식만 제거했다. 이미 활성화된 서비스는 진단 여부를 캐시할 수 있으므로 표식 제거를 기존 서비스의 즉시 기록 중단 보장으로 해석하지 않는다. 한오토마타 사용자 목록·시스템 등록·Word 시험 문서·이전 DLL은 보존했다. 문서 점검은 두 파일 모두 PASS였으며 가독성 경고는 남아 있다.

## 2026-10-13 교정 플래그 후보 적용 후 재시험 — 실패

사용자가 `d2-word-correction-20261013`을 적용했다. 설치 receipt는 **2026-10-13 05:58:19 KST**, `updated-restart-apps-required`이며 두 비트수의 selftest·시스템 등록은 종료 코드 0이다. 설치된 파일 4개의 SHA256과 묶음 manifest가 일치했고 일반 사용자의 등록 COM·활성 상태·언어 목록 검사가 통과했다. 같은 revision의 관리자 적용을 다시 요청하지 않는다.

새 Word 프로세스 PID 19916(문서6, 창 1379892)에서 `revisions/d2-word-correction-20261013/x64/hanautomata_tsf.dll` 적재를 확인했다. 트레이는 정상 종료한 상태였다. 문서가 열린 뒤 한오토마타를 활성화했고, 추적 로그의 서비스 1·스레드 22980이 **Auto**, `pass=false`, `paused=false`임을 확인했다. Word 옵션 화면에서 **한/영 자동 고침 ON**을 확인하고 값을 바꾸지 않은 채 대화상자를 닫았다.

| 순서 | 실제 Word의 합성 키 입력과 관찰 | 판정 |
|---|---|---|
| 1 | 본문에 `dkssud + Space + gksrmf + Space` → `안녕 한글` | 두 단어 조합·확정 확인. F2 대상은 문장 중간 단어 |
| 2 | F2 → 화면은 `안녕 한글`; 로그는 같은 서비스의 `fresh=true`, `handled=true` | 교정 플래그만으로 자동 고침 충돌을 막지 못함 |
| 3 | 다시 F2 → 화면 그대로; `reconversion skipped: text before the caret changed` | 호스트 변경으로 엔진이 예상한 텍스트와 달라져 안전 검사에서 거부 |
| 4 | Word의 Undo 표시가 `자동 고침 취소`인 것을 확인한 뒤 Ctrl+Z → `안녕 gksrmf` | F2 결과가 Word 자동 고침에 의해 다시 한글로 바뀌었음을 뒷받침하는 직접 화면 증거 |

따라서 **교정 플래그 후보의 Word F2 해결 판정은 실패**다. 로그의 `handled=true`와 설치 API 성공을 실제 재변환 통과로 계산하지 않는다. Ctrl+Z는 원인 확인용 관찰이며 제품의 정상 F2 사용 절차로 제안하지 않는다. 한/영 자동 고침 OFF를 해결책으로 적용하지도 않았다. 물리 키보드 결과와 나머지 앱 전체 검증은 별도이며 전체 승인은 **0/6**을 유지한다.

비공개 scratch에 `word-correction-autocorrect-on.png`, `word-correction-f2-first.png`, `word-correction-undo-autocorrect.png`, `word-correction-pid19916.log`를 보관했다. 시험 후 Microsoft 입력기 전환 `S_OK`와 후속 활성 TIP, 기존 트레이 프로세스를 확인했다. 이번에 만든 진단 표식을 제거했고 기존 문서·설치 DLL은 보존했다. 이 재시험에서는 소스 동작을 추가 변경하거나 빌드 검사를 반복하지 않았다. 다음 보강은 확정 텍스트의 언어 속성과 Word의 후속 교정을 함께 다뤄야 하며, 구체적 수정 방식의 효과는 아직 미검증이다.

실행 방법 주의: Word `/w`를 숨김으로 시작한 PID 9732는 화면 도구에 창을 노출하지 않았다. 일반 앱 실행은 이전 PID의 시작 창(문서5)을 열었으므로 해당 빈 창만 닫고 시험에서 제외했다. 실제 시험에는 표시된 새 `/w` 프로세스 PID 19916을 사용했다. 숨김 프로세스를 강제 종료하지 않았으며, 창 제목이나 실행 명령만으로 새 DLL 적재를 추정하지 않는다.

## 2026-10-14 Windows 품질 계획과 후속 후보

[Windows 품질 계획](../docs/WINDOWS-QUALITY-PLAN.md)과 [개발 시험 결과](../docs/WINDOWS-QUALITY-RESULTS.md)를 추가했다. `36794fe`에서 새 기준 빌드와 70개 검사를 실행했으며, 후속 후보는 보호 필드의 조회 실패·PIN/비공개 scope·컨텍스트 재조회, 긴 선택 영역 보존, F2 결과의 언어 속성을 보강한다. 첫 후보 빌드의 81개 검사와 양쪽 selftest는 통과했으나 **후속 후보는 아직 미설치이며 Word F2 해결 판정도 미검증**이다. 기존 설치·실패 근거와 전체 앱 승인 0/6은 유지한다.

## 재개·복구 절차

1. 메뉴 수정·서비스 격리·F2 보존 단축키·추적판·교정 플래그 후보의 관리자 적용은 완료했다. 같은 revision의 적용 스크립트를 반복하지 않는다.
2. 현재 설치 receipt는 `Program Files/Hanautomata-TSF-S0/revisions/d2-word-correction-20261013/update-result.json`이다. 일반 사용자 활성화는 유지돼 있다.
3. 교정 플래그 후보의 Word 재시험은 실패했다. 추가 관리자 설치를 요청하기 전에 Word 자동 고침과 확정 텍스트 속성의 상호작용을 보강해야 한다. 보이지 않는 UAC를 반복 요청하거나 관리자 터미널을 화면 자동화하지 않는다.
4. 후속 revision에서는 새 Word 프로세스의 DLL 경로와 Auto 모드를 확인하고, 한/영 자동 고침 ON 상태에서 문장 중간 F2 왕복을 먼저 시험한다. 자동 종료·로그오프·재부팅은 하지 않는다.
5. 메모장 실제 키보드 결과와 6종 앱의 나머지 항목을 검증한다. Chrome·Edge 실행면 확보, 시작 검색 도구 정책 제한, 관리자 메모장 권한은 미검증 조건이다. API 성공이나 합성 키 결과만으로 승인하지 않는다.
6. 기존 입력기로 복원하고 트레이 앱을 다시 실행한다. S0 시험을 종료할 때 일반 창에서 `disable-user`, 관리자 창에서 `install-machine.ps1 -Action Unregister`로 제거한다.

설치 스크립트는 등록 중 실패 시 등록 해제를 시도하고 파일과 진단 기록을 보관한다. **관리자 제거·실패 자동 복구의 실제 실행 검증은 아직 남아 있다.** 이번 기록은 설치 성공을 제품 동작 승인이나 공개 릴리즈 승인으로 확대하지 않는다.

이전 시험 종료 시에는 기존 Microsoft 한국어 입력기로 복원하는 API가 `S_OK`를 반환했고, 자체 TIP를 사용자 언어 목록에서 제거했다. 한국어 키보드 2개와 영어(미국) 1개가 남음을 확인했다. 기존 Hanautomata 트레이 앱도 다시 실행했다. 이번 재개에서는 **Program Files·HKLM 등록을 유지한 채 현재 사용자를 다시 활성화하고 자체 TIP를 추가했다.** 시스템 등록 제거에는 관리자 실행이 필요하며, 전체 제거 완료로 기록하지 않는다.

## 검증 방법 메모

`Get-WinUserLanguageList`는 이 호스트에서 열거되지 않는 List를 반환한다. 바로 `Select-Object LanguageTag,InputMethodTips`로 보내면 null 필드가 나왔다. 권한 거부 회귀 검사는 전체 List를 `ConvertTo-Json -Depth 5`로 직렬화하도록 수정하고 재실행했다. 최초 언어 목록 JSON의 null 값은 복구 기준으로 사용하지 않는다.

Windows PowerShell 5.1에서 Rust UTF-8 출력을 OEM 코드 페이지로 읽어 최초 설치 로그의 한글이 깨졌다. 후속 설치 스크립트는 UTF-8로 읽는다. HRESULT와 ASCII 등록 단계는 최초 기록에서도 판독 가능하다.

2026-10-13 후속 확인에서 메모장의 파일 → 새 창은 별도 창만 만들고 기존 PID 22616을 재사용했다. 이 프로세스가 적재한 DLL은 여전히 `d2-session-20261012`였다. 새 창을 최신 런타임 검증으로 계산하지 않았으며, 시험을 위해 만든 0자 빈 창만 닫고 기존 탭은 보존했다. 메모장 최신 DLL 시험에는 기존 문서의 안전한 보존과 프로세스 재시작이 필요하다. 같은 확인 시점에 교정 후보 설치 receipt는 없었고 두 비트수의 HKLM 경로는 추적판을 유지했으며, 에이전트의 권한은 일반 사용자였다.
