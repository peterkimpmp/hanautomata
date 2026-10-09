# HanFlow

**한영키를 바꾸지 않고 입력하는 Windows 트레이 앱 · v0.4.0 공개 베타**

HanFlow is a Windows tray app that lets you type Korean or English from the same physical keys without toggling the 한/영 mode. It decides word by word (hand lists first, then a syllable-bigram vs letter-bigram likelihood ratio), shows a small candidate window, commits on Space, and fixes mistakes with F2 (last word) or Shift+F2 (selected text). No admin rights, no network, no cloud model. MIT licensed; model tables are aggregate statistics from Wikipedia dumps (CC BY-SA 4.0).

`HanFlow.exe`를 실행하고 입력 칸에 타이핑하세요.
`dkssudgktpdy`는 **안녕하세요**, `hello world`는 **hello world**로 판별합니다.
작은 조합창에 후보가 보이고 **Space에서 실제 편집 칸에 확정**됩니다.
현재 Microsoft 입력기의 한/영 모드와 관계없이 같은 물리 키를 처리합니다.

> **베타 상태**: 코드는 Windows 1대(클라우드 PC)에서 기능 검사 101개와 설치를 확인한 v0.3.0과 같고, 이 공개판은 통계 모델 표를 공개 코퍼스로 바꾼 것입니다. 실물 PC의 물리 키보드와 실제 앱(브라우저·채팅·편집기)의 최종 표시 결과는 아직 확인하지 않았습니다. 빌드는 서명되어 있지 않습니다. 자세한 범위는 [검증 기록](docs/VERIFICATION.md)에 있습니다.

## 키

| 키·동작 | 기능 |
|---|---|
| Space | 후보와 공백 확정 |
| Enter | 후보 확정 후 Enter 전달. 채팅 앱에서는 전송될 수 있음 |
| `;` — 단어 조합 중 | 후보 바꾸기. 확정하면 선택을 기억 |
| `;;` — 단어 뒤 | 원래 후보로 돌아가 실제 `;` 문자 추가 |
| F2 — 단어 조합 중 | 후보 바꾸기. 확정하면 선택을 기억 |
| F2 — Space 확정 직후 | 직전 단어를 다른 후보로 교체하고 선택을 기억. 같은 창·같은 칸에서 15초 안이며 커서 이동·클릭·Enter 뒤에는 동작하지 않음 |
| Shift+F2 — 글자를 선택한 뒤 | 선택 영역의 한글↔영타를 그 자리에서 뒤집기. UI Automation으로 읽고, 안 되면 Ctrl+C 복사 뒤 클립보드 글자를 복원. 앱의 Ctrl+Z로 되돌림 |
| Backspace | 조합 중인 마지막 키 지우기. `;` 직후에는 후보 변경 취소 |
| Esc | 미확정 조합 취소 |
| F12 | 자동 판별 일시정지·재개 |
| 입력 연결 복구 버튼 | 후크를 다시 연결하고 미확정 조합을 복사용으로 보관 |
| 창 닫기 | 트레이로 숨기기 |
| 트레이 우클릭 → 종료 | 앱과 키보드 후크 종료 |

**Zero Admin**: 관리자 권한·서비스·계정·서버·API 키가 없습니다. 현대 Windows의 .NET Framework 4.8을 사용하는 휴대용 EXE입니다.
처음 실행하면 연습창이 열립니다. 트레이의 ‘한’ 아이콘을 더블클릭해 다시 열 수 있습니다.
Windows 시작 시 실행은 트레이에서 직접 켜는 사용자별 옵션이며 기본값은 꺼짐입니다.
트레이의 **판별 민감도**(보수·표준·적극)는 보류 구간의 폭만 바꿉니다. 오변환이 거슬리면 보수, 짧은 단어가 안 바뀌면 적극을 고르세요.

## 설치

소스에서 빌드해 현재 사용자에게 배치합니다(Windows 10/11, 별도 SDK 없이 기본 C# 컴파일러 사용).

```powershell
.\build.ps1
.\install.ps1          # %LOCALAPPDATA%\Programs\HanFlow + 바탕 화면·시작 메뉴 바로가기
.\install.ps1 -Startup # 자동 시작까지 원할 때
```

휴대용 폴더에서 쓰려면 `HanFlow.exe`와 `HanFlow.exe.config`를 같은 폴더에 두고 실행합니다. 제거는 트레이에서 자동 시작을 해제하고 종료한 뒤 앱 폴더와 바로가기를 지우면 됩니다. 앱별 제외 설정도 지우려면 `%LOCALAPPDATA%\HanFlow\settings.json`을 삭제합니다.
서명되지 않은 베타 빌드라 SmartScreen·백신이 경고할 수 있습니다. 직접 빌드하거나 CI 산출물의 SHA-256과 대조하세요([SECURITY.md](SECURITY.md)).

## 자동 판별의 범위

두벌식/QWERTY용입니다. 일반 영어·기술 용어는 작은 내장 목록으로 보호하고, 자주 쓰는 한국어 목록과 음절 통계로 후보를 고릅니다.
같은 키열을 한글 음절 bigram과 영문 자모 bigram으로 각각 점수화해 우도비로 판별하며, 판별이 갈리는 구간에서만 직전 단어의 언어를 참고합니다.
모델 표는 `data/korean-lm.txt.gz`(367KB)·`data/english-lm.txt`이고 대형 언어 모델·외부 사전·네트워크를 사용하지 않습니다.

- `go`처럼 영어 단어이면서 ‘해’가 될 수 있는 키열은 영어로 보존합니다. 한글을 원하면 `;` 또는 F2를 누르세요.
- `dkssudzz` → `안녕ㅋㅋ`, `rkatkgkqslekbb!` → `감사합니다ㅠㅠ!`처럼 한글 뒤의 채팅 표현도 처리합니다.
- `doWork`·`DoWork`처럼 내장 영어 단어로 나눌 수 있는 식별자는 영문으로 보존합니다.
- `AIdml` → `AI의`, `4wja` → `4점`, `rjawmd/wjrdyd` → `검증/적용`처럼 영단어·숫자·기호와 한국어의 혼합을 처리합니다(최대 64키·8구간).
- Caps Lock으로 대문자가 입력되어도 한글 영타를 판별합니다(`EHDWKRDKSGKA` → **동작안함**). Shift로 직접 입력한 대문자 약어는 보호합니다.
- 주소·파일명·경로는 보호합니다. 이름·신조어·짧은 단어에는 오판이 있을 수 있습니다. 의미상 오타·맞춤법 교정은 하지 않습니다.

후보창의 **‘확인 필요’**는 짧거나 선택 이력이 충돌하거나 혼합어 후보 비용이 비슷한 입력, **‘음절 추정’**은 사전보다 약한 통계 근거를 뜻합니다.

정식 TSF IME를 등록하는 앱은 아닙니다. **시스템 전역 입력 도우미**이며, 일반 권한의 편집 가능한 입력 칸을 Windows 접근성 API로 확인할 수 있을 때 동작합니다.
비밀번호·읽기 전용·관리자 실행 앱·입력 대상 확인 실패 상태는 기본 입력기로 통과합니다.
게임의 Raw Input, 보안 데스크톱, 터미널, 일부 웹 편집기에서는 동작하지 않을 수 있습니다.

## 로컬 데이터

앱은 문장 전체나 연속 키 기록을 저장하지 않으며 네트워크 통신을 하지 않습니다.
직접 후보를 바꾸고 확정한 단어에 한해 키열·한글/영문 선택·선택 가중치·앞 단어 지문을 `%LOCALAPPDATA%\HanFlow\learned-words.json`(읽을 수 있는 JSON)에 저장합니다. 앞 단어는 원문 대신 SHA-256 지문입니다.
설정 파일에는 제외한 프로세스 이름, 민감도, 개인 학습·세미콜론 전환 사용 여부를 저장합니다.
UI Automation에서는 편집 가능·비밀번호·포커스 등 속성만 읽고 대상 문서의 본문은 조회하지 않습니다.
`--diagnose-input <경로>`를 지정했을 때만 입력 원문 없이 앱·입력 방식·문자 종류·차단 사유를 순환 기록합니다.

## 선택을 기억하는 사용법

1. `rm`을 입력한 뒤 **`;` 또는 F2**로 `그`를 고릅니다.
2. **Space**로 확정합니다. 이때 선택을 기억합니다.
3. 다음에 `rm`을 쓰면 바로 `그`가 후보가 됩니다. 재실행해도 기억합니다.
4. 이미 Space로 확정한 뒤라면 바로 **F2**를 눌러 직전 단어를 바꿉니다.

같은 키열과 같은 앞 단어에서 최근 직접 선택을 우선하고, 새로운 문맥에서는 단어 전체의 누적 선택을 참고합니다. 자동으로 입력된 결과는 정답으로 학습하지 않습니다.
트레이에서 **‘개인 선택 기억 사용’**, **‘최근 학습 되돌리기’**(현재 실행에서 20회), **‘개인 학습 초기화’**를 쓸 수 있습니다. 단어와 문맥을 합쳐 최대 1,024개 항목을 기억합니다.
세미콜론을 자주 쓰는 코드 작성에서는 트레이의 **‘단어 뒤 ; 로 후보 전환’**을 끄면 기존 입력 방식으로 돌아갑니다.

## 개발 및 검증

```powershell
.\build.ps1 -Test          # 단위 검사(Framework csc)
.\build.ps1 -Integration   # 전용 시험창에서 실제 후크·Unicode 입력 검사
.\package.ps1              # deliverables/HanFlow-0.4.0-win.zip + SHA256SUMS
```

macOS·Linux(또는 Windows의 .NET SDK 9)에서는 `tests/dotnet/`로 같은 소스를 검사합니다. 전체 앱은 .NET Framework 4.8 참조 어셈블리에 C# 5로 컴파일만 하고, 단위 검사는 .NET 9로 실행합니다.

```sh
dotnet build tests/dotnet/HanFlow.App.Net48 -c Release
dotnet run --project tests/dotnet/HanFlow.Tests -f net9.0 -c Release
```

단위 검사에는 통계 판별기의 C# 동치 fixture 1,135행과 held-out 게이트(한글 변환 ≥96% · 영문 보존 ≥97% · 오변환 ≤0.5%)가 들어 있습니다. 모델 표를 다시 만드는 절차는 [tests/lm/README.md](tests/lm/README.md), 데이터 출처는 [NOTICE.md](NOTICE.md)입니다.

| 파일 | 책임 |
|---|---|
| `src/Core.cs` | 두벌식 오토마타, 언어 판별(손목록·혼합어·통계 우도비), 조합 상태 |
| `src/FocusMonitor.cs` | 별도 스레드의 편집 대상·권한 확인 |
| `src/InputController.cs` | 전용 스레드의 키보드·마우스 후크, 확정 순서, 확정 직후 F2 교체, Raw Input 후크 감시, Shift+F2 요청 |
| `src/SelectionReader.cs` | 선택 영역 읽기(UI Automation, 안 되면 클립보드 대체) |
| `src/PreferenceStore.cs` | 개인 선택 사전 저장·되돌리기 |
| `src/InputDiagnostics.cs` | 원문 없는 진단 기록 |
| `src/Native.cs` · `src/App.cs` · `src/IntegrationTests.cs` | Windows API 선언 · 트레이·연습창·조합창 · 실제 입력 경로 검사 |
| `data/` | 영어 보호 어휘, 한글 음절·영문 자모 bigram 표 |
| `tests/` | 단위 검사, C# 동치 fixture, 교차 플랫폼 검사 프로젝트, 모델 생성 도구 |

설계 노트: [docs/DESIGN.md](docs/DESIGN.md) · 참고 소스와 라이선스: [docs/REFERENCES.md](docs/REFERENCES.md) · 변경 기록: [CHANGELOG.md](CHANGELOG.md) · 기여: [CONTRIBUTING.md](CONTRIBUTING.md)

## 라이선스

코드는 [MIT](LICENSE)입니다. 데이터 표의 출처와 조건은 [NOTICE.md](NOTICE.md)에 있습니다.
