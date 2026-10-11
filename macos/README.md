# macos — 한오토마타 macOS 입력기 (P1 진행 중)

InputMethodKit 입력기다. 판별·조합·학습은 Rust 공통 엔진([core](../core/README.md))이 맡는다. 엔진은 UniFFI 계층([ffi](../ffi/src/lib.rs))을 거쳐 Swift 셸과 연결된다. Windows 트레이 앱과 같은 판별 결과를 낸다(동등성 게이트 G1).

## 빌드·설치·제거

```sh
cd macos
./build.sh --test     # Rust 엔진 검사 + Swift 키 매핑·엔진 연결 검사 → build/Hanautomata.app (이 Mac 아키텍처, ad-hoc 서명)
./install.sh          # ~/Library/Input Methods에 설치(관리자 권한 없음)하고 입력 소스로 등록
./uninstall.sh        # 입력 소스를 끄고 앱을 지운다. 학습 파일은 남긴다
```

- **처음 설치 때 한 번 직접 추가해야 한다.** 시스템 설정 → 키보드 → 입력 소스 → 편집… → + → 한국어 → 한오토마타 순서로 추가한다. macOS 26에서 `TISEnableInputSource`가 성공을 돌려줘도 실제로 켜지지 않는 것을 확인했다(2026-10-11). `install.sh`는 실제 상태를 보고, 켜지지 않았으면 이 안내를 출력한다.
- 필요한 것: Xcode Command Line Tools(swiftc), Rust(cargo). 출시용 universal 빌드와 서명·공증은 아직 없다(M-14, 결정 D15).
- 학습 파일은 `~/Library/Application Support/Hanautomata/words.json`이다. Windows 트레이 앱과 같은 문서 형식(Format 2)이다. 입력기 프로세스 하나만 쓴다(C-OPS-09).

## 지금 되는 것 (P1-a)

| 기능 | 정의서 ID | 구현 |
|---|---|---|
| 입력 소스 하나로 한·영 자동 판별 | M-01 · C-DET-01~06 | 엔진 그대로(G1) |
| 모드: 자동 판별 · 한글 고정 · 영문 고정, 일시정지 | C-MOD-01 · C-MOD-04 | 입력 메뉴 |
| ⇧Space 전환(단발 강제: 조합 중이면 그 단어만 반대로, 다시 누르면 원래대로, 조합이 없으면 다음 단어 예약) | C-MOD-11 · C-MOD-02 | Shift 뒤 다른 키 없이 Space일 때만 전환. 공백은 넣지 않는다 |
| 커서 자리 밑줄 조합, 보류(확인 필요)는 점선 밑줄 | C-CMP-01 · D12 | `setMarkedText` |
| 확정: Space(공백 포함, 문맥 유지) · Enter · Tab · 방향키·Home/End · 포커스 이동 · 클릭 | C-CMP-03 | 확정 뒤 키 전달 |
| 단축키(⌘·⌃·⌥) 직전 확정, Esc 취소, 키 단위 지우기 | C-CMP-04~06 | |
| `;`·F2 후보 바꾸기, `;;` 실제 세미콜론 | C-COR-01 | F2는 키보드 설정에 따라 fn이 필요할 수 있다 |
| 선택 기억(최대 1,024), 켬·끔, 최근 되돌리기, 초기화 | C-LEX-01 | 입력 메뉴. 학습 파일은 키 처리 밖에서 저장 |
| 민감도 보수·표준·적극 | C-DET-02 | 입력 메뉴 |
| 엔진 오류 시 입력을 잃지 않고 통과 모드로 | C-CMP-11 | 일부: Rust 패닉 → 일시정지 |
| 키 처리 지연 기록(내용 없이 1,000키마다 p50·p99) | C-OPS-05 | `log stream --predicate 'subsystem == "io.github.peterkimpmp.inputmethod.Hanautomata"'` |

## 아직 안 되는 것 (다음 묶음)

- Caps Lock 전환(C-MOD-10 · M-04, 입력 모니터링 권한 흐름 확인 필요 R11)과 오른쪽 ⌘ 단독 탭(M-03)
- 후보창(C-CMP-02). 지금은 점선 밑줄만으로 보류를 표시한다
- 직전 단어 재변환과 선택 영역 뒤집기(C-COR-02 · 03)
- 첫 실행 빠른 설정(C-OPS-07), 모드별 아이콘(C-MOD-08), 앱 제외(C-CTX-11), 설정 창(C-OPS-03)
- 호환 스위트(G2): 실물 키보드로 Safari·Chrome·VS Code·Terminal·카카오톡 등에서 입력 손실 0 확인

## 검사

- `Tests/KeyMap`: 물리 키 → 글자 대응(Windows `TryAscii`와 같은 47키), Caps Lock·Shift 조합, ⇧Space 판정(`I am` 오작동 없음·자동 반복 무시)
- `Tests/Engine`: Swift에서 UniFFI로 엔진 호출. 조합, 확정, 전환, 학습 저장과 재시작 복원, 모드, 민감도, 뒤집기
- 엔진 세션 규칙(OS 공통): `core/tests/session_tests.rs`
