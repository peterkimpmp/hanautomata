using System.Collections.Generic;
using System.Text;

namespace Hanautomata
{
    // Windows 11 Notepad (11.2607) defers keys while it checks the word a boundary just ended and then turns deferred Unicode
    // (VK_PACKET) keys into copies of the newest one. Measured 2026-10-17: a batch "할 수 있다" kept only "할"; "com done" after
    // "example." came out "eeeeeee"; backspaces and text in one batch left spaces; virtual-key batches lost letters and Shift.
    // After Latin words 110 ms failed 4 of 5 and 200 ms passed 5 of 5, yet a long word once took more than 250 ms; waiting 150 ms
    // and then until the process went quiet passed 40 of 40. Its check starts late: Enter 0-30 ms after "안녕하세요!" was lost in
    // 5 of 32 runs (the process still looked quiet), 50-200 ms in 0 of 32. Hence at least 60 ms after any boundary, then quiet.
    // Backspaces that erase a boundary need longer: the last of "할 거야 " was lost in 1 of 10 runs 60 ms later, 0 of 30 at 100-250 ms.
    // Word, Edge and WinForms kept a single batch. Output is therefore split after each boundary that more text follows and
    // InputController sends each part once the target has settled.
    public static class OutputSegments
    {
        public static List<string> Split(string text)
        {
            var parts = new List<string>(); var part = new StringBuilder();
            text = text ?? "";
            for (int i = 0; i < text.Length; i++)
            {
                part.Append(text[i]);
                if (Boundary(text[i]) && i + 1 < text.Length) { parts.Add(part.ToString()); part.Length = 0; }
            }
            if (part.Length > 0 || parts.Count == 0) parts.Add(part.ToString());
            return parts;
        }
        // Backspace counts that erase text from its end, each step ending right after an erased boundary.
        public static List<int> EraseSteps(string erased)
        {
            var steps = new List<int>(); int count = 0;
            erased = erased ?? "";
            for (int i = erased.Length - 1; i >= 0; i--)
            {
                count++;
                if (Boundary(erased[i]) && i > 0) { steps.Add(count); count = 0; }
            }
            if (count > 0) steps.Add(count);
            return steps;
        }
        // Latin words are spell-checked at a boundary, which takes Notepad far longer than Hangul.
        public static bool HasLatinLetter(string text)
        {
            foreach (char c in text ?? "") if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')) return true;
            return false;
        }
        public static bool Boundary(char c)
        { return c == ' ' || c == '\t' || c == '\n' || char.IsPunctuation(c) || char.IsSymbol(c); }
    }
}
