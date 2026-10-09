// Development-only comparison. No third-party code is shipped in Hanautomata.exe.
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const [probe, cache, report] = process.argv.slice(2);
if (!probe || !cache || !report) throw new Error('Expected probe.exe, oracle cache, report path.');
const H = require(path.join(cache, 'hangul.cjs'));
const Inko = require(path.join(cache, 'inko.cjs'));
const inko = new Inko({ allowDoubleConsonant: false });
const letters = 'rsefaqtdwczxvgkoiujphynbml';
const jamo = 'ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎㅏㅐㅑㅕㅓㅔㅗㅛㅜㅠㅡㅣ';
const shifted = { R:'ㄲ', E:'ㄸ', Q:'ㅃ', T:'ㅆ', W:'ㅉ', O:'ㅒ', P:'ㅖ' };
const mapped = c => shifted[c] || jamo[letters.indexOf(c.toLowerCase())] || c;
// Hangul.js joins standalone consonants. Hanautomata and default inko keep them separate.
// Expand only standalone compound jamo, NEVER complete syllables, to compare this policy.
const standalone = { 'ㄳ':'ㄱㅅ', 'ㄵ':'ㄴㅈ', 'ㄶ':'ㄴㅎ', 'ㄺ':'ㄹㄱ', 'ㄻ':'ㄹㅁ', 'ㄼ':'ㄹㅂ', 'ㄽ':'ㄹㅅ', 'ㄾ':'ㄹㅌ', 'ㄿ':'ㄹㅍ', 'ㅀ':'ㄹㅎ', 'ㅄ':'ㅂㅅ' };
const alignPolicy = text => [...text].map(c => standalone[c] || c).join('');
const cases = new Set();
const shortAlphabet = 'rsfqtwkohnplRT';
function visit(prefix, depth) {
  if (prefix) cases.add(prefix);
  if (depth) for (const c of shortAlphabet) visit(prefix + c, depth - 1);
}
visit('', 4);
const shortCases = cases.size;
let seed = 2311172;
const full = letters + 'REQTWOP';
for (let i = 0; i < 20000; i++) {
  let word = '';
  for (let j = 0; j < 12; j++) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    word += full[seed % full.length];
  }
  cases.add(word);
}
const syllables = new Map();
for (let cp = 0xac00; cp <= 0xd7a3; cp++) {
  const value = String.fromCharCode(cp), keys = inko.ko2en(value);
  syllables.set(keys, value); cases.add(keys);
}
for (const raw of ['dkssudgktpdy!', 'rkqtdjqtsms ekfrrkfql', '1qkr2dlf', 'dkdlvhs16',
  'hello world', 'peter@example.com', 'doWork', 'DoWork', 'qkRnjdy', 'dkssudzz', 'rkatkgkqslekbb!',
  '0123456789-_=+[]{};:,./?\\|', ...'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ']) cases.add(raw);
const inputs = [...cases];
const inputPath = path.join(cache, 'inputs.txt'), outputPath = path.join(cache, 'actual.tsv');
fs.writeFileSync(inputPath, inputs.join('\n') + '\n');
const run = spawnSync(probe, [inputPath, outputPath], { encoding:'utf8', windowsHide:true, timeout:60000 });
if (run.error || run.status !== 0) throw run.error || new Error(run.stderr || 'C# probe failed');
const outputs = fs.readFileSync(outputPath, 'utf8').trimEnd().split(/\r?\n/);
if (outputs.length !== inputs.length) throw new Error('Probe output count mismatch');
const mismatches = [], differences = [];
let policyDifferences = 0, reverseChecks = 0, syllableChecks = 0;
for (let i = 0; i < inputs.length; i++) {
  const raw = inputs[i], [actual, keys] = outputs[i].split('\t');
  const a = inko.en2ko(raw), b = H.assemble([...raw].map(mapped));
  const faults = [];
  if (actual !== a) faults.push('inko');
  if (actual !== alignPolicy(b)) faults.push('Hangul.js aligned standalone policy');
  if (actual !== b && actual === alignPolicy(b)) {
    policyDifferences++;
    if (differences.length < 8) differences.push({ raw, hanautomata:actual, hangulJs:b });
  }
  reverseChecks++;
  if (keys !== inko.ko2en(actual)) faults.push('reverse inko');
  if (syllables.has(raw)) {
    syllableChecks++;
    if (actual !== syllables.get(raw)) faults.push('independent syllable mapping');
  }
  if (faults.length) mismatches.push({ raw, actual, inko:a, hangulJs:b, faults });
}
const result = {
  observedAt: new Date().toISOString(), inputCases:inputs.length, shortCases,
  seededRandomCases:20000, randomSeed:2311172, randomLength:12,
  modernSyllables:syllableChecks, reverseChecks, policyDifferences,
  unexpectedMismatches:mismatches.length, policyExamples:differences, mismatches:mismatches.slice(0, 20),
  references: {
    inko:{ sha:'6bcb04b075f282b945dfcdecdf943d21fbc8a5ab', allowDoubleConsonant:false },
    hangulJs:{ sha:'325f7237a030741a10cbcbc2d9b8fac770d9e592', comparison:'expand standalone compound jamo only' }
  },
  scope:'Synthetic composition and reverse mapping; not language intent accuracy or Windows app compatibility.'
};
fs.writeFileSync(report, JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify({ inputCases:result.inputCases, modernSyllables:syllableChecks, reverseChecks, policyDifferences, unexpectedMismatches:mismatches.length }));
process.exitCode = mismatches.length ? 1 : 0;
