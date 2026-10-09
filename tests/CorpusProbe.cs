using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using System.Text.RegularExpressions;
using System.Web.Script.Serialization;
using HanFlow;

static class CorpusProbe
{
    static int Main(string[] args)
    {
        var detector = new Detector();
        var corrections = new Dictionary<string, string>();
        foreach (string row in File.ReadLines(args[1], Encoding.UTF8))
        {
            string[] fields = row.Split('\t');
            if (fields.Length == 2) corrections.Add(fields[0], fields[1]);
        }
        string original = File.ReadAllText(args[0], Encoding.UTF8);
        var rows = new List<object>(); var output = new StringBuilder(); var allKeys = new StringBuilder();
        int token = 0, line = 1, literalMatches = 0, intendedMatches = 0, physicalTokens = 0;
        int physicalLiteralMatches = 0, physicalIntendedMatches = 0, correctionTokens = 0, correctionMatches = 0;
        int originalOnlyTokens = 0, originalOnlyMatches = 0;
        foreach (Match match in Regex.Matches(original, @"\s+|\S+"))
        {
            string text = match.Value;
            if (char.IsWhiteSpace(text[0]))
            { output.Append(text); allKeys.Append(text); foreach (char c in text) if (c == '\n') line++; continue; }
            string raw = Hangul.ToKeys(text); Decision decision = detector.Decide(raw);
            string expected; bool correction = corrections.TryGetValue(text, out expected);
            if (!correction) expected = text;
            bool physical = true; foreach (char c in raw) if (c > 127) physical = false;
            bool exact = decision.Text == text, intended = decision.Text == expected;
            if (exact) literalMatches++; if (intended) intendedMatches++;
            if (physical) { physicalTokens++; if (exact) physicalLiteralMatches++; if (intended) physicalIntendedMatches++; }
            if (correction) { correctionTokens++; if (intended) correctionMatches++; }
            else { originalOnlyTokens++; if (exact) originalOnlyMatches++; }
            rows.Add(new { token = ++token, line, original = text, raw, expected, actual = decision.Text,
                alternative = decision.Alternative, reason = decision.Reason, exact, intended, asciiPhysical = physical });
            output.Append(decision.Text); allKeys.Append(raw);
        }
        var summary = new { tokens = token, literalMatches, intendedMatches, originalOnlyTokens, originalOnlyMatches,
            correctionTokens, correctionMatches, physicalTokens, physicalLiteralMatches, physicalIntendedMatches,
            unsupportedPhysicalTokens = token - physicalTokens, correctionMappings = corrections.Count,
            measurement = "Same user-supplied sample used to improve vocabulary; not independent general-language accuracy." };
        var report = new { summary, rows };
        var json = new JavaScriptSerializer { MaxJsonLength = 10000000 };
        File.WriteAllText(args[2] + ".json", json.Serialize(report), new UTF8Encoding(false));
        File.WriteAllText(args[2] + ".actual.txt", output.ToString(), new UTF8Encoding(false));
        File.WriteAllText(args[2] + ".keys.txt", allKeys.ToString(), new UTF8Encoding(false));
        Console.WriteLine(json.Serialize(summary));
        return 0;
    }
}
