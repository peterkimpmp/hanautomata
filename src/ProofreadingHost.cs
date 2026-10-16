using System;
using System.Linq;
using System.Windows.Automation;
using System.Windows.Automation.Text;
namespace Hanautomata
{
    internal static class ProofreadingHost
    {
        // Outside the keyboard hook; no clipboard fallback for an unverifiable range.
        internal static void Show(InputController input,FocusMonitor monitor)
        {
            try {
                var focus=monitor.Snapshot;
                lock (input.Sync) { if (!input.Enabled || !input.Composition.IsEmpty || !monitor.IsSafe(focus) || input.Excluded.Contains(focus.ProcessName)) return; }
                var field=AutomationElement.FocusedElement; object raw;
                if (field==null || field.Current.IsPassword || !field.TryGetCurrentPattern(TextPattern.Pattern,out raw)) return;
                var pattern=(TextPattern)raw; var selections=pattern.GetSelection(); if (selections.Length!=1) return;
                var range=selections[0].Clone(); string text=range.GetText(4097);
                if (string.IsNullOrEmpty(text) || text.Length>4096) return;
                var identity=field.GetRuntimeId();
                Func<bool> same=delegate {
                    try {
                        lock (input.Sync) { if (!input.Enabled || !input.Composition.IsEmpty || !monitor.IsSafe(focus) || input.Excluded.Contains(focus.ProcessName)) return false; }
                        var current=AutomationElement.FocusedElement;
                        if (current==null || Native.GetForegroundWindow()!=focus.Window || current.Current.IsPassword
                            || !current.Current.HasKeyboardFocus || !identity.SequenceEqual(current.GetRuntimeId())
                            || Native.GetFocus(focus.Window).Focus!=focus.NativeFocus) return false;
                        var selected=pattern.GetSelection();
                        return selected.Length==1 && selected[0].CompareEndpoints(TextPatternRangeEndpoint.Start,range,TextPatternRangeEndpoint.Start)==0
                            && selected[0].CompareEndpoints(TextPatternRangeEndpoint.End,range,TextPatternRangeEndpoint.End)==0 && selected[0].GetText(4097)==text;
                    } catch { return false; }
                };
                var request=new ProofRequest { request_id=Guid.NewGuid().ToString("N"), document_id=Guid.NewGuid().ToString("N"), revision="1", text=text, protected_words=new string[0] };
                var window=new ProofreadingWindow(request,same,delegate(string replacement) {
                    if (!same()) return false;
                    // UIA cannot atomically verify and replace a selection. Only an explicit copy is offered.
                    // Direct edits are reserved for the TSF read/write edit session adapter.
                    System.Windows.Forms.Clipboard.SetText(replacement); return true;
                },true); window.Show();
            } catch { input.LastError="맞춤법 검사: 선택 영역을 확인하지 못했습니다."; }
        }
    }
}
