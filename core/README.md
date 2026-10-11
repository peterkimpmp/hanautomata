# core — Hanautomata 공통 엔진 (Rust)

Windows 트레이 앱의 판별 엔진 `src/Core.cs`를 Rust crate `hanautomata-core`로 옮긴 것이다. macOS 입력기(InputMethodKit)와 Windows 입력기(TSF)가 같은 엔진을 쓰도록 하는 첫 단계(P0)다. 이 crate는 파일·OS API를 쓰지 않는다. 키 수신, 화면 표시, 학습 파일 저장은 각 OS 셸이 맡는다.

## 동등성 게이트 G1 — C#판과 100% 일치

2026-10-11 macOS 26 · .NET 9.0.20 · Rust 1.96.0에서 실측했다.

| 검사 | 기준값 | 비교 횟수 | 불일치 |
|---|---|---:|---:|
| 한글 조합·분해·뒤집기 | 73,911행: 완성형 11,172자 왕복·NFD·자모 영역·무작위 | 73,911 | 0 |
| 어휘 목록 내용·순서 | 6종과 모델·단어 목록 크기 | 9 | 0 |
| 통계 판별(우도비 비트 단위) | 8,319행 × 프리셋 3 × 앞 단어 3 | 16,638 | 0 |
| 단어 판별(`Decide`) | 204,423행 × 프리셋 3 × 앞 단어 3 | 1,839,807 | 0 |
| 상태 기계(학습·되돌리기·조합·세미콜론·캐시) | 시나리오 409개 | 83,645 | 0 |
| 민감도 프리셋 | held-out 4,000+4,000 × 3 | 3 | 0 |

- **CoreTests 이식**(`tests/core_tests.rs`): 단언 26,282개 통과. 파일 저장 검사는 셸 단계로 넘겼다. held-out 한국어 982/1000(오변환 3), 영어 997/1000(오변환 0)으로 C#판과 같다.
- **결함 주입**: 엔진에 결함 8종을 넣었고 모두 검출했다. 혼합어 여유폭, 문맥 가중치, 후보 동점 처리, 어휘 누락, 학습 감쇠, 세미콜론 복원, 모음 결합, 유니코드 소문자 표를 대상으로 했다. 검출 후 원본으로 되돌렸다.
- **판별 입력 출처**: C# 검사 6개 파일의 모든 문자열 리터럴(Caps Lock 변형 포함), corpora와 fixture, 손목록·조사·활용형·개발 도구 토큰, 공개 단어 목록 135,660개를 넣었다. 생성 입력으로 혼합어 20,000개, 무작위 키 24,000개, 64키 경계 400개를 넣었다. 주소·유니코드 함정과 홈 경로 별칭 5개 프로필도 포함했다.

민감도 실측값(C# = Rust):

| 프리셋 | 한국어 정답 변환 | 한국어 오변환 | 영어 유지 | 영어 오변환 |
|---|---:|---:|---:|---:|
| conservative | 3,928 | 6 | 3,981 | 3 |
| balanced | 3,944 | 6 | 3,980 | 5 |
| aggressive | 3,956 | 6 | 3,982 | 5 |

`LanguageScorer` 주석의 수치(balanced 3,927 · 오변환 2)와 다르다. 주석은 2026-10-09 측정값이고 지표 정의가 다를 수 있다. 위 표는 현재 C# 엔진 실측이다.

## 실행

```sh
cd core
cargo test --release            # 전체 기준값이 target/golden에 있으면 그것을, 없으면 tests/golden-lite를 쓴다
./golden.sh                     # C# 기준값(전체·축약)을 다시 만들고 검사한다 · .NET SDK 9 필요
cargo run --release --example latency -- ../tests/corpora/intelligence-v014.tsv
```

- 전체 기준값(약 6.7MB, `target/golden/`)은 저장소에 넣지 않는다. 축약본 `tests/golden-lite/`(40행마다 1행 + 손으로 쓴 행 전부, 약 0.6MB)만 커밋한다. 축약본은 .NET 없이 `cargo test`로 돌린다.
- `src/Core.cs`나 `data/`를 바꾸면 `./golden.sh`로 두 기준값을 다시 만든다. `src/unicode_tables.rs`는 추출기의 `--unicode` 출력물이다(직접 고치지 않는다).
- 민감도 검사는 `tests/lm/out-*` 표본(커밋하지 않음, `build_lm.py`로 생성)이 있는 기계에서만 돈다. 없으면 건너뛴다고 출력한다.

## 구조

| Rust | C# (`src/Core.cs`) | 비고 |
|---|---|---|
| `hangul` | `Hangul` | `compose`·`to_keys`·`flip` |
| `prefs` | `WordPreferences`·`LearnedWord` | `context_key`(SHA-256) · 스레드 안전 |
| `scorer` | `LanguageScorer`·`LanguageVerdict` | 모델은 `data/`를 `include_bytes!`로 내장, 프로세스당 1회 적재 |
| `detector` | `Detector`(사전 트라이·혼합어 DAG·`BestTwo`) | 어휘는 `lists.rs`(Core.cs에서 그대로 추출) |
| `composition` | `Composition` | 같은 캐시 규칙(학습 버전 기준) |
| `focus` | `FocusPolicy` | |
| `session` | `InputController`의 키 규칙 | 입력기 공통 세션: 모드 3종·단발 전환(22 §5.1a)·확정 경로·128자 상한. Space 확정 직후 F2 재변환(`SwapLastCommit`, 셸이 커서 앞 글자를 확인한 뒤 `reconvert`). 조합이 없을 때 Space 밖의 키·포커스 이동은 앞 단어 문맥을 끊는다(`PreservePending`). C#에 없는 확장 `Composition::force`·`clear_choice` |
| `store` | `PreferenceStore` 형식 | 학습 파일(C-OPS-09): Format 1·2 읽기, Format 2 쓰기, 512KB 상한, 원자 저장(같은 폴더 임시 파일 → 이름 바꾸기), `<파일>.lock` 잠금. 읽지 못한 파일은 바뀔 때까지 덮어쓰지 않음 |
| `unicode` + `unicode_tables` | .NET `char` 의미 | 생성된 BMP 표 |
| `hangul::Layout`·`compose_layout` | — | 세벌식 390·최종 조합(C-LAY-02). 받침이 다음 음절로 넘어가지 않고, 겹자음·겹모음·겹받침은 두 키로 만든다. 자판 표는 공개 배열을 따르고 libhangul 표와 대조했다(코드는 가져오지 않음) |
| `hangul::ms_clusters` | — | Windows식 겹자음(C-CMP-08): 모음 없이 ㄱ+ㅅ → ㄳ |
| `hangul::dominant_language` | — | 커서 앞 40자의 우세 언어(C-DET-08) |
| `exceptions` | — | 안 바꿀 단어·항상 바꿀 단어 목록(C-LEX-02). 판정과 개인 학습보다 앞서고, 지금 단어의 F2·`;`·전환 키만 이것보다 앞선다 |
| `hanja` | — | 한글 한 음절의 한자(C-LEX-06). `data/hanja.txt.gz`(Unicode Unihan `kHangul` · `core/tools/build_hanja.py`): 상용(KS X 1001, 교육용 기초 한자 먼저) 5,156자 · 확장 3,761자 |
| `bundle` | — | 내보내기·가져오기 묶음(C-LEX-04): 학습 파일 문서 + 예외 목록 + 셸 설정 객체. 가져오면 기존 것 뒤에 더하고 겹치면 가져온 쪽이 이긴다 |

## 입력기 확장 — C#판에 없는 기능 (모두 기본 꺼짐)

입력기에서만 쓰는 정책은 C# 판정 뒤에 얹고 기본값은 끈다. 그래서 G1 동등성은 그대로다.

| 기능 | 위치 | 동작 |
|---|---|---|
| 전환 의미 3종 (C-MOD-02) | `Session::set_switch_meaning` | 단발(기본) · 순환(자동→한→영) · 토글(한↔영, 자동에서는 화면 언어의 반대로) |
| 보호 강화 (앱 프로필, C-MOD-06) | `Composition::set_strict` | 통계 추정(`Pattern`)만으로 나온 한글은 원문으로 두고 확인 필요로 표시. 목록·개인 학습·혼합어 판정은 그대로 |
| 예외 목록 (C-LEX-02) | `Session::exceptions` | 위 표 |
| 학습 일시 중지 (C-LEX-09) | `WordPreferences::set_recording` | 기억한 선택은 계속 쓰고 새로 기록하지 않는다. 셸은 보안 입력 칸에서도 끈다(C-CTX-01) |
| 단어 하나 지우기 (C-LEX-03) | `WordPreferences::forget` | 전역·문맥 항목을 모두 지우고 되돌리기 기록을 비운다 |
| 음절 단위 지우기 (C-CMP-06) | `Session::set_delete_syllable` | 한글로 보이는 단어는 마지막 음절의 키를 모두 지운다 |
| 커서 앞 문맥 (C-DET-08) | `Session::set_caret_text` | 앞 단어 문맥이 없을 때만 앞 단어 언어처럼 근소 차이를 가른다 |
| 엔진 오류 대비 (C-CMP-11) | `Session::raw_pending` | FFI가 키마다 지금까지의 원문을 보관하고, 패닉이면 그대로 확정한 뒤 키를 넘긴다 |

C-DET-08 held-out 측정(`ime_features_tests`): 앞 문장이 한글이면 한글 단어 놓침이 1,000개 중 15 → 6, 영어 문장 앞에서는 그대로 15. 영어 단어는 영어 문장 앞에서 오변환 0 → 0, 한글 문장 속에서는 0 → 4. 혼용 문장에서 오변환이 늘어 정의서 인수 기준(증가 0)을 넘지 못하므로 셸 기본값은 끔이다.

## 동등성 계약과 의도적 차이

- C# 코드가 UTF-16 코드 단위에 의존하는 곳은 Rust도 UTF-16으로 처리한다. 길이·인덱스·통계 점수가 이에 해당한다. 그래서 이모지 같은 보충 평면 문자가 섞여도 결과가 같다.
- `char.IsLetter/IsUpper/IsLower`, `ToLowerInvariant`, `OrdinalIgnoreCase`의 문자 의미는 .NET 9(ICU)에서 뽑은 표를 쓴다. Windows 배포 런타임 net48(NLS)의 문자 표와 같은지는 **검증하지 않았다**. 차이는 비ASCII 대소문자 경계에서만 생길 수 있다.
- C#이 예외를 던지는 입력은 Rust가 처리를 이어 간다. 이 차이는 기준값 밖이다.
  - `keys`의 UTF-16 길이가 `raw`와 다르면 C#은 `ArgumentOutOfRangeException`을 던진다. Rust는 `raw`를 키로 쓴다.
  - 영문 모델 줄이 공백으로 시작하면 C#은 예외를 던진다. Rust는 그 줄을 건너뛴다. 배포 데이터에는 이런 줄이 없다.
- `PreferenceStore`의 파일 형식은 `store`로 옮겼다. 저장 담당은 macOS에서 입력기 프로세스 하나다. Windows(앱마다 DLL)의 다중 프로세스 설계는 S0 뒤에 정한다. 공유 잠금 아래 다시 읽고 합치는 방식이 후보다.
- `string.ToLowerInvariant`가 보충 평면 문자를 바꾸는 경로(`IsEnglish`·프리셋 이름)는 결과에 영향이 없다고 판단해 BMP 표만 쓴다 [추론].

## 지연(엔진만, 측정 범위)

`IntelligenceProbe`와 같은 절차로 쟀다. fixture 36행을 50회씩 반복하고, 입력 중 모든 접두어와 64키·128키 최악 경우를 더해 2,342회다. 같은 Mac에서 3회 측정했다.

| 엔진 | 평균 | p50 | p95 | p99 | 최대 |
|---|---:|---:|---:|---:|---:|
| Rust (release) | 0.0036ms | 0.0028ms | 0.0112ms | 0.021ms | 0.035ms |
| C# .NET 9 | 0.0100ms | 0.0030ms | 0.0140ms | 0.022ms | 5.2ms |

계획의 제안 기준(키 처리 p99 ≤ 5ms, C-OPS-05) 안이다. 이 값은 엔진 호출만 잰 것이다. 키 이벤트 수신부터 화면 표시까지의 종단 지연은 셸 단계에서 따로 잰다.
