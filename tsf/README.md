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
#                      rustup toolchain install stable-i686-pc-windows-gnu --profile minimal   # 32비트 링커 포함
cd tsf
.\build.ps1 -Test -X86 -Package   # 엔진·키 로직 검사 → x64·x86 DLL → selftest(등록 없음) → zip
```

사용자 폴더 이름에 한글이 있으면 MinGW 링커가 경로를 못 읽을 수 있다. 그때는 `RUSTUP_HOME`·`CARGO_HOME`·`CARGO_TARGET_DIR`을 영문 경로(예: `C:\dev\…`)로 두고 빌드한다. `build.ps1`은 `CARGO_TARGET_DIR`을 따른다.

Mac에서 하는 검사: `cargo test --release`(키 로직 12개, held-out 재생 약 2만 키) · `cargo check --target x86_64-pc-windows-gnu` · `cargo clippy --target x86_64-pc-windows-gnu --all-targets`.

## S0 시험 — 사람이 Windows 화면에서

등록하면 이 사용자 계정의 입력기 목록이 바뀐다. 끝나면 `unregister`로 되돌린다.

```powershell
.\build\tsf\x64\hanautomata-ime-setup.exe status                  # 읽기만
.\build\tsf\x64\hanautomata-ime-setup.exe register                # 1 COM(HKCU) · 2 프로필 · 3 카테고리 · 4 사용자 활성화
.\build\tsf\x64\hanautomata-ime-setup.exe register --ctf-hkcu     # 2·3이 거부될 때만: HKCU CTF 경로 실험(TextVN 방식)
.\build\tsf\x64\hanautomata-ime-setup.exe unregister
```

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
