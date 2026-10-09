# tests/lm — 판별 모델 도구 (개발용 Python)

제품 EXE는 C#만 사용합니다. 이 폴더는 `src/Core.cs`의 `LanguageScorer`를 만든 참조 구현과 평가 도구입니다.

| 파일 | 역할 |
|---|---|
| `hangul.py` | `Core.cs` `Hangul.Compose`/`ToKeys`의 충실한 이식(현대 한글 11,172자 왕복 검사 포함) |
| `wikitext2txt.py` | 위키백과 pages-articles XML 덤프(부분 스트림도 가능)를 평문 청크로 변환 |
| `build_lm.py` | 평문 폴더에서 한글 음절 unigram·bigram, 영문 자모 bigram **집계표**와 held-out 단어 표본 생성. 문장·단어 원문은 저장하지 않음 |
| `make_fixtures.py` | 배포 표로 `tests/corpora/`의 held-out 1,000×2와 C# 동치 fixture를 다시 생성 |
| `wordlist_from_stream.py` | 덤프의 **다음 슬라이스**(모델 표에 쓴 문서는 건너뜀)에서 어절·단어 빈도 목록 생성 → `data/*-words.txt.gz` |
| `detector.py` | 우도비 판별기 참조 구현. C# `LanguageScorer`와 동일한 식 |
| `eval.py` | held-out 4,000+4,000으로 임계값 스윕과 v0.1.8 규칙 비교 |

```sh
# 1) 공개 코퍼스 (예: 한국어 위키백과 앞 250 MB · Simple English 위키백과 앞 100 MB)
curl -r 0-262143999 https://dumps.wikimedia.org/kowiki/latest/kowiki-latest-pages-articles.xml.bz2 -o ko.bz2
curl -r 0-104857599 https://dumps.wikimedia.org/simplewiki/latest/simplewiki-latest-pages-articles.xml.bz2 -o en.bz2
bzip2 -dc ko.bz2 2>/dev/null | python3 wikitext2txt.py corpus/ko ko
bzip2 -dc en.bz2 2>/dev/null | python3 wikitext2txt.py corpus/en en

# 2) 집계표
python3 build_lm.py --repo corpus --roots ko --out out-ko
python3 build_lm.py --repo corpus --roots en --out out-en
gzip -9 -c out-ko/korean-syllable-lm-min30.txt > ../../data/korean-lm.txt.gz
cp out-en/english-letter-lm.txt ../../data/english-lm.txt

# 3) C# 검사용 표본·fixture, 평가
python3 make_fixtures.py --ko-out out-ko --en-out out-en
python3 eval.py

# 4) 보류 구간 전용 단어 목록 — 모델 표에 쓴 문서 수(kept)만큼 건너뛴 다음 슬라이스에서
curl -r 0-524287999 .../kowiki-latest-pages-articles.xml.bz2 -o ko500.bz2   # 앞 250MB는 모델 표에 쓴 구간
bzip2 -dc ko500.bz2 2>/dev/null | python3 wordlist_from_stream.py --lang ko --skip-pages 94575 --out ko-words.tsv
bzip2 -dc en200.bz2 2>/dev/null | python3 wordlist_from_stream.py --lang en --skip-pages 75241 --out en-words.tsv
# 한국어: count>=50 · 2음절 이상 · Core.cs 손목록 제외 → gzip → ../../data/korean-words.txt.gz
# 영어:  count>=5 · 3글자 이상 · [a-z]+ → gzip → ../../data/english-words.txt.gz
```

- 배포 표(v0.4.0)는 2026-10-01판 덤프로 만들었습니다. 출처와 조건은 [NOTICE.md](../../NOTICE.md)에 있습니다.
- `eval.py`의 사용자 표본 구간은 `tests/corpora/user-prompts.txt`·`user-corrections.tsv`가 있을 때만 실행됩니다(개인 개발 표본 · 배포하지 않음).
- `scorer-fixtures.tsv`는 C# 검사(`build.ps1 -Test` · `tests/dotnet/Hanautomata.Tests`)가 텍스트·판정을 정확히, 우도비를 0.05 안에서 재현해야 합니다.
