# tests/dotnet — 플랫폼 공용 컴파일 검사·단위 검사

Windows의 `build.ps1`(Framework csc) 없이도 같은 소스를 검증한다. 2026-10-09 macOS 26 · .NET SDK 9.0.318에서 실측.

| 프로젝트 | 대상 | 무엇을 하는가 | 무엇을 못 보는가 |
|---|---|---|---|
| `Hanautomata.App.Net48/` | net48 · C# 5 | `src/*.cs` 전체를 .NET Framework 4.8 참조 어셈블리(NuGet `Microsoft.NETFramework.ReferenceAssemblies`)로 컴파일. `build.ps1`과 같은 리소스 이름 | 실행 불가 — 후크·UI Automation·SendInput 동작은 Windows에서만 |
| `Hanautomata.Tests/` | net9.0 실행 + net48 컴파일 | `Core.cs`·`PreferenceStore.cs`·`tests/CoreTests.cs`를 `HANAUTOMATA_TESTS`로 빌드해 실행. 모델·fixture 리소스 포함 | Windows 파일 공유 잠금 검사 3건은 비Windows에서 건너뜀 |

```sh
# .NET SDK 9 (사용자 영역, 관리자 권한 없음)
curl -sSL https://dot.net/v1/dotnet-install.sh | bash -s -- --channel 9.0 --install-dir ~/.dotnet --no-path
export PATH="$HOME/.dotnet:$PATH"

dotnet build tests/dotnet/Hanautomata.App.Net48 -c Release
dotnet run --project tests/dotnet/Hanautomata.Tests -f net9.0 -c Release
```

- `LangVersion` 5는 Windows Framework 컴파일러(csc 4.0.30319)와 같은 문법 수준을 강제한다. 최신 C# 문법을 쓰면 여기서 먼저 걸린다.
- `JavaScriptSerializerShim.cs`는 .NET 9에 없는 `System.Web.Script.Serialization.JavaScriptSerializer`를 System.Text.Json으로 대신하는 검사용 파일이다. 앱 EXE에는 들어가지 않는다.
- `bin/`·`obj/`는 `.gitignore`로 제외한다. 첫 빌드는 NuGet에서 참조 어셈블리를 내려받는다.

## Windows 설치 후보로 넘기는 기준

위 명령은 Hanautomata 루트에서 실행한다. macOS 참조 빌드 산출물은 컴파일 확인용이며, 설치할 EXE는 Windows의 `build.ps1`로 만든다.
v0.3.0은 macOS 검사 성공 뒤 Windows에서 Raw Input 등록과 후크 수신의 호환 문제를 드러냈고, 감시 호환모드로 보완했다([검증 기록](../../docs/VERIFICATION.md)).

Windows에서도 같은 Hanautomata 루트에서 실행한다.

```powershell
.\build.ps1 -Test -Integration -OutputDirectory .\build\windows-candidate
.\verify-corpus.ps1 -OutputPrefix .\build\windows-candidate\corpus
.\install.ps1 -SourceDirectory .\build\windows-candidate
```

각 검사 실패 시 다음 단계로 진행하지 않는다. 설치 전 EXE를 백업하고, 통과한 EXE와 설치본의 SHA-256을 대조한다.
소스 커밋과 미커밋 diff, OS·SDK·컴파일러, 통과·실패·건너뜀, 사용자 표본과 설치본 관측을 함께 기록한다.
macOS의 지속 잠금 분기 건너뜀은 6개 단언에 해당한다. 검사 시나리오 수와 단언 수를 혼용하지 않는다.
후크 등록만으로 정상 판정하지 않으며 시작 후 콜백 수신·Shift+F2·강제 제거 후 복구를 확인한다.
자동 시험창 검사는 물리 키보드와 실제 앱(브라우저·채팅·편집기)의 실사용 검증을 대신하지 않는다.
