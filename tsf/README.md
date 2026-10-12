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

## 아직 없는 것(P3에서)

- 후보창(C-CMP-02)과 설정 창(W-23)
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

기존 DLL은 보관하며 이미 존재하는 revision에는 다시 쓰지 않는다. receipt는 `Program Files\Hanautomata-TSF-S0\revisions\<revision>\update-result.json`이다. 등록 실패 시 이전 경로로 복구를 시도한다. 관리자 적용·실패 복구의 실제 검증은 아직 남아 있다. 사용자 목록 변경, 기본 입력기 선택, 자동 앱 종료·로그오프·재부팅은 하지 않는다. 필요한 문서를 저장하고 시험 앱을 다시 연 뒤 새 DLL 적재와 실제 입력을 확인해야 한다.
