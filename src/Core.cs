using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;

namespace Hanautomata
{
    // Standard modern Dubeolsik tables. No third-party implementation is bundled.
    public static class Hangul
    {
        const string Initials = "ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ";
        const string Vowels = "ㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ";
        const string Finals = "\0ㄱㄲㄳㄴㄵㄶㄷㄹㄺㄻㄼㄽㄾㄿㅀㅁㅂㅄㅅㅆㅇㅈㅊㅋㅌㅍㅎ";
        static readonly string[] InitialKeys = "r R s e E f a q Q t T d w W c z x v g".Split(' ');
        static readonly string[] VowelKeys = "k o i O j p u P h hk ho hl y n nj np nl b m ml l".Split(' ');
        static readonly string[] FinalKeys = "|r|R|rt|s|sw|sg|e|f|fr|fa|fq|ft|fx|fv|fg|a|q|qt|t|T|d|w|c|z|x|v|g".Split('|');
        static char Map(char c)
        {
            if (c == 'R') return 'ㄲ'; if (c == 'E') return 'ㄸ'; if (c == 'Q') return 'ㅃ';
            if (c == 'T') return 'ㅆ'; if (c == 'W') return 'ㅉ'; if (c == 'O') return 'ㅒ'; if (c == 'P') return 'ㅖ';
            const string keys = "rsefaqtdwczxvgkoiujphynbml";
            const string jamo = "ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎㅏㅐㅑㅕㅓㅔㅗㅛㅜㅠㅡㅣ";
            int i = keys.IndexOf(char.ToLowerInvariant(c));
            return i < 0 ? c : jamo[i];
        }
        static int JoinVowel(int a, int b)
        {
            if (a == 8) { if (b == 0) return 9; if (b == 1) return 10; if (b == 20) return 11; }
            if (a == 13) { if (b == 4) return 14; if (b == 5) return 15; if (b == 20) return 16; }
            if (a == 18 && b == 20) return 19;
            return -1;
        }
        static int JoinFinal(int a, int b)
        {
            if (a == 1 && b == 19) return 3;
            if (a == 4) { if (b == 22) return 5; if (b == 27) return 6; }
            if (a == 8) { int i = Array.IndexOf(new[] { 1, 16, 17, 19, 25, 26, 27 }, b); if (i >= 0) return 9 + i; }
            if (a == 17 && b == 19) return 18;
            return -1;
        }
        static void SplitFinal(int t, out int first, out int second)
        {
            first = 0; second = t;
            if (t == 3) { first = 1; second = 19; }
            else if (t == 5 || t == 6) { first = 4; second = t == 5 ? 22 : 27; }
            else if (t >= 9 && t <= 15) { first = 8; second = new[] { 1, 16, 17, 19, 25, 26, 27 }[t - 9]; }
            else if (t == 18) { first = 17; second = 19; }
        }
        static void Flush(StringBuilder output, int l, int v, int t)
        {
            if (l >= 0 && v >= 0) output.Append((char)(0xac00 + (l * 21 + v) * 28 + t));
            else if (l >= 0) output.Append(Initials[l]);
            else if (v >= 0) output.Append(Vowels[v]);
        }
        public static string Compose(string keys)
        {
            var output = new StringBuilder(); int l = -1, v = -1, t = 0;
            foreach (char key in keys)
            {
                char j = Map(key); int nextL = Initials.IndexOf(j), nextV = Vowels.IndexOf(j);
                if (nextL >= 0)
                {
                    int nextT = Finals.IndexOf(j);
                    if (l >= 0 && v >= 0 && nextT > 0)
                    {
                        if (t == 0) { t = nextT; continue; }
                        int joined = JoinFinal(t, nextT);
                        if (joined >= 0) { t = joined; continue; }
                    }
                    Flush(output, l, v, t); l = nextL; v = -1; t = 0;
                }
                else if (nextV >= 0)
                {
                    if (v < 0) v = nextV;
                    else if (t > 0)
                    {
                        int first, second; SplitFinal(t, out first, out second);
                        Flush(output, l, v, first); l = Initials.IndexOf(Finals[second]); v = nextV; t = 0;
                    }
                    else
                    {
                        int joined = JoinVowel(v, nextV);
                        if (joined >= 0) v = joined;
                        else { Flush(output, l, v, t); l = -1; v = nextV; t = 0; }
                    }
                }
                else { Flush(output, l, v, t); l = v = -1; t = 0; output.Append(key); }
            }
            Flush(output, l, v, t); return output.ToString();
        }
        public static string ToKeys(string text)
        {
            var output = new StringBuilder();
            foreach (char c in text)
            {
                if (c >= 0xac00 && c <= 0xd7a3)
                {
                    int cp = c - 0xac00;
                    output.Append(InitialKeys[cp / 588]).Append(VowelKeys[cp % 588 / 28]).Append(FinalKeys[cp % 28]);
                }
                // Modern conjoining jamo (Unicode NFD). Do not normalize unrelated scripts.
                else if (c >= 0x1100 && c <= 0x1112) output.Append(InitialKeys[c - 0x1100]);
                else if (c >= 0x1161 && c <= 0x1175) output.Append(VowelKeys[c - 0x1161]);
                else if (c >= 0x11a8 && c <= 0x11c2) output.Append(FinalKeys[c - 0x11a7]);
                else
                {
                    int l = Initials.IndexOf(c), v = Vowels.IndexOf(c), t = Finals.IndexOf(c);
                    if (l >= 0) output.Append(InitialKeys[l]);
                    else if (v >= 0) output.Append(VowelKeys[v]);
                    else if (t > 0) output.Append(FinalKeys[t]);
                    else output.Append(c);
                }
            }
            return output.ToString();
        }
        // Explicit layout flip of text that was already typed (Shift+F2 on a selection): every Latin-letter run is
        // composed into Hangul and every Hangul run is decomposed into QWERTY keys. Digits, punctuation, line breaks and
        // other scripts stay. Flip(Flip(x)) == x for complete syllables and plain letters; shifted jamo keep their Shift letters.
        public static string Flip(string text)
        {
            if (string.IsNullOrEmpty(text)) return text ?? "";
            var output = new StringBuilder(text.Length); var run = new StringBuilder(); int kind = 0;
            foreach (char c in text)
            {
                int next = IsLatinLetter(c) ? 1 : IsHangulChar(c) ? 2 : 0;
                if (next != kind) { FlushRun(output, run, kind); kind = next; }
                if (next == 0) output.Append(c); else run.Append(c);
            }
            FlushRun(output, run, kind);
            return output.ToString();
        }
        static void FlushRun(StringBuilder output, StringBuilder run, int kind)
        {
            if (run.Length == 0) return;
            output.Append(kind == 1 ? Compose(run.ToString()) : ToKeys(run.ToString()));
            run.Length = 0;
        }
        static bool IsLatinLetter(char c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'); }
        static bool IsHangulChar(char c) { return (c >= 0xac00 && c <= 0xd7a3) || (c >= 0x3131 && c <= 0x318e) || (c >= 0x1100 && c <= 0x11ff); }
    }

    public enum ConfidenceLevel { Uncertain, Pattern, Known, Personal, Explicit }
    public sealed class Decision
    {
        public readonly string Text, Alternative, Reason;
        public readonly bool IsKorean;
        public readonly ConfidenceLevel Confidence;
        public bool NeedsConfirmation { get { return Confidence == ConfidenceLevel.Uncertain; } }
        public Decision(string text, string alternative, bool korean, string reason, ConfidenceLevel confidence = ConfidenceLevel.Known)
        { Text = text; Alternative = alternative; IsKorean = korean; Reason = reason; Confidence = confidence; }
    }
    public sealed class LearnedWord
    {
        public string Raw { get; set; }
        public string Keys { get; set; }
        public bool Korean { get; set; }
        public long Sequence { get; set; }
        public string Context { get; set; }
        public double KoreanWeight { get; set; }
        public double EnglishWeight { get; set; }
    }
    // Explicit, confirmed choices only. No filesystem work is allowed in this class.
    public sealed class WordPreferences
    {
        public const int Capacity = 1024;
        const string Edges = ".,!?;:()[]{}\"'";
        readonly object sync = new object();
        readonly Dictionary<string, LearnedWord> words = new Dictionary<string, LearnedWord>(StringComparer.Ordinal);
        sealed class Change { internal string Key; internal LearnedWord Previous, Evicted; }
        readonly List<List<Change>> undo = new List<List<Change>>();
        long sequence; int version; bool enabled = true;
        public int Version { get { lock (sync) return version; } }
        public int Count { get { lock (sync) return words.Count; } }
        public bool CanUndo { get { lock (sync) return undo.Count > 0; } }
        public bool Enabled
        {
            get { lock (sync) return enabled; }
            set { lock (sync) { if (enabled != value) { enabled = value; version++; } } }
        }
        static bool Normalize(string raw, string keys, out string identity, out string body, out string keyBody)
        {
            identity = body = keyBody = null;
            if (string.IsNullOrEmpty(raw) || keys == null || raw.Length != keys.Length) return false;
            int start = 0, end = raw.Length;
            while (start < end && Edges.IndexOf(raw[start]) >= 0) start++;
            while (end > start && Edges.IndexOf(raw[end - 1]) >= 0) end--;
            if (end == start || end - start > 64) return false;
            for (int i = start; i < end; i++)
                if (!Letter(raw[i]) || !Letter(keys[i])) return false;
            body = raw.Substring(start, end - start); keyBody = keys.Substring(start, end - start);
            identity = body + "\t" + keyBody; return true;
        }
        static bool Letter(char c) { return c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z'; }
        // Only the preceding word committed by this app is eligible. Never inspect surrounding UI text.
        public static string ContextKey(string text)
        {
            if (string.IsNullOrEmpty(text) || text.Length > 64 || text.Any(c => !Letter(c) && !(c >= 0xac00 && c <= 0xd7a3))) return "";
            using (var hash = SHA256.Create())
                return BitConverter.ToString(hash.ComputeHash(Encoding.UTF8.GetBytes(text))).Replace("-", "").ToLowerInvariant();
        }
        static bool ValidContext(string value)
        { return string.IsNullOrEmpty(value) || value.Length == 64 && value.All(c => c >= '0' && c <= '9' || c >= 'a' && c <= 'f'); }
        static string Identity(LearnedWord word) { return word.Raw + "\t" + word.Keys + "\t" + (word.Context ?? ""); }
        static LearnedWord Copy(LearnedWord word)
        { return new LearnedWord { Raw = word.Raw, Keys = word.Keys, Korean = word.Korean, Sequence = word.Sequence,
            Context = word.Context ?? "", KoreanWeight = word.KoreanWeight, EnglishWeight = word.EnglishWeight }; }
        public bool TryGet(string raw, string keys, out bool korean)
        { bool contextMatch, conflict; return TryGet(raw, keys, "", out korean, out contextMatch, out conflict); }
        public bool TryGet(string raw, string keys, string context, out bool korean, out bool contextMatch, out bool conflict)
        {
            korean = contextMatch = conflict = false; string identity, body, keyBody;
            if (!Normalize(raw, keys, out identity, out body, out keyBody)) return false;
            lock (sync)
            {
                LearnedWord value;
                if (!enabled) return false;
                if (!string.IsNullOrEmpty(context) && words.TryGetValue(identity + "\t" + context, out value))
                { korean = value.Korean; contextMatch = true; return true; }
                if (!words.TryGetValue(identity + "\t", out value)) return false;
                // Smoothed, exponentially discounted evidence; this is a selection score, not measured accuracy.
                double score = (value.KoreanWeight + 0.5) / (value.KoreanWeight + value.EnglishWeight + 1.0);
                if (score >= 0.72 || score <= 0.28) { korean = score >= 0.72; return true; }
                conflict = true; return false;
            }
        }
        public bool Remember(string raw, string keys, bool korean)
        { return Remember(raw, keys, korean, ""); }
        public bool Remember(string raw, string keys, bool korean, string context)
        {
            string identity, body, keyBody;
            if (!Normalize(raw, keys, out identity, out body, out keyBody) || !ValidContext(context)) return false;
            lock (sync)
            {
                if (!enabled) return false;
                var record = new List<Change>();
                Update(body, keyBody, korean, "", record);
                if (!string.IsNullOrEmpty(context)) Update(body, keyBody, korean, context, record);
                undo.Add(record); if (undo.Count > 20) undo.RemoveAt(0);
                version++; return true;
            }
        }
        void Update(string raw, string keys, bool korean, string context, List<Change> changes)
        {
            string identity = raw + "\t" + keys + "\t" + context;
            LearnedWord previous; words.TryGetValue(identity, out previous);
            var change = new Change { Key = identity, Previous = previous };
            if (previous == null && words.Count >= Capacity)
            {
                var oldest = words.OrderBy(pair => pair.Value.Sequence).First();
                change.Evicted = oldest.Value; words.Remove(oldest.Key);
            }
            words[identity] = new LearnedWord { Raw = raw, Keys = keys, Korean = korean, Context = context, Sequence = ++sequence,
                KoreanWeight = (previous == null ? 0 : previous.KoreanWeight * 0.85) + (korean ? 1 : 0),
                EnglishWeight = (previous == null ? 0 : previous.EnglishWeight * 0.85) + (korean ? 0 : 1) };
            changes.Add(change);
        }
        public bool UndoLast()
        {
            lock (sync)
            {
                if (undo.Count == 0) return false;
                var record = undo[undo.Count - 1]; undo.RemoveAt(undo.Count - 1);
                for (int i = record.Count - 1; i >= 0; i--)
                {
                    var change = record[i];
                    if (change.Previous == null) words.Remove(change.Key); else words[change.Key] = change.Previous;
                    if (change.Evicted != null) words[Identity(change.Evicted)] = change.Evicted;
                }
                version++; return true;
            }
        }
        public void Clear() { lock (sync) { words.Clear(); undo.Clear(); version++; } }
        public LearnedWord[] Export(out int revision)
        { lock (sync) { revision = version; return words.Values.OrderBy(word => word.Sequence).Select(Copy).ToArray(); } }
        public void Restore(IEnumerable<LearnedWord> entries)
        {
            lock (sync)
            {
                words.Clear(); undo.Clear(); sequence = 0;
                foreach (var entry in (entries ?? new LearnedWord[0]).Where(word => word != null).OrderByDescending(word => word.Sequence).Take(Capacity).OrderBy(word => word.Sequence))
                {
                    string identity, body, keyBody;
                    if (!Normalize(entry.Raw, entry.Keys, out identity, out body, out keyBody) || !ValidContext(entry.Context) ||
                        double.IsNaN(entry.KoreanWeight) || double.IsInfinity(entry.KoreanWeight) || entry.KoreanWeight < 0 ||
                        double.IsNaN(entry.EnglishWeight) || double.IsInfinity(entry.EnglishWeight) || entry.EnglishWeight < 0) continue;
                    var value = Copy(entry); value.Raw = body; value.Keys = keyBody; value.Sequence = ++sequence;
                    value.KoreanWeight = Math.Min(16, value.KoreanWeight); value.EnglishWeight = Math.Min(16, value.EnglishWeight);
                    // Format 1 entries have only the last choice, which migrates as one explicit observation.
                    if (value.KoreanWeight + value.EnglishWeight == 0)
                    { value.KoreanWeight = value.Korean ? 1 : 0; value.EnglishWeight = value.Korean ? 0 : 1; }
                    words[Identity(value)] = value;
                }
                version++;
            }
        }
    }
    public sealed class LanguageVerdict
    {
        public readonly double Llr;
        public readonly bool IsKorean, IsEnglish, Lexical, Nudged;
        public readonly string Composed;
        public bool Uncertain { get { return !IsKorean && !IsEnglish; } }
        internal LanguageVerdict(double llr, bool korean, bool english, bool lexical, bool nudged, string composed)
        { Llr = llr; IsKorean = korean; IsEnglish = english; Lexical = lexical; Nudged = nudged; Composed = composed; }
    }
    // Two character-statistics models score the same physical key sequence: a Korean syllable bigram model
    // over the composed Hangul and an English letter bigram model over the literal letters. The log-likelihood
    // ratio decides; a band around zero abstains. Tables hold aggregate counts only (no words, no sentences).
    // Reference implementation and evaluation: tests/lm/detector.py. Thresholds are design constants.
    public sealed class LanguageScorer
    {
        public const double DefaultKoreanThreshold = 2.0, DefaultEnglishThreshold = 2.0, LexiconBonus = 1.5, ContinuityPrior = 1.5;
        // Sensitivity presets move only the abstention band. Measured on held-out 4,000 + 4,000 words of the public-corpus tables (2026-10-09):
        // conservative T=3.0 → Korean 3,859 converted · English wrong 1; balanced T=2.0 → 3,927 · 2; aggressive T=1.5 → 3,958 · 2.
        double koreanThreshold = DefaultKoreanThreshold, englishThreshold = DefaultEnglishThreshold;
        public double KoreanThreshold { get { return koreanThreshold; } }
        public double EnglishThreshold { get { return englishThreshold; } }
        public void Configure(double korean, double english)
        { koreanThreshold = Math.Max(0.5, korean); englishThreshold = Math.Max(0.5, english); }
        public static string PresetThresholds(string preset, out double korean, out double english)
        {
            string name = (preset ?? "").Trim().ToLowerInvariant();
            if (name == "conservative") { korean = english = 3.0; return name; }
            if (name == "aggressive") { korean = english = 1.5; return name; }
            korean = DefaultKoreanThreshold; english = DefaultEnglishThreshold; return "balanced";
        }
        const double StrayJamoPenalty = 1.0, BoundaryBackoff = -1.0969, DefaultBackoff = -0.30103, UnknownLetter = -4.0;
        readonly Dictionary<char, double> unigram = new Dictionary<char, double>(), backoff = new Dictionary<char, double>();
        readonly Dictionary<int, double> bigram = new Dictionary<int, double>();
        readonly double[,] english = new double[27, 27];
        readonly Func<string, bool> lexicon;
        double floor = -8.268;
        public int BigramCount { get { return bigram.Count; } }
        public LanguageScorer(Func<string, bool> lexicon)
        {
            this.lexicon = lexicon;
            Assembly assembly = Assembly.GetExecutingAssembly();
            using (Stream stream = assembly.GetManifestResourceStream("Hanautomata.KoreanLM.txt.gz"))
            {
                if (stream == null) throw new InvalidOperationException("Korean language model missing.");
                using (var gzip = new GZipStream(stream, CompressionMode.Decompress))
                using (var reader = new StreamReader(gzip, Encoding.UTF8))
                {
                    string line;
                    while ((line = reader.ReadLine()) != null)
                    {
                        if (line.StartsWith("#floor ", StringComparison.Ordinal)) { floor = Parse(line.Substring(7)); continue; }
                        if (line.Length < 4 || line[0] == '#') continue;
                        string[] parts = line.Split(' ');
                        if (line[0] == 'U' && parts.Length == 4 && parts[1].Length == 1)
                        { unigram[parts[1][0]] = Parse(parts[2]); backoff[parts[1][0]] = Parse(parts[3]); }
                        else if (line[0] == 'B' && parts.Length == 3 && parts[1].Length == 2)
                            bigram[Pair(parts[1][0], parts[1][1])] = Parse(parts[2]);
                    }
                }
            }
            using (Stream stream = assembly.GetManifestResourceStream("Hanautomata.EnglishLM.txt"))
            {
                if (stream == null) throw new InvalidOperationException("English language model missing.");
                using (var reader = new StreamReader(stream, Encoding.UTF8))
                {
                    string line;
                    while ((line = reader.ReadLine()) != null)
                    {
                        if (line.Length == 0 || line[0] == '#') continue;
                        string[] parts = line.Split(' ');
                        int row = Row(parts[0][0]);
                        if (row < 0 || parts.Length != 28) continue;
                        for (int i = 0; i < 27; i++) english[row, i] = Parse(parts[i + 1]);
                    }
                }
            }
            if (unigram.Count == 0 || bigram.Count == 0) throw new InvalidOperationException("Korean language model empty.");
        }
        static int Pair(char a, char b) { return (a << 16) | b; }
        static int Row(char c) { return c == '^' ? 0 : c >= 'a' && c <= 'z' ? c - 'a' + 1 : -1; }
        static double Parse(string value) { return double.Parse(value, CultureInfo.InvariantCulture); }
        static bool IsSyllable(char c) { return c >= 0xac00 && c <= 0xd7a3; }
        double Transition(char previous, char current)
        {
            double value;
            if (bigram.TryGetValue(Pair(previous, current), out value)) return value;
            double weight; if (!backoff.TryGetValue(previous, out weight)) weight = DefaultBackoff;
            double single; if (!unigram.TryGetValue(current, out single)) single = current == '$' ? BoundaryBackoff : floor;
            return weight + single;
        }
        // log10 P_ko of the Hangul composed from the keys. Stray jamo (keys that never formed a syllable) are worse than a rare syllable.
        public double Korean(string keys, out string composed, out int stray)
        {
            composed = Hangul.Compose(keys); stray = 0; double total = 0; char previous = '^';
            foreach (char c in composed)
            {
                if (!IsSyllable(c)) { stray++; total += floor - StrayJamoPenalty; previous = c; continue; }
                total += Transition(previous, c); previous = c;
            }
            return total + Transition(previous, '$');
        }
        // log10 P_en of the literal letters under a case-insensitive letter bigram model with word boundaries.
        public double English(string raw)
        {
            double total = 0; int previous = 0;
            foreach (char c in raw)
            {
                char lower = char.ToLowerInvariant(c);
                int column = lower >= 'a' && lower <= 'z' ? lower - 'a' : -1;
                if (column < 0) { total += UnknownLetter; previous = 0; continue; }
                total += english[previous, column]; previous = column + 1;
            }
            return total + english[previous, 26];
        }
        public LanguageVerdict Score(string raw, string keys, bool? previousKorean)
        {
            string composed; int stray;
            double korean = Korean(keys, out composed, out stray);
            double letters = English(raw);
            bool lexical = lexicon != null && lexicon(raw);
            if (lexical) letters += LexiconBonus;
            double llr = korean - letters;
            if (raw.Length >= 2 && raw == keys && AllUpper(raw)) return new LanguageVerdict(llr, false, true, lexical, false, composed);
            double nudged = llr; bool nudgedFlag = false;
            if (previousKorean.HasValue && llr > -EnglishThreshold && llr < KoreanThreshold)
            { nudged = llr + (previousKorean.Value ? ContinuityPrior : -ContinuityPrior); nudgedFlag = true; }
            if (nudged >= KoreanThreshold) return new LanguageVerdict(llr, true, false, lexical, nudgedFlag, composed);
            if (nudged <= -EnglishThreshold) return new LanguageVerdict(llr, false, true, lexical, nudgedFlag, composed);
            return new LanguageVerdict(llr, false, false, lexical, false, composed);
        }
        static bool AllUpper(string value) { foreach (char c in value) if (!char.IsUpper(c)) return false; return true; }
    }
    public sealed class Detector
    {
        public readonly WordPreferences Preferences;
        readonly HashSet<string> english = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        readonly string profilePath, profileKeys;
        readonly Dictionary<string, string> particles = new Dictionary<string, string>();
        readonly HashSet<string> morphology = new HashSet<string>(StringComparer.Ordinal);
        readonly Lexicon englishTrie = new Lexicon(), koreanTrie = new Lexicon(), particleTrie = new Lexicon();
        readonly LanguageScorer scorer;
        public LanguageScorer Scorer { get { return scorer; } }
        // Word lists from a public corpus slice disjoint from the LM tables. They act only inside the scorer's abstention band:
        // a listed Korean word with the Korean side ahead by WordListMargin converts; a listed English word stays English without a prompt.
        readonly HashSet<string> koreanWords = new HashSet<string>(StringComparer.Ordinal), englishWords = new HashSet<string>(StringComparer.Ordinal);
        internal const double WordListMargin = 1.0;
        public int KoreanWordCount { get { return koreanWords.Count; } }
        public int EnglishWordCount { get { return englishWords.Count; } }
        static readonly Regex TokenParts = new Regex("[A-Za-z]+|[^A-Za-z]+", RegexOptions.Compiled);
        const string EdgePunctuation = ".,!?;:()[]{}\"'";
        readonly HashSet<string> korean = new HashSet<string>((
            "안녕 회의 오늘 내일 어제 지금 다음 이전 먼저 다시 네 아니요 확인 부탁 감사합니다 반갑습니다 안녕하세요 뛰다 " +
            "좋아요 좋아요 고마워 고맙습니다 미안 죄송합니다 수고하세요 잘했어요 있어요 없어요 해주세요 해줘 됩니다 합니다 " +
            "개발 작업 일정 결과 보고 자료 문서 코드 수정 검토 요청 응답 질문 답변 메일 전화 카톡 문자 연락 방법 진행 " +
            "완료 준비 공유 저장 삭제 추가 변경 설정 실행 중지 시작 종료 설치 제거 입력 출력 자동 수동 한글 영문 영어 " +
            "한국 한국어 서울 부산 대구 인천 대전 광주 사람 이름 시간 날짜 오전 오후 점심 저녁 아침 주말 다음주 이번주 " +
            "파일 폴더 화면 버튼 메뉴 창 상태 모드 변환 오류 문제 해결 원인 테스트 검증 기능 시스템 프로젝트 데이터 " +
            "안전 성공 실패 버전 배포 사용자 관리자 권한 계정 로그인 비밀번호 보안 정책 제품 고객 회사 업무 내용 제목 " +
            "안건 회의록 정리 분석 연구 조사 리서치 디자인 기획 계획 목표 범위 위험 이슈 비용 품질 성능 용량 위치 경로 " +
            "검색 선택 복사 붙여넣기 잘못 맞습니다 맞아요 그렇죠 그렇습니다 괜찮아요 어떻게 왜 무엇 어디 언제 누구 " +
            "우리 저희 저는 나는 제가 우리가 나의 이것 그것 저것 여기 거기 모든 다른 같은 많은 적은 새로운 중요한 필요한 " +
            "그리고 하지만 그래서 따라서 또는 또한 아직 이미 항상 자주 가끔 정말 너무 매우 모두 함께 각각 특히 바로 " +
            "부탁드립니다 확인했습니다 전달드립니다 보내주세요 알려주세요 확인해주세요 검토해주세요 진행해주세요 " +
            "윈도우 맥 컴퓨터 키보드 오토마타 입력기 트레이 온톨로지 실록 기술 모델 교육 일정표 회의실 자료실 " +
            // Hand-curated common/workplace vocabulary added after the user-sample audit.
            // This is sample-informed coverage, not an independently measured language model.
            "위의 우선 아래 현황 하자 이에 전략 을 원가 렌즈 수준 판가 저하 으로 상기 를 한 이를 대응 요망 와 " +
            "개선 외에 점프 필요 요구 것은 안됨 커버 중점 항목 강건 관리 의한 것을 도구 적용 의 보강 중국 동향 구글 " +
            "세션 흐름 우유 런칭 동사 명사 하기 않기 화상 반사 달성 면적 수율 불량 실험 소재 기구 단품 과제 측정 방안 " +
            "수립 금형 높이 치수 신규 협의 한다 부품 확보 구동 현상 파악 관련 통해 유형 분류 가설 이상 공정 검출 광학 " +
            "왜곡 설계 조도 양산 시험 대한 점 시 개 명 년 월 일 번 장 곳 수 차 건 등 중 후 전 및 로 에 은 는 이 가 과 의도 " +
            "핵심 지능화 자동화 최적화 신뢰성 구동력 요구사항 산출물 워크숍 알고리즘").Split(' '));
        public Detector() : this(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile)) { }
        public Detector(string userProfile) : this(userProfile, new WordPreferences()) { }
        public Detector(WordPreferences preferences) : this(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), preferences) { }
        public Detector(string userProfile, WordPreferences preferences)
        {
            Preferences = preferences ?? new WordPreferences();
            profilePath = (userProfile ?? "").TrimEnd('\\', '/'); profileKeys = Hangul.ToKeys(profilePath);
            foreach (string particle in "으로부터 으로 에서는 에서 에게 에도 의 를 을 은 는 이 가 와 과 로 에 만 도 보다 처럼".Split(' '))
                particles[Hangul.ToKeys(particle)] = particle;
            using (var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("Hanautomata.English.txt"))
            {
                if (stream == null) throw new InvalidOperationException("English lexicon missing.");
                using (var reader = new StreamReader(stream))
                    foreach (string word in reader.ReadToEnd().Split((char[])null, StringSplitOptions.RemoveEmptyEntries)) english.Add(word);
            }
            scorer = new LanguageScorer(IsEnglish);
            LoadWordList("Hanautomata.KoreanWords.txt.gz", koreanWords);
            LoadWordList("Hanautomata.EnglishWords.txt.gz", englishWords);
            BuildLexicons();
        }
        // Optional resources: a build without them keeps the statistical band as is (fail-open for text, never for safety).
        static void LoadWordList(string resource, HashSet<string> target)
        {
            using (Stream stream = Assembly.GetExecutingAssembly().GetManifestResourceStream(resource))
            {
                if (stream == null) return;
                using (var gzip = new GZipStream(stream, CompressionMode.Decompress))
                using (var reader = new StreamReader(gzip, Encoding.UTF8))
                {
                    string line;
                    while ((line = reader.ReadLine()) != null)
                    { if (line.Length == 0 || line[0] == '#') continue; target.Add(line.Trim()); }
                }
            }
        }
        // Small, typed surface-form rules, not a full Korean morphological analyzer.
        void BuildLexicons()
        {
            const string nouns = "회의 작업 일정 결과 보고 자료 문서 코드 메일 전화 방법 개발 입력 출력 기능 설정 " +
                "제품 고객 회사 업무 내용 제목 안건 회의록 정리 분석 연구 조사 기획 계획 목표 범위 위험 이슈 비용 품질 " +
                "성능 용량 위치 경로 검색 선택 변환 오류 문제 해결 원인 테스트 검증 기술 모델 교육 시스템 프로젝트 데이터 " +
                "전략 원가 렌즈 수준 판가 요구 중점 항목 관리 도구 적용 설계 공정 수율 불량 실험 소재 기구 과제 측정 방안 " +
                "수립 금형 높이 치수 부품 구동 현상 파악 가설 확보 개선 핵심 지능화 자동화 최적화 신뢰성 구동력 요구사항 산출물 알고리즘";
            const string hadaNouns = "개발 수정 검토 요청 진행 완료 준비 공유 저장 삭제 추가 변경 설정 실행 중지 시작 종료 " +
                "설치 제거 입력 출력 변환 해결 검증 정리 분석 연구 조사 기획 계획 검색 선택 복사 개선 관리 적용 실험 측정 " +
                "수립 협의 확보 파악 분류 설계 확인 자동화 최적화";
            foreach (string noun in nouns.Split(' '))
                foreach (string particle in particles.Values)
                    if (ParticleFits(noun, particle)) morphology.Add(noun + particle);
            foreach (string stem in hadaNouns.Split(' '))
                foreach (string ending in "하다 하는 하고 하면 하며 합니다 했습니다 해주세요 해줘 해요 했어요 해도 하지 하겠습니다 했고 했다 하였다 합니다만 하도록 하기".Split(' '))
                    morphology.Add(stem + ending);
            foreach (string word in korean)
                if (word.Length >= 2) koreanTrie.Add(Hangul.ToKeys(word), word, 10);
            foreach (string word in morphology) koreanTrie.Add(Hangul.ToKeys(word), word, 13);
            foreach (var pair in particles) particleTrie.Add(pair.Key, pair.Value, 8);
            foreach (string word in english)
            {
                string lower = word.ToLowerInvariant(); englishTrie.Add(lower, lower, 10);
                foreach (string suffix in new[] { "s", "es", "ed", "ing", "ly" })
                {
                    if (lower.Length > 2) englishTrie.Add(lower + suffix, lower + suffix, 10);
                    if (lower.Length > 3 && lower.EndsWith("e", StringComparison.Ordinal))
                    {
                        string stem = lower.Substring(0, lower.Length - 1);
                        englishTrie.Add(stem + suffix, stem + suffix, 10);
                    }
                }
            }
        }
        static bool ParticleFits(string noun, string particle)
        {
            int final = (noun[noun.Length - 1] - 0xac00) % 28;
            if (particle == "은" || particle == "이" || particle == "을" || particle == "과") return final != 0;
            if (particle == "는" || particle == "가" || particle == "를" || particle == "와") return final == 0;
            if (particle == "으로" || particle == "으로부터") return final != 0 && final != 8;
            if (particle == "로") return final == 0 || final == 8;
            return true;
        }
        bool IsEnglish(string word)
        {
            if (english.Contains(word)) return true;
            string lower = word.ToLowerInvariant();
            foreach (string suffix in new[] { "s", "es", "ed", "ing", "ly" })
                if (lower.Length > suffix.Length + 2 && lower.EndsWith(suffix, StringComparison.Ordinal))
                {
                    string stem = lower.Substring(0, lower.Length - suffix.Length);
                    if (english.Contains(stem) || english.Contains(stem + "e")) return true;
                }
            return false;
        }
        bool IsEnglishIdentifier(string word)
        {
            // Require known English components; Shift also produces Korean double consonants.
            int start = 0, parts = 0;
            for (int i = 1; i < word.Length; i++)
            {
                bool boundary = char.IsUpper(word[i]) && (char.IsLower(word[i - 1]) ||
                    (i + 1 < word.Length && char.IsUpper(word[i - 1]) && char.IsLower(word[i + 1])));
                if (!boundary) continue;
                if (!IsEnglish(word.Substring(start, i - start))) return false;
                start = i; parts++;
            }
            return parts > 0 && IsEnglish(word.Substring(start));
        }
        static bool IsChatJamo(char c) { return c == 'ㅋ' || c == 'ㅎ' || c == 'ㅠ' || c == 'ㅜ'; }
        bool IsKoreanChat(string word, string raw, string keys)
        {
            int prefixLength = word.Length;
            while (prefixLength > 0 && IsChatJamo(word[prefixLength - 1])) prefixLength--;
            if (prefixLength == word.Length) return false;
            if (prefixLength == 0) return word.Length >= 2;
            if (korean.Contains(word.Substring(0, prefixLength))) return true;
            // Every trailing chat jamo came from exactly one key, so the body's keys are the remaining prefix.
            int keyLength = keys.Length - (word.Length - prefixLength);
            return keyLength > 0 && keyLength <= raw.Length && scorer.Score(raw.Substring(0, keyLength), keys.Substring(0, keyLength), null).IsKorean;
        }
        public Decision Decide(string raw) { return Decide(raw, raw); }
        public Decision Decide(string raw, string hangulKeys)
        { return Decide(raw, hangulKeys, ""); }
        public Decision Decide(string raw, string hangulKeys, string context) { return Decide(raw, hangulKeys, context, null); }
        // previousKorean: language of the word this app committed just before; it only breaks statistical near-ties.
        public Decision Decide(string raw, string hangulKeys, string context, bool? previousKorean)
        {
            Decision automatic = DecideDefault(raw, hangulKeys, previousKorean); bool korean, contextMatch, conflict;
            if (!Preferences.TryGet(raw, hangulKeys, context, out korean, out contextMatch, out conflict) || automatic.Text == automatic.Alternative)
                return conflict ? new Decision(automatic.Text, automatic.Alternative, automatic.IsKorean, "선택 이력 충돌 · " + automatic.Reason, ConfidenceLevel.Uncertain) : automatic;
            bool same = korean == automatic.IsKorean;
            return new Decision(same ? automatic.Text : automatic.Alternative, same ? automatic.Alternative : automatic.Text, korean,
                contextMatch ? "앞 단어에 맞춘 개인 선택" : "기억한 개인 선택", ConfidenceLevel.Personal);
        }
        Decision DecideDefault(string raw, string hangulKeys, bool? previousKorean)
        {
            // Restore only the exact OS-provided home-directory alias; never guess path names.
            if (profilePath.Length > 0 && profilePath != profileKeys && raw.StartsWith(profileKeys, StringComparison.OrdinalIgnoreCase) &&
                (raw.Length == profileKeys.Length || raw[profileKeys.Length] == '\\' || raw[profileKeys.Length] == '/'))
                return new Decision(profilePath + raw.Substring(profileKeys.Length), raw, true, "현재 사용자 홈 경로 복원");
            string body = raw.Trim(EdgePunctuation.ToCharArray());
            if (body.IndexOf('\\') >= 0 || body.StartsWith("/", StringComparison.Ordinal) ||
                (body.Length >= 2 && char.IsLetter(body[0]) && body[1] == ':') ||
                body.IndexOf('@') >= 0 || body.IndexOf('_') >= 0 || body.IndexOf('.') >= 0 || body.Contains("://"))
                return new Decision(raw, Hangul.Compose(hangulKeys), false, "주소·파일명·식별자 유지");
            var output = new StringBuilder(); var alternative = new StringBuilder();
            bool changed = false; string reason = "기호 유지";
            ConfidenceLevel confidence = ConfidenceLevel.Known;
            foreach (Match part in TokenParts.Matches(raw))
            {
                if (!IsAsciiLetter(part.Value[0])) { output.Append(part.Value); alternative.Append(part.Value); continue; }
                Decision decision = DecideAtom(part.Value, hangulKeys.Substring(part.Index, part.Length), previousKorean);
                output.Append(decision.Text);
                alternative.Append(decision.Alternative);
                if (decision.Confidence < confidence) confidence = decision.Confidence;
                if (decision.IsKorean) changed = true;
                reason = decision.Reason;
            }
            return new Decision(output.ToString(), changed ? raw : alternative.ToString(), changed, reason, confidence);
        }
        static bool IsAsciiLetter(char c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'); }
        sealed class Edge
        {
            internal int End, Language, Cost;
            internal string Text;
            internal bool Particle;
        }
        sealed class Lexicon
        {
            sealed class Node
            {
                internal readonly Dictionary<char, Node> Next = new Dictionary<char, Node>();
                internal string Text;
                internal int Cost;
            }
            readonly Node root = new Node();
            internal void Add(string keys, string text, int cost)
            {
                Node node = root;
                foreach (char c in keys)
                {
                    Node next;
                    if (!node.Next.TryGetValue(c, out next)) node.Next[c] = next = new Node();
                    node = next;
                }
                if (node.Text == null || cost < node.Cost) { node.Text = text; node.Cost = cost; }
            }
            internal void Match(string input, string raw, int start, int language, bool particle, List<Edge> edges)
            {
                Node node = root;
                for (int end = start; end < input.Length; end++)
                {
                    char c = input[end];
                    if (language == 0 || "REQTWOP".IndexOf(c) < 0) c = char.ToLowerInvariant(c);
                    if (!node.Next.TryGetValue(c, out node)) break;
                    if (node.Text != null && end - start >= 1)
                        edges.Add(new Edge { End = end + 1, Language = language, Cost = node.Cost, Particle = particle,
                            Text = language == 0 ? raw.Substring(start, end + 1 - start) : node.Text });
                }
            }
        }
        sealed class MixedPath
        {
            internal string Text;
            internal int Cost;
        }
        sealed class BestTwo
        {
            internal MixedPath First, Second;
            internal void Add(MixedPath item)
            {
                if (First != null && First.Text == item.Text)
                { if (item.Cost < First.Cost) First = item; return; }
                if (Second != null && Second.Text == item.Text)
                { if (item.Cost >= Second.Cost) return; Second = null; }
                if (First == null || item.Cost < First.Cost) { Second = First; First = item; }
                else if (Second == null || item.Cost < Second.Cost) Second = item;
            }
        }
        internal const int MixedKeyLimit = 64, MixedPartLimit = 8, MixedMargin = 3;
#if HANAUTOMATA_TESTS
        // Synthetic colliding lexemes exercise the decision contract without production data.
        internal void AddLexemeForTest(string surface, bool isKorean, int cost)
        {
            if (isKorean) koreanTrie.Add(Hangul.ToKeys(surface), surface, cost);
            else englishTrie.Add(surface.ToLowerInvariant(), surface, cost);
        }
#endif
        Decision MixedWord(string raw, string keys)
        {
            // A bounded DAG decoder. Keep two distinct surfaces per complete state, including
            // span count: pruning a longer state must not discard a shorter feasible completion.
            if (raw.Length > MixedKeyLimit || raw.Length < 4) return null;
            var paths = new BestTwo[raw.Length + 1, 2, 4, MixedPartLimit + 1];
            paths[0, 0, 0, 0] = new BestTwo(); paths[0, 0, 0, 0].Add(new MixedPath { Text = "", Cost = 0 });
            for (int start = 0; start < raw.Length; start++)
            {
                List<Edge> edges = null;
                for (int lang = 0; lang < 2; lang++) for (int mask = 0; mask < 4; mask++)
                    for (int parts = 0; parts < MixedPartLimit; parts++)
                    {
                        BestTwo previous = paths[start, lang, mask, parts];
                        if (previous == null) continue;
                        if (edges == null)
                        {
                            edges = new List<Edge>();
                            englishTrie.Match(raw, raw, start, 0, false, edges);
                            koreanTrie.Match(keys, raw, start, 1, false, edges);
                            particleTrie.Match(keys, raw, start, 1, true, edges);
                            // Known camel/PascalCase components retain their literal case.
                            for (int end = start + 3; end <= raw.Length; end++)
                            {
                                string literal = raw.Substring(start, end - start);
                                if (IsEnglishIdentifier(literal))
                                    edges.Add(new Edge { End = end, Language = 0, Cost = 12, Text = literal });
                            }
                        }
                        foreach (Edge edge in edges)
                        {
                            if (mask != 0 && edge.Language == lang || edge.Particle && (mask == 0 || lang != 0)) continue;
                            int nextMask = mask | (edge.Language == 0 ? 1 : 2);
                            BestTwo next = paths[edge.End, edge.Language, nextMask, parts + 1];
                            if (next == null) paths[edge.End, edge.Language, nextMask, parts + 1] = next = new BestTwo();
                            foreach (MixedPath path in new[] { previous.First, previous.Second })
                                if (path != null) next.Add(new MixedPath { Text = path.Text + edge.Text,
                                    Cost = path.Cost + edge.Cost + (parts == 0 ? 0 : 2) });
                        }
                    }
            }
            var winners = new BestTwo();
            for (int lang = 0; lang < 2; lang++) for (int parts = 2; parts <= MixedPartLimit; parts++)
            {
                BestTwo pair = paths[raw.Length, lang, 3, parts];
                if (pair == null) continue;
                if (pair.First != null) winners.Add(pair.First);
                if (pair.Second != null) winners.Add(pair.Second);
            }
            if (winners.First == null) return null;
            // These costs are hand-set ranking evidence, not calibrated probabilities.
            if (winners.Second != null && winners.Second.Cost - winners.First.Cost < MixedMargin)
                return new Decision(raw, winners.First.Text, false, "혼합어 후보 경합 · 확인 필요", ConfidenceLevel.Uncertain);
            return new Decision(winners.First.Text, raw, true, "어휘·활용형 비용으로 나눈 한영 혼합어");
        }
        Decision DecideAtom(string raw, string hangulKeys, bool? previousKorean)
        {
            string composed = Hangul.Compose(hangulKeys);
            string body = raw;
            string koBody = composed;
            bool koreanChoice = false; string reason = "모호한 입력 · 원문 유지"; Decision mixed;
            ConfidenceLevel confidence = ConfidenceLevel.Known;
            bool technical = body.Any(c => !char.IsLetter(c));
            if (body.Length == 0 || technical) reason = "기호·주소·식별자 유지";
            else if (IsEnglish(body)) reason = "영어 단어 유지";
            else if (IsEnglishIdentifier(body)) reason = "영어 식별자 유지";
            // Caps Lock changes literal case, but Korean keys retain the actual Shift state.
            // Keep deliberate Shift acronyms; do not suppress Korean merely because Caps Lock is on.
            else if (body.Length >= 2 && body.All(char.IsUpper) && raw == hangulKeys) reason = "영문 약어 유지";
            // Bare one-syllable words can be near-ties; let statistics and continuity decide them.
            else if (koBody.Length > 1 && korean.Contains(koBody)) { koreanChoice = true; reason = "자주 쓰는 한국어"; }
            else if (morphology.Contains(koBody)) { koreanChoice = true; reason = "한국어 조사·활용형"; }
            else if ((mixed = MixedWord(raw, hangulKeys)) != null)
                return mixed;
            else if (IsKoreanChat(koBody, raw, hangulKeys))
            { koreanChoice = true; reason = "한국어 채팅 표현"; }
            else
            {
                // Statistical fallback for everything the hand lists miss (most one- and two-syllable words).
                // The previous committed word's language only breaks near-ties; clear evidence is never overridden.
                // Model-based results are reported as a pattern estimate, not lexical certainty.
                LanguageVerdict verdict = scorer.Score(raw, hangulKeys, previousKorean);
                if (verdict.IsKorean)
                { koreanChoice = true; reason = verdict.Nudged ? "앞 단어 문맥 · 음절 통계" : "음절 통계"; confidence = ConfidenceLevel.Pattern; }
                else if (verdict.IsEnglish) reason = verdict.Nudged ? "앞 단어 문맥 · 영문 통계" : "영문 통계";
                // Inside the band, a real word tips the balance: listed Korean (2+ syllables, Korean side ahead) converts,
                // a listed English word (3+ letters) stays literal without asking. One-syllable words stay statistical.
                else if (koBody.Length >= 2 && koBody.All(c => c >= 0xac00 && c <= 0xd7a3) && verdict.Llr >= WordListMargin
                    && koreanWords.Contains(koBody) && !englishWords.Contains(body.ToLowerInvariant()))
                { koreanChoice = true; reason = "단어 목록 · 음절 통계"; confidence = ConfidenceLevel.Pattern; }
                else if (body.Length >= 3 && englishWords.Contains(body.ToLowerInvariant()) && !koreanWords.Contains(koBody))
                    reason = "영어 단어 목록";
                else confidence = ConfidenceLevel.Uncertain;
            }
            return new Decision(koreanChoice ? composed : raw, koreanChoice ? raw : composed, koreanChoice, reason, confidence);
        }
    }
    public sealed class Composition
    {
        readonly Detector detector;
        readonly StringBuilder literal = new StringBuilder(), keys = new StringBuilder();
        bool? forcedKorean;
        bool semicolonArmed; bool? beforeSemicolon;
        string context = "";
        bool? previousKorean;
        bool confirmed;
        public string Context { get { return context; } }
        // Language of the word this composition committed just before (null after Enter, cancel, navigation or symbols).
        public bool? PreviousKorean { get { return previousKorean; } }
        Decision cached; int cachedVersion;
        public Composition(Detector detector) { this.detector = detector; }
        public bool IsEmpty { get { return literal.Length == 0; } }
        public int Length { get { return literal.Length; } }
        public string Raw { get { return literal.ToString(); } }
        public string Keys { get { return keys.ToString(); } }
        public string Korean { get { var current = Current; return current.IsKorean ? current.Text : current.Alternative; } }
        public Decision Current
        {
            get
            {
                if (cached != null && cachedVersion == detector.Preferences.Version) return cached;
                cachedVersion = detector.Preferences.Version;
                Decision auto = detector.Decide(Raw, keys.ToString(), context, previousKorean);
                if (!forcedKorean.HasValue) return cached = auto;
                bool same = forcedKorean.Value == auto.IsKorean;
                return cached = new Decision(same ? auto.Text : auto.Alternative, same ? auto.Alternative : auto.Text, forcedKorean.Value, "직접 선택", ConfidenceLevel.Explicit);
            }
        }
        public void Append(char raw, char key) { semicolonArmed = false; literal.Append(raw); keys.Append(key); cached = null; }
        public void Semicolon()
        {
            if (semicolonArmed)
            {
                forcedKorean = beforeSemicolon; semicolonArmed = false;
                Append(';', ';'); return;
            }
            if (IsEmpty || Current.Text == Current.Alternative) { Append(';', ';'); return; }
            beforeSemicolon = forcedKorean; Toggle(); semicolonArmed = true;
        }
        // Called only after the input controller has accepted a commit into the checked target.
        public bool ConfirmSelection()
        {
            if (confirmed || !forcedKorean.HasValue || IsEmpty) return false;
            confirmed = true;
            return detector.Preferences.Remember(Raw, keys.ToString(), Current.IsKorean, context);
        }
        public void AcceptCommit(bool continueContext)
        {
            string committed = Current.Text;
            ConfirmSelection(); Clear(); ResetContext(committed, continueContext);
        }
        // Re-establish the preceding-word context after a commit or after the previous word was swapped.
        public void ResetContext(string committedText, bool continueContext)
        {
            context = continueContext ? WordPreferences.ContextKey(committedText) : "";
            previousKorean = continueContext ? LanguageOf(committedText) : null;
        }
        // Explicit choices made outside a live composition (for example F2 right after Space) still train only on confirmation.
        public bool RememberExplicit(string raw, string keys, bool korean, string context)
        { return detector.Preferences.Remember(raw, keys, korean, context ?? ""); }
        static bool? LanguageOf(string text)
        {
            bool hangul = false, latin = false;
            foreach (char c in text)
            {
                if (c >= 0xac00 && c <= 0xd7a3) hangul = true;
                else if (c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z') latin = true;
            }
            if (hangul == latin) return null;
            return hangul;
        }
        public void Backspace()
        {
            if (semicolonArmed) { forcedKorean = beforeSemicolon; semicolonArmed = false; cached = null; return; }
            if (IsEmpty) return;
            literal.Length--; keys.Length--; cached = null;
            if (IsEmpty) forcedKorean = null;
        }
        public void Toggle() { semicolonArmed = false; if (!IsEmpty) { forcedKorean = !Current.IsKorean; cached = null; } }
        public void Clear() { literal.Clear(); keys.Clear(); forcedKorean = null; semicolonArmed = false; beforeSemicolon = null; cached = null; context = ""; previousKorean = null; confirmed = false; }
    }
    public static class FocusPolicy
    {
        public static bool CanCapture(bool known, bool password, bool writable, bool elevated, bool focused, long ageMs, bool sameTarget)
        { return known && !password && writable && !elevated && focused && ageMs >= 0 && ageMs <= 300 && sameTarget; }
    }
}
