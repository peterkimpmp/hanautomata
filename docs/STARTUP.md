# Windows 로그인 후 자동 시작

v0.4.1은 현재 사용자가 로그인하면 15초 뒤 Hanautomata를 실행하는 작업 스케줄러 항목을 사용합니다. Windows를 켠 뒤 로그인하기 전에는 실행하지 않습니다. 단순한 원격 데스크톱 재접속은 새 로그인이 아닐 수 있으므로 트리거 발생과 구분해야 합니다.

트레이에서 **Windows 로그인 시 실행**을 켜거나 `install.ps1 -Startup`을 실행하세요. 업데이트 전부터 켜져 있던 v0.4.0 Run 등록은 일반 설치에서도 자동으로 이전합니다. 이때 먼저 새 작업 등록을 확인하고 옛 Run 항목을 제거합니다. 작업 스케줄러에서 꺼 둔 작업은 일반 실행·업데이트가 임의로 다시 켜지 않습니다.

| 항목 | 동작 |
|---|---|
| 등록 | `Hanautomata-<현재 사용자 SID>` · 사용자별 1개 |
| 실행 계정 | 현재 사용자 · InteractiveToken · LeastPrivilege |
| 실행 명령 | 설치한 `Hanautomata.exe --background --autostart` |
| 작업 폴더 | 실행 파일이 있는 폴더 |
| 시작 조건 | 현재 사용자 로그인 15초 뒤 · 배터리에서도 허용 · 네트워크/유휴 상태 불필요 |
| 실패 복구 | 실패 코드로 종료되면 1분 간격으로 최대 3회 재시도 |
| 실행 제한 | 시간 제한 없음 · 중복 작업 실행 무시 · 앱의 세션별 mutex도 유지 |
| 종료 | 트레이 종료 또는 `--exit`는 정상 종료이며 자동 재시도를 유발하지 않음 |
| 해제 | 트레이 옵션을 끄면 해당 작업과 옛 Run 항목을 제거하며, 실행 중인 앱은 유지 |

암호를 저장하지 않고 관리자 권한·Windows 서비스·컴퓨터 전체 시작 설정을 요구하지 않습니다. 작업 기반 자동 시작은 작업 관리자 ‘시작 앱’의 Run 항목과 다릅니다. **트레이 → 자동 시작 상태 확인** 또는 Windows 작업 스케줄러에서 확인하세요. 조직 정책이나 Task Scheduler 접근 거부가 있으면 성공으로 표시하지 않고 오류를 알립니다.

## 자동 시작이 되지 않을 때

1. Hanautomata를 직접 실행하고 **자동 시작 상태 확인**을 엽니다. 등록 경로·활성 상태가 정상인지 확인합니다. 설치 경로를 옮겼으면 옵션을 다시 켜 등록을 갱신합니다.
2. `%LOCALAPPDATA%\Hanautomata\startup.log`에서 해당 로그인 시각의 `starting`과 같은 프로세스 번호의 `ready`를 찾습니다. 기록이 전혀 없으면 앱 실행 이전 단계, `failed`/`unhandled`가 있으면 앱 초기화·실행 단계 문제로 구분합니다. `ready`는 메시지 루프가 시작됐다는 의미이며 실제 키 입력 검증을 대신하지 않습니다.
3. 작업 스케줄러의 최근 실행 결과를 대조합니다. 실행 중인 작업의 `267009`(`0x41301`)는 실행 중 상태입니다. 이를 실패 코드로 해석하지 마세요.
4. 필요하면 앱을 정상 종료한 뒤 해당 작업을 **실행**하여 시작 경로를 확인합니다. 이 검사는 실제 로그인 트리거가 발생했음을 증명하지 않습니다. 실제 재부팅·로그인 후 트레이 아이콘과 시작 기록을 확인해야 마지막 검증이 됩니다.

로그는 시각·프로세스 번호·상태·실행 방식·오류 종류와 코드만 포함합니다. 문장·키열·학습 단어는 남기지 않습니다. 64 KiB에서 `startup.log.previous` 한 개로 회전합니다.

명령줄에서 등록 상태를 JSON으로 확인할 수도 있습니다. 작업 관리자 등에서 끈 설정을 읽기만 할 때는 `status`를 사용합니다.

```powershell
$exe = Join-Path $env:LOCALAPPDATA 'Programs/Hanautomata/Hanautomata.exe'
$report = Join-Path $env:TEMP 'hanautomata-startup-status.json'
$process = Start-Process -FilePath $exe -ArgumentList @('--startup', 'status', ('"' + $report + '"')) -Wait -PassThru
Get-Content -LiteralPath $report -Raw
```

`--startup enable <보고서 경로>`는 등록·복구, `disable`은 해제, `migrate`는 켜진 옛 등록만 이전합니다. 트레이와 설치 스크립트도 이 등록 코드를 사용합니다.

구현 근거: Microsoft의 [Run/RunOnce 실행 지연](https://learn.microsoft.com/en-us/windows/win32/setupapi/run-and-runonce-registry-keys), [로그인 작업 정의](https://learn.microsoft.com/en-us/windows/win32/taskschd/logon-trigger-example--xml-), [작업의 보안 컨텍스트](https://learn.microsoft.com/en-us/windows/win32/taskschd/security-contexts-for-running-tasks), [작업 등록 API](https://learn.microsoft.com/en-us/windows/win32/taskschd/taskfolder-registertask).
