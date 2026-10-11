//! Standard modern Dubeolsik tables. No third-party implementation is bundled.

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
