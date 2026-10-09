"""Reference implementation of the Hanautomata v0.2 language-likelihood scorer (Python).

Decision for a run of ASCII letters typed on a QWERTY layout:
  ko = log10 P_ko(syllable sequence composed from the keys)   [Korean syllable bigram LM, boundary tokens]
  en = log10 P_en(letter sequence)                             [English letter bigram LM] + lexicon bonus
  llr = ko - en
  llr >= T_KO  -> Korean,  llr <= -T_EN -> English,  otherwise Uncertain (keep raw, offer alternative)
Both models score the same key sequence, so the log-likelihood ratio is directly comparable.
"""
import math, os, re, sys
sys.path.insert(0, os.path.dirname(__file__))
from hangul import compose

import gzip
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, 'out')
HANAUTOMATA = os.path.dirname(os.path.dirname(HERE))
SHIPPED_KO = os.path.join(HANAUTOMATA, 'data', 'korean-lm.txt.gz')
SHIPPED_EN = os.path.join(HANAUTOMATA, 'data', 'english-lm.txt')


class LanguageModel:
    def __init__(self, ko_path=SHIPPED_KO, en_path=SHIPPED_EN, english_words=os.path.join(HANAUTOMATA, 'data', 'english.txt')):
        self.uni, self.backoff, self.bi = {}, {}, {}
        self.floor = -6.0
        opener = gzip.open if ko_path.endswith('.gz') else open
        for line in opener(ko_path, 'rt', encoding='utf-8'):
            if line.startswith('#floor'):
                self.floor = float(line.split()[1]); continue
            if line.startswith('#') or not line.strip():
                continue
            kind, rest = line[0], line[2:].rstrip('\n')
            if kind == 'U':
                s, lp, bw = rest.split(' ')
                self.uni[s] = float(lp); self.backoff[s] = float(bw)
            elif kind == 'B':
                pair, lp = rest.split(' ')
                self.bi[pair] = float(lp)
        self.en = {}
        for line in open(en_path, encoding='utf-8'):
            if line.startswith('#'):
                continue
            parts = line.split()
            a, row = parts[0], parts[1:]
            for b, lp in zip('abcdefghijklmnopqrstuvwxyz$', row):
                self.en[a + b] = float(lp)
        self.words = set(w.lower() for w in open(english_words, encoding='utf-8').read().split())

    # --- Korean: syllable bigram with backoff; stray jamo and unknown syllables get the floor.
    def korean(self, keys):
        text = compose(keys)
        total = 0.0
        prev = '^'
        stray = 0
        for s in text:
            if not (0xAC00 <= ord(s) <= 0xD7A3):
                stray += 1
                total += self.floor - 1.0  # a stray jamo is worse than a rare syllable
                prev = s
                continue
            lp = self.bi.get(prev + s)
            if lp is None:
                lp = self.backoff.get(prev, math.log10(0.5)) + self.uni.get(s, self.floor)
            total += lp
            prev = s
        end = self.bi.get(prev + '$')
        if end is None:
            end = self.backoff.get(prev, math.log10(0.5)) + math.log10(0.08)  # typical boundary mass
        total += end
        return total, text, stray

    # --- English: letter bigram; words in the small lexicon (and simple inflections) get a lexical bonus.
    def english(self, raw):
        lower = raw.lower()
        total = 0.0
        prev = '^'
        for c in lower:
            total += self.en.get(prev + c, -4.0)
            prev = c
        total += self.en.get(prev + '$', -4.0)
        return total

    def in_lexicon(self, raw):
        lower = raw.lower()
        if lower in self.words:
            return True
        for suffix in ('s', 'es', 'ed', 'ing', 'ly'):
            if len(lower) > len(suffix) + 2 and lower.endswith(suffix):
                stem = lower[:-len(suffix)]
                if stem in self.words or stem + 'e' in self.words:
                    return True
        return False


class Decision:
    __slots__ = ('text', 'alternative', 'korean', 'reason', 'llr', 'uncertain')

    def __init__(self, text, alternative, korean, reason, llr, uncertain=False):
        self.text, self.alternative, self.korean, self.reason, self.llr, self.uncertain = text, alternative, korean, reason, llr, uncertain


class Scorer:
    """Mirrors the planned C# `LanguageScorer`. Thresholds are design constants, calibrated offline."""
    LEXICON_BONUS = 2.5     # log10 units added to the English side for a lexicon word
    T_KO = 1.0              # llr above this -> Korean
    T_EN = 1.0              # llr below -this -> English
    CONTINUITY = 1.5        # prior nudge toward the previous committed word's language, only inside the uncertain band

    def __init__(self, lm):
        self.lm = lm

    def decide(self, raw, keys=None, previous_korean=None):
        keys = keys if keys is not None else raw
        ko, composed, stray = self.lm.korean(keys)
        en = self.lm.english(raw)
        lexical = self.lm.in_lexicon(raw)
        if lexical:
            en += self.LEXICON_BONUS
        llr = ko - en
        if len(raw) >= 2 and raw.isupper() and raw == keys:
            return Decision(raw, composed, False, 'acronym', llr)
        nudged = llr
        if previous_korean is not None and -self.T_EN < llr < self.T_KO:
            nudged = llr + (self.CONTINUITY if previous_korean else -self.CONTINUITY)
        if nudged >= self.T_KO:
            return Decision(composed, raw, True, 'ko-llr' + ('+ctx' if nudged != llr else ''), llr)
        if nudged <= -self.T_EN:
            return Decision(raw, composed, False, 'en-llr' + ('+ctx' if nudged != llr else '') + ('+lex' if lexical else ''), llr)
        # Uncertain: keep the raw letters unless the Korean side is clearly a complete word and English is not lexical.
        return Decision(raw, composed, False, 'uncertain', llr, uncertain=True)


# --- Emulation of the current (v0.1.8) rule order for pure-letter tokens, for gap measurement only.
def load_hand_lists():
    core = open(os.path.join(HANAUTOMATA, 'src/Core.cs'), encoding='utf-8').read()
    def block(name):
        m = re.search(name + r'\s*=\s*(?:new HashSet<string>\(\()?\s*((?:"[^"]*"\s*\+?\s*)+)', core)
        return ' '.join(re.findall(r'"([^"]*)"', m.group(1))).split() if m else []
    korean = set(block('korean'))
    nouns = block('const string nouns')
    hada = block('const string hadaNouns')
    particles = "으로부터 으로 에서는 에서 에게 에도 의 를 을 은 는 이 가 와 과 로 에 만 도 보다 처럼".split()
    def fits(noun, p):
        final = (ord(noun[-1]) - 0xAC00) % 28
        if p in ('은', '이', '을', '과'): return final != 0
        if p in ('는', '가', '를', '와'): return final == 0
        if p in ('으로', '으로부터'): return final != 0 and final != 8
        if p == '로': return final == 0 or final == 8
        return True
    morph = set()
    for n in nouns:
        for p in particles:
            if fits(n, p): morph.add(n + p)
    endings = "하다 하는 하고 하면 하며 합니다 했습니다 해주세요 해줘 해요 했어요 해도 하지 하겠습니다 했고 했다 하였다 합니다만 하도록 하기".split()
    for s in hada:
        for e in endings:
            morph.add(s + e)
    return korean, morph


def current_engine(raw, keys, korean, morph, lm):
    """Approximate v0.1.8 DecideAtom for a pure-letter token (mixed-word DAG not emulated)."""
    composed = compose(keys)
    if lm.in_lexicon(raw):
        return False, 'english-word'
    if len(raw) >= 2 and raw.isupper() and raw == keys:
        return False, 'acronym'
    if composed in korean:
        return True, 'list'
    if composed in morph:
        return True, 'morphology'
    if len(composed) >= 3 and all(0xAC00 <= ord(c) <= 0xD7A3 for c in composed):
        return True, 'pattern'
    return False, 'uncertain'


if __name__ == '__main__':
    lm = LanguageModel()
    sc = Scorer(lm)
    for w in ['dkssudgktpdy', 'hello', 'go', 'rmfjs', 'skdml', 'ej', 'wbs', 'tjfrP', 'eogks', 'report', 'world', 'gksrmf', 'zjavbxj', 'flare', 'API', 'dh', 'uk', 'qkdtks', 'tlfgod']:
        d = sc.decide(w)
        print('%-14s llr=%6.2f  -> %s  [%s]  alt=%s' % (w, d.llr, d.text, d.reason, d.alternative))
