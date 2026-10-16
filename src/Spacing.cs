using System.Collections.Generic;
using System.Text;

namespace Hanautomata
{
    // Automatic spacing for the commonest Korean dependent-noun and auxiliary constructions (한글 맞춤법 제42항: 의존 명사는 띄어 쓴다;
    // "할 수 있다", "하는 것", "할 때", "해야 한다"). It only inserts U+0020 between two Hangul syllables of one run, so removing the
    // inserted spaces always restores the input exactly. Every rule needs a closed continuation or a listed stem: an unlisted form
    // is left as typed, because a wrong space costs the writer more than a missed one. Pure and bounded; it runs inside the keyboard hook.
    public static class Spacing
    {
        public const int MaxLength = 256;
        const int Nieun = 4, Rieul = 8;

        public static string Correct(string text) { return Correct(text, null); }
        // rules, when given, receives one identifier per inserted space (in text order) for diagnostics and tests.
        public static string Correct(string text, List<string> rules)
        {
            if (string.IsNullOrEmpty(text) || text.Length > MaxLength) return text ?? "";
            StringBuilder output = null; int copied = 0;
            for (int start = 0; start < text.Length; )
            {
                if (!Syllable(text[start])) { start++; continue; }
                int end = start;
                while (end < text.Length && Syllable(text[end])) end++;
                if (end - start >= 2)
                {
                    string word = text.Substring(start, end - start);
                    string[] marks = Splits(word);
                    if (marks != null)
                    {
                        if (output == null) output = new StringBuilder(text.Length + 8);
                        output.Append(text, copied, start - copied);
                        for (int i = 0; i < word.Length; i++)
                        {
                            if (marks[i] != null) { output.Append(' '); if (rules != null) rules.Add(marks[i]); }
                            output.Append(word[i]);
                        }
                        copied = end;
                    }
                }
                start = end;
            }
            if (output == null) return text;
            output.Append(text, copied, text.Length - copied);
            return output.ToString();
        }

        // marks[i] != null means a space before word[i]; null when nothing changes.
        static string[] Splits(string w)
        {
            var marks = new string[w.Length]; bool any = false;
            for (int i = 0; i < w.Length; i++)
            {
                switch (w[i])
                {
                    case '수': any |= Su(w, i, marks); break;
                    case '것': any |= Geot(w, i, marks, GeotTails, GeotNouns, null, "ko-space-geot"); break;
                    case '거': any |= Geot(w, i, marks, GeoTails, GeoNouns, GeoEndStems, "ko-space-geo"); break;
                    case '때': any |= Ttae(w, i, marks); break;
                    case '줄': any |= Jul(w, i, marks); break;
                    case '적': any |= Jeok(w, i, marks); break;
                    case '듯': any |= Deut(w, i, marks); break;
                    case '야': any |= Eoya(w, i, marks); break;
                    case '테': case '텐': any |= Te(w, i, marks); break;
                    case '리': any |= Ri(w, i, marks); break;
                    case '뻔': any |= After(w, i, marks, Rieul, "ko-space-ppeon"); break;
                    case '중': any |= Jung(w, i, marks); break;
                    case '게': any |= Ge(w, i, marks); break;
                }
            }
            return any ? marks : null;
        }

        // ㄹ 수 있다/없다 · ㄹ 수밖에 · ㄹ 수도/가/는 있다. "할수록" (-ㄹ수록) and nouns such as 실수·홀수·말수 stay joined.
        static bool Su(string w, int i, string[] marks)
        {
            if (i == 0) return At(w, 1, "있") && Mark(marks, 1, "ko-space-su");  // "수있다"; "수없이/수없다" are single words
            char stem = w[i - 1];
            if (Final(stem) != Rieul || Contains(SuNouns, stem)) return false;
            int j = i + 1;
            if (j == w.Length) return Contains(SuStems, stem) && Mark(marks, i, "ko-space-su");
            if (Existence(w, j)) return Mark(marks, i, "ko-space-su") & Mark(marks, j, "ko-space-su");
            foreach (string particle in SuParticles)
            {
                if (!At(w, j, particle)) continue;
                int k = j + particle.Length;
                if (k == w.Length) return particle != "를" && Contains(SuStems, stem) && Mark(marks, i, "ko-space-su");  // 줄수를 = line count
                if (Existence(w, k)) return Mark(marks, i, "ko-space-su") & Mark(marks, k, "ko-space-su");
                return false;
            }
            return false;
        }

        // ㄴ/ㄹ 것(거): "하는것" → "하는 것", "할거야" → "할 거야", "할것같다" → "할 것 같다". Only listed endings follow the noun.
        static bool Geot(string w, int i, string[] marks, string[] tails, string[] nouns, string endStems, string id)
        {
            int j = i + 1; bool any = false;
            if (At(w, j, "같") && !At(w, j, "같이")) any |= Mark(marks, j, id);  // 것 같다; "그것같이" keeps the particle 같이
            else
                foreach (string particle in GeotParticles)
                {
                    int k = j + particle.Length;
                    if (At(w, j, particle) && k < w.Length && (Existence(w, k) || At(w, k, "아니") || (At(w, k, "같") && !At(w, k, "같이"))))
                    { any |= Mark(marks, k, id); break; }
                }
            if (i == 0) return any;
            int last = Final(w[i - 1]);
            if (last != Nieun && last != Rieul || EndsWithListed(w, i, nouns)) return any;
            if (!any && j < w.Length && !Tail(w, j, tails)) return false;
            if (!any && j == w.Length && i == 1 && endStems != null && !Contains(endStems, w[0])) return false;
            return Mark(marks, i, id) | any;
        }

        // ㄹ 때 ("할때마다" → "할 때마다") and the always-separate 때문 ("그것때문에" → "그것 때문에").
        static bool Ttae(string w, int i, string[] marks)
        {
            if (i == 0) return false;
            if (At(w, i + 1, "문")) return Mark(marks, i, "ko-space-ttaemun");
            return Final(w[i - 1]) == Rieul && !EndsWithListed(w, i, TtaeNouns) && (i + 1 == w.Length || Tail(w, i + 1, TtaeTails))
                && Mark(marks, i, "ko-space-ttae");
        }

        // ㄹ 줄 알다/모르다: "할줄알아" → "할 줄 알아".
        static bool Jul(string w, int i, string[] marks)
        {
            if (i == 0 || Final(w[i - 1]) != Rieul) return false;
            int k = Verb(w, i + 1, JulParticles, JulVerbs);
            return k > 0 && (Mark(marks, i, "ko-space-jul") & Mark(marks, k, "ko-space-jul"));
        }

        // ㄴ 적 있다/없다: "본적있어" → "본 적 있어". Stems are listed because -적 (的) nouns are countless: 흔적, 인적, 전적.
        static bool Jeok(string w, int i, string[] marks)
        {
            if (i == 0 || !Contains(JeokStems, w[i - 1])) return false;
            int k = Verb(w, i + 1, JeokParticles, ExistenceVerbs);
            return k > 0 && (Mark(marks, i, "ko-space-jeok") & Mark(marks, k, "ko-space-jeok"));
        }

        // ㄴ/ㄹ 듯: "할듯" → "할 듯"; 듯하다·듯싶다·듯이 stay attached to 듯.
        static bool Deut(string w, int i, string[] marks)
        {
            if (i == 0) return false;
            int last = Final(w[i - 1]);
            return (last == Nieun || last == Rieul) && !EndsWithListed(w, i, DeutNouns) && Mark(marks, i, "ko-space-deut");
        }

        // -어야 하다/되다: "해야한다" → "해야 한다". 해야겠다·해야지 are endings and stay joined.
        static bool Eoya(string w, int i, string[] marks)
        {
            if (i == 0 || i + 1 >= w.Length || Final(w[i - 1]) != 0 || !EoVowel(w[i - 1])) return false;
            foreach (string verb in HadaVerbs) if (At(w, i + 1, verb)) return Mark(marks, i + 1, "ko-space-eoya");
            return false;
        }

        // ㄹ 테니 · ㄹ 텐데: "할테니까" → "할 테니까".
        static bool Te(string w, int i, string[] marks)
        {
            if (i == 0 || Final(w[i - 1]) != Rieul || EndsWithListed(w, i, TeNouns)) return false;
            bool tail = w[i] == '텐' ? At(w, i + 1, "데") : Tail(w, i + 1, TeTails);
            return tail && Mark(marks, i, "ko-space-te");
        }

        // ㄹ 리가 없다: "할리가없다" → "할 리가 없다". The particle is required: 멀리·널리·달리 + 있다 are adverbs.
        static bool Ri(string w, int i, string[] marks)
        {
            if (i == 0 || Final(w[i - 1]) != Rieul || Contains(RiNouns, w[i - 1])) return false;
            foreach (string particle in RiParticles)
            {
                int k = i + 1 + particle.Length;
                if (At(w, i + 1, particle) && (Existence(w, k) || At(w, k, "만무")))
                    return Mark(marks, i, "ko-space-ri") & Mark(marks, k, "ko-space-ri");
            }
            return false;
        }

        // 는/던 중: "하는중이다" → "하는 중이다".
        static bool Jung(string w, int i, string[] marks)
        { return i > 0 && (w[i - 1] == '는' || w[i - 1] == '던') && Mark(marks, i, "ko-space-jung"); }

        // 는/던 게 (= 것이) before a predicate: "하는게좋아" → "하는 게 좋아". 게임·게시 and other nouns are not predicates.
        static bool Ge(string w, int i, string[] marks)
        {
            if (i == 0 || w[i - 1] != '는' && w[i - 1] != '던') return false;
            int j = i + 1;
            if (j == w.Length) return Mark(marks, i, "ko-space-ge");
            foreach (string predicate in GePredicates)
                if (At(w, j, predicate)) return Mark(marks, i, "ko-space-ge") & Mark(marks, j, "ko-space-ge");
            return false;
        }

        static bool After(string w, int i, string[] marks, int final, string id)
        { return i > 0 && Final(w[i - 1]) == final && Mark(marks, i, id); }

        // Index of a listed verb at j, optionally behind one listed particle; 0 when absent (j is never 0).
        static int Verb(string w, int j, string[] particles, string[] verbs)
        {
            foreach (string verb in verbs) if (At(w, j, verb)) return j;
            foreach (string particle in particles)
                if (At(w, j, particle))
                    foreach (string verb in verbs) if (At(w, j + particle.Length, verb)) return j + particle.Length;
            return 0;
        }
        static bool Existence(string w, int k) { return At(w, k, "있") || At(w, k, "없"); }
        static bool Tail(string w, int j, string[] tails)
        {
            foreach (string tail in tails) if (tail.Length == w.Length - j && At(w, j, tail)) return true;
            return false;
        }
        // Nouns that end in the anchor (선거, 날것, 물때). '^' marks a noun that must start the run: "탈것", but "만들것" → "만들 것".
        static bool EndsWithListed(string w, int i, string[] nouns)
        {
            foreach (string noun in nouns)
            {
                bool whole = noun[0] == '^';
                string body = whole ? noun.Substring(1) : noun;
                int start = i - body.Length + 1;
                if (start >= 0 && (!whole || start == 0) && At(w, start, body.Substring(0, body.Length - 1))) return true;
            }
            return false;
        }
        static bool Mark(string[] marks, int i, string id) { if (i > 0 && i < marks.Length && marks[i] == null) marks[i] = id; return true; }
        static bool At(string w, int i, string s)
        { return i >= 0 && i + s.Length <= w.Length && string.CompareOrdinal(w, i, s, 0, s.Length) == 0; }
        static bool Contains(string set, char c) { return set.IndexOf(c) >= 0; }
        static bool Syllable(char c) { return c >= '가' && c <= '힣'; }
        static int Final(char c) { return (c - 0xac00) % 28; }
        // ㅏ ㅐ ㅓ ㅕ ㅘ ㅙ ㅝ: the vowels an -아/-어 form ends with (해야, 가야, 봐야, 써야, 돼야, 줘야, 먹어야).
        static bool EoVowel(char c)
        { int v = (c - 0xac00) / 28 % 21; return v == 0 || v == 1 || v == 4 || v == 6 || v == 9 || v == 10 || v == 14; }

        static readonly string SuNouns = "실말별술홀철골";               // 실수, 말수, 별수, 술수, 홀수, (김)철수, 골수
        static readonly string SuStems = "할될볼올쓸잘알놀울을를럴쩔걸줄칠팔풀빌질";  // standalone "할수", "그럴수도"
        static readonly string[] SuParticles = { "밖에", "조차", "마저", "도", "가", "는", "를", "만" };
        static readonly string[] GeotParticles = { "이", "은", "도", "만", "을" };
        static readonly string[] GeotTails = { "이", "을", "은", "도", "만", "과", "보다", "처럼", "만큼", "뿐", "들", "들이", "들을", "들은", "들도", "들의",
            "이다", "입니다", "이에요", "이야", "이지", "이지만", "이고", "이며", "이니", "이니까", "이라", "이라고", "이라는", "이라면", "이란", "이라서",
            "이었다", "이었고", "이었는데", "이었어", "이었어요", "이었습니다", "인데", "인지", "인가", "인가요", "일까", "일까요", "일지", "일지도",
            "으로", "으로는", "으로도", "에", "에는", "에도", "에서", "이나", "이든", "이든지", "이면", "까지", "마저", "조차",
            "이요", "이죠", "이네", "이네요", "이군", "이잖아", "이므로", "임", "임을", "의" };
        static readonly string[] GeoTails = { "야", "예요", "에요", "다", "지", "지요", "죠", "고", "면", "냐", "요", "는", "를", "도", "만",
            "라고", "랑", "잖아", "잖아요", "네", "네요", "임", "였어", "였다", "였는데", "였어요", "였지", "인데", "인지", "일까", "일걸",
            "야말로", "니까", "니깐", "보다", "처럼", "밖에", "만큼", "든가", "였음" };
        static readonly string[] GeotNouns = { "^날것", "^탈것", "^들것", "^별것", "^군것" };
        static readonly string[] GeoNouns = { "선거", "근거", "철거", "설거", "열거", "은거", "별거", "일거", "본거", "연거", "군거", "전거", "준거", "논거" };
        // A run that ends in 거 is a name as often as a sentence ("윤거"): one-syllable prefixes must be common adnominal forms.
        static readonly string GeoEndStems = "할갈올볼쓸살알줄될한간온본산된쓴준난탄낸큰단긴먼찬짠";
        static readonly string[] TtaeNouns = { "물때" };
        static readonly string[] TtaeTails = { "는", "도", "마다", "까지", "부터", "만", "에", "에는", "에도", "에만", "가", "를", "의", "면", "나", "라도",
            "처럼", "와", "랑", "엔", "보다", "이면", "마저", "조차", "이다", "였다", "야", "였어", "예요", "이에요", "인데", "이나", "이든", "이라도" };
        static readonly string[] DeutNouns = { "그럴듯", "반듯", "번듯", "불현듯" };  // single-word adjectives and an adverb
        static readonly string[] JulParticles = { "도", "은", "을", "만", "는", "이" };
        static readonly string[] JulVerbs = { "알", "몰", "모르", "모를", "아는", "안다" };
        static readonly string JeokStems = "본한간온난든쓴은던운탄준낸된친산린른진긴핀";
        static readonly string[] JeokParticles = { "이", "은", "도", "는" };
        static readonly string[] ExistenceVerbs = { "있", "없" };
        static readonly string[] HadaVerbs = { "하", "한", "할", "합", "해", "했", "되", "된", "될", "됩", "돼", "됐" };
        static readonly string[] TeNouns = { "이를테" };  // 이를테면 is an adverb
        static readonly string[] TeTails = { "니", "니까", "고", "야", "지", "지만", "다", "요", "면" };
        static readonly string RiNouns = "멀널달실물";
        static readonly string[] RiParticles = { "가", "는", "도", "만" };
        static readonly string[] GePredicates = { "아니", "아냐", "있", "없", "좋", "낫", "나아", "맞", "많", "중요", "문제", "어때" };
    }
}
