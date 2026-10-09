# Hanautomata 참고 소스와 라이선스

2026-10-09 · 제품 EXE에는 아래 외부 라이브러리의 코드를 포함하거나 링크하지 않습니다.
표준 두벌식 표와 C# 구현은 Hanautomata 소스에 있으며, 공개 구현은 설계 비교와 독립 검사에 활용했습니다.

| 저장소 | 확인한 commit | 원본 라이선스 | 활용 |
|---|---|---|---|
| [libhangul](https://github.com/libhangul/libhangul) | `5094421d9586294b2aad09924b9a54e2e6060f06` | [LGPL-2.1-or-later 소스 헤더](https://github.com/libhangul/libhangul/blob/5094421d9586294b2aad09924b9a54e2e6060f06/hangul/hangulinputcontext.c) | 상태 수명·reset·입력 이력 계약 참고 |
| [es-hangul](https://github.com/toss/es-hangul) | `0ecc91ee2e3f130824d726595ad246c859f88656` | [MIT](https://github.com/toss/es-hangul/blob/0ecc91ee2e3f130824d726595ad246c859f88656/LICENSE) | 키 단위 삭제 기대 결과 비교 |
| [Hangul.js](https://github.com/e-/Hangul.js) | `325f7237a030741a10cbcbc2d9b8fac770d9e592` | [MIT](https://github.com/e-/Hangul.js/blob/325f7237a030741a10cbcbc2d9b8fac770d9e592/LICENSE) | 개발용 차등 검사 |
| [inko](https://github.com/738/inko) | `6bcb04b075f282b945dfcdecdf943d21fbc8a5ab` | [MIT](https://github.com/738/inko/blob/6bcb04b075f282b945dfcdecdf943d21fbc8a5ab/LICENSE) | 개발용 양방향 변환 차등 검사 |
| [kokey](https://github.com/devslab-kr/kokey) | `a03439d51e053b3a3efbf73b74352fead4932cc8` | [MIT](https://github.com/devslab-kr/kokey/blob/a03439d51e053b3a3efbf73b74352fead4932cc8/LICENSE) | 조합·추론 분리와 오탐 사례 검토 |

`verify-references.ps1`만 외부 네트워크를 사용합니다. 고정된 두 MIT 소스를 개발 캐시에 내려받고
파일 해시 확인 뒤 Node에서 실행합니다. npm 설치나 외부 설치 스크립트 실행은 없습니다.
원본 소스·캐시를 별도 재배포할 때는 연결된 원본 라이선스와 저작권 고지도 함께 보존해야 합니다.

Hangul.js의 독립 겹자음 결합 정책은 Hanautomata와 다릅니다. 차등 검사는 완성 음절을 변경하지 않고,
독립 겹자음 `ㄳ` 등을 `ㄱㅅ`로 풀어 이 정책 차이만 정렬합니다. inko는 기본 옵션 그대로 비교합니다.
자세한 입력 집합은 `tests/reference-differential.cjs`에 있습니다.

v0.1.5에는 다음 설계 참고 자료를 추가했습니다.

| 저장소 | 확인한 commit | 원본 라이선스 | 활용 |
|---|---|---|---|
| [Kiwi](https://github.com/bab2min/Kiwi) | `280302af5ff9e739bec1ce56d6df5f9436ac41d4` | [Apache-2.0](https://github.com/bab2min/Kiwi/blob/280302af5ff9e739bec1ce56d6df5f9436ac41d4/LICENSE) | 형태 후보와 경로 점수 분리·top-N 평가 |
| [Rime](https://github.com/rime/librime) | `7bc3fb0005a03aff1c086611e4d52bb7f6444fdd` | [BSD-3-Clause](https://github.com/rime/librime/blob/7bc3fb0005a03aff1c086611e4d52bb7f6444fdd/LICENSE) | 확정·삭제·되돌림의 학습 경계 |
| [Mozc](https://github.com/google/mozc) | `921b8cc99904c8d31b771e395513da0a5d55182a` | [소스 BSD 3-Clause 및 사전별 고지](https://github.com/google/mozc/blob/921b8cc99904c8d31b771e395513da0a5d55182a/LICENSE) | 후보 비용 차이·이력 관리 |

논문·표준은 인용 영향력과 한글 직접 관련성을 구분해 읽었으며, 전체 모델 이식이 아니라 설계 원칙만 채택했습니다.

v0.2.0에는 다음 자료를 추가했습니다. 별·포크·인용 수는 2026-10-08 스냅샷입니다. 코드를 포함하거나 링크한 외부 라이브러리는 없습니다.

| 자료 | 활용 |
|---|---|
| Microsoft Learn — [LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc) · [KEYBDINPUT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-keybdinput) · [Text Service Registration](https://learn.microsoft.com/en-us/windows/win32/tsf/text-service-registration) | 전용 후크 스레드 설계, `VK_PACKET` 경로 유지, TSF 전환 조건 |
| [Chromium `ax_platform_node_win.cc`](https://raw.githubusercontent.com/chromium/chromium/main/ui/accessibility/platform/ax_platform_node_win.cc) | 웹 편집기의 UIA ControlType·IsReadOnly 출처 → 편집 칸 판정 완화 |
| [rime/weasel](https://github.com/rime/weasel) · [NavilIME](https://github.com/navilera/NavilIME) · [Virtual-KR-IME](https://github.com/JeongminYoon/Virtual-KR-IME) | Windows TSF 프런트엔드, 한글 TSF IME 설치 절차, 후크+SendInput 동종 설계의 비교(코드 미사용) |
| Cavnar & Trenkle 1994 · Kukich 1992 · Brill & Moore 2000 · Lui & Baldwin 2012 · Golding & Roth 1999 | 문자 n-gram 언어 판별과 noisy-channel 틀 → `LanguageScorer` |
| [KR0123403B1](https://patents.google.com/patent/KR0123403B1/ko)(1994) · [KR100213910B1](https://patents.google.com/patent/KR100213910B1/ko)(삼성전자 1997) — 만료 특허 · [한컴오피스 글자판 자동 변경](https://help.hancom.com/hoffice/multi/ko_kr/hwp/tools/autoengkor/autoengkor(active).htm) | 조합규칙·사전·Space 판정·수동 전환의 선행 설계 |

통계 모델 `data/korean-lm.txt.gz`·`data/english-lm.txt`는 한국어 위키백과·Simple English 위키백과 덤프의 음절·자모 bigram 집계이며 문장·단어 목록을 담지 않습니다(출처·조건은 [NOTICE.md](../NOTICE.md), 절차는 `tests/lm/README.md`).

v0.3.0에는 다음 자료를 추가했습니다. 별 수는 2026-10-09 스냅샷입니다. 앱에 포함하거나 링크한 외부 코드는 없습니다.

| 자료 | 활용 |
|---|---|
| [RuSwitcher](https://github.com/rashn/RuSwitcher)(MIT) · [keyboop](https://github.com/iffuno/keyboop)(MIT) · [PolterType](https://github.com/Just-Code-NET/PolterType)(MIT) · [dotSwitcher](https://github.com/kurumpa/dotSwitcher)(GPL-3) · [xneur](https://manpages.debian.org/bookworm/xneur/xneur.1.en.html) | 레이아웃 전환기 계열의 조작·예외·되돌림 설계 비교(코드 미사용) |
| [Using Raw Input](https://learn.microsoft.com/en-us/windows/win32/inputdev/using-raw-input) · [The Old New Thing 2016-09-26](https://devblogs.microsoft.com/oldnewthing/20160926-00/?p=94385) | 메시지 전용 창 + `RIDEV_INPUTSINK` 감시, 합성 입력의 null 장치 핸들 |
| [Wikimedia TJones — 잘못된 키보드 감지 설계](https://www.mediawiki.org/wiki/User:TJones_(WMF)/Notes/Implementation_Design_and_Parameter_Optimization_for_Wrong_Keyboard_Detection_and_Suggestion) · [US6326953B1](https://patents.google.com/patent/US6326953B1/en) · [US11880511B1](https://patents.google.com/patent/US11880511B1/en) | 정밀도 우선 규칙 · 사후 변환 · 온라인 임계값 조정의 근거 |

개발 전용 의존성(앱 런타임 아님): .NET 9 SDK(MIT · `~/.dotnet` 사용자 영역) · NuGet `Microsoft.NETFramework.ReferenceAssemblies` 1.0.3(MIT · net48 참조 어셈블리). `tests/dotnet/`의 `JavaScriptSerializerShim.cs`는 검사 실행용 대체물이며 배포 EXE에 들어가지 않습니다.
