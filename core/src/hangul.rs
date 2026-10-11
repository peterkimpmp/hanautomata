//! Standard modern Dubeolsik tables, Sebeolsik 390 and Final composition (C-LAY-02) and the MS Windows consonant-cluster
//! option (C-CMP-08). No third-party implementation is bundled; the Sebeolsik key tables follow the published layouts and
//! were checked against libhangul's tables.

use crate::unicode::lower_char;

const INITIALS: [char; 19] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];
const VOWELS: [char; 21] = [
    'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ', 'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ', 'ㅣ',
];
const FINALS: [char; 28] = [
    '\0', 'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ', 'ㅀ', 'ㅁ', 'ㅂ', 'ㅄ', 'ㅅ', 'ㅆ',
    'ㅇ', 'ㅈ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];
const INITIAL_KEYS: [&str; 19] = [
    "r", "R", "s", "e", "E", "f", "a", "q", "Q", "t", "T", "d", "w", "W", "c", "z", "x", "v", "g",
];
const VOWEL_KEYS: [&str; 21] = [
    "k", "o", "i", "O", "j", "p", "u", "P", "h", "hk", "ho", "hl", "y", "n", "nj", "np", "nl", "b", "m", "ml", "l",
];
const FINAL_KEYS: [&str; 28] = [
    "", "r", "R", "rt", "s", "sw", "sg", "e", "f", "fr", "fa", "fq", "ft", "fx", "fv", "fg", "a", "q", "qt", "t", "T", "d", "w",
    "c", "z", "x", "v", "g",
];
/// Finals that split into two: (final, first part, second part as a final index).
const LIQUID_FINALS: [i32; 7] = [1, 16, 17, 19, 25, 26, 27];

fn index_of(table: &[char], c: char) -> i32 {
    table.iter().position(|&x| x == c).map_or(-1, |i| i as i32)
}

fn map(c: char) -> char {
    match c {
        'R' => 'ㄲ',
        'E' => 'ㄸ',
        'Q' => 'ㅃ',
        'T' => 'ㅆ',
        'W' => 'ㅉ',
        'O' => 'ㅒ',
        'P' => 'ㅖ',
        _ => {
            const KEYS: &str = "rsefaqtdwczxvgkoiujphynbml";
            const JAMO: &str = "ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎㅏㅐㅑㅕㅓㅔㅗㅛㅜㅠㅡㅣ";
            let lower = lower_char(c);
            match KEYS.chars().position(|k| k == lower) {
                Some(i) => JAMO.chars().nth(i).unwrap_or(c),
                None => c,
            }
        }
    }
}

fn join_vowel(a: i32, b: i32) -> i32 {
    match (a, b) {
        (8, 0) => 9,
        (8, 1) => 10,
        (8, 20) => 11,
        (13, 4) => 14,
        (13, 5) => 15,
        (13, 20) => 16,
        (18, 20) => 19,
        _ => -1,
    }
}

fn join_final(a: i32, b: i32) -> i32 {
    if a == 1 && b == 19 {
        return 3;
    }
    if a == 4 {
        if b == 22 {
            return 5;
        }
        if b == 27 {
            return 6;
        }
    }
    if a == 8 {
        if let Some(i) = LIQUID_FINALS.iter().position(|&x| x == b) {
            return 9 + i as i32;
        }
    }
    if a == 17 && b == 19 {
        return 18;
    }
    -1
}

fn split_final(t: i32) -> (i32, i32) {
    match t {
        3 => (1, 19),
        5 => (4, 22),
        6 => (4, 27),
        9..=15 => (8, LIQUID_FINALS[(t - 9) as usize]),
        18 => (17, 19),
        _ => (0, t),
    }
}

fn flush(output: &mut String, l: i32, v: i32, t: i32) {
    if l >= 0 && v >= 0 {
        output.push(char::from_u32((0xac00 + (l * 21 + v) * 28 + t) as u32).unwrap());
    } else if l >= 0 {
        output.push(INITIALS[l as usize]);
    } else if v >= 0 {
        output.push(VOWELS[v as usize]);
    }
}

/// Composes Dubeolsik key letters into Hangul. Characters that are not keys pass through and end the syllable.
pub fn compose(keys: &str) -> String {
    let mut output = String::with_capacity(keys.len());
    let (mut l, mut v, mut t) = (-1i32, -1i32, 0i32);
    for key in keys.chars() {
        let j = map(key);
        let next_l = index_of(&INITIALS, j);
        let next_v = index_of(&VOWELS, j);
        if next_l >= 0 {
            let next_t = index_of(&FINALS, j);
            if l >= 0 && v >= 0 && next_t > 0 {
                if t == 0 {
                    t = next_t;
                    continue;
                }
                let joined = join_final(t, next_t);
                if joined >= 0 {
                    t = joined;
                    continue;
                }
            }
            flush(&mut output, l, v, t);
            l = next_l;
            v = -1;
            t = 0;
        } else if next_v >= 0 {
            if v < 0 {
                v = next_v;
            } else if t > 0 {
                let (first, second) = split_final(t);
                flush(&mut output, l, v, first);
                l = index_of(&INITIALS, FINALS[second as usize]);
                v = next_v;
                t = 0;
            } else {
                let joined = join_vowel(v, next_v);
                if joined >= 0 {
                    v = joined;
                } else {
                    flush(&mut output, l, v, t);
                    l = -1;
                    v = next_v;
                    t = 0;
                }
            }
        } else {
            flush(&mut output, l, v, t);
            l = -1;
            v = -1;
            t = 0;
            output.push(key);
        }
    }
    flush(&mut output, l, v, t);
    output
}

/// Decomposes Hangul (precomposed, conjoining or compatibility jamo) back into Dubeolsik keys. Other text passes through.
pub fn to_keys(text: &str) -> String {
    let mut output = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        let u = c as u32;
        if (0xac00..=0xd7a3).contains(&u) {
            let cp = (u - 0xac00) as usize;
            output.push_str(INITIAL_KEYS[cp / 588]);
            output.push_str(VOWEL_KEYS[cp % 588 / 28]);
            output.push_str(FINAL_KEYS[cp % 28]);
        }
        // Modern conjoining jamo (Unicode NFD). Do not normalize unrelated scripts.
        else if (0x1100..=0x1112).contains(&u) {
            output.push_str(INITIAL_KEYS[(u - 0x1100) as usize]);
        } else if (0x1161..=0x1175).contains(&u) {
            output.push_str(VOWEL_KEYS[(u - 0x1161) as usize]);
        } else if (0x11a8..=0x11c2).contains(&u) {
            output.push_str(FINAL_KEYS[(u - 0x11a7) as usize]);
        } else {
            let (l, v, t) = (index_of(&INITIALS, c), index_of(&VOWELS, c), index_of(&FINALS, c));
            if l >= 0 {
                output.push_str(INITIAL_KEYS[l as usize]);
            } else if v >= 0 {
                output.push_str(VOWEL_KEYS[v as usize]);
            } else if t > 0 {
                output.push_str(FINAL_KEYS[t as usize]);
            } else {
                output.push(c);
            }
        }
    }
    output
}

/// Explicit layout flip of text that was already typed: every Latin-letter run is composed into Hangul and every Hangul run
/// is decomposed into QWERTY keys. Digits, punctuation, line breaks and other scripts stay. `flip(flip(x)) == x` for
/// complete syllables and plain letters; shifted jamo keep their Shift letters.
pub fn flip(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut run = String::new();
    let mut kind = 0u8;
    for c in text.chars() {
        let next = if c.is_ascii_alphabetic() {
            1
        } else if is_hangul_char(c) {
            2
        } else {
            0
        };
        if next != kind {
            flush_run(&mut output, &mut run, kind);
            kind = next;
        }
        if next == 0 {
            output.push(c);
        } else {
            run.push(c);
        }
    }
    flush_run(&mut output, &mut run, kind);
    output
}

fn flush_run(output: &mut String, run: &mut String, kind: u8) {
    if run.is_empty() {
        return;
    }
    output.push_str(&if kind == 1 { compose(run) } else { to_keys(run) });
    run.clear();
}

fn is_hangul_char(c: char) -> bool {
    let u = c as u32;
    (0xac00..=0xd7a3).contains(&u) || (0x3131..=0x318e).contains(&u) || (0x1100..=0x11ff).contains(&u)
}

/// Korean keyboard layouts (C-LAY-01·02). The automatic decision reads Dubeolsik keys, so Sebeolsik layouts compose in the
/// fixed Korean mode (Sebeolsik detection is C-DET-10, later).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Dubeolsik,
    Sebeolsik390,
    SebeolsikFinal,
}

// Printable ASCII '!'..='~' → conjoining jamo (choseong U+1100…, jungseong U+1161…, jongseong U+11A8…) or a symbol.
#[rustfmt::skip]
const SEBEOL_390: [char; 94] = [
    '\u{11bd}', '"', '#', '$', '%', '&', '\u{1110}', '(', ')', '*', '+', ',',
    '-', '.', '\u{1169}', '\u{110f}', '\u{11c2}', '\u{11bb}', '\u{11b8}', '\u{116d}', '\u{1172}', '\u{1163}', '\u{1168}', '\u{1174}',
    '\u{116e}', ':', '\u{1107}', '2', '=', '3', '?', '@', '\u{11ae}', '!', '\u{11b1}', '\u{11b0}',
    '\u{11bf}', '\u{11a9}', '/', '\'', '8', '4', '5', '6', '1', '0', '9', '>',
    '\u{11c1}', '\u{1164}', '\u{11ad}', ';', '7', '\u{11b6}', '\u{11c0}', '\u{11b9}', '<', '\u{11be}', '[', '\\',
    ']', '^', '_', '`', '\u{11bc}', '\u{116e}', '\u{1166}', '\u{1175}', '\u{1167}', '\u{1161}', '\u{1173}', '\u{1102}',
    '\u{1106}', '\u{110b}', '\u{1100}', '\u{110c}', '\u{1112}', '\u{1109}', '\u{110e}', '\u{1111}', '\u{11ba}', '\u{1162}', '\u{11ab}', '\u{1165}',
    '\u{1103}', '\u{1169}', '\u{11af}', '\u{11a8}', '\u{1105}', '\u{11b7}', '{', '|', '}', '~',
];
#[rustfmt::skip]
const SEBEOL_FINAL: [char; 94] = [
    '\u{11a9}', '\u{b7}', '\u{11bd}', '\u{11b5}', '\u{11b4}', '\u{201c}', '\u{1110}', '\'', '~', '\u{201d}', '+', ',',
    ')', '.', '\u{1169}', '\u{110f}', '\u{11c2}', '\u{11bb}', '\u{11b8}', '\u{116d}', '\u{1172}', '\u{1163}', '\u{1168}', '\u{1174}',
    '\u{116e}', '4', '\u{1107}', ',', '>', '.', '!', '\u{11b0}', '\u{11ae}', '?', '\u{11bf}', '\u{11b2}',
    '\u{11ac}', '\u{11b1}', '\u{1164}', '0', '7', '1', '2', '3', '"', '-', '8', '9',
    '\u{11c1}', '\u{11b6}', '\u{11ad}', '\u{11b3}', '6', '\u{11aa}', '\u{11c0}', '\u{11b9}', '5', '\u{11be}', '(', ':',
    '<', '=', ';', '*', '\u{11bc}', '\u{116e}', '\u{1166}', '\u{1175}', '\u{1167}', '\u{1161}', '\u{1173}', '\u{1102}',
    '\u{1106}', '\u{110b}', '\u{1100}', '\u{110c}', '\u{1112}', '\u{1109}', '\u{110e}', '\u{1111}', '\u{11ba}', '\u{1162}', '\u{11ab}', '\u{1165}',
    '\u{1103}', '\u{1169}', '\u{11af}', '\u{11a8}', '\u{1105}', '\u{11b7}', '%', '\\', '/', '\u{203b}',
];

/// Double consonants, compound vowels and compound finals typed as two keys in Sebeolsik.
fn sebeol_combine(a: u32, b: u32) -> Option<u32> {
    Some(match (a, b) {
        (0x1100, 0x1100) => 0x1101,
        (0x1103, 0x1103) => 0x1104,
        (0x1107, 0x1107) => 0x1108,
        (0x1109, 0x1109) => 0x110a,
        (0x110c, 0x110c) => 0x110d,
        (0x1169, 0x1161) => 0x116a,
        (0x1169, 0x1162) => 0x116b,
        (0x1169, 0x1175) => 0x116c,
        (0x116e, 0x1165) => 0x116f,
        (0x116e, 0x1166) => 0x1170,
        (0x116e, 0x1175) => 0x1171,
        (0x1173, 0x1175) => 0x1174,
        (0x11a8, 0x11a8) => 0x11a9,
        (0x11a8, 0x11ba) => 0x11aa,
        (0x11ab, 0x11bd) => 0x11ac,
        (0x11ab, 0x11c2) => 0x11ad,
        (0x11af, 0x11a8) => 0x11b0,
        (0x11af, 0x11b7) => 0x11b1,
        (0x11af, 0x11b8) => 0x11b2,
        (0x11af, 0x11ba) => 0x11b3,
        (0x11af, 0x11c0) => 0x11b4,
        (0x11af, 0x11c1) => 0x11b5,
        (0x11af, 0x11c2) => 0x11b6,
        (0x11b8, 0x11ba) => 0x11b9,
        (0x11ba, 0x11ba) => 0x11bb,
        _ => return None,
    })
}

/// The jamo or symbol a Sebeolsik key types (other characters pass).
pub fn sebeol_jamo(layout: Layout, key: char) -> char {
    let table = match layout {
        Layout::Sebeolsik390 => &SEBEOL_390,
        _ => &SEBEOL_FINAL,
    };
    let u = key as u32;
    if (0x21..=0x7e).contains(&u) {
        table[(u - 0x21) as usize]
    } else {
        key
    }
}

#[derive(Default)]
struct Syllable {
    cho: Option<u32>,
    jung: Option<u32>,
    jong: Option<u32>,
}

impl Syllable {
    fn is_empty(&self) -> bool {
        self.cho.is_none() && self.jung.is_none() && self.jong.is_none()
    }

    /// A complete syllable as one precomposed character; a partial one as compatibility jamo.
    fn flush(&mut self, output: &mut String) {
        match (self.cho, self.jung) {
            (Some(l), Some(v)) => {
                let t = self.jong.map_or(0, |t| t - 0x11a7);
                output.push(char::from_u32(0xac00 + ((l - 0x1100) * 21 + (v - 0x1161)) * 28 + t).unwrap());
            }
            _ => {
                if let Some(l) = self.cho {
                    output.push(INITIALS[(l - 0x1100) as usize]);
                }
                if let Some(v) = self.jung {
                    output.push(VOWELS[(v - 0x1161) as usize]);
                }
                if let Some(t) = self.jong {
                    output.push(FINALS[(t - 0x11a7) as usize]);
                }
            }
        }
        *self = Syllable::default();
    }
}

/// Composes Sebeolsik keys: every key types a choseong, jungseong or jongseong, so a final never moves to the next
/// syllable. Doubled initials, compound vowels and compound finals combine as two keys.
fn compose_sebeol(layout: Layout, keys: &str) -> String {
    let mut output = String::with_capacity(keys.len());
    let mut s = Syllable::default();
    for key in keys.chars() {
        let j = sebeol_jamo(layout, key) as u32;
        if (0x1100..=0x1112).contains(&j) {
            if s.jung.is_none() && s.jong.is_none() {
                if let Some(double) = s.cho.and_then(|l| sebeol_combine(l, j)) {
                    s.cho = Some(double);
                    continue;
                }
                if s.cho.is_none() {
                    s.cho = Some(j);
                    continue;
                }
            }
            s.flush(&mut output);
            s.cho = Some(j);
        } else if (0x1161..=0x1175).contains(&j) {
            if s.jong.is_none() {
                match s.jung {
                    None => {
                        s.jung = Some(j);
                        continue;
                    }
                    Some(v) => {
                        if let Some(compound) = sebeol_combine(v, j) {
                            s.jung = Some(compound);
                            continue;
                        }
                    }
                }
            }
            s.flush(&mut output);
            s.jung = Some(j);
        } else if (0x11a8..=0x11c2).contains(&j) {
            if s.cho.is_some() && s.jung.is_some() {
                match s.jong {
                    None => {
                        s.jong = Some(j);
                        continue;
                    }
                    Some(t) => {
                        if let Some(compound) = sebeol_combine(t, j) {
                            s.jong = Some(compound);
                            continue;
                        }
                    }
                }
            }
            s.flush(&mut output);
            s.jong = Some(j);
        } else {
            s.flush(&mut output);
            output.push(char::from_u32(j).unwrap_or(key));
        }
    }
    if !s.is_empty() {
        s.flush(&mut output);
    }
    output
}

/// Composes keys in the given layout.
pub fn compose_layout(layout: Layout, keys: &str) -> String {
    match layout {
        Layout::Dubeolsik => compose(keys),
        _ => compose_sebeol(layout, keys),
    }
}

/// MS Windows style (C-CMP-08): two consonants typed without a vowel join into a compound final jamo (ㄱ+ㅅ → ㄳ), as the
/// Windows Korean IME shows them. Only standalone compatibility jamo change; syllables and other text stay.
pub fn ms_clusters(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() {
            let joined = match (chars[i], chars[i + 1]) {
                ('ㄱ', 'ㅅ') => Some('ㄳ'),
                ('ㄴ', 'ㅈ') => Some('ㄵ'),
                ('ㄴ', 'ㅎ') => Some('ㄶ'),
                ('ㄹ', 'ㄱ') => Some('ㄺ'),
                ('ㄹ', 'ㅁ') => Some('ㄻ'),
                ('ㄹ', 'ㅂ') => Some('ㄼ'),
                ('ㄹ', 'ㅅ') => Some('ㄽ'),
                ('ㄹ', 'ㅌ') => Some('ㄾ'),
                ('ㄹ', 'ㅍ') => Some('ㄿ'),
                ('ㄹ', 'ㅎ') => Some('ㅀ'),
                ('ㅂ', 'ㅅ') => Some('ㅄ'),
                _ => None,
            };
            if let Some(joined) = joined {
                output.push(joined);
                i += 2;
                continue;
            }
        }
        output.push(chars[i]);
        i += 1;
    }
    output
}

/// The language of the text before the caret (C-DET-08): `Some(true)` when Hangul syllables clearly dominate the letters,
/// `Some(false)` when Latin letters do, `None` when it is mixed or too short. Only the last 40 characters count.
pub fn dominant_language(text: &str) -> Option<bool> {
    let tail: Vec<char> = text.chars().rev().take(40).collect();
    let hangul = tail.iter().filter(|&&c| ('\u{ac00}'..='\u{d7a3}').contains(&c)).count();
    let latin = tail.iter().filter(|c| c.is_ascii_alphabetic()).count();
    let total = (hangul + latin) as f64;
    if hangul >= 4 && hangul as f64 >= 0.7 * total {
        Some(true)
    } else if latin >= 6 && latin as f64 >= 0.7 * total {
        Some(false)
    } else {
        None
    }
}
