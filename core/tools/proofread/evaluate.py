#!/usr/bin/env python3
"""Frozen original development corpus; rates are descriptive, never population estimates."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent

def cases():
    raw = json.loads((HERE / 'evaluation/cases.json').read_text(encoding='utf-8'))
    result = []
    for group, text in raw['normal']:
        result.append(dict(id='normal-' + group, group=group, split='evaluation', text=text, gold=[], protected=[]))
    for group, split, text, original, replacements in raw['errors']:
        assert text.count(original) == 1
        result.append(dict(id='error-' + group, group=group, split=split, text=text,
                           gold=[text.replace(original, r) for r in replacements], protected=[]))
    for group, text, protected in raw['protected']:
        result.append(dict(id='protected-' + group, group=group, split='protected', text=text, gold=[], protected=protected))
    assert len({c['id'] for c in result}) == len(result)
    return result

def percentile(values, p):
    return sorted(values)[max(0, math.ceil(len(values) * p) - 1)] if values else None

def workloads(corpus):
    normal=' '.join(c['text'] for c in corpus if c['id'].startswith('normal-'))[:300]
    mixed=' '.join(c['text'] for c in corpus if c['gold'])[:300]
    assert len(normal)==len(mixed)==300
    return {'normal_300':normal, 'errors_300':mixed}

def measure(corpus, inspect):
    result={}
    for name,text in workloads(corpus).items():
        samples=[inspect(dict(id=name+'-'+str(i),text=text,protected=[])) for i in range(30)]
        result[name]=dict(characters=len(text), utf8_bytes=len(text.encode()),
            text_sha256=hashlib.sha256(text.encode()).hexdigest(), repeats=30,
            p50_ms=statistics.median(samples),p95_ms=percentile(samples,.95))
    return result

def score(rows):
    positives = [r for r in rows if r['gold']]
    negatives = [r for r in rows if not r['gold']]
    flagged = [r for r in rows if r['flagged']]
    found = [r for r in positives if r['flagged']]
    # Sentence classification; an error sentence with a spurious flag is not proof of locating the error.
    return dict(n=len(rows), error_sentences=len(positives), normal_or_protected=len(negatives),
        flagged_sentences=len(flagged), sentence_precision=len(found)/len(flagged) if flagged else None,
        sentence_recall=len(found)/len(positives) if positives else None,
        exact_sentence_top1=sum(r['top1'] for r in positives)/len(positives) if positives else None,
        exact_sentence_top3=sum(r['top3'] for r in positives)/len(positives) if positives else None,
        normal_sentence_false_positive_rate=sum(r['flagged'] for r in negatives)/len(negatives) if negatives else None,
        no_op_rate=sum(not r['flagged'] for r in negatives)/len(negatives) if negatives else None,
        sentences_with_skips=sum(r['skipped'] for r in rows), limited=sum(r['limited'] for r in rows),
        warm_p50_ms=statistics.median(r['ms'] for r in rows), warm_p95_ms=percentile([r['ms'] for r in rows], .95))

def run_native(binary, data, rules, corpus):
    process = subprocess.Popen([str(binary), '--serve', '--dictionary', str(data)] + (['--rules'] if rules else []),
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, encoding='utf-8')
    assert json.loads(process.stdout.readline())['event'] == 'ready'
    peak_rss = 0
    def check(case):
        nonlocal peak_rss
        request = dict(request_id=case['id'], document_id='evaluation', revision='1', text=case['text'], protected_words=case['protected'])
        start = time.perf_counter()
        process.stdin.write(json.dumps(dict(schema_version=1, command='check', request=request), ensure_ascii=False)+'\n'); process.stdin.flush()
        reply = json.loads(process.stdout.readline())
        elapsed = (time.perf_counter()-start)*1000
        if reply['event'] != 'result':
            raise RuntimeError('evaluation request failed: ' + str(reply.get('code')))
        peak_rss = max(peak_rss, reply.get('worker_peak_sampled_rss_bytes', 0))
        return reply['report'], elapsed
    try:
        _, cold = check(dict(id='warmup', text='안녕하세요.', protected=[]))
        rows=[]
        for case in corpus:
            report, elapsed=check(case)
            # Only one error per case: one candidate edit must produce a complete accepted sentence.
            ranked = [[], [], []]
            raw=case['text'].encode()
            for d in report['diagnostics']:
                start,end=d['utf8']['start'],d['utf8']['end']
                assert raw[start:end].decode() == d['original']
                for i,s in enumerate(d['suggestions'][:3]):
                    ranked[i].append((raw[:start]+s.encode()+raw[end:]).decode())
            top1=any(s in case['gold'] for s in ranked[0])
            top3=any(s in case['gold'] for candidates in ranked for s in candidates)
            rows.append(dict(case, flagged=bool(report['diagnostics']), top1=top1, top3=top3,
                             skipped=bool(report['skipped']), limited=report['limited'], ms=elapsed, diagnostics=report['diagnostics']))
        performance=measure(corpus,lambda case:check(case)[1])
        return dict(cold_ms=cold, peak_sampled_worker_rss_bytes=peak_rss, rows=rows, performance=performance)
    finally:
        process.stdin.close()
        try: process.wait(timeout=5)
        except subprocess.TimeoutExpired: process.kill(); process.wait()

def kiwi(corpus):
    import resource
    from kiwipiepy import Kiwi
    import importlib.metadata
    start=time.perf_counter()
    engine=Kiwi(num_workers=1, typos='basic')
    engine.space('안녕하세요.')
    cold=(time.perf_counter()-start)*1000
    rows=[]
    for case in corpus:
        start=time.perf_counter()
        # Raw spacing proposal with whole-request protected-word abstention. Not a production adapter.
        result=case['text'] if case['protected'] else engine.space(case['text'], reset_whitespace=False)
        elapsed=(time.perf_counter()-start)*1000
        rows.append(dict(case, flagged=result!=case['text'], top1=result in case['gold'], top3=result in case['gold'],
                         skipped=bool(case['protected']), limited=False, ms=elapsed, suggestion=result))
    def timed(case):
        start=time.perf_counter(); engine.space(case['text'],reset_whitespace=False)
        return (time.perf_counter()-start)*1000
    performance=measure(corpus,timed)
    peak=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    return dict(cold_ms=cold, peak_rss_bytes=peak if sys.platform=='darwin' else peak*1024, rows=rows,
                versions={p:importlib.metadata.version(p) for p in ['kiwipiepy','kiwipiepy_model','numpy','tqdm']},
                performance=performance)

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--binary',type=Path)
    parser.add_argument('--dictionary',type=Path)
    parser.add_argument('--kiwi-only',action='store_true')
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args(); corpus=cases()
    start=time.perf_counter()
    if args.kiwi_only:
        engines={'kiwi-space-basic-raw':kiwi(corpus)}
    else:
        engines={name:run_native(args.binary.resolve(),args.dictionary.resolve(),rules,corpus)
                 for name,rules in [('hunspell',False),('hunspell-rules',True)]}
    for result in engines.values():
        result['all']=score(result['rows'])
        result['holdout']=score([r for r in result['rows'] if r['split']!='rule-development'])
        result['protected']=score([r for r in result['rows'] if r['split']=='protected'])
        result['normal_only']=score([r for r in result['rows'] if r['id'].startswith('normal-')])
    output=dict(schema_version=1, corpus_sha256=hashlib.sha256((HERE/'evaluation/cases.json').read_bytes()).hexdigest(),
                platform=platform.platform(), machine=platform.machine(), elapsed_seconds=time.perf_counter()-start,
                caveats=['Synthetic developer-labeled data; no independent human review.',
                    'Five rule-development positives excluded from holdout.',
                    'Precision/recall are sentence-level; top-k requires an exact accepted full sentence.',
                    'Top-k accepts any single-diagnostic candidate at rank <= k; not a global sentence ranking.',
                    'Kiwi is a raw spacing baseline, not a safety-equivalent replacement.',
                    'Native RSS is sampled at 5ms on macOS; Kiwi RSS is the OS process high-water mark, including Python/model. Not equivalent measurements.'], engines=engines)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(output,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({name:{k:v for k,v in result.items() if k!='rows'} for name,result in engines.items()},ensure_ascii=False,indent=2))
if __name__=='__main__': main()
