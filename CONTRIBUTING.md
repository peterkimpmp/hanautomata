# 기여 안내

## 빌드와 검사
- Windows: `.\build.ps1 -Test`(단위 검사) → `.\build.ps1 -Integration`(전용 시험창에서 실제 후크·입력 검사) → `.\install.ps1`.
- macOS·Linux·Windows(.NET SDK 9): `dotnet build tests/dotnet/Hanautomata.App.Net48 -c Release`(전체 앱 컴파일 검사) · `dotnet run --project tests/dotnet/Hanautomata.Tests -f net9.0 -c Release`(단위 검사). 자세한 설명은 `tests/dotnet/README.md`.
- PR 전에 두 검사가 모두 통과해야 합니다. CI(`.github/workflows/ci.yml`)가 같은 명령을 실행합니다.

## 코드 규칙
- C# 5 문법 · .NET Framework 4.8 · 외부 패키지 없음(앱은 Windows 기본 컴파일러 `csc 4.0.30319`로 빌드됩니다).
- 저수준 후크 콜백 안에서는 무거운 작업을 하지 않습니다. 후크 스레드 밖에서 `Sync` 잠금을 잡은 채 `SendInput`을 호출하지 않습니다.
- 판별 규칙을 바꾸면 `tests/CoreTests.cs`에 실패하는 검사를 먼저 추가하고, held-out 게이트(한글 변환 ≥96% · 영문 보존 ≥97% · 오변환 ≤0.5%)를 유지합니다.

## 모델 표 다시 만들기
`tests/lm/README.md`를 따릅니다. 공개 코퍼스(위키백과 덤프)만 사용하고, 개인 문서나 고객 문서로 만든 표는 올리지 않습니다. 표를 바꾸면 `make_fixtures.py`로 fixture를 다시 만들고 검사가 통과하는지 확인합니다.

## 이슈
재현 절차, Windows 버전, 대상 앱 이름, `--diagnose-input` 진단 파일(원문이 들어가지 않습니다)을 함께 적어 주세요.
