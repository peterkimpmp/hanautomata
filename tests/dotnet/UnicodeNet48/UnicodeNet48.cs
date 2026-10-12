// Compares the .NET character semantics the Rust core copies (core/src/unicode_tables.rs, generated on .NET 9 / ICU)
// with the runtime the Windows tray app ships on: .NET Framework 4.8 (NLS). Plan 21 S0, core/README.md "net48".
// Builds with the csc.exe that comes with Windows, no SDK (C# 5):
//   & "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe" /nologo /out:unicode-net48.exe tests\dotnet\UnicodeNet48\UnicodeNet48.cs
//   .\unicode-net48.exe core\src\unicode_tables.rs [net48-tables.rs]
// Prints, per table, the entries only one runtime has; exit code 0 when the tables are identical, 1 when they differ.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.RegularExpressions;

static class UnicodeNet48
{
    static char Fold(char c)
    {
        char u = char.ToUpperInvariant(c);
        return string.Equals(c.ToString(), u.ToString(), StringComparison.OrdinalIgnoreCase) ? u : c;
    }

    static bool Surrogate(int c) { return c >= 0xd800 && c <= 0xdfff; }

    static List<string> Ranges(Func<char, bool> test)
    {
        var items = new List<string>();
        int start = -1;
        for (int c = 0; c <= 0x10000; c++)
        {
            bool inside = c <= 0xffff && !Surrogate(c) && test((char)c);
            if (inside && start < 0) start = c;
            else if (!inside && start >= 0) { items.Add("(0x" + start.ToString("X4") + ", 0x" + (c - 1).ToString("X4") + ")"); start = -1; }
        }
        return items;
    }

    static List<string> Pairs(Func<char, char> map)
    {
        var items = new List<string>();
        for (int c = 0; c <= 0xffff; c++)
        {
            if (Surrogate(c)) continue;
            char m = map((char)c);
            if (m != c) items.Add("(0x" + c.ToString("X4") + ", 0x" + ((int)m).ToString("X4") + ")");
        }
        return items;
    }

    // Expands ranges to single code points so a moved boundary shows as the code points that changed.
    static SortedSet<string> Points(List<string> entries, bool ranges)
    {
        var set = new SortedSet<string>(StringComparer.Ordinal);
        foreach (string e in entries)
        {
            Match m = Regex.Match(e, @"0x([0-9A-F]{4}), 0x([0-9A-F]{4})");
            int a = Convert.ToInt32(m.Groups[1].Value, 16), b = Convert.ToInt32(m.Groups[2].Value, 16);
            if (!ranges) { set.Add(a.ToString("X4") + "->" + b.ToString("X4")); continue; }
            for (int c = a; c <= b; c++) set.Add(c.ToString("X4"));
        }
        return set;
    }

    static Dictionary<string, List<string>> Parse(string text)
    {
        var tables = new Dictionary<string, List<string>>();
        foreach (Match t in Regex.Matches(text, @"static (\w+): &\[\(u16, u16\)\] = &\[(.*?)\];", RegexOptions.Singleline))
            tables[t.Groups[1].Value] = Regex.Matches(t.Groups[2].Value, @"\(0x[0-9A-F]{4}, 0x[0-9A-F]{4}\)").Cast<Match>().Select(x => x.Value).ToList();
        return tables;
    }

    static int Main(string[] args)
    {
        if (args.Length < 1) { Console.Error.WriteLine("usage: unicode-net48 <core/src/unicode_tables.rs> [out.rs]"); return 2; }
        Console.WriteLine("runtime " + Environment.Version + " (" + typeof(object).Assembly.GetName().Name + ")");  // mscorlib = .NET Framework

        int foldMismatches = 0;
        for (int c = 0; c <= 0xffff; c++)
        {
            if (Surrogate(c)) continue;
            char ch = (char)c, lower = char.ToLowerInvariant(ch), upper = char.ToUpperInvariant(ch);
            if (string.Equals(ch.ToString(), lower.ToString(), StringComparison.OrdinalIgnoreCase) != (Fold(ch) == Fold(lower))) foldMismatches++;
            if (string.Equals(ch.ToString(), upper.ToString(), StringComparison.OrdinalIgnoreCase) != (Fold(ch) == Fold(upper))) foldMismatches++;
            if (Fold(Fold(ch)) != Fold(ch)) foldMismatches++;
        }
        Console.WriteLine("OrdinalIgnoreCase per-character fold mismatches: " + foldMismatches);

        var mine = new Dictionary<string, List<string>>();
        mine["LETTER"] = Ranges(char.IsLetter);
        mine["UPPER"] = Ranges(char.IsUpper);
        mine["LOWER"] = Ranges(char.IsLower);
        mine["TO_LOWER"] = Pairs(char.ToLowerInvariant);
        mine["FOLD"] = Pairs(Fold);

        if (args.Length > 1)
        {
            var sb = new StringBuilder("// .NET Framework " + Environment.Version + " char semantics, same layout as core/src/unicode_tables.rs\n\n");
            foreach (var kv in mine)
            {
                sb.Append("pub(crate) static ").Append(kv.Key).Append(": &[(u16, u16)] = &[\n");
                for (int i = 0; i < kv.Value.Count; i += 8) sb.Append("    ").Append(string.Join(", ", kv.Value.Skip(i).Take(8))).Append(",\n");
                sb.Append("];\n\n");
            }
            File.WriteAllText(args[1], sb.ToString(), new UTF8Encoding(false));
        }

        var theirs = Parse(File.ReadAllText(args[0]));
        int differences = foldMismatches;
        var touched = new SortedSet<int>();
        foreach (var kv in mine)
        {
            bool ranges = kv.Key == "LETTER" || kv.Key == "UPPER" || kv.Key == "LOWER";
            List<string> other;
            if (!theirs.TryGetValue(kv.Key, out other)) { Console.WriteLine(kv.Key + ": missing in " + args[0]); differences++; continue; }
            SortedSet<string> a = Points(kv.Value, ranges), b = Points(other, ranges);
            var onlyNet48 = a.Except(b).ToList();
            var onlyNet9 = b.Except(a).ToList();
            differences += onlyNet48.Count + onlyNet9.Count;
            foreach (string e in onlyNet48.Concat(onlyNet9)) touched.Add(Convert.ToInt32(e.Substring(0, 4), 16));
            Console.WriteLine(kv.Key + ": net48 " + a.Count + " · core " + b.Count + " · only net48 " + onlyNet48.Count + " · only core " + onlyNet9.Count);
            if (onlyNet48.Count > 0) Console.WriteLine("  only net48: " + string.Join(" ", onlyNet48.Take(40)) + (onlyNet48.Count > 40 ? " …" : ""));
            if (onlyNet9.Count > 0) Console.WriteLine("  only core:  " + string.Join(" ", onlyNet9.Take(40)) + (onlyNet9.Count > 40 ? " …" : ""));
        }
        // Where the differences fall: the engine reads ASCII keys and Hangul; other scripts only reach it as surrounding text.
        var blocks = new SortedDictionary<string, int>(StringComparer.Ordinal);
        foreach (int c in touched)
        {
            string block = c < 0x80 ? "ASCII" : c < 0x100 ? "Latin-1" : (c >= 0x1100 && c < 0x1200) || (c >= 0x3130 && c < 0x3190)
                || (c >= 0xa960 && c < 0xa980) || (c >= 0xac00 && c < 0xd800) ? "Hangul" : "other";
            int n;
            blocks.TryGetValue(block, out n);
            blocks[block] = n + 1;
        }
        Console.WriteLine("code points that differ by block: " + string.Join(" · ", blocks.Select(kv => kv.Key + " " + kv.Value)));
        foreach (int c in touched.Where(x => x < 0x100)) Console.WriteLine("  U+" + c.ToString("X4") + " " + (char)c);
        Console.WriteLine(differences == 0 ? "IDENTICAL" : "DIFFERENT: " + differences);
        return differences == 0 ? 0 : 1;
    }
}
