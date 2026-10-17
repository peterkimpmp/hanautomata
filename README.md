# Hanautomata

**한영키를 바꾸지 않고 입력한다 — 단어 단위 한/영 자동 판별 입력 도구 (Windows·macOS) v0.6.1**

Hanautomata는 같은 물리 키로 한국어와 영어를 입력하면 **단어 단위로 언어를 판별**해 주는 로컬 입력 도구입니다. 손목록 먼저, 다음은 한글 음절 bigram 대 영문 자모 bigram의 우도비로 판별하고, 애매할 때만 작은 후보창을 보여 주며, Space로 확정하고 F2(직전 단어)·Shift+F2(선택 영역)로 고칩니다. `dkssudgktpdy`는 **안녕하세요**, `hello world`는 **hello world**가 됩니다.

입력 처리에 네트워크·클라우드 모델·외부 사전이 없습니다. 무엇을 저장하고 무엇을 하지 않는지는 [PRIVACY.md](PRIVACY.md)에 있습니다. 코드는 MIT, 모델 표는 위키백과 덤프의 집계 통계(CC BY-SA 4.0)입니다.

## 세 가지 구성

| 구성 | 방식 | 설치 권한 | 검증 상태 |
|---|---|---|---|
| **Windows 트레이 앱** (기본 권장) | 시스템 전역 입력 도우미(후크) — 기존 입력기를 바꾸지 않음 | 불필요(Zero Admin) | v0.4.0-beta 공개 검증 · 0.5.0 회귀 통과 |
| **Windows 입력기(TSF)** | Windows 정식 텍스트 서비스 — 입력기 목록에 추가 | 관리자 1회 | 시험판 — 릴리스 빌드 6앱 승인 2/6(메모장·Word, 합성 키) · [검증 원장](docs/WINDOWS-QUALITY-RESULTS.md) |
| **macOS 입력기** | InputMethodKit 입력기 | 불필요 | 자동 검사 통과 · 실물 점검표([G2](macos/COMPATIBILITY.md)) 진행 전 |

세 구성은 같은 판별 결과를 냅니다(트레이는 C# 엔진, 입력기판·macOS는 Rust 엔진 · 동등성 게이트 G1). 띄어쓰기 자동 교정은 Windows 트레이 0.6.0, macOS 0.6.1부터 지원합니다. 구성별 기능 차이는 [기능 비교](docs/FEATURES.md)에 있습니다. Windows에서 트레이 앱과 입력기(TSF)판을 같이 켜 두면 입력기판이 트레이를 감지하고 키를 그대로 통과시킵니다 — 하나만 쓰세요.

Windows·macOS 공통 확장 방향은 [제품 로드맵](docs/PRODUCT-ROADMAP.md)에 있습니다. a 한영 입력 → b 띄어쓰기 → c 맞춤법 → d 문맥 문장 작성과 개인화 추천을 구분하며, 계획과 현재 구현 상태를 따로 표시합니다.

개발 작업의 전체 순서·기술 의존성·완료 조건은 [기술 로드맵](docs/TECHNICAL-ROADMAP.md), 첫 구현 범위는 [내 표현 MVP 명세](docs/specs/PERSONAL-ASSIST-MVP.md)에 있습니다.

## 띄어쓰기 자동 교정 (Windows 트레이 · macOS)

Space로 확정할 때 의존 명사와 보조 구문을 띄어 씁니다. `gkftndlTek` + Space → **할 수 있다**, `gksmsrjt` → **하는 것**, `gkfrjdi` → **할 거야**, `godigksek` → **해야 한다**. 공백만 넣으므로 글자는 바뀌지 않습니다. 확정 직후 **F2**를 누르면 입력한 그대로 돌아가고, 그 단어는 이번 실행 동안 다시 고치지 않습니다. 트레이 메뉴 **‘띄어쓰기 자동 교정’**으로 끌 수 있습니다. 다루는 14개 규칙·예외·정밀도 근거는 [docs/SPACING.md](docs/SPACING.md)에 있습니다. Windows 11 메모장은 공백·문장부호 직후 한꺼번에 들어온 글자를 잃기 때문에, 트레이는 경계마다 앱이 처리를 마칠 때까지 기다렸다 보냅니다. 그래서 메모장에서는 교정된 단어가 0.1~0.4초에 걸쳐 나타납니다.

macOS는 입력 메뉴·설정 → 입력의 **‘자동 띄어쓰기’**로 끄고 켭니다. Space 외 Enter·Tab·방향키·포커스 이동에서도 확정할 때 같은 규칙을 적용합니다. 조합 밑줄은 그대로 두고 후보창에 결과를 미리 보여 줍니다. F2 원문 복원은 Space 확정 뒤 같은 칸·같은 커서·바뀌지 않은 원문에서 15초 안에만 가능하며 학습하지 않습니다. [macOS 빌드·설치 안내](macos/README.md), [검증 상태](macos/SPACING-VALIDATION.md)를 참고하세요.

## 내 표현 추천 개발판

신규 소스 빌드의 Windows 트레이·macOS 입력 메뉴에서 **내 표현…**을 엽니다. 직접 등록한 단어·짧은 표현을 두 글자부터 찾아 최대 세 개 추천하고, 복사 선택을 다음 순위에 반영합니다. 최근 순위 기억 되돌리기·개별 삭제·전체 삭제·추천 끄기를 제공합니다. 추천은 기본 꺼짐이고, 등록한 표현은 이 기기에 평문으로 저장합니다.

이 첫 MVP는 별도 창에서 검색하고 복사하는 방식입니다. 입력 중 자동 팝업·다음 단어 예측·문장 자동 수집·LLM 생성·TSF 진입 UI는 후속 단계입니다. 공개 릴리스에 포함됐다는 뜻은 아닙니다. [검증 범위](docs/PERSONAL-ASSIST-VALIDATION.md), [개인정보 안내](PRIVACY.md)를 확인하세요.

독립 실행: Windows `Hanautomata.exe --personal-expressions`, macOS `Hanautomata.app/Contents/MacOS/Hanautomata --personal-expressions`. 별도 입력기 등록 없이 내 표현 창만 열 수 있습니다.

## 맞춤법 검사 개발판

macOS의 `package.sh --with-proofreading` 묶음에는 검사기·사전·소스·고지가 포함됩니다. 현재 0.6.1 macOS 배포 후보는 이 방식으로 만들었습니다. 포함 여부와 실제 앱 검증 상태는 [검증 기록](macos/SPACING-VALIDATION.md)에 있습니다.

선택문을 검사하는 독립 로컬 프로세스와 제안 UI를 실험 기능으로 구현했습니다. 공개 한국어 Hunspell 사전과 작은 자체 규칙을 사용하며 자동으로 수정하지 않습니다(위 띄어쓰기 자동 교정과 별개이며, 기본 배포 zip에는 검사기 묶음이 없어 Ctrl+Shift+F7이 꺼져 있습니다). Windows 트레이는 제안 복사, TSF·macOS는 원문 재검증 후 사용자 선택 적용 경로를 제공합니다. [빌드·사용법](docs/PROOFREADING.md), [로드맵](docs/PROOFREADING-ROADMAP.md), [실측과 미통과 게이트](docs/PROOFREADING-VALIDATION.md)를 참고하세요. 기본 배포에는 자동 활성화하지 않으며 문맥·띄어쓰기 전체 교정을 보장하지 않습니다.

## 키

| 키·동작 | 기능 |
|---|---|
| Space | 후보와 공백 확정 |
| Enter | 후보 확정 후 Enter 전달. 채팅 앱에서는 전송될 수 있음 |
| `;` — 단어 조합 중 | 후보 바꾸기. 확정하면 선택을 기억 |
| `;;` — 단어 뒤 | 원래 후보로 돌아가 실제 `;` 문자 추가 |
| F2 — 단어 조합 중 | 후보 바꾸기. 확정하면 선택을 기억 |
| F2 — Space 확정 직후 | 직전 단어를 다른 후보로 교체하고 선택을 기억. 띄어쓰기가 자동 교정된 단어는 먼저 입력한 그대로 되돌림(학습 안 함). 같은 창·같은 칸에서 15초 안이며 커서 이동·클릭·Enter 뒤에는 동작하지 않음 |
| Shift+F2 — 글자를 선택한 뒤 | 선택 영역의 한글↔영타를 그 자리에서 뒤집기. UI Automation으로 읽고, 안 되면 Ctrl+C 복사 뒤 클립보드 글자를 복원. 앱의 Ctrl+Z로 되돌림 |
| Backspace | 조합 중인 마지막 키 지우기. `;` 직후에는 후보 변경 취소 |
| Esc | 미확정 조합 취소 |
| F12 | 자동 판별 일시정지·재개 |
| 입력 연결 복구 버튼 | 후크를 다시 연결하고 미확정 조합을 복사용으로 보관 |
| 창 닫기 | 트레이로 숨기기 |
| 트레이 우클릭 → 종료 | 앱과 키보드 후크 종료 |

**Zero Admin**: 관리자 권한·서비스·계정·서버·API 키가 없습니다. 현대 Windows의 .NET Framework 4.8을 사용하는 휴대용 EXE입니다.
처음 실행하면 연습창이 열립니다. 트레이의 ‘한’ 아이콘을 더블클릭해 다시 열 수 있습니다.
Windows 로그인 시 실행은 트레이에서 직접 켜는 사용자별 옵션이며 기본값은 꺼짐입니다.
v0.4.1부터 현재 사용자의 작업 스케줄러에 등록하여 로그인 15초 뒤 실행하고, 시작 실패 시 1분 간격으로 최대 3회 재시도합니다. 배터리·네트워크·유휴 상태를 실행 조건으로 요구하지 않습니다.
트레이 **자동 시작 상태 확인**에서 실제 등록 상태·최근 실행 결과·시작 기록 위치를 볼 수 있습니다. [자동 시작 점검](docs/STARTUP.md)
트레이의 **판별 민감도**(보수·표준·적극)는 보류 구간의 폭만 바꿉니다. 오변환이 거슬리면 보수, 짧은 단어가 안 바뀌면 적극을 고르세요.

## 다운로드·설치

[릴리스 페이지](https://github.com/peterkimpmp/hanautomata/releases)에서 받으세요. 받은 zip은 `SHA256SUMS.txt`(또는 `.sha256`)와 대조합니다. 빌드는 아직 코드 서명이 없어 SmartScreen·Gatekeeper가 경고할 수 있습니다 — 체크섬 대조 또는 직접 빌드를 권합니다([SECURITY.md](SECURITY.md)).

### Windows 트레이 앱 — `Hanautomata-<판>-win.zip`

Windows 10/11과 .NET Framework 4.8이 필요합니다.

1. `Get-FileHash .\Hanautomata-<판>-win.zip -Algorithm SHA256` 결과를 체크섬과 대조합니다.
2. ZIP을 풀고 `Hanautomata.exe`를 실행합니다(휴대용). 현재 사용자에게 설치하려면:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1          # %LOCALAPPDATA%\Programs\Hanautomata + 바로가기
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1 -Startup # 자동 시작까지
```

제거는 `uninstall.ps1`입니다(자동 시작 해제 → 종료 → 폴더·바로가기 제거). 학습 단어·설정까지 지우려면 `-PurgeData`를 붙입니다. 이전 HanFlow 사용자는 [데이터 이전](docs/MIGRATION.md)을 먼저 확인하세요.

소스 빌드는 Rust 1.89+와 Windows 기본 C# 컴파일러를 사용합니다. 저장소 루트에서 `.\build.ps1` 후 `.\install.ps1`입니다. Rust는 빌드할 때 필요하며 사용자는 배포된 `hanautomata-assist.exe`를 함께 설치하면 됩니다.

### Windows 입력기(TSF) — `Hanautomata-ime-<판>-win.zip` (시험판)

정식 입력기 방식이라 **관리자 1회 설치**가 필요하고, 설치 후 입력기 목록에 '한오토마타'가 추가됩니다. 설치·갱신·제거 절차와 현재 검증 범위는 [tsf/README.md](tsf/README.md)를 따르세요. 트레이 앱과 함께 켜지 않습니다.

### macOS — `Hanautomata-<판>-macos.zip`

macOS 14 이상(Apple Silicon·Intel universal).

1. `shasum -a 256 Hanautomata-<판>-macos.zip` 결과를 체크섬과 대조합니다.
2. zip을 풀어 `Hanautomata.app`을 `~/Library/Input Methods/`에 둡니다.
3. 시스템 설정 → 키보드 → 입력 소스 → 편집… → `+` → 한국어 → **한오토마타**를 추가합니다.

서명이 없는 빌드는 Gatekeeper가 차단할 수 있습니다: `xattr -dr com.apple.quarantine ~/Library/Input\ Methods/Hanautomata.app` 후 다시 추가하세요. 제거는 `macos/uninstall.sh` 또는 입력 소스 제거 후 앱 삭제입니다. 자세한 사용법·전환 키·설정은 [macos/README.md](macos/README.md)에 있습니다.

## 자동 판별의 범위

두벌식/QWERTY용입니다. 일반 영어·기술 용어는 작은 내장 목록으로 보호하고, 자주 쓰는 한국어 목록과 음절 통계로 후보를 고릅니다.
같은 키열을 한글 음절 bigram과 영문 자모 bigram으로 각각 점수화해 우도비로 판별하며, 판별이 갈리는 구간에서만 직전 단어의 언어를 참고합니다.
모델 표는 `data/korean-lm.txt.gz`(367KB)·`data/english-lm.txt`이고, 통계가 보류한 구간에서만 단어 목록 2종(`data/korean-words.txt.gz` 61,759 어절 · `data/english-words.txt.gz` 73,901 단어)을 참고합니다. 대형 언어 모델·외부 사전·네트워크를 사용하지 않습니다.

- `go`처럼 영어 단어이면서 ‘해’가 될 수 있는 키열은 영어로 보존합니다. 한글을 원하면 `;` 또는 F2를 누르세요.
- `dkssudzz` → `안녕ㅋㅋ`, `rkatkgkqslekbb!` → `감사합니다ㅠㅠ!`처럼 한글 뒤의 채팅 표현도 처리합니다.
- `doWork`·`DoWork`처럼 내장 영어 단어로 나눌 수 있는 식별자는 영문으로 보존합니다.
- `AIdml` → `AI의`, `4wja` → `4점`, `rjawmd/wjrdyd` → `검증/적용`처럼 영단어·숫자·기호와 한국어의 혼합을 처리합니다(최대 64키·8구간).
- Caps Lock으로 대문자가 입력되어도 한글 영타를 판별합니다(`EHDWKRDKSGKA` → **동작안함**). Shift로 직접 입력한 대문자 약어는 보호합니다.
- 주소·파일명·경로는 보호합니다. 이름·신조어·짧은 단어에는 오판이 있을 수 있습니다. 기본 한영 판정은 의미상 오타·맞춤법을 교정하지 않으며, 별도 맞춤법 실험 기능은 위 안내를 참고하세요.

**0.5.0 보강**([검토 근거](docs/REFERENCES.md#claude-hangul-검토--2026-10-09)): `cp`·`gh`·`mcp` 등 개발 도구·약어와 `sha256`·`k8s` 같은 등록 토큰을 영문으로 보존합니다. 독립된 `ee` → `ㄷㄷ`, `dz` → `ㅇㅋ`, `rt` → `ㄱㅅ` 등 채팅 표현도 처리합니다. 명령과 겹치는 `dd`·`tr`·`fd`는 영문이 기본이며, 한글을 원하면 F2 또는 `;`로 선택하고 확정해 기억시킬 수 있습니다. v0.4.0-beta에는 없고 0.5.0부터 들어 있습니다.

후보창의 **‘확인 필요’**는 짧거나 선택 이력이 충돌하거나 혼합어 후보 비용이 비슷한 입력, **‘음절 추정’**은 사전보다 약한 통계 근거를 뜻합니다.

트레이 앱은 정식 TSF IME를 등록하지 않습니다(그 역할은 입력기판이 합니다). **시스템 전역 입력 도우미**이며, 일반 권한의 편집 가능한 입력 칸을 Windows 접근성 API로 확인할 수 있을 때 동작합니다.
비밀번호·읽기 전용·관리자 실행 앱·입력 대상 확인 실패 상태는 기본 입력기로 통과합니다.
게임의 Raw Input, 보안 데스크톱, 터미널, 일부 웹 편집기에서는 동작하지 않을 수 있습니다.

## 로컬 데이터

세 구성 공통의 저장 항목·위치·삭제 방법은 [PRIVACY.md](PRIVACY.md)에 있습니다. 트레이 앱 기준으로: 문장 전체나 연속 키 기록을 저장하지 않으며 네트워크 통신을 하지 않습니다.
직접 후보를 바꾸고 확정한 단어에 한해 키열·한글/영문 선택·선택 가중치·앞 단어 지문을 `%LOCALAPPDATA%\Hanautomata\learned-words.json`(읽을 수 있는 JSON)에 저장합니다. 앞 단어는 원문 대신 SHA-256 지문입니다.
설정 파일에는 제외한 프로세스 이름, 민감도, 개인 학습·세미콜론 전환·띄어쓰기 자동 교정 사용 여부를 저장합니다. 띄어쓰기를 되돌린 단어 목록은 메모리에만 두고 저장하지 않습니다.
자동 판별을 위한 UI Automation 조회는 편집 가능·비밀번호·포커스 등 속성에 한정됩니다. **Shift+F2를 누르면 변환할 선택 영역의 글자를 읽습니다**(최대 4,096자). 문서 전체를 수집하거나 저장하지 않습니다.
`--diagnose-input <경로>`를 지정했을 때만 입력 원문 없이 앱·입력 방식·문자 종류·차단 사유를 순환 기록합니다.
별도로 `startup.log`에는 시작·준비·종료 시각, 프로세스 번호, 실행 방식, 오류 종류·코드를 기록합니다. 입력한 글자는 포함하지 않으며 64 KiB마다 이전 기록 1개를 남기고 교체합니다.

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
.\tests\ReleaseTests.ps1   # ZIP·체크섬·휴대용 설치 입력 검사
.\package.ps1              # deliverables/Hanautomata-<판>-win.zip + SHA256SUMS (입력기판: .\tsf\build.ps1 -Test -X86 -Package · macOS: macos/package.sh)
```

macOS·Linux(또는 Windows의 .NET SDK 9)에서는 `tests/dotnet/`로 같은 소스를 검사합니다. 전체 앱은 .NET Framework 4.8 참조 어셈블리에 C# 5로 컴파일만 하고, 단위 검사는 .NET 9로 실행합니다.

```sh
dotnet build tests/dotnet/Hanautomata.App.Net48 -c Release
dotnet run --project tests/dotnet/Hanautomata.Tests -f net9.0 -c Release
cargo test --locked --manifest-path core/tools/assist/Cargo.toml
dotnet run --project tests/dotnet/Hanautomata.Assist -c Release -- core/tools/assist/target/debug/hanautomata-assist
```

위 마지막 명령의 서비스 이름은 Windows에서 `hanautomata-assist.exe`입니다. macOS `build.sh --test`에는 같은 실제 서비스에 대한 Swift 연결·네이티브 창 회귀가 포함됩니다.

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
| `data/` | 영어 보호 어휘, 한글 음절·영문 자모 bigram 표, 보류 구간 전용 단어 목록 2종 |
| `tests/` | 단위 검사, C# 동치 fixture, 교차 플랫폼 검사 프로젝트, 모델 생성 도구 |

설계 노트: [docs/DESIGN.md](docs/DESIGN.md) · 참고 소스와 라이선스: [docs/REFERENCES.md](docs/REFERENCES.md) · 변경 기록: [CHANGELOG.md](CHANGELOG.md) · 기여: [CONTRIBUTING.md](CONTRIBUTING.md)

## 라이선스

코드는 [MIT](LICENSE)입니다. 데이터 표의 출처와 조건은 [NOTICE.md](NOTICE.md)에 있습니다. 입력기(TSF)·macOS 실행 파일에 들어가는 Rust 크레이트 등 제3자 코드와 그 라이선스는 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)에 있습니다. 세 zip 모두 이 파일들을 함께 담습니다.
