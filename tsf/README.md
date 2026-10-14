# Hanautomata TSF 입력기 — S0 빌드

Windows 정식 입력기(계획 21의 P3)로 가기 전, **등록 권한 시험(S0)**을 하려고 만든 첫 빌드다. TSF 텍스트 서비스 DLL에 macOS판과 같은 Rust 공통 엔진을 연결했다. 등록 4단계를 하나씩 보고하는 설치 도구도 함께 만든다.

| 파일 | 내용 |
|---|---|
| `hanautomata_tsf.dll` | TSF 텍스트 서비스. 키 이벤트 → 엔진 → 조합 표시·확정(편집 세션), 표시 속성(밑줄), 시스템 입력 표시기 가/A |
| `hanautomata-ime-setup.exe` | `register` · `unregister` · `status` · `selftest` — 단계별 결과 출력 |
| `src/shell.rs` | TSF와 떼어 낸 키 로직. Mac에서도 검사가 돈다(재생 검사 포함) |

## 빌드

Visual Studio와 관리자 권한 없이 Rust GNU 툴체인으로 빌드한다.

```powershell
# Rust(사용자 영역): rustup-init.exe -y --default-host x86_64-pc-windows-gnu --profile minimal
# 32비트 DLL을 만들 때: rustup target add i686-pc-windows-gnu
#                      rustup toolchain install stable-i686-pc-windows-gnu --profile minimal --force-non-host   # 32비트 링커 포함
cd tsf
.\build.ps1 -Test -X86 -Package   # 엔진·키 로직 검사 → x64·x86 DLL → selftest(등록 없음) → zip
```

사용자 폴더 이름에 한글이 있으면 `CARGO_TARGET_DIR`을 영문 경로(예: `C:\dev\target`)로 둔다. `build.ps1`은 그 값을 따르고, `TEMP`에 한글이 있으면 빌드 동안만 그 안의 `tmp`를 쓴다(MinGW ld가 한글 경로의 응답 파일을 못 읽는다). `windows` crate는 0.59로 고정했다: 0.60부터는 raw-dylib라 Rust GNU 툴체인에 없는 MinGW dlltool·as가 필요하고, llvm-dlltool로 대신 만든 검사 실행 파일은 실행 중 죽었다(2026-10-12 Windows 365 실측 · `Cargo.toml` 주석).

Mac에서 하는 검사: `cargo test --release`(키 로직 12개, held-out 재생 약 2만 키) · `cargo check --target x86_64-pc-windows-gnu` · `cargo clippy --target x86_64-pc-windows-gnu --all-targets`.

## S0 시험 — 사람이 Windows 화면에서

등록하면 이 사용자 계정의 입력기 목록이 바뀐다. 끝나면 `unregister`로 되돌린다.

```powershell
.\build\tsf\x64\hanautomata-ime-setup.exe status                  # 읽기만(GetProfile · EnumProfiles · 언어 목록 포함)
.\build\tsf\x64\hanautomata-ime-setup.exe register                # 1 COM(HKCU) · 2 프로필 · 3 카테고리 · 4 사용자 활성화
.\build\tsf\x64\hanautomata-ime-setup.exe register --ctf-hkcu     # 2·3이 거부될 때만: HKCU CTF 경로 실험(TextVN 방식)
#   TextVN 순서: 4 뒤에 HKCU 프로필을 다시 쓰고 카테고리를 쓴 뒤, 5 언어 목록(User Profile\ko)에 값을 더한다. unregister가 5도 지운다
.\build\tsf\x64\hanautomata-ime-setup.exe unregister
.\build\tsf\x64\hanautomata-ime-setup.exe probe-hkcu [--enable]   # HKCU CTF 키가 남는지 10초 지켜본 뒤 스스로 지움(S0 실험)
.\build\tsf\x64\hanautomata-ime-setup.exe active                  # 지금 세션의 입력기를 토큰으로 출력(되돌릴 때 씀)
.\build\tsf\x64\hanautomata-ime-setup.exe activate [--to TOKEN]   # 세션 입력기를 우리 것으로(또는 토큰으로 되돌림) — Win+Space와 같은 전환
.\tsf\s0-live.ps1 [-Registered]                                    # S0-4 대리: (등록 →) 전환 → 시험 창에 합성 키 → 읽기 → 되돌림(·해제)
```

`s0-live.ps1`의 합성 키(SendKeys) 통과는 물리 키보드 결과가 아니다. Windows가 입력기를 적재해 돌리는지만 답한다. 시험 창이 활성 창일 때만 키를 보낸다. 원격 셸에서 띄운 창은 활성 창이 되지 못하므로 화면 앞의 PowerShell 창에서 돌린다.

2026-10-12 Windows 11 26H2(표준 사용자)에서 `register --ctf-hkcu`는 언어 목록까지 들어갔지만 전환(`ActivateProfile`)이 `0x80004005`로 거부됐다. 같은 셸에서 영어 자판·MS IME 전환은 ok였다. 기존 `register`는 관리자 실행에서도 COM을 HKCU에 쓰므로, 다른 관리자 계정으로 설치하는 D2에는 아래의 별도 절차를 쓴다.

### D2: 관리자 1회 설치 + 실제 사용자 활성화

진행 근거와 미검증 항목: [D2 검증 기록](D2-VALIDATION.md). 설치 성공과 6개 앱 동작 승인은 구분한다.

S0 시험용이며 정식 설치 프로그램이 아니다. x64·x86 DLL을 `Program Files\Hanautomata-TSF-S0`에 보관하고 각 비트수의 HKLM COM과 TSF 프로필·카테고리를 등록한다. 프로필은 Microsoft SampleIME처럼 기본 사용 가능 상태로 등록하되, 관리자 계정의 언어 목록이나 선택된 입력기는 바꾸지 않는다. 사용할 계정의 목록 추가·선택은 일반 창에서 별도로 한다. 기존 등록이나 설치 폴더가 있으면 교체를 거부한다.

```powershell
# 빌드 후 일반 PowerShell: 설치할 4개 파일의 해시 목록
$hashes = [ordered]@{}
foreach ($arch in @('x64','x86')) {
    foreach ($name in @('hanautomata-ime-setup.exe','hanautomata_tsf.dll')) {
        $hashes["$arch/$name"] = (Get-FileHash "./build/tsf/$arch/$name" -Algorithm SHA256).Hash
    }
}
$hashes | ConvertTo-Json | Set-Content ./build/tsf/sha256.json -Encoding ASCII

# 관리자 PowerShell (표준 계정 PC이면 관리자의 인증 필요)
.\tsf\install-machine.ps1

# 실제 사용할 계정의 일반 PowerShell
$setup = "$env:ProgramFiles\Hanautomata-TSF-S0\x64\hanautomata-ime-setup.exe"
& $setup enable-user
if ($LASTEXITCODE -ne 0) { throw '현재 사용자 활성화 실패: 위 FAIL 확인' }
& $setup selftest-registered
if ($LASTEXITCODE -ne 0) { throw '현재 사용자 준비 상태 실패: 앱 시험 중단' }
& $setup status
# 기존 트레이 앱을 종료한 후 입력기를 선택하여 6개 앱을 시험
# 종료 시 기존 입력기로 전환한 뒤:
& $setup disable-user

# 관리자 PowerShell에서 시스템 등록을 제거 (진단 파일은 보관)
& "$env:ProgramFiles\Hanautomata-TSF-S0\install-machine.ps1" -Action Unregister
```

설치 결과는 설치 폴더의 `install-result.json`이다. 중간 등록 실패 시 시도한 비트수의 등록을 되돌리고 파일·오류 기록을 남긴다. 관리자 명령의 권한 거부 경로는 일반 PowerShell의 `.\tsf\test-machine-registration.ps1`로 검사한다. `enable-user`/`disable-user`는 현재 사용자에게만 적용되므로 관리자 창에서 실행하면 거부한다. 이때 쓰는 공식 API는 [EnableLanguageProfile](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofiles-enablelanguageprofile)과 [RegisterProfile](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofilemgr-registerprofile)이다.

`enable-user`는 API 호출 뒤 현재 사용자의 활성 상태와 언어 목록을 다시 읽어 확인한다. `selftest-registered`도 COM 적재·사용자 활성화·언어 목록의 세 조건을 모두 요구한다. 비활성(`false`) 또는 목록 누락이면 `FAIL`과 종료 코드 1이다. 이전 도구는 `IsEnabledLanguageProfile=false`도 조회 성공이라는 뜻의 `ok`로 출력했으므로 그 결과를 준비 완료로 해석하면 안 된다. `status`는 존재 여부를 보여 주는 진단 명령이며, 종료 코드만으로 준비 상태를 판단하지 않는다. 이 검사들은 실제 메뉴 표시·선택·앱 입력을 보증하지 않는다.

- 각 단계의 ok/FAIL과 HRESULT를 기록한다. 이것이 결정 D2(사용자별 등록 / 관리자 1회 설치)의 근거다.
- 입력기를 고른 뒤 메모장·Chrome·Edge·Word·시작 검색·관리자 권한 메모장에서 다음을 확인한다.
  - 조합 밑줄, Space·Enter·Tab·방향키·클릭 확정
  - ⇧Space · Caps Lock 짧게/길게 · 한/영 · 오른쪽 Alt
  - F2 · Shift+F2 · F12 일시정지
  - 작업 표시줄 가/A
- 기록 파일은 `%LOCALAPPDATA%\Hanautomata\ime-s0.log`이다. 프로세스·활성화·권한 상승·AppContainer·트레이 앱 여부만 남고, 친 글자는 남지 않는다.

## 개발 중인 설정 화면

`codex/windows-quality` 개발 소스에는 입력 후크를 실행하지 않는 전용 설정 앱이 있습니다. 저장 형식은 `%LOCALAPPDATA%\Hanautomata\tsf-settings.json`이며 기존 트레이 앱의 `settings.json`과 분리됩니다. 아직 S0 설치·갱신 묶음과 Windows 입력기 메뉴에는 연결하지 않았습니다.

```powershell
# 저장소 루트에서 .NET Framework 설정 앱과 기존 트레이 앱 빌드
.\build.ps1
.\build\Hanautomata.ImeOptions.exe
# 동일한 편집기만 열기 — 후크·자동 시작 등록은 실행하지 않음
.\build\Hanautomata.exe --ime-settings
# 격리 파일을 쓰는 저장 회귀
.\tests\ImeSettingsTests.ps1
# 실제 사용자 설정과 분리된 UI 시험 실행 파일도 빌드 (자동 실행하지 않음)
.\tests\ImeSettingsTests.ps1 -BuildPreview
```

입력 모드·자판·판별 감도·전환/교정 키·세션 학습·제외 앱을 설정합니다. 저장 뒤 다른 입력기로 바꿨다가 한오토마타를 다시 선택해야 새 세션에 반영됩니다. 앱이 기존 서비스를 유지하면 앱도 다시 열어야 합니다. 키 콜백에서는 설정 파일을 읽지 않습니다. 설정이 손상되거나 읽을 수 없으면 해당 활성화에서는 키를 그대로 넘깁니다. 없는 파일만 호환 기본값을 사용합니다.

저장 직전 읽은 버전이 달라졌으면 충돌을 알리고, 정상 교체 때는 직전 파일 1개를 `.previous`로 보관합니다(다음 일반 저장 때 교체). 손상 파일을 읽기만 해서는 수정하지 않습니다. `파일에서 복원…`에서 `.previous`, 남은 `.tmp`, 설정 JSON을 선택하면 옵션을 미리 볼 수 있습니다. 확인 후 `저장`해야 적용되며, 이 복원 저장은 선택한 보관 파일을 유지하고 현재 파일을 별도의 `.before-restore.<ID>.json`에 백업합니다. 복원 미리보기 후 다른 창에서 저장했으면 복원도 충돌로 거부합니다. `기본값`을 선택하고 저장하면 모든 옵션을 초기화합니다. 용량 초과·권한 오류는 파일을 직접 수정한 뒤 다시 읽어야 합니다. 저장이 시작됐다는 `.lock` 또는 백업만 남고 본 파일이 없으면 첫 실행 기본값으로 간주하지 않습니다. 복구 전에는 TSF가 입력을 그대로 통과시키며, 중단된 교체의 임시 파일도 보존합니다.

제외 앱 이름은 Windows의 대소문자 비교와 NFC 정규화를 함께 사용합니다. Windows 파일 시스템이 구분할 수 있는 NFC/NFD 이름도 **제외 정책에서는 같은 이름으로 처리**합니다. 악센트와 호환 문자(예: 전각 영문)는 별개입니다. 실행 파일 경로가 길면 버퍼를 늘려 읽고, 8.3 축약 이름은 확장합니다. 이름을 끝까지 확인하지 못했는데 제외 목록이 있으면 키를 통과시킵니다. 프로세스 이름은 한 번만 확인해 키마다 경로 조회를 반복하지 않습니다.

다른 편집 창의 일반 저장은 `.previous`를 교체할 수 있습니다. 복원 미리보기 중 충돌하면 화면의 옵션을 유지하고, 최신 파일을 별도로 백업한 뒤 미리보기로 바꿀지 확인합니다. 동의 후에도 다시 충돌하면 저장을 거부합니다. 복원 성공 안내에는 복원 전 백업 위치가 표시됩니다. 복원 백업을 자동 삭제하지 않으므로 필요 없어진 백업은 사용자가 정리합니다. 파일 이름으로 제외하므로 이름을 바꾼 사본·별도 하드 링크를 같은 앱으로 식별하지는 않습니다. `~`를 포함한 이름의 확장에 실패하면 이 역시 알 수 없는 이름으로 처리합니다.

AppContainer·패키지 앱·관리자 앱에서 편집기와 동일한 사용자 설정 경로를 읽는지는 아직 검증하지 않았습니다. 이 접근성과 실제 UI 검증 전에는 새 설정 지원 DLL을 설치 후보로 승인하지 않습니다. 제외 앱은 TSF를 적재한 프로세스 이름에 해당하므로 콘솔에서는 명령 프로그램 이름과 입력을 담당하는 호스트 이름이 다를 수 있습니다.

설정의 새 학습은 현재 세션에만 유지됩니다. 아래 영속 학습·후보 UI는 아직 미구현이며 이 화면이 기능 완성을 뜻하지 않습니다. 실제 UI 저장·재열기와 DPI 검수도 남아 있습니다.

## 아직 없는 것(P3에서)

- 후보창(C-CMP-02), 설정 창의 설치·메뉴 연결 및 실제 화면 검증(W-23)
- 학습 저장: 앱마다 DLL이 하나씩 올라오는 구조라 S0 뒤 저장 방식을 정한다(`core/src/store.rs`)
- ARM64 DLL과 코드 서명(D15)
- 첫 실행 설정(C-OPS-07)과 후크판 학습 이전(W-17 이전 부분)

트레이 앱(후크판)이 실행 중이면 입력기는 키를 그대로 넘긴다(W-17 상호 배제). 비밀번호 칸(InputScope)에서도 넘긴다.

### D2 런타임 수정본 적용

2026-10-12 실제 Word 시험에서 서비스가 추가 활성화될 때 첫 글자가 사라지는 결함을 확인했다. Windows는 한 프로세스에 여러 TSF 서비스를 만들 수 있으므로 조합 엔진을 서비스별로 분리했다. 회귀 검사와 실제 앱 재시험의 범위는 [D2 검증 기록](D2-VALIDATION.md)에 남긴다. S0의 학습 파일 저장은 아직 없으며, 같은 파일을 읽는 것과 조합 상태를 공유하는 것은 구분한다.

기존 설치를 갱신할 때 `install-machine.ps1`은 덮어쓰기를 거부한다. 검증한 빌드와 `sha256.json`을 준비한 뒤 아래 전용 절차를 쓴다.

```powershell
# 일반 창: 해시·현재 등록 경로만 검사. 파일이나 설정을 바꾸지 않는다.
.\tsf\update-machine.ps1 -Revision d2-session-20261012 -CheckOnly
# 관리자 창: 새 revision 폴더에 배치하고 등록 경로를 갱신한다.
.\tsf\update-machine.ps1 -Revision d2-session-20261012
```

기존 DLL은 보관하며 이미 존재하는 revision에는 다시 쓰지 않는다. receipt는 `Program Files\Hanautomata-TSF-S0\revisions\<revision>\update-result.json`이다. 2026-10-12 `d2-session-20261012`의 관리자 적용, 설치 해시와 COM 경로, 새 Word·메모장의 DLL 적재를 확인했다. 이 revision을 적용한 PC에서는 같은 명령을 반복하지 않는다. 등록 실패 시 이전 경로로 복구를 시도하지만, 실패 복구의 실제 시험은 아직 남아 있다. 사용자 목록 변경, 기본 입력기 선택, 자동 앱 종료·로그오프·재부팅은 하지 않는다. 실제 앱 승인 범위는 [D2 검증 기록](D2-VALIDATION.md)을 따른다.

후속 시험에서는 메모장의 F2 재변환이 동작하고 Word에서는 반응이 없었다. F2를 TSF 보존 단축키로도 등록해 같은 재변환 명령에 연결했다. 변환할 직전 단어가 없거나 공존·일시정지·기능 꺼짐 상태이면 앱으로 넘기며, 등록 실패는 로그에 남기고 일반 키 경로를 유지한다. 70개 검사와 양쪽 비트수 빌드·selftest 후, 2026-10-13 사용자가 `d2-f2-20261012`를 적용했다. 새 Word의 DLL 적재와 F2 보존 콜백 도달을 확인했지만, 합성 키·실제 키보드 모두 `handled=false`이고 재변환되지 않았다. **Word 문제는 미해결이며 앱 전체 승인은 0/6**이다. 같은 적용 묶음 `build/tsf/d2-f2-fix-20261012`를 반복 실행하지 않는다.

원인 추적용 `d2-f2-trace-20261013`은 입력 동작 대신 진단 정보를 추가한다. 일반 Word에서 `%LOCALAPPDATA%\Hanautomata\tsf-diagnostics.enabled`가 있을 때만 서비스별 모드·조합·재변환 가능 여부와 이벤트 순서를 기록한다. 로그의 `tid`와 `service`로 확정과 F2의 수신 대상을 구분하며, 입력 문자·키 코드·문서 내용은 기록하지 않는다. 표식은 서비스 활성화 때 읽는다. 상세 결과와 적용 상태는 [D2 검증 기록](D2-VALIDATION.md)을 따른다.

2026-10-13 추적판의 관리자 적용과 새 Word 적재를 확인했다. 최초 Word의 모드 알림 뒤 ⇧Space는 한글 고정 모드였고, 입력기를 다시 선택해 Auto가 된 뒤에는 F2가 처리됐다. Word의 **한/영 자동 고침 OFF에서 F2 왕복 성공, 원래 ON 복원 후 다시 실패**를 관찰했다. 설정은 ON으로 복원했으며 이를 해결 완료로 계산하지 않는다. `hello` 확정 결과만으로 Auto 모드를 판정하지 않는다.

후속 후보 `build/tsf/d2-word-correction-20261013`은 F2·Shift+F2의 확정 텍스트 교체에 `TF_ST_CORRECTION`을 사용한다. 70개 검사와 x64·x86 빌드·selftest 후 **2026-10-13 05:58 KST 관리자 적용을 확인했지만 Word 재시험은 실패**했다. 새 DLL을 적재한 Word의 Auto 모드에서 문장 중간 `한글`을 F2로 바꾸면 화면은 그대로였다. Word의 `자동 고침 취소`를 실행하자 `gksrmf`가 나타났다. 교정 플래그만으로 Word의 후속 자동 고침을 막지 못했다. 이번 시험에서 Word 옵션은 변경하지 않았다. 같은 revision을 다시 설치하지 않으며, 설치 API·처리 콜백 성공만으로 앱 승인을 올리지 않는다. 현재 전체 앱 승인은 0/6이다.
