"""Stream a MediaWiki pages-articles XML dump (possibly truncated) from stdin and write plain text chunk files.
Usage: bzip2 -dc dump.bz2 | python3 wikitext2txt.py <outdir> <lang>   — aggregate-only downstream (build_lm.py)."""
import sys, re, html, os
out, lang = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
CHUNK = 20 * 1024 * 1024
REF = re.compile(r'<ref[^>/]*/>|<ref[^>]*>.*?</ref>', re.S | re.I)
TAG = re.compile(r'<[^>]+>')
TEMPLATE = re.compile(r'\{\{[^{}]*\}\}', re.S)
TABLE = re.compile(r'\{\|.*?\|\}', re.S)
FILE = re.compile(r'\[\[(?:File|Image|파일|그림|분류|Category):[^\[\]]*(?:\[\[[^\[\]]*\]\][^\[\]]*)*\]\]', re.I)
LINK2 = re.compile(r'\[\[[^\[\]|]*\|([^\[\]]*)\]\]')
LINK1 = re.compile(r'\[\[([^\[\]]*)\]\]')
EXT = re.compile(r'\[https?://[^\s\]]*\s*([^\]]*)\]')
URL = re.compile(r'https?://\S+')
HEAD = re.compile(r'^=+\s*(.*?)\s*=+\s*$', re.M)
MARK = re.compile(r"'''?|^[*#:;]+\s*", re.M)
def clean(t):
    t = html.unescape(t)
    t = REF.sub(' ', t); t = TABLE.sub(' ', t)
    for _ in range(6):
        t2 = TEMPLATE.sub(' ', t)
        if t2 == t: break
        t = t2
    t = FILE.sub(' ', t); t = LINK2.sub(r'\1', t); t = LINK1.sub(r'\1', t); t = EXT.sub(r'\1', t); t = URL.sub(' ', t)
    t = TAG.sub(' ', t); t = HEAD.sub(r'\1', t); t = MARK.sub('', t)
    return t
pages = kept = 0; buf = []; size = 0; n = 0; ns = '0'; in_text = False; text = []
def flush():
    global buf, size, n
    if not buf: return
    with open(os.path.join(out, '%s-%03d.txt' % (lang, n)), 'w', encoding='utf-8') as f: f.write('\n'.join(buf))
    n += 1; buf = []; size = 0
for line in sys.stdin:
    if not in_text:
        if '<ns>' in line:
            m = re.search(r'<ns>(\d+)</ns>', line); ns = m.group(1) if m else '0'
        i = line.find('<text')
        if i >= 0:
            j = line.find('>', i)
            if j < 0: continue
            rest = line[j+1:]
            in_text = True; text = []
            if '</text>' in rest:
                text.append(rest[:rest.find('</text>')]); in_text = False
            else:
                text.append(rest); continue
        else:
            continue
    else:
        if '</text>' in line:
            text.append(line[:line.find('</text>')]); in_text = False
        else:
            text.append(line); continue
    pages += 1
    raw = ''.join(text)
    if ns != '0' or raw.lstrip().lower().startswith(('#redirect', '#넘겨주기')): continue
    c = clean(raw)
    kept += 1; buf.append(c); size += len(c)
    if size >= CHUNK: flush()
flush()
print('pages', pages, 'kept', kept, 'chunks', n, file=sys.stderr)
