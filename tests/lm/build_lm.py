"""Build compact Korean syllable and English letter n-gram tables from a local text corpus.

Usage: python3 build_lm.py --repo DIR [--roots a,b] [--out DIR]
Ship: gzip -9 out/korean-syllable-lm-min30.txt > ../../data/korean-lm.txt.gz ; cp out/english-letter-lm.txt ../../data/english-lm.txt

Only aggregate character statistics leave this script; no sentences are stored.
Outputs (scratchpad/hanautomata-lm/out):
  korean-syllable-lm.txt  — unigram + bigram log10 probabilities (quantized), boundary token '^' and '$'
  english-letter-lm.txt   — 28x28 letter bigram log10 probabilities with boundary
  heldout-korean.txt      — Korean words (not in the Hanautomata hand lists) with their QWERTY keys
  heldout-english.txt     — English tokens with frequency
  stats.json
"""
import json, math, os, random, re, sys
from collections import Counter

sys.path.insert(0, os.path.dirname(__file__))
from hangul import to_keys, compose

import argparse
HERE = os.path.dirname(os.path.abspath(__file__))
HANAUTOMATA = os.path.dirname(os.path.dirname(HERE))
parser = argparse.ArgumentParser(description='Build Hanautomata n-gram tables from a directory of markdown/text files (aggregate counts only).')
parser.add_argument('--repo', required=True, help='corpus root directory holding plain .txt/.md files (e.g. a Wikipedia dump extracted with wikitext2txt.py)')
parser.add_argument('--roots', default='.', help='comma-separated subfolders under --repo (default: the root itself)')
parser.add_argument('--out', default=os.path.join(HERE, 'out'))
parser.add_argument('--extensions', default='.md,.txt')
args = parser.parse_args()
REPO = args.repo
ROOTS = args.roots.split(',')
OUT = args.out
EXTENSIONS = tuple(args.extensions.split(','))
os.makedirs(OUT, exist_ok=True)

KO = re.compile(r'[가-힣]+')
EN = re.compile(r'[A-Za-z]+')
FENCE = re.compile(r'```.*?```', re.S)

uni, bi = Counter(), Counter()
en_bi = Counter()
ko_words, en_words = Counter(), Counter()
files = 0
for root in ROOTS:
    for dp, dn, fn in os.walk(os.path.join(REPO, root)):
        dn[:] = [d for d in dn if not d.startswith('.') and d not in ('node_modules', 'build', 'deliverables')]
        for f in fn:
            if not f.endswith(EXTENSIONS):
                continue
            # Exclude Hanautomata's own corpus so evaluation stays independent of the model data.
            if dp.startswith(HANAUTOMATA):
                continue
            try:
                t = open(os.path.join(dp, f), encoding='utf-8', errors='ignore').read()
            except OSError:
                continue
            files += 1
            t = FENCE.sub(' ', t)
            for w in KO.findall(t):
                ko_words[w] += 1
                prev = '^'
                for s in w:
                    uni[s] += 1
                    bi[(prev, s)] += 1
                    prev = s
                bi[(prev, '$')] += 1
            for w in EN.findall(t):
                if len(w) < 2:
                    continue
                lw = w.lower()
                en_words[lw] += 1
                prev = '^'
                for c in lw:
                    en_bi[(prev, c)] += 1
                    prev = c
                en_bi[(prev, '$')] += 1

total_syl = sum(uni.values())
print('files', files, 'korean-words', sum(ko_words.values()), 'syllables', total_syl, 'distinct-syl', len(uni), 'bigrams', len(bi))
print('english-tokens', sum(en_words.values()), 'distinct', len(en_words))

# --- Korean syllable LM: keep every syllable seen >= 2 and bigrams seen >= 3, add-k smoothing with backoff.
V = 11172
K_UNI = 0.5
uni_total = total_syl + K_UNI * V
uni_lp = {s: math.log10((c + K_UNI) / uni_total) for s, c in uni.items() if c >= 2}
uni_floor = math.log10(K_UNI / uni_total)
# Conditional bigram P(b|a) with absolute-discount style backoff to unigram.
ctx_total = Counter()
for (a, b), c in bi.items():
    ctx_total[a] += c
D = 0.75
bi_lp = {}
ctx_types = Counter()
for (a, b), c in bi.items():
    ctx_types[a] += 1
for (a, b), c in bi.items():
    if c < 3:
        continue
    p = max(c - D, 0) / ctx_total[a]
    bi_lp[(a, b)] = math.log10(p)
backoff_w = {a: math.log10(D * ctx_types[a] / ctx_total[a]) for a in ctx_total}
print('kept uni', len(uni_lp), 'kept bi', len(bi_lp))

def write_lm(path, min_count, digits):
    fmt = '%%.%df' % digits
    with open(path, 'w', encoding='utf-8') as f:
        f.write('# Hanautomata Korean syllable LM v1 · log10 probabilities · aggregate counts only · min-bigram-count %d\n' % min_count)
        f.write('#floor %.3f\n' % uni_floor)
        for s, lp in sorted(uni_lp.items(), key=lambda x: -x[1]):
            f.write(('U %s ' + fmt + ' ' + fmt + '\n') % (s, lp, backoff_w.get(s, math.log10(0.5))))
        f.write(('U ^ 0 ' + fmt + '\n') % backoff_w.get('^', math.log10(0.5)))
        for (a, b), lp in sorted(bi_lp.items(), key=lambda x: -x[1]):
            if bi[(a, b)] < min_count:
                continue
            f.write(('B %s%s ' + fmt + '\n') % (a, b, lp))
write_lm(os.path.join(OUT, 'korean-syllable-lm.txt'), 3, 3)
for mc in (10, 30, 100, 300):
    write_lm(os.path.join(OUT, 'korean-syllable-lm-min%d.txt' % mc), mc, 2)

# --- English letter bigram LM (28 symbols: ^ a-z $)
alphabet = '^' + 'abcdefghijklmnopqrstuvwxyz' + '$'
en_ctx = Counter()
for (a, b), c in en_bi.items():
    en_ctx[a] += c
with open(os.path.join(OUT, 'english-letter-lm.txt'), 'w', encoding='utf-8') as f:
    f.write('# Hanautomata English letter bigram LM v1 · log10 P(b|a) · add-0.5 smoothing\n')
    for a in alphabet[:-1]:
        row = []
        for b in alphabet[1:]:
            p = (en_bi.get((a, b), 0) + 0.5) / (en_ctx.get(a, 0) + 0.5 * 27)
            row.append('%.3f' % math.log10(p))
        f.write('%s %s\n' % (a, ' '.join(row)))

# --- Held-out sets. Korean: frequent words (count >= 3) of length 1..7, keyed as QWERTY.
hand = set()
core = open(os.path.join(HANAUTOMATA, 'src/Core.cs'), encoding='utf-8').read()
for w in KO.findall(core):
    hand.add(w)
random.seed(20261008)
ko_candidates = [(w, c) for w, c in ko_words.items() if c >= 3 and 1 <= len(w) <= 7 and w not in hand]
ko_candidates.sort(key=lambda x: -x[1])
ko_sample = random.sample(ko_candidates[:20000], min(4000, len(ko_candidates[:20000])))
with open(os.path.join(OUT, 'heldout-korean.txt'), 'w', encoding='utf-8') as f:
    for w, c in ko_sample:
        k = to_keys(w)
        assert compose(k) == w, (w, k)
        f.write('%s\t%s\t%d\n' % (k, w, c))
english_list = set(open(os.path.join(HANAUTOMATA, 'data/english.txt'), encoding='utf-8').read().split())
en_candidates = [(w, c) for w, c in en_words.items() if c >= 3 and 2 <= len(w) <= 12 and w.isalpha()]
en_candidates.sort(key=lambda x: -x[1])
en_sample = random.sample(en_candidates[:20000], min(4000, len(en_candidates[:20000])))
with open(os.path.join(OUT, 'heldout-english.txt'), 'w', encoding='utf-8') as f:
    for w, c in en_sample:
        f.write('%s\t%d\t%s\n' % (w, c, 'dict' if w in english_list else 'nodict'))
json.dump({'files': files, 'korean_words': sum(ko_words.values()), 'syllables': total_syl, 'distinct_syllables': len(uni),
           'kept_unigrams': len(uni_lp), 'kept_bigrams': len(bi_lp), 'english_tokens': sum(en_words.values()),
           'heldout_korean': len(ko_sample), 'heldout_english': len(en_sample), 'hand_list_words': len(hand)},
          open(os.path.join(OUT, 'stats.json'), 'w'), indent=1)
print('done')
