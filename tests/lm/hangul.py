"""Faithful Python port of Hanautomata src/Core.cs `Hangul` (Compose / ToKeys).

Used only to build fixtures and evaluate the detector offline; the product stays C#.
"""
INITIALS = "ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ"
VOWELS = "ㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ"
FINALS = "\0ㄱㄲㄳㄴㄵㄶㄷㄹㄺㄻㄼㄽㄾㄿㅀㅁㅂㅄㅅㅆㅇㅈㅊㅋㅌㅍㅎ"
INITIAL_KEYS = "r R s e E f a q Q t T d w W c z x v g".split(' ')
VOWEL_KEYS = "k o i O j p u P h hk ho hl y n nj np nl b m ml l".split(' ')
FINAL_KEYS = "|r|R|rt|s|sw|sg|e|f|fr|fa|fq|ft|fx|fv|fg|a|q|qt|t|T|d|w|c|z|x|v|g".split('|')

_SHIFT = {'R': 'ㄲ', 'E': 'ㄸ', 'Q': 'ㅃ', 'T': 'ㅆ', 'W': 'ㅉ', 'O': 'ㅒ', 'P': 'ㅖ'}
_KEYS = "rsefaqtdwczxvgkoiujphynbml"
_JAMO = "ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎㅏㅐㅑㅕㅓㅔㅗㅛㅜㅠㅡㅣ"


def _map(c):
    if c in _SHIFT:
        return _SHIFT[c]
    i = _KEYS.find(c.lower())
    return c if i < 0 else _JAMO[i]


def _join_vowel(a, b):
    if a == 8:
        if b == 0: return 9
        if b == 1: return 10
        if b == 20: return 11
    if a == 13:
        if b == 4: return 14
        if b == 5: return 15
        if b == 20: return 16
    if a == 18 and b == 20:
        return 19
    return -1


def _join_final(a, b):
    if a == 1 and b == 19: return 3
    if a == 4:
        if b == 22: return 5
        if b == 27: return 6
    if a == 8:
        tbl = [1, 16, 17, 19, 25, 26, 27]
        if b in tbl: return 9 + tbl.index(b)
    if a == 17 and b == 19: return 18
    return -1


def _split_final(t):
    if t == 3: return 1, 19
    if t in (5, 6): return 4, (22 if t == 5 else 27)
    if 9 <= t <= 15: return 8, [1, 16, 17, 19, 25, 26, 27][t - 9]
    if t == 18: return 17, 19
    return 0, t


def _flush(out, l, v, t):
    if l >= 0 and v >= 0:
        out.append(chr(0xAC00 + (l * 21 + v) * 28 + t))
    elif l >= 0:
        out.append(INITIALS[l])
    elif v >= 0:
        out.append(VOWELS[v])


def compose(keys):
    out = []
    l = v = -1
    t = 0
    for key in keys:
        j = _map(key)
        next_l = INITIALS.find(j)
        next_v = VOWELS.find(j)
        if next_l >= 0:
            next_t = FINALS.find(j)
            if l >= 0 and v >= 0 and next_t > 0:
                if t == 0:
                    t = next_t
                    continue
                joined = _join_final(t, next_t)
                if joined >= 0:
                    t = joined
                    continue
            _flush(out, l, v, t)
            l, v, t = next_l, -1, 0
        elif next_v >= 0:
            if v < 0:
                v = next_v
            elif t > 0:
                first, second = _split_final(t)
                _flush(out, l, v, first)
                l = INITIALS.find(FINALS[second])
                v, t = next_v, 0
            else:
                joined = _join_vowel(v, next_v)
                if joined >= 0:
                    v = joined
                else:
                    _flush(out, l, v, t)
                    l, v, t = -1, next_v, 0
        else:
            _flush(out, l, v, t)
            l = v = -1
            t = 0
            out.append(key)
    _flush(out, l, v, t)
    return ''.join(out)


def to_keys(text):
    out = []
    for c in text:
        o = ord(c)
        if 0xAC00 <= o <= 0xD7A3:
            cp = o - 0xAC00
            out.append(INITIAL_KEYS[cp // 588] + VOWEL_KEYS[cp % 588 // 28] + FINAL_KEYS[cp % 28])
        elif 0x1100 <= o <= 0x1112:
            out.append(INITIAL_KEYS[o - 0x1100])
        elif 0x1161 <= o <= 0x1175:
            out.append(VOWEL_KEYS[o - 0x1161])
        elif 0x11A8 <= o <= 0x11C2:
            out.append(FINAL_KEYS[o - 0x11A7])
        else:
            l = INITIALS.find(c); v = VOWELS.find(c); t = FINALS.find(c)
            if l >= 0: out.append(INITIAL_KEYS[l])
            elif v >= 0: out.append(VOWEL_KEYS[v])
            elif t > 0: out.append(FINAL_KEYS[t])
            else: out.append(c)
    return ''.join(out)


if __name__ == '__main__':
    # Round-trip sanity over all modern syllables, mirroring the C# unit test.
    bad = 0
    for cp in range(0xAC00, 0xD7A4):
        s = chr(cp)
        if compose(to_keys(s)) != s:
            bad += 1
    print('roundtrip-mismatch', bad)
    for k, e in [('dkssudgktpdy', '안녕하세요'), ('gksrmfrhk', '한글과'), ('Enlek', '뛰다'), ('rt', 'ㄱㅅ'), ('tlstprP', '신세계')]:
        print(k, compose(k), 'OK' if compose(k) == e else 'EXPECTED ' + e)
