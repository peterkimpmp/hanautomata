//! Bounded, insertion-only Korean spacing. Keep semantics in parity with src/Spacing.cs (Windows 0.6.0).
//! Only NFC Hangul runs are examined; other scalars, existing whitespace and UTF-16 length limits are preserved.
pub const MAX_LENGTH: usize = 256;
const NIEUN: u32 = 4;
const RIEUL: u32 = 8;
type Marks = [Option<&'static str>];

#[derive(Debug, PartialEq, Eq)]
pub struct Correction {
    pub text: String,
    /// One rule per inserted space, in output order. No input is retained.
    pub rules: Vec<&'static str>,
}

pub fn correct(text: &str) -> Correction {
    if text.encode_utf16().take(MAX_LENGTH + 1).count() > MAX_LENGTH {
        return Correction {
            text: text.to_owned(),
            rules: Vec::new(),
        };
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = Correction {
        text: String::with_capacity(text.len()),
        rules: Vec::new(),
    };
    let mut start = 0;
    while start < chars.len() {
        if !syllable(chars[start]) {
            out.text.push(chars[start]);
            start += 1;
            continue;
        }
        let end = start + chars[start..].iter().take_while(|&&c| syllable(c)).count();
        let word = &chars[start..end];
        let mut marks = vec![None; word.len()];
        for i in 0..word.len() {
            match word[i] {
                '수' => {
                    su(word, i, &mut marks);
                }
                '것' => {
                    geot(word, i, &mut marks, GEOT_TAILS, GEOT_NOUNS, None, "ko-space-geot");
                }
                '거' => {
                    geot(word, i, &mut marks, GEO_TAILS, GEO_NOUNS, Some(GEO_END_STEMS), "ko-space-geo");
                }
                '때' if i > 0 => {
                    if at(word, i + 1, "문") {
                        mark(&mut marks, i, "ko-space-ttaemun");
                    } else if final_of(word[i - 1]) == RIEUL
                        && !ends_listed(word, i, TTAE_NOUNS)
                        && (i + 1 == word.len() || tail(word, i + 1, TTAE_TAILS))
                    {
                        mark(&mut marks, i, "ko-space-ttae");
                    }
                }
                '줄' if i > 0 && final_of(word[i - 1]) == RIEUL => {
                    if let Some(k) = verb(word, i + 1, JUL_PARTICLES, JUL_VERBS) {
                        mark(&mut marks, i, "ko-space-jul");
                        mark(&mut marks, k, "ko-space-jul");
                    }
                }
                '적' if i > 0 && JEOK_STEMS.contains(word[i - 1]) => {
                    if let Some(k) = verb(word, i + 1, JEOK_PARTICLES, EXISTENCE_VERBS) {
                        mark(&mut marks, i, "ko-space-jeok");
                        mark(&mut marks, k, "ko-space-jeok");
                    }
                }
                '듯' if i > 0 && [NIEUN, RIEUL].contains(&final_of(word[i - 1])) && !ends_listed(word, i, DEUT_NOUNS) => {
                    mark(&mut marks, i, "ko-space-deut");
                }
                '야' if i > 0 && i + 1 < word.len() && final_of(word[i - 1]) == 0 && eo_vowel(word[i - 1]) => {
                    if HADA_VERBS.iter().any(|v| at(word, i + 1, v)) {
                        mark(&mut marks, i + 1, "ko-space-eoya");
                    }
                }
                '테' | '텐' if i > 0 && final_of(word[i - 1]) == RIEUL && !ends_listed(word, i, TE_NOUNS) => {
                    if if word[i] == '텐' {
                        at(word, i + 1, "데")
                    } else {
                        tail(word, i + 1, TE_TAILS)
                    } {
                        mark(&mut marks, i, "ko-space-te");
                    }
                }
                '리' if i > 0 && final_of(word[i - 1]) == RIEUL && !RI_NOUNS.contains(word[i - 1]) => {
                    for particle in RI_PARTICLES {
                        let k = i + 1 + particle.chars().count();
                        if at(word, i + 1, particle) && (existence(word, k) || at(word, k, "만무")) {
                            mark(&mut marks, i, "ko-space-ri");
                            mark(&mut marks, k, "ko-space-ri");
                            break;
                        }
                    }
                }
                '뻔' if i > 0 && final_of(word[i - 1]) == RIEUL => {
                    mark(&mut marks, i, "ko-space-ppeon");
                }
                '중' if i > 0 && matches!(word[i - 1], '는' | '던') => {
                    mark(&mut marks, i, "ko-space-jung");
                }
                '게' if i > 0 && matches!(word[i - 1], '는' | '던') => {
                    if i + 1 == word.len() {
                        mark(&mut marks, i, "ko-space-ge");
                    } else if GE_PREDICATES.iter().any(|p| at(word, i + 1, p)) {
                        mark(&mut marks, i, "ko-space-ge");
                        mark(&mut marks, i + 1, "ko-space-ge");
                    }
                }
                _ => {}
            }
        }
        for (i, &c) in word.iter().enumerate() {
            if let Some(rule) = marks[i] {
                out.text.push(' ');
                out.rules.push(rule);
            }
            out.text.push(c);
        }
        start = end;
    }
    out
}

fn su(w: &[char], i: usize, marks: &mut Marks) -> bool {
    if i == 0 {
        return at(w, 1, "있") && mark(marks, 1, "ko-space-su");
    }
    let stem = w[i - 1];
    if final_of(stem) != RIEUL || SU_NOUNS.contains(stem) {
        return false;
    }
    let j = i + 1;
    if j == w.len() {
        return SU_STEMS.contains(stem) && mark(marks, i, "ko-space-su");
    }
    if existence(w, j) {
        return mark(marks, i, "ko-space-su") & mark(marks, j, "ko-space-su");
    }
    for particle in SU_PARTICLES {
        if !at(w, j, particle) {
            continue;
        }
        let k = j + particle.chars().count();
        if k == w.len() {
            return *particle != "를" && SU_STEMS.contains(stem) && mark(marks, i, "ko-space-su");
        }
        if existence(w, k) {
            return mark(marks, i, "ko-space-su") & mark(marks, k, "ko-space-su");
        }
        return false;
    }
    false
}

fn geot(
    w: &[char],
    i: usize,
    marks: &mut Marks,
    tails: &[&str],
    nouns: &[&str],
    end_stems: Option<&str>,
    id: &'static str,
) -> bool {
    let j = i + 1;
    let mut any = false;
    if at(w, j, "같") && !at(w, j, "같이") {
        any |= mark(marks, j, id);
    } else {
        for particle in GEOT_PARTICLES {
            let k = j + particle.chars().count();
            if at(w, j, particle) && k < w.len() && (existence(w, k) || at(w, k, "아니") || (at(w, k, "같") && !at(w, k, "같이")))
            {
                any |= mark(marks, k, id);
                break;
            }
        }
    }
    if i == 0 {
        return any;
    }
    if ![NIEUN, RIEUL].contains(&final_of(w[i - 1])) || ends_listed(w, i, nouns) {
        return any;
    }
    if !any && j < w.len() && !tail(w, j, tails) {
        return false;
    }
    if !any && j == w.len() && i == 1 && end_stems.is_some_and(|s| !s.contains(w[0])) {
        return false;
    }
    mark(marks, i, id) | any
}

fn verb(w: &[char], j: usize, particles: &[&str], verbs: &[&str]) -> Option<usize> {
    if verbs.iter().any(|v| at(w, j, v)) {
        return Some(j);
    }
    for particle in particles {
        let k = j + particle.chars().count();
        if at(w, j, particle) && verbs.iter().any(|v| at(w, k, v)) {
            return Some(k);
        }
    }
    None
}
fn existence(w: &[char], k: usize) -> bool {
    at(w, k, "있") || at(w, k, "없")
}
fn tail(w: &[char], j: usize, tails: &[&str]) -> bool {
    w.get(j..)
        .is_some_and(|end| tails.iter().any(|t| end.iter().copied().eq(t.chars())))
}
fn ends_listed(w: &[char], i: usize, nouns: &[&str]) -> bool {
    nouns.iter().any(|noun| {
        let whole = noun.starts_with('^');
        let body = noun.strip_prefix('^').unwrap_or(noun);
        let n = body.chars().count();
        if n > i + 1 {
            return false;
        }
        let start = i + 1 - n;
        (!whole || start == 0) && w[start..i].iter().copied().eq(body.chars().take(n - 1))
    })
}
fn mark(marks: &mut Marks, i: usize, id: &'static str) -> bool {
    if i > 0 && i < marks.len() && marks[i].is_none() {
        marks[i] = Some(id);
    }
    true
}
fn at(w: &[char], i: usize, s: &str) -> bool {
    w.get(i..).is_some_and(|tail| {
        let mut chars = tail.iter();
        s.chars().all(|c| chars.next() == Some(&c))
    })
}
fn syllable(c: char) -> bool {
    ('가'..='힣').contains(&c)
}
fn final_of(c: char) -> u32 {
    (c as u32 - 0xac00) % 28
}
fn eo_vowel(c: char) -> bool {
    matches!((c as u32 - 0xac00) / 28 % 21, 0 | 1 | 4 | 6 | 9 | 10 | 14)
}

const SU_NOUNS: &str = "실말별술홀철골";
const SU_STEMS: &str = "할될볼올쓸잘알놀울을를럴쩔걸줄칠팔풀빌질";
const SU_PARTICLES: &[&str] = &["밖에", "조차", "마저", "도", "가", "는", "를", "만"];
const GEOT_PARTICLES: &[&str] = &["이", "은", "도", "만", "을"];
const GEOT_TAILS: &[&str] = &[
    "이",
    "을",
    "은",
    "도",
    "만",
    "과",
    "보다",
    "처럼",
    "만큼",
    "뿐",
    "들",
    "들이",
    "들을",
    "들은",
    "들도",
    "들의",
    "이다",
    "입니다",
    "이에요",
    "이야",
    "이지",
    "이지만",
    "이고",
    "이며",
    "이니",
    "이니까",
    "이라",
    "이라고",
    "이라는",
    "이라면",
    "이란",
    "이라서",
    "이었다",
    "이었고",
    "이었는데",
    "이었어",
    "이었어요",
    "이었습니다",
    "인데",
    "인지",
    "인가",
    "인가요",
    "일까",
    "일까요",
    "일지",
    "일지도",
    "으로",
    "으로는",
    "으로도",
    "에",
    "에는",
    "에도",
    "에서",
    "이나",
    "이든",
    "이든지",
    "이면",
    "까지",
    "마저",
    "조차",
    "이요",
    "이죠",
    "이네",
    "이네요",
    "이군",
    "이잖아",
    "이므로",
    "임",
    "임을",
    "의",
];
const GEO_TAILS: &[&str] = &[
    "야",
    "예요",
    "에요",
    "다",
    "지",
    "지요",
    "죠",
    "고",
    "면",
    "냐",
    "요",
    "는",
    "를",
    "도",
    "만",
    "라고",
    "랑",
    "잖아",
    "잖아요",
    "네",
    "네요",
    "임",
    "였어",
    "였다",
    "였는데",
    "였어요",
    "였지",
    "인데",
    "인지",
    "일까",
    "일걸",
    "야말로",
    "니까",
    "니깐",
    "보다",
    "처럼",
    "밖에",
    "만큼",
    "든가",
    "였음",
];
const GEOT_NOUNS: &[&str] = &["^날것", "^탈것", "^들것", "^별것", "^군것"];
const GEO_NOUNS: &[&str] = &[
    "선거", "근거", "철거", "설거", "열거", "은거", "별거", "일거", "본거", "연거", "군거", "전거", "준거", "논거",
];
const GEO_END_STEMS: &str = "할갈올볼쓸살알줄될한간온본산된쓴준난탄낸큰단긴먼찬짠";
const TTAE_NOUNS: &[&str] = &["물때"];
const TTAE_TAILS: &[&str] = &[
    "는",
    "도",
    "마다",
    "까지",
    "부터",
    "만",
    "에",
    "에는",
    "에도",
    "에만",
    "가",
    "를",
    "의",
    "면",
    "나",
    "라도",
    "처럼",
    "와",
    "랑",
    "엔",
    "보다",
    "이면",
    "마저",
    "조차",
    "이다",
    "였다",
    "야",
    "였어",
    "예요",
    "이에요",
    "인데",
    "이나",
    "이든",
    "이라도",
];
const DEUT_NOUNS: &[&str] = &["그럴듯", "반듯", "번듯", "불현듯"];
const JUL_PARTICLES: &[&str] = &["도", "은", "을", "만", "는", "이"];
const JUL_VERBS: &[&str] = &["알", "몰", "모르", "모를", "아는", "안다"];
const JEOK_STEMS: &str = "본한간온난든쓴은던운탄준낸된친산린른진긴핀";
const JEOK_PARTICLES: &[&str] = &["이", "은", "도", "는"];
const EXISTENCE_VERBS: &[&str] = &["있", "없"];
const HADA_VERBS: &[&str] = &["하", "한", "할", "합", "해", "했", "되", "된", "될", "됩", "돼", "됐"];
const TE_NOUNS: &[&str] = &["이를테"];
const TE_TAILS: &[&str] = &["니", "니까", "고", "야", "지", "지만", "다", "요", "면"];
const RI_NOUNS: &str = "멀널달실물";
const RI_PARTICLES: &[&str] = &["가", "는", "도", "만"];
const GE_PREDICATES: &[&str] = &[
    "아니", "아냐", "있", "없", "좋", "낫", "나아", "맞", "많", "중요", "문제", "어때",
];
