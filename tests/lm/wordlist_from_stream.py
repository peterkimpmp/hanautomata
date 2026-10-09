"""Count word frequencies from a MediaWiki pages-articles XML stream on stdin, skipping the first N kept pages so the
word-list slice stays disjoint from the pages that built the LM tables and held-out sets.
Usage: bzip2 -dc dump.bz2 | python3 wordlist_from_stream.py --lang ko --skip-pages 94575 --out korean-words.tsv
       bzip2 -dc dump.bz2 | python3 wordlist_from_stream.py --lang en --skip-pages 75241 --out english-words.tsv
Ship: filter by count/length and gzip into data/korean-words.txt.gz · data/english-words.txt.gz (see README)."""
import sys, os, re, html, argparse
from collections import Counter
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from wikitext2txt import clean  # same cleaning as the LM corpus
ap = argparse.ArgumentParser(); ap.add_argument('--skip-pages', type=int, default=0); ap.add_argument('--out', required=True); ap.add_argument('--lang', default='ko'); args = ap.parse_args()
KO = re.compile(r'[가-힣]+'); EN = re.compile(r'[A-Za-z]+')
pages = kept = counted = 0; ns = '0'; in_text = False; text = []; words = Counter()
for line in sys.stdin:
    if not in_text:
        if '<ns>' in line:
            m = re.search(r'<ns>(\d+)</ns>', line); ns = m.group(1) if m else '0'
        i = line.find('<text')
        if i < 0: continue
        j = line.find('>', i)
        if j < 0: continue
        rest = line[j+1:]; in_text = True; text = []
        if '</text>' in rest: text.append(rest[:rest.find('</text>')]); in_text = False
        else: text.append(rest); continue
    else:
        if '</text>' in line: text.append(line[:line.find('</text>')]); in_text = False
        else: text.append(line); continue
    pages += 1
    raw = ''.join(text)
    if ns != '0' or raw.lstrip().lower().startswith(('#redirect', '#넘겨주기')): continue
    kept += 1
    if kept <= args.skip_pages: continue
    counted += 1
    cleaned = clean(raw)
    if args.lang == 'ko':
        for w in KO.findall(cleaned):
            if 1 <= len(w) <= 8: words[w] += 1
    else:
        for w in EN.findall(cleaned):
            if 2 <= len(w) <= 20: words[w.lower()] += 1
with open(args.out, 'w', encoding='utf-8') as f:
    for w, c in words.most_common():
        if c < 3: break
        f.write('%s\t%d\n' % (w, c))
print('pages', pages, 'kept', kept, 'counted', counted, 'distinct>=3', sum(1 for c in words.values() if c >= 3), file=sys.stderr)
