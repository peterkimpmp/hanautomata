using System;
using System.Globalization;
using System.IO;
using System.Reflection;
using System.Text;
using System.Threading;
using HanFlow;

static class CoreTests
{
    static int count, failures;
    static void Equal(string expected, string actual, string label)
    {
        count++;
        if (expected != actual) { failures++; Console.Error.WriteLine(label + ": expected [" + expected + "] actual [" + actual + "]"); }
    }
    static void True(bool actual, string label) { Equal("True", actual.ToString(), label); }
    static void Confirm(Composition value) { value.ConfirmSelection(); }
    static void Semicolon(Composition value) { value.Semicolon(); }
    static void Type(Composition value, string text) { foreach (char c in text) value.Append(c, c); }
    static void IntelligenceTests()
    {
        var words = new WordPreferences(); var detector = new Detector(words); var input = new Composition(detector);
        string koreanContext = WordPreferences.ContextKey("나는"), englishContext = WordPreferences.ContextKey("please");
        words.Remember("go", "go", true, koreanContext);
        words.Remember("go", "go", false, englishContext);
        Equal("해", detector.Decide("go", "go", koreanContext).Text, "Korean context keeps its selected meaning");
        Equal("go", detector.Decide("go", "go", englishContext).Text, "English context keeps its selected meaning");
        Equal("go", detector.Decide("go").Text, "conflicting global evidence does not spread a local choice");
        True(detector.Decide("go").NeedsConfirmation, "conflicting evidence is visibly uncertain");
        words.UndoLast();
        Equal("해", detector.Decide("go").Text, "one undo restores global and contextual learning together");
        True(words.Count == 2, "undo removes the English context as one transaction");
        words.Remember("go", "go", false, koreanContext);
        Equal("go", detector.Decide("go", "go", koreanContext).Text, "same context immediately honors the latest correction");
        words.Clear();
        for (int i = 0; i < 6; i++) words.Remember("go", "go", true);
        words.Remember("go", "go", false);
        Equal("해", detector.Decide("go").Text, "one contradictory event does not erase repeated global evidence");
        for (int i = 0; i < 8; i++) words.Remember("go", "go", false);
        Equal("go", detector.Decide("go").Text, "discounted history adapts to repeated new choices");
        words.Clear();
        Type(input, "sksms"); input.AcceptCommit(true);
        Equal(koreanContext, input.Context, "space establishes preceding committed word context");
        Type(input, "go"); input.Toggle(); True(input.ConfirmSelection(), "explicit contextual choice recorded");
        int revision = words.Version;
        True(!input.ConfirmSelection() && revision == words.Version, "a commit cannot count the same confirmation twice");
        input.AcceptCommit(true); input.Clear();
        Type(input, "sksms"); input.AcceptCommit(true); Type(input, "go");
        Equal("해", input.Current.Text, "composition uses its own preceding word");
        input.Clear(); Equal("", input.Context, "cancel and focus reset remove context");
        Type(input, "hello"); input.AcceptCommit(false); Equal("", input.Context, "navigation or Enter clears context");
        Type(input, "hello!"); input.AcceptCommit(true); Equal("", input.Context, "sentence punctuation clears context");
        words.Clear();
        for (int i = 0; i < 10; i++) { Type(input, "hello"); input.AcceptCommit(true); }
        Equal("0", words.Count.ToString(), "automatic output is never fed back as a training label");
        input.Clear();
        True(detector.Decide("rm").NeedsConfirmation, "near-tie short input requires a choice");
        True(!detector.Decide("world").NeedsConfirmation, "known English stays stable");
        Equal("Pattern", detector.Decide("dkssudgktlqslRk").Confidence.ToString(), "syllable heuristic is not claimed as lexical certainty");
        foreach (string value in new[] { "mail@example.com", "user_id", "hello!", "12", new string('a', 65) })
            Equal("", WordPreferences.ContextKey(value), "unsafe context is not retained");
        True(!words.Remember("go", "go", true, "plain text context"), "only bounded context fingerprints can be stored");
        words.Restore(new[] { new LearnedWord { Raw = "go", Keys = "go", Korean = true, KoreanWeight = double.NaN } });
        Equal("0", words.Count.ToString(), "invalid numeric model data is rejected");
        string directory = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "test-context-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);
        try
        {
            string path = Path.Combine(directory, "words.json");
            File.WriteAllText(path, "{\"Format\":1,\"Entries\":[{\"Raw\":\"gksms\",\"Keys\":\"gksms\",\"Korean\":true,\"Sequence\":1}]}");
            var migrated = new WordPreferences(); var store = new PreferenceStore(path, migrated);
            Equal("하는", new Detector(migrated).Decide("gksms").Text, "format 1 personal dictionary migrates without losing choices");
            migrated.Remember("go", "go", true, koreanContext);
            migrated.Remember("go", "go", false, englishContext);
            True(store.FlushNow(), "context and evidence persisted");
            True(File.ReadAllText(path).Contains("\"Format\":2"), "new saves use versioned format 2");
            True(!File.ReadAllText(path).Contains("나는") && !File.ReadAllText(path).Contains("please"), "context text is not stored in plaintext");
            var restored = new WordPreferences(); new PreferenceStore(path, restored); var restoredDetector = new Detector(restored);
            Equal("해", restoredDetector.Decide("go", "go", koreanContext).Text, "Korean context survives restart");
            Equal("go", restoredDetector.Decide("go", "go", englishContext).Text, "English context survives restart");
            Equal("하는", restoredDetector.Decide("gksms").Text, "legacy choice survives a version 2 save");
            restored.Enabled = false;
            Equal("go", restoredDetector.Decide("go", "go", koreanContext).Text, "learning off also disables contextual prediction");
            // Investigate the observed one-off reset failure with queued-save/reset interleavings.
            for (int i = 0; i < 40; i++)
            {
                migrated.Remember("go", "go", i % 2 == 0, koreanContext); store.ScheduleSave();
                migrated.Clear(); bool persisted = store.FlushNow(); True(persisted, "queued-save reset persists at iteration " + i + " " + store.LastFailureCode);
                var empty = new WordPreferences(); new PreferenceStore(path, empty);
                Equal("0", empty.Count.ToString(), "queued-save reset reloads empty at iteration " + i);
            }
        }
        finally { foreach (string file in Directory.GetFiles(directory)) File.Delete(file); Directory.Delete(directory); }
    }
    static void PreferenceTests()
    {
        var words = new WordPreferences(); var detector = new Detector(words); var input = new Composition(detector);
        // "rm" (그) is a statistical near-tie, so it stays literal until the user chooses; "gksms" now converts automatically.
        Type(input, "rm"); input.Toggle(); input.Clear();
        Equal("0", words.Count.ToString(), "cancelled manual choice is not learned");
        Type(input, "rm"); input.Toggle(); while (!input.IsEmpty) input.Backspace();
        Type(input, "hello"); input.ConfirmSelection(); input.Clear();
        Equal("0", words.Count.ToString(), "deleted choice cannot train the next word");
        Type(input, "rm"); input.Semicolon(); input.Backspace();
        Equal("rm", input.Current.Text, "backspace undoes pending semicolon toggle");
        True(!input.ConfirmSelection(), "undone semicolon is not learned"); input.Clear();
        Type(input, "hello"); input.Semicolon(); input.Semicolon();
        Equal("hello;", input.Current.Text, "literal semicolon restores previous choice");
        True(!input.ConfirmSelection(), "literal escape cannot train the dictionary"); input.Clear();
        input.Semicolon(); Equal(";", input.Current.Text, "semicolon without a word stays literal"); input.Clear();
        Type(input, "rm"); input.Semicolon(); input.ConfirmSelection(); input.Clear();
        Equal("그", detector.Decide("rm").Text, "learned choice used without another shortcut");
        Equal("(그!)", detector.Decide("(rm!)").Text, "surrounding punctuation preserved with learned word");
        Type(input, "rm"); input.Toggle(); input.ConfirmSelection(); input.Clear();
        Equal("rm", detector.Decide("rm").Text, "opposite confirmed choice replaces earlier preference");
        True(words.UndoLast(), "recent learning can be undone");
        Equal("그", detector.Decide("rm").Text, "undo restores earlier preference");
        words.Enabled = false;
        Equal("rm", detector.Decide("rm").Text, "disabled personal choices do not apply");
        True(!words.Remember("ej", "ej", true), "disabled learning cannot add entries");
        words.Enabled = true;
        Equal("그", detector.Decide("rm").Text, "re-enabled choices remain available");
        foreach (string text in new[] { "mail@example.com", "C:\\work\\file", "https://host/test", "api_key", "12345", new string('a', 65) })
            True(!words.Remember(text, text, true), "paths identifiers and oversized inputs are not learned");
        words.Remember("wbs", "wbs", true);
        Equal("쥰", detector.Decide("wbs").Text, "explicit choice can override a known English word");
        Equal("WBS", detector.Decide("WBS").Text, "case remains significant for learned keys");
        Type(input, "wbs"); Equal("쥰", input.Current.Text, "candidate cache initially learned");
        words.Clear(); Equal("wbs", input.Current.Text, "clearing learning invalidates candidate cache"); input.Clear();
        for (int i = 0; i <= WordPreferences.Capacity; i++)
        {
            string key = "term" + (char)('a' + i / 676) + (char)('a' + i / 26 % 26) + (char)('a' + i % 26);
            words.Remember(key, key, true);
        }
        Equal(WordPreferences.Capacity.ToString(), words.Count.ToString(), "learning capacity is bounded");
        bool choice;
        True(!words.TryGet("termaaa", "termaaa", out choice), "oldest entry is evicted at capacity");
        words.UndoLast(); True(words.TryGet("termaaa", "termaaa", out choice), "undo also restores evicted entry");
        words.Clear();
        string directory = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "test-learning-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);
        try
        {
            string path = Path.Combine(directory, "words.json");
            var store = new PreferenceStore(path, words);
            words.Remember("gksms", "gksms", true); store.ScheduleSave();
            words.Remember("ej", "ej", true); True(store.FlushNow(), "personal choices saved atomically");
            var reloaded = new WordPreferences(); new PreferenceStore(path, reloaded);
            Equal("하는", new Detector(reloaded).Decide("gksms").Text, "preference survives restart");
            Equal("더", new Detector(reloaded).Decide("ej").Text, "latest revision survives queued save");
            True(!File.Exists(path + ".tmp"), "no unfinished save file remains");
            words.Clear(); True(store.FlushNow(), "reset save succeeds: " + store.LastError);
            var cleared = new WordPreferences(); new PreferenceStore(path, cleared);
            Equal("0", cleared.Count.ToString(), "reset is persistent");
            File.WriteAllText(path, "not valid json");
            var invalidWords = new WordPreferences(); var invalid = new PreferenceStore(path, invalidWords);
            Equal("0", invalidWords.Count.ToString(), "malformed file falls back to empty preferences");
            True(invalid.LastError.Length > 0, "malformed file is reported"); invalid.FlushNow();
            Equal("not valid json", File.ReadAllText(path), "unchanged malformed file is not silently overwritten");
            string blocker = Path.Combine(directory, "blocked"); File.WriteAllText(blocker, "x");
            var unsavedWords = new WordPreferences(); var unavailable = new PreferenceStore(Path.Combine(blocker, "words.json"), unsavedWords);
            unsavedWords.Remember("gksms", "gksms", true);
            True(!unavailable.FlushNow(), "save failure is contained");
            Equal("하는", new Detector(unsavedWords).Decide("gksms").Text, "save failure keeps in-memory preference");
        }
        finally
        {
            foreach (string file in Directory.GetFiles(directory)) File.Delete(file);
            Directory.Delete(directory); // Non-recursive: only this freshly created test directory.
        }
    }
    static int ReadResourceRows(string name, Action<string[]> row)
    {
        int rows = 0;
        using (var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream(name))
        {
            if (stream == null) throw new InvalidOperationException("Test resource missing: " + name);
            using (var reader = new StreamReader(stream, Encoding.UTF8))
            {
                string line;
                while ((line = reader.ReadLine()) != null)
                {
                    if (line.Length == 0 || line[0] == '#') continue;
                    rows++; row(line.Split('\t'));
                }
            }
        }
        return rows;
    }
    static void ScorerTests()
    {
        var detector = new Detector(); var scorer = detector.Scorer;
        True(scorer.BigramCount > 10000, "Korean language model loaded (" + scorer.BigramCount + " bigrams)");
        True(detector.KoreanWordCount > 10000 && detector.EnglishWordCount > 10000, "word lists loaded (" + detector.KoreanWordCount + " Korean, " + detector.EnglishWordCount + " English)");
        // dirks/야간 (llr 1.97) sits in the band; the Korean word list converts it. gown/해주 (llr 0.94) is a listed English word and stays literal.
        Equal("야간", detector.Decide("dirks").Text, "listed Korean word inside the band converts");
        Equal("Pattern", detector.Decide("dirks").Confidence.ToString(), "word-list evidence is still a pattern estimate");
        Equal("gown", detector.Decide("gown").Text, "listed English word inside the band stays literal even though 해주 is also a listed Korean word");
        Equal("torch", detector.Decide("torch").Text, "listed English word with no Korean reading stays literal");
        True(!detector.Decide("torch").NeedsConfirmation, "listed English word with no Korean reading needs no confirmation");
        True(detector.Decide("rory").NeedsConfirmation || detector.Decide("rory").Text == "rory", "unlisted band token is never converted by the word list below the margin");
        Equal("그런", detector.Decide("rmfjs").Text, "short Korean outside the hand lists converts");
        Equal("Pattern", detector.Decide("rmfjs").Confidence.ToString(), "statistical evidence is a pattern estimate, not lexical certainty");
        Equal("하는", detector.Decide("gksms").Text, "common two-syllable word converts without a shortcut");
        Equal("더", detector.Decide("ej").Text, "common one-syllable word converts without a shortcut");
        Equal("rm", detector.Decide("rm").Text, "near-tie stays literal without context");
        True(detector.Decide("rm").NeedsConfirmation, "near-tie asks for a choice");
        // dh/오 (llr 1.86 under the public-corpus tables) sits inside the abstention band and is absent from every hand list; rk/가 is a hand-list word.
        Equal("오", detector.Decide("dh", "dh", "", true).Text, "Korean context breaks a near-tie toward Korean");
        Equal("dh", detector.Decide("dh", "dh", "", false).Text, "English context breaks a near-tie toward English");
        Equal("dh", detector.Decide("dh").Text, "no context keeps a near-tie literal");
        Equal("hello", detector.Decide("hello", "hello", "", true).Text, "context never overrides clear English");
        Equal("dori", detector.Decide("dori", "dori", "", true).Text, "context never overrides clear non-lexicon English");
        Equal("figjam", detector.Decide("figjam").Text, "English statistics protect words composing into three Hangul syllables");
        True(!detector.Decide("figjam").NeedsConfirmation, "clear English statistics stay decisive");
        Equal("workauto", detector.Decide("workauto").Text, "uncertain statistics preserve complete-syllable English input");
        True(!detector.Decide("workauto").IsKorean, "complete-syllable English input is never converted automatically");
        Equal("안녕하세요", detector.Decide("dkssudgktpdy", "dkssudgktpdy", "", false).Text, "context never overrides clear Korean");
        Equal("go", detector.Decide("go", "go", "", true).Text, "known English word stays English even after a Korean word");
        Equal("그런ㅋㅋ", detector.Decide("rmfjszz").Text, "statistically Korean body accepts a chat suffix");
        Equal("EHDWKRDKSGKA", detector.Decide("EHDWKRDKSGKA").Text, "Shift acronym policy survives the statistical fallback");
        Equal("동작안함", detector.Decide("EHDWKRDKSGKA", "ehdwkrdksgka").Text, "Caps Lock Korean still converts through the statistical fallback");
        var input = new Composition(detector);
        Type(input, "sksms"); input.AcceptCommit(true); Type(input, "dh");
        Equal("오", input.Current.Text, "composition carries the previous word's language");
        True(input.PreviousKorean == true, "Korean continuity recorded after Space");
        input.AcceptCommit(true); Type(input, "dh"); Equal("오", input.Current.Text, "Korean continuity persists across Korean words");
        input.Clear(); Type(input, "dh"); Equal("dh", input.Current.Text, "cancel clears continuity");
        Type(input, ""); input.Clear(); Type(input, "hello"); input.AcceptCommit(true); Type(input, "dh");
        Equal("dh", input.Current.Text, "an English word sets English continuity");
        True(input.PreviousKorean == false, "English continuity recorded after Space");
        input.Clear(); Type(input, "sksms"); input.AcceptCommit(false); Type(input, "dh");
        Equal("dh", input.Current.Text, "Enter clears continuity");
        True(input.PreviousKorean == null, "no continuity after Enter");
        input.Clear(); Type(input, "123"); input.AcceptCommit(true); True(input.PreviousKorean == null, "symbols carry no language");
        input.Clear();
        // Fixtures generated by the Python reference implementation (tests/lm/detector.py); the C# port must match exactly.
        int rows = ReadResourceRows("HanFlow.ScorerFixtures.tsv", delegate(string[] f)
        {
            bool? previous = f[2] == "K" ? true : f[2] == "E" ? (bool?)false : null;
            var verdict = scorer.Score(f[0], f[1], previous);
            string text = verdict.IsKorean ? verdict.Composed : f[0];
            string kind = verdict.IsKorean ? "K" : verdict.IsEnglish ? "E" : "U";
            Equal(f[3], text, "reference text " + f[0] + "/" + f[2]);
            Equal(f[4], kind, "reference verdict " + f[0] + "/" + f[2]);
            double expected = double.Parse(f[5], CultureInfo.InvariantCulture);
            True(Math.Abs(verdict.Llr - expected) < 0.05, "reference llr " + f[0] + "/" + f[2] + " expected " + f[5] + " actual " + verdict.Llr.ToString("F2", CultureInfo.InvariantCulture));
        });
        True(rows >= 800, "reference fixture rows loaded: " + rows);
        // Held-out gates: Korean words absent from the hand lists, English tokens from the same corpus. Thresholds sit
        // below the measured Python rates (99.1% and 98.7% on these exact samples) to tolerate pipeline differences.
        int koreanTotal = 0, koreanOk = 0, koreanWrong = 0;
        var koreanWrongList = new StringBuilder();
        ReadResourceRows("HanFlow.HeldoutKorean.tsv", delegate(string[] f)
        {
            Decision d = detector.Decide(f[0]); koreanTotal++;
            if (d.IsKorean && d.Text == f[1]) koreanOk++;
            else if (d.IsKorean) { koreanWrong++; if (koreanWrong <= 30) koreanWrongList.Append(' ').Append(f[0]).Append('→').Append(d.Text).Append('≠').Append(f[1]); }
        });
        True(koreanTotal >= 900, "held-out Korean rows loaded: " + koreanTotal);
        True(koreanOk * 100 >= koreanTotal * 96, "held-out Korean conversion rate " + koreanOk + "/" + koreanTotal);
        True(koreanWrong * 200 <= koreanTotal, "held-out Korean wrong conversions " + koreanWrong + "/" + koreanTotal + ":" + koreanWrongList);
        if (koreanWrong > 0) Console.WriteLine("held-out Korean mismatches:" + koreanWrongList);
        int englishTotal = 0, englishOk = 0, englishWrong = 0;
        var englishWrongList = new StringBuilder();
        ReadResourceRows("HanFlow.HeldoutEnglish.tsv", delegate(string[] f)
        {
            Decision d = detector.Decide(f[0]); englishTotal++;
            if (!d.IsKorean && !d.NeedsConfirmation) englishOk++;
            else if (d.IsKorean) { englishWrong++; if (englishWrong <= 30) englishWrongList.Append(' ').Append(f[0]).Append('→').Append(d.Text).Append('(').Append(d.Reason).Append(')'); }
        });
        True(englishTotal >= 900, "held-out English rows loaded: " + englishTotal);
        True(englishOk * 100 >= englishTotal * 97, "held-out English retention rate " + englishOk + "/" + englishTotal);
        True(englishWrong * 200 <= englishTotal, "held-out English wrong conversions " + englishWrong + "/" + englishTotal + ":" + englishWrongList);
        if (englishWrong > 0) Console.WriteLine("held-out English wrong conversions:" + englishWrongList);
        Console.WriteLine("scorer gates: korean " + koreanOk + "/" + koreanTotal + " (wrong " + koreanWrong + "), english " + englishOk + "/" + englishTotal + " (wrong " + englishWrong + "), fixtures " + rows);
    }
    static void FlipTests()
    {
        Equal("안녕", Hangul.Flip("dkssud"), "flip Latin run to Hangul");
        Equal("dkssud", Hangul.Flip("안녕"), "flip Hangul run to keys");
        Equal("안녕 ㅗ디ㅣㅐ!", Hangul.Flip("dkssud hello!"), "flip keeps spaces and punctuation");
        Equal("dkssud hello!", Hangul.Flip(Hangul.Flip("dkssud hello!")), "flip is its own inverse for letters");
        Equal("꽈 3 뛰다", Hangul.Flip("Rhk 3 Enlek"), "shifted letters compose doubled jamo");
        Equal("Enlek", Hangul.Flip("뛰다"), "doubled jamo decompose to shifted keys");
        Equal("gksrmf 2026", Hangul.Flip("한글 2026"), "digits stay");
        Equal("", Hangul.Flip(""), "empty flip");
        Equal("rk\nsk", Hangul.Flip("가\n나"), "line breaks stay");
        // Sensitivity presets move only the abstention band; clear evidence is unaffected.
        var detector = new Detector(); var scorer = detector.Scorer; double ko, en;
        Equal("conservative", LanguageScorer.PresetThresholds("conservative", out ko, out en), "conservative preset name");
        scorer.Configure(ko, en);
        True(detector.Decide("gksms").IsKorean, "conservative still converts clear Korean");
        True(!detector.Decide("tp").IsKorean && detector.Decide("tp").NeedsConfirmation, "conservative abstains below its threshold"); // tp/세 llr 2.42: balanced converts, conservative waits
        True(!detector.Decide("hello").IsKorean && !detector.Decide("hello").NeedsConfirmation, "conservative keeps clear English certain");
        Equal("aggressive", LanguageScorer.PresetThresholds("aggressive", out ko, out en), "aggressive preset name");
        scorer.Configure(ko, en);
        Equal("지", detector.Decide("wl").Text, "aggressive converts a band word"); // wl/지 llr 1.87
        Equal("balanced", LanguageScorer.PresetThresholds("whatever", out ko, out en), "unknown preset falls back to balanced");
        scorer.Configure(ko, en);
        Equal("wl", detector.Decide("wl").Text, "balanced keeps the band word literal");
        True(detector.Decide("wl").NeedsConfirmation, "balanced abstention is visible");
        True(scorer.KoreanThreshold == LanguageScorer.DefaultKoreanThreshold && scorer.EnglishThreshold == LanguageScorer.DefaultEnglishThreshold, "balanced equals the defaults");
    }
    static void Round2Tests()
    {
        var detector = new Detector();
        foreach (string value in new[] { "핵심", "개선을", "회의에서", "개선합니다", "검증하고", "설계해주세요" })
        {
            string raw = Hangul.ToKeys(value);
            Equal(value, detector.Decide(raw).Text, "round2 lexical/morphological " + value);
            Equal("Known", detector.Decide(raw).Confidence.ToString(), "round2 lexical evidence " + value);
        }
        foreach (string[] pair in new[] {
            new[] { "API" + Hangul.ToKeys("개선합니다"), "API개선합니다" },
            new[] { "report" + Hangul.ToKeys("검증하고"), "report검증하고" },
            new[] { Hangul.ToKeys("회의에서") + "report", "회의에서report" },
            new[] { "AI" + Hangul.ToKeys("핵심") + "API" + Hangul.ToKeys("개선") + "SDK" + Hangul.ToKeys("검증") + "HTML" + Hangul.ToKeys("결과"), "AI핵심API개선SDK검증HTML결과" }
        }) Equal(pair[1], detector.Decide(pair[0]).Text, "round2 mixed morphology " + pair[0]);
        foreach (string text in new[] { "각", "값", "뷁", "한글", "힣" })
            Equal(Hangul.ToKeys(text), Hangul.ToKeys(text.Normalize(NormalizationForm.FormD)), "canonical jamo keys " + text);
        Equal("Cafe\u0301-" + Hangul.ToKeys("한글"), Hangul.ToKeys("Cafe\u0301-한글".Normalize(NormalizationForm.FormD)), "non-Hangul combining marks preserved");
        for (int cp = 0xac00; cp <= 0xd7a3; cp++)
        {
            string syllable = ((char)cp).ToString();
            Equal(syllable, Hangul.Compose(Hangul.ToKeys(syllable.Normalize(NormalizationForm.FormD))), "NFD roundtrip U+" + cp.ToString("X4"));
        }
        string longMixed = "AI" + Hangul.ToKeys("핵심") + "API" + Hangul.ToKeys("개선") + "SDK" + Hangul.ToKeys("검증") + "HTML" + Hangul.ToKeys("결과") + "API";
        Equal(longMixed, detector.Decide(longMixed).Text, "ninth alternating span is outside automatic decoding bound");
        foreach (int gap in new[] { 0, 1, 4 })
        {
            var colliding = new Detector();
            colliding.AddLexemeForTest("가나", true, 10); colliding.AddLexemeForTest("rk", false, 10);
            colliding.AddLexemeForTest("가", true, 10); colliding.AddLexemeForTest("rksk", false, 10 + gap);
            Decision choice = colliding.Decide("rkrksk");
            Equal(gap < Detector.MixedMargin ? "rkrksk" : "rk가나", choice.Text, "candidate cost gap " + gap);
            Equal((gap < Detector.MixedMargin).ToString(), choice.NeedsConfirmation.ToString(), "abstention gap " + gap);
            if (gap < Detector.MixedMargin)
            {
                True(choice.Alternative == "rk가나" || choice.Alternative == "가rksk", "abstained mixed candidate remains selectable");
                var input = new Composition(colliding); Type(input, "rkrksk"); input.Toggle();
                Equal(choice.Alternative, input.Current.Text, "F2 selects the best mixed alternative after abstention");
            }
        }
        // Reproduce Windows sharing violations deterministically, without reading personal files.
        string folder = Path.Combine(Path.GetTempPath(), "HanFlow-Round2-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(folder); string path = Path.Combine(folder, "choices.json");
        try
        {
            var words = new WordPreferences(); var store = new PreferenceStore(path, words);
            words.Remember("go", "go", true); True(store.FlushNow(), "sharing fixture seeded");
            var locked = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
            var release = new Thread(delegate() { Thread.Sleep(80); locked.Dispose(); }); release.Start();
            words.Clear(); bool saved = store.FlushNow(); release.Join();
            True(saved, "temporary sharing violation retries the atomic save");
            var reloaded = new WordPreferences(); new PreferenceStore(path, reloaded);
            Equal("0", reloaded.Count.ToString(), "reset survives transient sharing lock and reload");
            words.Remember("go", "go", true); True(store.FlushNow(), "pending-revision fixture seeded");
            using (var block = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read))
            {
                words.Remember("ej", "ej", true); store.ScheduleSave(); Thread.Sleep(40);
                words.Clear(); store.ScheduleSave();
            }
            bool latest = false;
            for (int attempt = 0; attempt < 60 && !latest; attempt++)
            {
                Thread.Sleep(20); var loaded = new WordPreferences(); var read = new PreferenceStore(path, loaded);
                latest = read.LastError == "" && loaded.Count == 0;
            }
            True(latest, "queued writer drains reset arriving during atomic replacement");
            // Windows sharing violations only: POSIX file systems allow the replace while a reader holds the file open.
            if (Environment.OSVersion.Platform == PlatformID.Win32NT)
            {
                words.Remember("go", "go", true); True(store.FlushNow(), "persistent-lock fixture seeded");
                using (var block = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read))
                {
                    words.Clear(); True(!store.FlushNow(), "persistent sharing lock fails within bounded retries");
                    True(store.LastError.Length > 0, "persistent sharing failure is visible");
                    var old = new WordPreferences(); new PreferenceStore(path, old);
                    Equal("1", old.Count.ToString(), "failed save preserves previous valid file");
                }
                True(store.FlushNow(), "save recovers after persistent lock released");
                Equal("", store.LastError, "successful retry clears error");
            }
            else Console.WriteLine("persistent sharing-lock checks skipped: Windows sharing semantics only");
        }
        finally { Directory.Delete(folder, true); }
    }
    static int Main()
    {
        try
        {
            Equal("안녕하세요", Hangul.Compose("dkssudgktpdy"), "greeting");
            Equal("한글", Hangul.Compose("gksrmf"), "basic");
            Equal("값없는", Hangul.Compose("rkqtdjqtsms"), "compound finals");
            Equal("닭갈비", Hangul.Compose("ekfrrkfql"), "compound final");
            Equal("달기", Hangul.Compose("ekfrl"), "split compound final before vowel");
            Equal("안자", Hangul.Compose("dkswk"), "carry final");
            Equal("뛰다", Hangul.Compose("Enlek"), "shift and compound vowel");
            Equal("괜찮아요", Hangul.Compose("rhoscksgdkdy"), "mixed compound vowels and finals");
            Equal("ㅋㅋㅋ", Hangul.Compose("zzz"), "laughter");
            Equal("ㅠㅠ", Hangul.Compose("bb"), "standalone vowels");
            Equal("hello", Hangul.ToKeys("ㅗ디ㅣㅐ"), "wrong-mode recovery");
            Equal("안녕하세요! 123", Hangul.Compose("dkssudgktpdy! 123"), "punctuation");
            for (int cp = 0xac00; cp <= 0xd7a3; cp++)
            {
                string text = ((char)cp).ToString();
                Equal(text, Hangul.Compose(Hangul.ToKeys(text)), "syllable " + cp);
            }
            var detector = new Detector();
            foreach (string[] pair in new[] {
                new[] { "gksrmffh", "한글로" }, new[] { "dleofh", "이대로" }, new[] { "skdml", "나의" },
                new[] { "ahems", "모든" }, new[] { "rjtdmf", "것을" }
            }) Equal(pair[1], detector.Decide(pair[0]).Text, "user screenshot lowercase regression " + pair[0]);
            Equal("안녕하세요", detector.Decide("DKSSUDGKTPDY", "dkssudgktpdy").Text, "Caps Lock Korean greeting");
            Equal("동작안함", detector.Decide("EHDWKRDKSGKA", "ehdwkrdksgka").Text, "Caps Lock user report");
            Equal("동작하도록", detector.Decide("EHDWKRGKEHFHR", "ehdwkrgkehfhr").Text, "Caps Lock user request");
            Equal("해줘", detector.Decide("GOWNJ", "gownj").Text, "Caps Lock short request");
            Equal("hanflow가", detector.Decide("hanflowRK", "hanflowrk").Text, "Caps Lock mixed product name");
            Equal("API", detector.Decide("API", "api").Text, "Caps Lock known acronym");
            Equal("NASA", detector.Decide("NASA", "nasa").Text, "Caps Lock unrecognized acronym");
            Equal("GO", detector.Decide("GO", "go").Text, "Caps Lock ambiguous English");
            Equal("EHDWKRDKSGKA", detector.Decide("EHDWKRDKSGKA").Text, "Shift uppercase acronym policy remains");
            foreach (string[] pair in new[] {
                new[] { "APIrotjs", "API개선" },
                new[] { "AIrjawmd", "AI검증" },
                new[] { "tjfrPAPI", "설계API" },
                new[] { "AIrotjsAPIrjawmd", "AI개선API검증" },
                new[] { "ghldmlmeetingwnsql", "회의meeting준비" },
                new[] { "tndbfreport", "수율report" }
            }) Equal(pair[1], detector.Decide(pair[0]).Text, "intelligent mixed segmentation " + pair[0]);
            Equal("안녕하세요", detector.Decide("dkssudgktpdy").Text, "automatic Korean");
            Equal("반갑습니다", detector.Decide("qksrkqtmqslek").Text, "second Korean");
            Equal("회의", detector.Decide("ghldml").Text, "short common Korean");
            Equal("프로젝트", detector.Decide("vmfhwprxm").Text, "project Korean");
            Equal("hello", detector.Decide("hello").Text, "English greeting");
            Equal("world", detector.Decide("world").Text, "valid Korean-shaped English");
            Equal("for", detector.Decide("for").Text, "short English collision");
            Equal("go", detector.Decide("go").Text, "go versus hae ambiguity");
            Equal("API", detector.Decide("API").Text, "acronym");
            Equal("OpenAI", detector.Decide("OpenAI").Text, "product name");
            Equal("github.com", detector.Decide("github.com").Text, "domain");
            Equal("peter@example.com", detector.Decide("peter@example.com").Text, "email");
            Equal("C:\\work\\report", detector.Decide("C:\\work\\report").Text, "path");
            Equal("user_name", detector.Decide("user_name").Text, "identifier");
            Equal("안녕하세요!", detector.Decide("dkssudgktpdy!").Text, "sentence punctuation");
            Equal("vy", detector.Decide("vy").Text, "ambiguous one syllable retained"); // vy/표 llr 0.62
            Equal("ㅋㅋㅋ", detector.Decide("zzz").Text, "Korean chat laughter");
            Equal("안녕ㅋㅋ", detector.Decide("dkssudzz").Text, "known Korean followed by laughter");
            Equal("감사합니다ㅠㅠ!", detector.Decide("rkatkgkqslekbb!").Text, "complete Korean followed by tears");
            Equal("ㅋㅎㅋㅎ", detector.Decide("zgzg").Text, "mixed Korean chat consonants");
            Equal("ㅠㅜㅠㅜ", detector.Decide("bnbn").Text, "mixed Korean chat vowels");
            Equal("zgbn", detector.Decide("zgbn").Text, "ambiguous consonant vowel chat remains literal");
            Equal("안녕ㅋ", detector.Decide("dkssudz").Text, "single chat suffix after known Korean");
            Equal("hellozzz", detector.Decide("hellozzz").Text, "English with chat-like suffix stays literal");
            Equal("world", detector.Decide("world").Text, "kokey false-positive regression English");
            Equal("go", detector.Decide("go").Text, "kokey false-positive regression short English");
            Equal("bb.com", detector.Decide("bb.com").Text, "chat-like domain stays literal");
            Equal("z", detector.Decide("z").Text, "one isolated letter stays literal");
            Equal("안녕ㅠㅠ", detector.Decide("안녕ㅠㅠ").Text, "existing Korean chat stays unchanged");
            Equal("doWork", detector.Decide("doWork").Text, "English camelCase identifier stays literal");
            Equal("DoWork", detector.Decide("DoWork").Text, "English PascalCase identifier stays literal");
            Equal("바꿔요", detector.Decide("qkRnjdy").Text, "required Korean shift is not mistaken for camelCase");
            foreach (string[] pair in new[] {
                new[] { "dndb", "우유" }, new[] { "rotjs", "개선" }, new[] { "tndbf", "수율" },
                new[] { "wjsfir", "전략" }, new[] { "dml", "의" }, new[] { "fmf", "를" },
                new[] { "eogks", "대한" }, new[] { "tjfrP", "설계" }, new[] { "wheh", "조도" },
                new[] { "didtks", "양산" }, new[] { "AIdml", "AI의" }, new[] { "flarefmf", "flare를" },
                new[] { "thwochange,", "소재change," }, new[] { "1988rhkwp", "1988과제" },
                new[] { "4wja", "4점" }, new[] { "wkrdjqwltltj(tkdidtj)", "작업지시서(사양서)" },
                new[] { "rjawmd/wjrdyd", "검증/적용" }, new[] { "roqkf/didtks", "개발/양산" },
                new[] { "tlgja/rjawmd,", "시험/검증," },
                new[] { "wbs;tlwkdqnstjrdmfgksek,", "wbs;시장분석을한다," },
                new[] { "wjsfirtnflq-ASIS-aorfkrqnstjr-eodmdwjsfir-20261007", "전략수립-ASIS-맥락분석-대응전략-20261007" },
                // Held-out combinations: these exact strings are absent from the supplied sample.
                new[] { "APIdml", "API의" }, new[] { "reportfmf", "report를" },
                new[] { "(ghldml/wnsql)", "(회의/준비)" }, new[] { "10wja", "10점" },
                new[] { "dnjsdlschange", "원인change" }
            }) Equal(pair[1], detector.Decide(pair[0]).Text, "mixed-input regression " + pair[0]);
            foreach (string literal in new[] { "rotjs@example.com", "https://example.com/rotjs", "C:\\work\\rotjs.txt",
                "user_rotjs", "sub1-sub2", "50mm", "95%", ">=", "clock-prd.md", "01-type-slug.md/html", "who.what.why",
                "Verification/Validation", "dori", "sudoku", "Café", "wbs", "DoD", "P.E" })
                Equal(literal, detector.Decide(literal).Text, "protected literal " + literal);
            var profileDetector = new Detector(@"C:\Users\Test사용자");
            Equal(@"C:\Users\Test사용자\GitHub\project", profileDetector.Decide(@"C:\Users\Testtkdydwk\GitHub\project").Text, "exact known profile alias");
            Equal(@"C:\Users\Testtkdydwk2\GitHub", profileDetector.Decide(@"C:\Users\Testtkdydwk2\GitHub").Text, "profile alias requires segment boundary");
            var composition = new Composition(detector);
            foreach (char c in "dkssud") composition.Append(c, c);
            Equal("안녕", composition.Current.Text, "preedit");
            composition.Backspace();
            Equal("안녀", composition.Korean, "one keystroke backspace");
            composition.Append('d', 'd');
            composition.Toggle();
            Equal("dkssud", composition.Current.Text, "manual English override");
            composition.Toggle();
            Equal("안녕", composition.Current.Text, "manual Korean override");
            composition.Clear();
            True(composition.IsEmpty, "composition cleared");
            foreach (char c in "dkssud") composition.Append(c, c);
            composition.Toggle();
            while (!composition.IsEmpty) composition.Backspace();
            foreach (char c in "dkssud") composition.Append(c, c);
            Equal("안녕", composition.Current.Text, "deleting everything resets manual English selection");
            composition.Clear();
            composition.Toggle();
            foreach (char c in "hello") composition.Append(c, c);
            Equal("hello", composition.Current.Text, "toggle on empty buffer has no effect");
            composition.Clear();
            foreach (char c in "go") composition.Append(c, c);
            composition.Toggle();
            composition.Backspace(); composition.Backspace(); composition.Backspace();
            foreach (char c in "hello") composition.Append(c, c);
            Equal("hello", composition.Current.Text, "deleting everything resets manual Korean selection");
            composition.Clear();
            foreach (char c in "rkqtk") composition.Append(c, c);
            Equal("갑사", composition.Korean, "compound final carried before deletion");
            composition.Backspace(); Equal("값", composition.Korean, "backspace restores compound final");
            composition.Backspace(); Equal("갑", composition.Korean, "backspace removes second final key");
            composition.Backspace(); Equal("가", composition.Korean, "backspace removes first final key");
            composition.Backspace(); Equal("ㄱ", composition.Korean, "backspace retains initial");
            composition.Backspace(); Equal("", composition.Korean, "backspace empties preedit");
            foreach (char c in "rhk") composition.Append(c, c);
            Equal("과", composition.Korean, "compound vowel before deletion");
            composition.Backspace(); Equal("고", composition.Korean, "compound vowel loses one key");
            composition.Clear();
            foreach (char c in "akfrh") composition.Append(c, c);
            Equal("말고", composition.Korean, "final carry across pending word");
            composition.Backspace(); Equal("맑", composition.Korean, "whole pending word undo restores prior final");
            composition.Clear();
            // es-hangul boundary cases, adapted to HanFlow's whole-word physical-key undo.
            // Deleting P from tlstprP replays tlstpr -> 신섹; es-hangul preserves 신세ㄱ instead.
            foreach (string pair in new[] { "전화:전호", "값:갑", "깎:까", "신세계:신섹", "끓:끌", "왅:완" })
            {
                string[] values = pair.Split(':');
                foreach (char c in Hangul.ToKeys(values[0])) composition.Append(c, c);
                composition.Backspace(); Equal(values[1], composition.Korean, "reference deletion " + values[0]);
                composition.Clear();
            }
            var personalDetector = new Detector();
            var personalInput = new Composition(personalDetector);
            foreach (char c in "vy") personalInput.Append(c, c); // vy/표 llr 0.62: one syllable stays literal by default, F2 offers the Korean reading
            personalInput.Toggle(); Confirm(personalInput); personalInput.Clear();
            Equal("표", personalDetector.Decide("vy").Text, "confirmed F2 choice is remembered");
            foreach (char c in "rm") personalInput.Append(c, c);
            Semicolon(personalInput);
            Equal("그", personalInput.Current.Text, "semicolon selects the other candidate");
            Confirm(personalInput); personalInput.Clear();
            Equal("그", personalDetector.Decide("rm").Text, "confirmed semicolon choice is remembered");
            foreach (char c in "hello") personalInput.Append(c, c);
            Semicolon(personalInput); Semicolon(personalInput);
            Equal("hello;", personalInput.Current.Text, "double semicolon escapes literal punctuation");
            personalInput.Clear();
            PreferenceTests();
            IntelligenceTests();
            Round2Tests();
            ScorerTests();
            FlipTests();
            True(FocusPolicy.CanCapture(true, false, true, false, true, 100, true), "safe target");
            True(!FocusPolicy.CanCapture(true, true, true, false, true, 100, true), "password bypass");
            True(!FocusPolicy.CanCapture(true, false, false, false, true, 100, true), "read only bypass");
            True(!FocusPolicy.CanCapture(false, false, true, false, true, 100, true), "unknown bypass");
            True(!FocusPolicy.CanCapture(true, false, true, true, true, 100, true), "elevated bypass");
            True(!FocusPolicy.CanCapture(true, false, true, false, true, 900, true), "stale bypass");
            True(!FocusPolicy.CanCapture(true, false, true, false, true, 100, false), "changed target bypass");
            Console.WriteLine((failures == 0 ? "PASS: " : "FAIL: ") + count + " assertions; " + failures + " failures, including all 11,172 modern Hangul syllables.");
            return failures == 0 ? 0 : 1;
        }
        catch (Exception ex) { Console.Error.WriteLine("FAIL: " + ex.Message); return 1; }
    }
}
