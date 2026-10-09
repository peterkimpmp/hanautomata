# NOTICE — 데이터 출처와 라이선스

코드(`src/`, `tests/`, 스크립트)는 [MIT](LICENSE)입니다. 아래 데이터 파일은 출처별로 조건이 다릅니다.

| 파일 | 내용 | 출처 | 조건 |
|---|---|---|---|
| `data/korean-lm.txt.gz` | 한글 음절 unigram·bigram log10 확률표(집계만 · 문장·단어 없음) | 한국어 위키백과 덤프 `kowiki-latest-pages-articles.xml.bz2`(2026-10-01판) 압축 스트림 앞 250 MB → 문서 94,575개 · 음절 1억 2,503만 | 위키백과 본문은 [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/). 이 집계표도 같은 조건으로 배포하며, 저작자 표시는 "Wikipedia contributors" |
| `data/english-lm.txt` | 영문 자모 bigram log10 확률표 | Simple English Wikipedia 덤프 `simplewiki-latest-pages-articles.xml.bz2`(2026-10-01판) 앞 100 MB → 문서 75,241개 · 토큰 1,756만 | 위와 같음(CC BY-SA 4.0) |
| `tests/corpora/heldout-korean-1000.tsv` · `heldout-english-1000.tsv` · `scorer-fixtures.tsv` | 위 두 덤프에서 뽑은 단어·빈도 표본과 판별기 기대값 | 위와 같음 | 위와 같음 |
| `data/english.txt` | 한영 판별에서 보호할 영어 어휘(손으로 관리) | 프로젝트 저자 | MIT |
| `tests/corpora/intelligence-v014.tsv` · `tests/round2-fixtures.tsv` | 손으로 쓴 개발 예제 | 프로젝트 저자 | MIT |

- 표 생성 절차: `tests/lm/README.md`(`build_lm.py` → `make_fixtures.py`). 덤프에서 평문을 뽑는 `wikitext2txt.py`는 같은 폴더에 있습니다.
- 앱 실행 파일에는 외부 라이브러리 코드를 포함하거나 링크하지 않습니다. 설계 참고 자료와 그 라이선스는 [docs/REFERENCES.md](docs/REFERENCES.md)에 있습니다.
- 개발 전용 의존성(앱 런타임 아님): .NET SDK 9(MIT) · NuGet `Microsoft.NETFramework.ReferenceAssemblies`(MIT).
