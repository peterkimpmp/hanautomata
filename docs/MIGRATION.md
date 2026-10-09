# 이전 버전에서 Hanautomata로 이전

이 문서의 `HanFlow`는 v0.3.0까지 사용한 이전 앱 식별자입니다. 현재 제품·실행 파일·설치 경로는 Hanautomata입니다. 이전 사용자의 데이터를 찾고 두 키보드 후크의 동시 실행을 막기 위해 호환 코드에는 옛 식별자가 남아 있습니다.

1. 이전 앱을 트레이에서 종료합니다. 실행 중이면 새 앱과 설치 스크립트 모두 중복 실행을 차단합니다.
2. ZIP을 푼 폴더에서 `install.ps1`을 실행합니다. 새 경로는 `%LOCALAPPDATA%\Programs\Hanautomata`입니다.
3. 새 앱을 처음 실행하면 `%LOCALAPPDATA%\HanFlow`의 파일을 `%LOCALAPPDATA%\Hanautomata`로 복사합니다. 새 데이터 폴더가 이미 있으면 기존 데이터를 덮어쓰지 않습니다. 원본은 자동 삭제하지 않습니다.
4. 기존 `HanFlow` 자동 시작 설정이 있었다면 새 이름의 설정으로 이전합니다. 자동 시작이 꺼져 있었다면 그대로 꺼집니다. 새로 켜려면 `install.ps1 -Startup` 또는 트레이 메뉴를 사용합니다.
5. 학습 단어와 설정을 확인한 뒤 옛 설치 폴더 `%LOCALAPPDATA%\Programs\HanFlow`, 바탕 화면·시작 메뉴의 `HanFlow.lnk`를 삭제할 수 있습니다. 옛 데이터 폴더는 새 파일을 대조하고 백업한 뒤 정리하세요.

설치 전 경로와 파일만 확인하려면 `install.ps1 -ValidateOnly`를 실행합니다. 이 옵션은 앱을 실행하거나 파일·바로가기·자동 시작을 변경하지 않습니다.

앱 실행·설치·데이터 이전 검사와 실제 로그오프 후 로그인 검사는 서로 다릅니다. 실제 검증 범위는 [VERIFICATION.md](VERIFICATION.md)에 기록합니다.
