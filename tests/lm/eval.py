"""Evaluate the v0.2 scorer against held-out sets and the user corpus; sweep thresholds; compare with v0.1.8 emulation."""
import json, os, re, sys
sys.path.insert(0, os.path.dirname(__file__))
from hangul import to_keys
from detector import LanguageModel, Scorer, load_hand_lists, current_engine, OUT, HANAUTOMATA

HANGUL = re.compile(r'^[가-힣]+$')
LETTERS = re.compile(r'^[A-Za-z]+$')


def load_heldout():
    ko = [l.rstrip('\n').split('\t') for l in open(os.path.join(OUT, 'heldout-korean.txt'), encoding='utf-8')]
    en = [l.rstrip('\n').split('\t') for l in open(os.path.join(OUT, 'heldout-english.txt'), encoding='utf-8')]
    return ko, en


USER_PROMPTS = os.path.join(HANAUTOMATA, 'tests/corpora/user-prompts.txt')          # optional private development sample (not distributed)
USER_CORRECTIONS = os.path.join(HANAUTOMATA, 'tests/corpora/user-corrections.tsv')


def load_user_corpus():
    if not (os.path.exists(USER_PROMPTS) and os.path.exists(USER_CORRECTIONS)):
        return [], [], {}, 0
    text = open(USER_PROMPTS, encoding='utf-8').read()
    corrections = {}
    for l in open(USER_CORRECTIONS, encoding='utf-8'):
        if l.startswith('#') or not l.strip():
            continue
        raw, exp = l.rstrip('\n').split('\t')[:2]
        corrections[raw] = exp
    ko, en, skipped = [], [], 0
    for tok in text.split():
        if tok in corrections:
            continue
        if HANGUL.match(tok):
            ko.append((to_keys(tok), tok))
        elif LETTERS.match(tok):
            en.append(tok)
        else:
            skipped += 1
    return ko, en, corrections, skipped


def run(scorer, ko_items, en_items, prev=None):
    ko_ok = ko_unc = 0
    en_ok = en_unc = 0
    ko_fail, en_fail = [], []
    for keys, word in ko_items:
        d = scorer.decide(keys, keys, prev)
        if d.korean and d.text == word: ko_ok += 1
        elif d.uncertain: ko_unc += 1; ko_fail.append((keys, word, round(d.llr, 2), 'unc'))
        else: ko_fail.append((keys, word, round(d.llr, 2), d.text))
    for raw in en_items:
        d = scorer.decide(raw, raw, prev)
        if not d.korean and not d.uncertain: en_ok += 1
        elif d.uncertain: en_unc += 1; en_fail.append((raw, round(d.llr, 2), 'unc'))
        else: en_fail.append((raw, round(d.llr, 2), d.text))
    return ko_ok, ko_unc, en_ok, en_unc, ko_fail, en_fail


def main():
    lm = LanguageModel()
    korean_list, morph = load_hand_lists()
    ko_h, en_h = load_heldout()
    ko_items = [(k, w) for k, w, c in ko_h]
    en_items = [w for w, c, d in en_h]
    print('heldout korean', len(ko_items), 'english', len(en_items))
    # Length distribution of Korean words in real text (by token frequency) to size the "short word" problem.
    short = sum(int(c) for k, w, c in ko_h if len(w) <= 2); total = sum(int(c) for k, w, c in ko_h)
    print('share of Korean word tokens with <=2 syllables (held-out, freq-weighted): %.1f%%' % (100.0 * short / total))

    # --- v0.1.8 emulation
    cur_ko = sum(1 for k, w in ko_items if current_engine(k, k, korean_list, morph, lm)[0])
    cur_en = sum(1 for w in en_items if not current_engine(w, w, korean_list, morph, lm)[0])
    reasons = {}
    for k, w in ko_items:
        ok, r = current_engine(k, k, korean_list, morph, lm)
        if not ok: reasons[r] = reasons.get(r, 0) + 1
    print('v0.1.8 emulation: korean converted %d/%d (%.1f%%)  english kept %d/%d (%.1f%%)  korean-miss reasons %s' % (
        cur_ko, len(ko_items), 100.0 * cur_ko / len(ko_items), cur_en, len(en_items), 100.0 * cur_en / len(en_items), reasons))
    by_len = {}
    for k, w in ko_items:
        ok, r = current_engine(k, k, korean_list, morph, lm)
        a = by_len.setdefault(len(w), [0, 0]); a[1] += 1; a[0] += ok
    print('v0.1.8 korean by syllable length:', {n: '%d/%d' % (v[0], v[1]) for n, v in sorted(by_len.items())})

    # --- threshold sweep
    print('\nsweep  T  lexbonus | ko-ok ko-unc ko-wrong | en-ok en-unc en-wrong')
    best = None
    for bonus in (1.5, 2.5, 3.5):
        for t in (0.5, 1.0, 1.5, 2.0, 3.0):
            sc = Scorer(lm); sc.T_KO = sc.T_EN = t; sc.LEXICON_BONUS = bonus
            ko_ok, ko_unc, en_ok, en_unc, kf, ef = run(sc, ko_items, en_items)
            ko_wrong = len(ko_items) - ko_ok - ko_unc; en_wrong = len(en_items) - en_ok - en_unc
            print('      %4.1f  %4.1f    | %5d %6d %8d | %5d %6d %8d' % (t, bonus, ko_ok, ko_unc, ko_wrong, en_ok, en_unc, en_wrong))
            score = ko_ok + en_ok - 5 * (ko_wrong + en_wrong)
            if best is None or score > best[0]: best = (score, t, bonus)
    print('best', best)
    sc = Scorer(lm); sc.T_KO = sc.T_EN = best[1]; sc.LEXICON_BONUS = best[2]
    ko_ok, ko_unc, en_ok, en_unc, kf, ef = run(sc, ko_items, en_items)
    print('\nchosen T=%.1f bonus=%.1f: korean %d ok / %d uncertain / %d wrong ; english %d ok / %d uncertain / %d wrong' % (
        best[1], best[2], ko_ok, ko_unc, len(ko_items) - ko_ok - ko_unc, en_ok, en_unc, len(en_items) - en_ok - en_unc))
    print('korean failures (first 25):', kf[:25])
    print('english failures (first 25):', ef[:25])
    by_len = {}
    for k, w in ko_items:
        d = sc.decide(k, k)
        a = by_len.setdefault(len(w), [0, 0]); a[1] += 1; a[0] += (d.korean and d.text == w)
    print('v0.2 korean by syllable length:', {n: '%d/%d' % (v[0], v[1]) for n, v in sorted(by_len.items())})

    # --- continuity effect on the uncertain band (Korean context vs English context)
    ko_ok_c, ko_unc_c, en_ok_c, en_unc_c, _, _ = run(sc, ko_items, [], prev=True)
    _, _, en_ok_e, en_unc_e, _, _ = run(sc, [], en_items, prev=False)
    print('with Korean-context prior: korean ok %d (unc %d) ; with English-context prior: english ok %d (unc %d)' % (ko_ok_c, ko_unc_c, en_ok_e, en_unc_e))
    _, _, en_ok_k, en_unc_k, _, enf_k = run(sc, [], en_items, prev=True)
    ko_ok_e, ko_unc_e, _, _, kof_e, _ = run(sc, ko_items, [], prev=False)
    print('cross prior (adversarial): english after Korean word ok %d unc %d wrong %d ; korean after English word ok %d unc %d wrong %d' % (
        en_ok_k, en_unc_k, len(en_items) - en_ok_k - en_unc_k, ko_ok_e, ko_unc_e, len(ko_items) - ko_ok_e - ko_unc_e))

    # --- user corpus (development sample; the hand lists were tuned on it)
    uko, uen, corrections, skipped = load_user_corpus()
    if not uko and not uen:
        print('\nuser corpus: not present (tests/corpora/user-prompts.txt + user-corrections.tsv are a private development sample) — skipped')
        json.dump({'T': best[1], 'lexicon_bonus': best[2]}, open(os.path.join(OUT, 'chosen-thresholds.json'), 'w'))
        return
    print('\nuser corpus tokens: korean %d english %d corrections %d skipped(mixed/other) %d' % (len(uko), len(uen), len(corrections), skipped))
    ko_ok, ko_unc, en_ok, en_unc, kf, ef = run(sc, uko, uen)
    print('v0.2 on user corpus: korean %d/%d (unc %d) english %d/%d (unc %d)' % (ko_ok, len(uko), ko_unc, en_ok, len(uen), en_unc))
    print(' korean failures:', kf[:40]); print(' english failures:', ef[:40])
    cur_ko = sum(1 for k, w in uko if current_engine(k, k, korean_list, morph, lm)[0])
    cur_en = sum(1 for w in uen if not current_engine(w, w, korean_list, morph, lm)[0])
    print('v0.1.8 emulation on user corpus: korean %d/%d english %d/%d' % (cur_ko, len(uko), cur_en, len(uen)))
    print('corrections:')
    for raw, exp in corrections.items():
        if LETTERS.match(raw):
            d = sc.decide(raw, raw); print('  %-18s expect %-10s got %-10s llr=%.2f %s' % (raw, exp, d.text, d.llr, d.reason))
        else:
            print('  %-18s expect %-10s (mixed token, DAG path)' % (raw, exp))
    # ambiguous probes
    print('\nambiguous probes:')
    for w in ['go', 'so', 'do', 'no', 'dh', 'uk', 'ej', 'wbs', 'dj', 'rk', 'dl', 'skdml', 'rmfjs', 'gksek', 'tjfrP', 'eogks', 'report', 'flare', 'change', 'meeting', 'api', 'tjdqn', 'rhkwp']:
        d = sc.decide(w, w); dk = sc.decide(w, w, True); de = sc.decide(w, w, False)
        print('  %-8s llr=%6.2f  none->%-8s ko-ctx->%-8s en-ctx->%-8s' % (w, d.llr, d.text, dk.text, de.text))
    json.dump({'T': best[1], 'lexicon_bonus': best[2]}, open(os.path.join(OUT, 'chosen-thresholds.json'), 'w'))


if __name__ == '__main__':
    main()
