using System;
using System.Collections.Generic;
using System.Threading;
using System.Windows.Automation;
using System.Windows.Automation.Text;

namespace HanFlow
{
    // Reads the text the user selected in the focused field. UI Automation first (no clipboard involvement); a
    // tagged Ctrl+C with clipboard restore only when the provider exposes no selection. Runs on a worker, never in the hook.
    internal static class SelectionReader
    {
        internal const int MaxLength = 4096;
        internal static string Read(out string source, out string error)
        {
            source = "uia"; error = "";
            try
            {
                AutomationElement element = AutomationElement.FocusedElement;
                object pattern;
                if (element != null && element.TryGetCurrentPattern(TextPattern.Pattern, out pattern))
                {
                    TextPatternRange[] ranges = ((TextPattern)pattern).GetSelection();
                    if (ranges != null && ranges.Length > 0)
                    {
                        string text = ranges[0].GetText(MaxLength + 1);
                        if (!string.IsNullOrEmpty(text)) return text;
                    }
                }
            }
            catch { }
            source = "clipboard";
            return CopyThroughClipboard(out error);
        }
        static string CopyThroughClipboard(out string error)
        {
            error = "";
            int state = Native.ClipboardState();
            if (state == 2) { error = "클립보드에 글자가 아닌 내용이 있어 선택 영역을 읽지 않았습니다."; return null; }
            string previous = state == 1 ? Native.ReadClipboardText() : null;
            uint before = Native.GetClipboardSequenceNumber();
            var copy = new List<Native.Input>
            {
                Native.Key(0x11, false, Native.OutputTag, false), Native.Key(0x43, false, Native.OutputTag, false),
                Native.Key(0x43, true, Native.OutputTag, false), Native.Key(0x11, true, Native.OutputTag, false)
            };
            if (!Native.Inject(copy)) { error = "선택 영역 복사 키를 보내지 못했습니다."; return null; }
            string text = null;
            for (int attempt = 0; attempt < 16 && text == null; attempt++)
            {
                Thread.Sleep(25);
                if (Native.GetClipboardSequenceNumber() != before) text = Native.ReadClipboardText();
            }
            // Put the user's clipboard back exactly as found: previous text, or empty.
            Native.WriteClipboardText(previous);
            if (text == null) error = "선택한 글자를 읽지 못했습니다. 글자를 선택한 뒤 Shift+F2를 누르세요.";
            return text;
        }
    }
}
