using System;
using System.Diagnostics;
using System.Drawing;
using System.Linq;
using System.Threading;
using System.Windows.Automation;

namespace HanFlow
{
    internal sealed class FocusSnapshot
    {
        internal IntPtr Window, NativeFocus;
        internal string Identity = "", ProcessName = "", Reason = "입력 칸 확인 중", Diagnostic = "";
        internal bool Known, Password, Writable, Elevated, Focused, Pending;
        internal long Ticks, Generation;
        internal Point Position;
        // A pending copy keeps the last verified properties while a refresh is in flight. Capture may continue
        // against an unchanged window and native focus; the candidate window waits for a fresh verification.
        internal FocusSnapshot AsPending(long generation)
        {
            return new FocusSnapshot { Window = Window, NativeFocus = NativeFocus, Identity = Identity, ProcessName = ProcessName, Reason = Reason,
                Diagnostic = Diagnostic, Known = Known, Password = Password, Writable = Writable, Elevated = Elevated, Focused = Focused,
                Pending = true, Ticks = Ticks, Generation = generation, Position = Position };
        }
    }
    internal sealed class FocusMonitor : IDisposable
    {
        // Identity (window + native focus) is the safety boundary. Age only protects against a stalled worker.
        internal const long CaptureStaleMs = 3000, FreshMs = 1500, SettleMs = 20;
        readonly Thread worker;
        readonly int ownIntegrity;
        volatile bool stopped;
        long generation, notBefore;
        internal string LastInvalidation = "startup", QueryPhase = "startup";
        internal volatile FocusSnapshot Snapshot = new FocusSnapshot();
        internal FocusMonitor()
        {
            ownIntegrity = Native.IntegrityLevel((uint)Process.GetCurrentProcess().Id);
            worker = new Thread(Run) { IsBackground = true, Name = "HanFlow field properties" };
            worker.SetApartmentState(ApartmentState.MTA); worker.Start();
        }
        internal void Invalidate(string reason = "explicit")
        {
            LastInvalidation = reason;
            long next = Interlocked.Increment(ref generation);
            Interlocked.Exchange(ref notBefore, DateTime.UtcNow.AddMilliseconds(SettleMs).Ticks);
            Snapshot = Snapshot.AsPending(next);
        }
        internal bool IsSafe(FocusSnapshot snapshot)
        { return GuardReason(snapshot) == "safe"; }
        // Fresh: verified after the latest invalidation, so its password/editor properties describe the current field.
        internal bool IsFresh(FocusSnapshot snapshot)
        {
            if (snapshot == null || snapshot.Pending || snapshot.Generation != Interlocked.Read(ref generation) || !IsSafe(snapshot)) return false;
            long age = (DateTime.UtcNow.Ticks - snapshot.Ticks) / TimeSpan.TicksPerMillisecond;
            return age >= 0 && age <= FreshMs;
        }
        internal string GuardReason(FocusSnapshot snapshot)
        {
            if (snapshot == null || snapshot.Ticks == 0) return "no-snapshot";
            IntPtr foreground = Native.GetForegroundWindow();
            var native = Native.GetFocus(foreground);
            if (snapshot.Window != foreground) return "window-changed";
            if (snapshot.NativeFocus != native.Focus) return "native-focus-changed";
            if (snapshot.Password || Native.IsProtectedNativeEdit(native.Focus)) return "protected";
            if (!snapshot.Known) return "unknown-field";
            if (!snapshot.Writable) return "read-only";
            if (snapshot.Elevated) return "higher-integrity";
            if (!snapshot.Focused) return "not-focused";
            long age = (DateTime.UtcNow.Ticks - snapshot.Ticks) / TimeSpan.TicksPerMillisecond;
            if (age < 0 || age > CaptureStaleMs) return "stale";
            return "safe";
        }
        static bool ReadBool(AutomationElement element, AutomationProperty property, out bool value)
        {
            object result = element.GetCurrentPropertyValue(property, true);
            value = result is bool && (bool)result; return result is bool;
        }
        int Integrity(uint pid, int fallback)
        {
            int level = Native.IntegrityLevel(pid);
            return level < 0 ? fallback : Math.Max(level, fallback);
        }
        void Run()
        {
            AutomationFocusChangedEventHandler changed = delegate { Invalidate("uia-focus-event"); };
            try { Automation.AddAutomationFocusChangedEventHandler(changed); } catch { }
            while (!stopped)
            {
                try
                {
                    long startGeneration = Interlocked.Read(ref generation);
                    long queryStarted = DateTime.UtcNow.Ticks;
                    QueryPhase = "native";
                    if (DateTime.UtcNow.Ticks < Interlocked.Read(ref notBefore)) { Thread.Sleep(10); continue; }
                    var snapshot = new FocusSnapshot();
                    snapshot.Window = Native.GetForegroundWindow();
                    var native = Native.GetFocus(snapshot.Window);
                    snapshot.NativeFocus = native.Focus;
                    uint pid; Native.GetWindowThreadProcessId(snapshot.Window, out pid);
                    int integrity = Native.IntegrityLevel(pid);
                    snapshot.Elevated = integrity < 0 || ownIntegrity < 0 || integrity > ownIntegrity;
                    using (var process = Process.GetProcessById((int)pid)) snapshot.ProcessName = process.ProcessName;
                    if (snapshot.Elevated) snapshot.Reason = "높은 권한 또는 접근 불가 · 기본 입력";
                    else
                    {
                        QueryPhase = "focused-element";
                        AutomationElement field = AutomationElement.FocusedElement;
                        if (field != null)
                        {
                            QueryPhase = "field-properties";
                            bool password, focused, enabled;
                            bool properties = ReadBool(field, AutomationElement.IsPasswordProperty, out password)
                                & ReadBool(field, AutomationElement.HasKeyboardFocusProperty, out focused)
                                & ReadBool(field, AutomationElement.IsEnabledProperty, out enabled);
                            snapshot.Password = password; snapshot.Focused = focused && enabled;
                            object type = field.GetCurrentPropertyValue(AutomationElement.ControlTypeProperty, true);
                            bool editType = ControlType.Edit.Equals(type) || ControlType.Document.Equals(type);
                            object valuePattern, textPattern;
                            bool hasValue = field.TryGetCurrentPattern(ValuePattern.Pattern, out valuePattern);
                            bool hasText = field.TryGetCurrentPattern(TextPattern.Pattern, out textPattern);
                            // Web editors (contenteditable regions) can surface as Group, Pane or Custom; a value or text pattern marks them.
                            bool containerEditor = (ControlType.Group.Equals(type) || ControlType.Pane.Equals(type) || ControlType.Custom.Equals(type)) && (hasValue || hasText);
                            bool editor = editType || containerEditor;
                            // The focused element may legitimately belong to another process (WebView2, browser sandboxes,
                            // out-of-process hosts). Elevation is checked for that process too; equality is not required.
                            object fieldPid = field.GetCurrentPropertyValue(AutomationElement.ProcessIdProperty, true);
                            if (fieldPid is int && (int)fieldPid != (int)pid)
                            {
                                int fieldIntegrity = Integrity((uint)(int)fieldPid, integrity);
                                snapshot.Elevated = fieldIntegrity < 0 || fieldIntegrity > ownIntegrity;
                            }
                            snapshot.Diagnostic = "properties=" + properties + " editor=" + editor + " type=" + (type is ControlType ? ((ControlType)type).ProgrammaticName : "?") +
                                " value=" + hasValue + " text=" + hasText + " focused=" + focused + " password=" + password;
                            snapshot.Identity = string.Join(".", field.GetRuntimeId().Select(x => x.ToString()).ToArray());
                            if (properties && editor && !password && !snapshot.Elevated)
                            {
                                QueryPhase = "writability";
                                if (hasValue) snapshot.Writable = !((ValuePattern)valuePattern).Current.IsReadOnly;
                                else if (hasText)
                                {
                                    // Providers that do not report the attribute (or report it as mixed) keep the field writable;
                                    // explicit read-only reports and native ES_READONLY styles still exclude it.
                                    object readOnly = ((TextPattern)textPattern).DocumentRange.GetAttributeValue(TextPattern.IsReadOnlyAttribute);
                                    snapshot.Writable = !(readOnly is bool && (bool)readOnly);
                                }
                                else snapshot.Writable = editType;
                                snapshot.Known = true;
                            }
                            snapshot.Reason = password ? "비밀번호 칸 · 기본 입력" : snapshot.Writable ? "자동 판별 준비" : "확인된 편집 칸에서만 동작";
                            QueryPhase = "bounds";
                            var rect = field.Current.BoundingRectangle;
                            snapshot.Position = new Point((int)rect.Left + 8, (int)Math.Min(rect.Bottom, rect.Top + 80) + 6);
                            if (native.Caret != IntPtr.Zero)
                            {
                                var caret = new Native.Point { X = native.CaretRect.Left, Y = native.CaretRect.Bottom + 10 };
                                if (Native.ClientToScreen(native.Caret, ref caret)) snapshot.Position = new Point(caret.X, caret.Y);
                            }
                        }
                    }
                    // A query can be slow or outlive a focus change. Never renew an old target's lease.
                    if (startGeneration == Interlocked.Read(ref generation) && snapshot.Window == Native.GetForegroundWindow() &&
                        snapshot.NativeFocus == Native.GetFocus(snapshot.Window).Focus)
                    {
                        snapshot.Generation = startGeneration; snapshot.Ticks = queryStarted;
                        Snapshot = snapshot;
                    }
                    QueryPhase = "idle";
                }
                catch
                {
                    // Transient UIA failures (element gone, provider busy) keep the last verified target as pending;
                    // identity checks still guard it, and CaptureStaleMs ends the lease if the failure persists.
                    QueryPhase = "exception";
                    FocusSnapshot previous = Snapshot;
                    Snapshot = previous.Ticks != 0 ? previous.AsPending(Interlocked.Read(ref generation)) : new FocusSnapshot { Reason = "입력 대상 확인 불가 · 기본 입력" };
                }
                Thread.Sleep(65);
            }
            try { Automation.RemoveAutomationFocusChangedEventHandler(changed); } catch { }
        }
        public void Dispose() { stopped = true; Invalidate(); }
    }
}
