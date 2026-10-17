---
title: "제품 이름은 다듬(Dadeum)으로 통일한다"
date: 2026-10-17
layout: a4
purpose: reference
status: accepted
---

# 제품 이름은 다듬(Dadeum)으로 통일한다

2026-10-17 사용자가 세 후보 중 **다듬(dadeum)**을 선택했다. 이전 제품명은 Hanautomata이며 같은 무료 오픈소스 프로젝트를 이어 간다.

| 용도 | 표기 |
|---|---|
| 한국어 제품명 | 다듬 |
| 영문 제품명 | Dadeum |
| 소문자 브랜드 표기 | dadeum |
| 첫 소개·정보 화면 | 다듬 (Dadeum) |
| 한국어 입력기 목록·메뉴 | 다듬 · 다듬 설정 · 다듬 정보 |
| 이전 버전 설명 | 다듬(이전 이름 Hanautomata) |

## 이름에 담는 의미

‘다듬다’의 어간에서 가져온 이름이다. 한영 입력에서 띄어쓰기·맞춤법·문장 표현까지, 사용자의 뜻을 살리면서 글을 다듬는 방향을 담는다. 개인화와 AI 기능의 구현 상태는 [제품 로드맵](PRODUCT-ROADMAP.md)과 [기술 로드맵](TECHNICAL-ROADMAP.md)을 따르며 이름 결정이 기능 완료를 뜻하지 않는다.

공식 제품명만 확정했다. 로고·슬로건·도메인·상표는 이번 결정에 포함하지 않는다. ‘내 뜻은 그대로, 표현은 정갈하게’는 작명 당시의 문구 제안이다.

## 설치 호환성을 위한 내부 이름

표시 이름을 바꾸어도 기존 사용자의 설정·학습 파일과 OS 입력기 등록은 이어져야 한다. 이번 변경은 표시명에 적용하고 아래 식별자는 유지한다.

| 대상 | 유지하는 이름·경로 |
|---|---|
| 저장소·공개 릴리스 주소 | `peterkimpmp/hanautomata` |
| Windows 실행·설치 파일 | `Hanautomata.exe`, 기존 `Hanautomata.*.exe`, ZIP 이름과 `Hanautomata.lnk` |
| macOS 앱·실행 파일 | `Hanautomata.app/Contents/MacOS/Hanautomata` |
| macOS 등록·설정 | `io.github.peterkimpmp.inputmethod.Hanautomata`와 모드·연결 ID |
| Windows 등록·자동 시작 | 기존 TSF GUID, mutex·pipe·task·registry 이름 |
| 사용자 데이터 | `%LOCALAPPDATA%\Hanautomata`, `~/Library/Application Support/Hanautomata` |
| 개발·도구 인터페이스 | `hanautomata-*` crate·helper·CLI, C# namespace·resource와 Swift/ObjC 클래스 이름 |

OS 권한 화면이나 프로세스 목록은 실행 파일명을 사용해 이전 이름을 표시할 수도 있다. 이들을 바꾸려면 별도의 데이터·등록·설치·롤백 이행 검증을 먼저 한다. 이름 변경 때문에 사용자 데이터 폴더를 새로 만들거나 기존 데이터를 이동하지 않는다.

## 적용 상태

Windows 트레이·TSF와 macOS의 사용자 표시 문자열, macOS 한국어·영어 입력기 이름, Windows 제품 메타데이터, 현행 소개·로드맵·MVP 문서에 반영한다. 과거 변경 기록·검증 원장은 당시 이름을 보존한다.

이 결정과 소스 변경은 공개 릴리스나 설치본 갱신이 아니다. 이미 배포한 ZIP과 설치된 앱은 새 빌드를 설치하기 전까지 이전 이름으로 표시될 수 있다.
