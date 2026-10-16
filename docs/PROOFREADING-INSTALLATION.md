---
title: "트레이·macOS 설치를 확인했으며 TSF 수정본 최종 확인이 남는다"
date: 2026-10-16
updated: 2026-10-17
layout: a4
purpose: operations
reader_job: "Windows·macOS 개발판의 설치 상태, 복구 지점과 남은 확인을 파악한다."
core_message: "Windows 트레이와 macOS 설치·실행은 확인했으며, TSF 수정본은 패키지 검증 후 최종 설치 결과 회수가 남았다."
status: partially-verified-tsf-final-result-pending
---

# 트레이·macOS 설치를 확인했으며 TSF 수정본 최종 확인이 남는다

- Windows 트레이와 macOS의 설치·실행을 확인했다.
- TSF 수정 패키지의 100개 파일과 중첩 목록 검증이 통과했다.
- TSF 최종 설치 결과 회수는 Aside의 402 오류로 미완료다.
- 전체 앱 적용·실행 취소 인수와 공개 배포는 별도다.

사용자가 지정한 Aside 원격 호스트를 통해 기존 0.5.0 설치를 같은 버전의 맞춤법 개발 증분으로 교체했다. 이 문서는 실제 설치 경로에서 관측한 결과다. 최초 빌드 때의 “시스템 설치 미수행” 기록은 [검증 보고서](PROOFREADING-VALIDATION.md)에 역사로 보존한다. 검사 정확도·지연의 승격 조건은 바뀌지 않는다.

```mermaid
flowchart LR
  V[소스·파일 해시 검증] --> B[기존 설치 백업]
  B --> S[새 파일 배치]
  S --> R[OS 등록·활성화]
  R --> C[설치 경로의 검사 응답]
  C --> A[실제 앱 적용·실행 취소 인수]
```

## 현재 판정

| 대상 | 파일·등록 | 실행·검사 | 남은 확인 |
|---|---|---|---|
| Windows TSF x64·x86 | 첫 `proofreading-20261016` 등록·100/100 해시·양쪽 registered selftest 통과. 후속 `fix1` 패키지의 100개 파일·중첩 목록도 검증 | 첫 배치의 검사 응답과 수정 helper의 합성 UI 적용 통과. 최종 `fix1` 설치 경로의 실행은 미확인 | 마지막 관리자 실행 결과·최종 COM 경로·설치 해시·실행 확인 미완료: Aside 402 오류 |
| Windows 트레이 | 수정본 50개 파일 검증, 기존 사용자 설치 경로·시작 작업·바로가기 보존 | 한글 긴 경로에서 단발·감독기 검사 통과, 기본 3초 기한 유지. 기존 시작 작업으로 실행한 PID 24236의 지속 실행 확인 | 실제 앱의 선택·제안 복사 인수 미확인 |
| macOS arm64 | 새 앱 40/40 파일, 서명 및 내부 MANIFEST 30/30 일치 | 새 앱 PID·파일 해시 일치, 설치 checker의 JSONL·문장 해시·`돼요` 제안 확인. TextEdit `가나다` 입력은 사용자 확인 | 실제 편집기의 교정 제안 적용·실행 취소 인수 미확인 |

Windows의 x86 배치 위치에도 **외부 x64 검사기**가 있다. x86 DLL/설정 도구의 등록 자체 검사 통과를 실제 x86 편집기 교정 성공으로 해석하지 않는다. Mac의 `codesign --verify --deep --strict` 통과는 ad-hoc 서명의 무결성 확인이며 공증을 뜻하지 않는다.

## 설치에서 발견한 결함과 수정

### TSF 패키지에 교정 payload 누락

기존 고정 6개 파일 목록은 helper·사전·고지를 빠뜨렸다. 새 테스트에서 누락 실패를 재현하고 `tsf/payload.ps1`을 보완했다. `sha256.json`에 명시된 교정 파일을 양쪽 아키텍처에 완전하게 배치하며, 불완전한 구성·목록 밖 파일·경로 이탈·재분석 지점을 거부한다. 복사 전후 모두 해시를 검사한다.

Windows에서 새 회귀 48개 단언, 기존 payload 회귀 28개 단언이 통과했다. 기존 테스트는 짧은 경로의 격리된 Git fixture에서 실행했으며 설치 대상이나 사용자 설정을 테스트 데이터로 쓰지 않았다.

바이너리만 교체하는 후속 패키지에서는 `CARGO-MANIFEST.json → MANIFEST.json → sha256.json`의 참조를 함께 갱신해야 한다. 새 TSF stage에서 가장 안쪽 목록에 이전 검사기 해시가 남은 것을 발견해 해당 등록 요청을 취소했다. 외부 목록의 파일 해시 통과만으로 내부 목록의 내용까지 검증됐다고 판단하지 않는다. 최초 관리자 wrapper의 PowerShell 5.1 배열 처리 실패도 공식 updater 호출 전 중단으로 기록했으며, 이전 등록과 파일은 유지됐다.

### 한글 사용자 설치 경로

Rust의 사전 해시 검사는 통과했지만 GNU 빌드의 Hunspell이 UTF-8 절대경로를 좁은 문자 파일명 API로 열지 못했다. 동일 파일을 기존 ASCII 짧은 경로로 읽었을 때만 성공해 원인을 좁혔다. 제품 수정은 8.3 경로 생성에 의존하지 않는다.

독립 CLI가 스레드를 만들기 전에 Unicode OS API로 사전 폴더를 작업 디렉터리로 설정하고, native에는 ASCII 상대 파일명을 준다. 변경은 검사 프로세스와 그 자식에만 적용한다. 실패 시 기존 감독기의 `ready → error → 다음 요청` 계약을 유지한다. 한글·공백·이모지 폴더의 단발 검사, 상대경로 감독기 검사 및 변조 사전 거부를 하나의 실제 사전 회귀로 묶었다.

Windows의 기본 회귀 21개, release 실사전 2개와 worker 2개가 통과했다. debug 사전 초기화는 약 2.72초로 기본 3초 기한에 근접했고 초기 실행에서 timeout을 관측했다. 같은 소스의 debug 재실행은 통과했지만 실패 기록을 보존했다. CI의 실사전·worker 통합 검사는 release 프로파일로 실행한다. 제품 기본 기한을 늘리지 않았으며, 새 release 바이너리는 실제 한글 설치 사전의 긴 경로에서 단발·감독기 검사를 모두 통과했다.

### 콘솔 없는 Windows helper

TSF와 같은 WinExe·`CreateNoWindow`·표준 파이프 조건에서 `Console.InputEncoding`과 `Console.OutputEncoding` 설정이 각각 `System.IO.IOException` (`0x80070006`)을 발생시켰다. JSON 파싱과 UI 생성 전에 종료되던 원인이다.

helper는 표준 입력·출력 핸들에 UTF-8 `StreamReader`/`StreamWriter`를 연결한다. 출력은 BOM 없이 즉시 flush하고, 기존 입력 프레임 상한을 유지한다. 콘솔 창을 추가하거나 TSF 실행 방식을 바꾸지 않는다.

기존 helper에서 조기 종료를 재현했고, 수정 helper는 유효 입력의 창 생성·정상 닫기와 잘못된 JSON 거부를 포함한 8개 단언을 통과했다. 최초 자동 적용 검사는 UIA 수집 결과가 비어 있거나 서로 모순되어 판정할 수 없었다. Aside의 Windows 365 화면에서 후보를 보고 적용 버튼을 눌러 진단 복사본의 정상 응답을 확인한 뒤, 강타입 UIA 수집기로 원본 수정 helper를 다시 검사했다. 적용 직전에 활성화된 버튼과 `되요 → 돼요` 후보를 관측했고, exit 0과 요청·문서·revision 식별자를 보존한 교정 JSON을 확인했다. 제품 버튼 결함은 재현되지 않았으며 최초 수집기의 구체적 결함 위치는 확정하지 않았다. 이 결과는 합성 UI 검사이며 실제 사용자 문서에 적용한 증거가 아니다.

## Windows 트레이 실행 전환

수정 검사기·helper와 해당 manifest만 교체했으며 본체·사전·런타임·고지 등 나머지 45개 파일은 기존 SHA-256과 일치했다. 설치된 50개 파일, 외부 manifest의 49개 항목, 내부 manifest의 45개 항목을 검증했다. 한글 사용자 폴더의 긴 경로로 `되요 → 돼요`를 확인했고, 감독기는 기본 `timeout_ms=3000`으로 `ready → check → result → shutdown`을 완료했다.

Aside에서 ShellExecute로 시작한 첫 PID 8724는 후속 조회에서 사라졌다. 정확한 종료 원인은 확정하지 않았다. 등록 내용을 변경하지 않고 기존 사용자의 `--background --autostart` 시작 작업을 실행했다. 작업의 사용자·실행 경로·일반 권한을 대조했고, 명령 종료 후에도 새 PID 24236이 같은 설치 본체 해시로 실행 중임을 별도 조회에서 확인했다. 기존 작업 XML·Run 등록·바로가기·이전 백업을 보존했다.

## TSF 마지막 설치 결과의 미확인 경계

보완한 `proofreading-20261016-fix1` stage는 100개 payload와 양쪽 아키텍처의 Cargo 목록 각 32개, 내부 목록 각 45개 항목을 모두 검증했다. 이전 패키지 대비 helper·검사기·두 내부 목록을 합한 8개 파일이 바뀌었고 92개 파일은 동일했다. 공식 updater의 `-CheckOnly`는 exit 0이었다. stage의 `sha256.json` 해시는 `6d6872266afe54bbbdba879a60eeab4ed445c6a25c759bd1d7bf2acdd68c3d50`이다.

최종 요청은 `evidence/windows-tsf-fix1-admin-cargo.ps1`로, UAC 화면에서 경로를 대조했다. 이때 Aside 원격 에이전트가 `402 Insufficient credits`로 종료됐다. Aside REPL의 화면 제어는 계속 사용했으며 ‘예’ 입력 후 승인 창이 닫힌 것을 관측했다. 그러나 관리자 결과 JSON과 새 등록 상태를 회수하지 못했으므로 **창이 닫힌 사실을 설치 성공으로 판정하지 않는다.** 이후 원격 화면의 터미널 조작도 확인되지 않았다. 다른 브라우저 실행면이나 보안 우회는 사용하지 않았다.

재개 시 첫 작업은 `C:\dev\hanautomata-proofreading-20261016\evidence\windows-tsf-fix1-admin-cargo-result.json`을 읽는 것이다. 기존 실행 결과와 현재 등록을 확인하기 전에는 RunAs를 다시 실행하지 않는다. 설치 성공이 확인되면 준비된 `windows-tsf-fix1-checker-test.ps1`, `windows-tsf-fix1-helper-ui-test.ps1` 및 양쪽 등록 자체 검사로 설치 경로의 실행을 확인한다. 최종 `fix1`의 관리자 결과·설치 후 해시·자체 검사 통과를 현재 문서에서 주장하지 않는다.

## macOS 실행 전환의 경계

기존 입력기는 종료 직후 재시작되어 파일 교체와 경합했다. 별도 백업 후 기존 앱 전체를 입력기 검색 폴더 밖으로 옮겨 실행 중 inode와 원본 바이트를 보존하고, 검증된 새 앱을 설치 경로에 배치했다. 강제 종료·로그아웃·재부팅은 수행하지 않았다.

Aside로 복사한 새 앱은 Gatekeeper에서 차단되었고, 사용자가 시스템 설정에서 Hanautomata를 승인했다. 임시 `hanautomata-install-native`는 사용을 중단했으며 승인 대상으로 삼지 않았다. quarantine 제거와 전역 보안 설정 변경은 하지 않았다.

승인 후 설치된 checker는 약 202ms의 JSONL 결과를 반환했다. 입력기 본체를 Aside에서 직접 실행한 시도에서는 HIServices/LaunchServices 연결 오류와 IMK NSConnection 등록 실패를 관측했다. 이 시도의 실패와 사용자 세션의 정상 실행은 구분했다.

사용자가 구판 종료 후 입력기를 선택했고, **App Translocation 경로의 새 PID 12221**에서 설치 바이너리와 동일한 SHA-256(`1d790429e4e6f9bf086e4018302bcf3581afc8b0ab91256a3c21e5a56750351e`)을 확인했다. 설치 경로에 한정한 최초 `lsof` 조회는 OS의 보호용 실행 경로를 놓친 false negative였다. 뒤이어 사용자가 TextEdit에서 `가나다` 정상 입력을 확인하고 정보 화면을 제공했다. 이 직접 진술은 도구 검증과 구분해 기록했다.

정보 화면은 `0.5.0 (빌드 1) · macOS 26.6.2 · arm64`다. 이번 개발 증분은 버전 번호를 유지했으므로 버전 문자열만으로 구판과 새 빌드를 구별할 수 없다. 실행 파일 해시와 동봉된 교정 payload로 식별했다. `--check`의 종료 코드 0이나 저장된 preference만으로 현재 GUI 입력 소스를 판정하지 않는다.

## 복구와 증거

| 플랫폼 | 보존한 복구 지점 | 원칙 |
|---|---|---|
| Windows TSF | 기존 `v0-5-0` revision과 후속 revision 디렉터리 | 공식 updater의 이전 DLL 경로·rollback tool을 이용한다. 로드된 DLL을 덮어쓰거나 삭제하지 않는다. |
| Windows 트레이 | 교체 전 앱 디렉터리와 별도 백업 | 정확한 설치 본체의 정상 종료 후 검증된 디렉터리를 되돌린다. 시작 설정·학습 자료는 복구 대상으로 덮어쓰지 않는다. |
| macOS | `~/Library/Application Support/Hanautomata/backups/`의 앱 백업과 이동해 보존한 원본 | 파일 해시·서명을 먼저 확인하고 앱 전체의 rename으로 되돌린다. 복구 파일과 실행 중 프로세스를 구분한다. |

원격 Windows의 `C:\dev\hanautomata-proofreading-20261016\evidence\`, 로컬 저장소의 ignored `build/evidence/`에 전체 명령·해시·설치 영수증을 보존한다. 사용자별 절대경로와 환경 정보가 있어 공개 배포물에는 자동 포함하지 않는다. 설치 검사는 자체 합성 문장만 사용했다.

`words.json`은 Mac 설치 전후 동일했다. preference에는 기존 앱이 실행 중인 동안 `capsLock=0` 추가가 관측되었다. 설치 명령이 설정을 쓴 증거는 없으며 원인을 단정하거나 원래 값으로 덮어쓰지 않았다. 파일 교체 후의 비교에서는 설정 내용이 유지됐다.

공개 릴리스, 공증, 독립 검수 1,000문장, 모든 실제 앱의 포커스·조합·보호 필드·적용·실행 취소 인수는 완료하지 않았다. 이 설치 기록이 [로드맵의 승격 게이트](PROOFREADING-ROADMAP.md)를 대체하지 않는다.
