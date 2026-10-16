using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Forms;

namespace Hanautomata
{
    // The low-level hooks live on a dedicated thread that only pumps hook messages and reinstall requests.
    // Windows removes a low-level hook silently when its callback overruns LowLevelHooksTimeout, so painting,
    // timers, diagnostics serialization and file I/O on the WinForms thread must never sit in front of it.
    // Shared state is guarded by Sync; the hook callback itself does bounded work and never touches disk.
    internal sealed class InputController : IDisposable
    {
        const uint ReconnectMessage = 0x8001, QuitMessage = 0x12;
        const long SwapWindowMs = 15000;
        internal readonly object Sync = new object();
        internal Action ProofreadRequested;
        readonly FocusMonitor focus;
        readonly Native.HookProc keyboardProc, mouseProc;
        readonly Native.WinEventProc focusProc;
        readonly HashSet<int> swallowed = new HashSet<int>();
        readonly Thread hookThread;
        readonly ManualResetEvent ready = new ManualResetEvent(false);
        readonly Stopwatch clock = Stopwatch.StartNew();
        uint hookThreadId;
        IntPtr keyboardHook, mouseHook, foregroundHook;
        string startupError; int startupCode;
        bool probePending; volatile bool disposed;
        long probeSent, nextProbe, nextReconnect;
        internal volatile bool HookHealthy;
        internal int HookRestarts, HardwareKeys, InjectedKeys, FilteredKeys;
        internal readonly Composition Composition;
        internal volatile bool Enabled = true;
        internal volatile bool SemicolonShortcut = true;
        internal volatile bool AutoSpacing = true;
        internal IntPtr TestWindow;
        internal volatile string Recovery = "", LastError = "";
        internal int Commits, CapturedKeys, BypassedKeys;
        internal volatile string LastBypass = "none";
        internal volatile InputDiagnostics Diagnostics;
        internal readonly HashSet<string> Excluded = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        FocusSnapshot owner;
        // The last word this controller committed into the current target; F2 right after Space swaps it in place.
        sealed class CommittedWord
        {
            internal string Raw, Keys, Text, Alternative, Trailing, Context;
            internal string Unspaced; // the decided text before automatic spacing; F2 restores it once
            internal bool Korean; internal FocusSnapshot Owner; internal long Time;
        }
        CommittedWord lastCommit;
        internal bool CanSwapLastCommit { get { lock (Sync) return lastCommit != null; } }
        // Raw Input sentinel. Windows removes a timed-out low-level hook without notice; a message-only window on the
        // hook thread also receives every hardware key through Raw Input (RIDEV_INPUTSINK). A hardware key-down that the
        // hook did not stamp first means the hook is gone; two in a row reinstall it without waiting for the probe cycle.
        // Keys the hook swallows never reach Raw Input and synthesized keys carry a null device, so neither can alarm.
        SentinelWindow sentinel; long lastHardwareTick = -1; int sentinelMisses; bool flipPending;
        internal volatile bool SentinelActive;
        internal volatile bool SentinelFallback;
        internal volatile int HookCallbacks;
        internal int RawHardwareKeys, SentinelAlarms, AutoConversions, SwapsAfterSpace, SelectionFlips, Respacings, RespacingReverts;
        // Words whose automatic spacing the writer undid with F2 stay as typed until the app exits. Memory only, bounded.
        readonly HashSet<string> respacingDeclined = new HashSet<string>(StringComparer.Ordinal);
        // Paced output (see OutputSegments). Windows 11 Notepad defers keys while it checks the word a boundary just ended, then
        // turns deferred Unicode keys into copies of the newest one: text sent right after a boundary, after backspaces in the same
        // batch, or keys replayed in one batch came out lost or garbled. Output that would follow a boundary too soon is queued and a
        // pacer thread sends each step once the target has settled. Until the queue is empty every other key event is held in
        // arrival order, then replayed one at a time with ReplayTag; the hook processes each (queueing any output it makes) first.
        internal const int SettleMs = 60, CheckSettleMs = 150, SettleMaxMs = 400, SettleFallbackMs = 100, QuietWindowMs = 15, ReplayAckMs = 1000;
        internal const int PaceSegmentLimit = 64;
        const ulong QuietCyclesPerMs = 200000;
        sealed class OutputStep
        {
            internal List<Native.Input> Prefix, Suffix; internal string Text = ""; internal FocusSnapshot Target;
            internal bool Aborted, Failed;
        }
        readonly Queue<OutputStep> outputs = new Queue<OutputStep>();
        readonly Queue<Native.Input> held = new Queue<Native.Input>();
        bool pacerRunning; long replaySent, replayAcked;
        // The last output ended at a word boundary at settleAt (clock ms, -1 once settled) in settleWindow. Only Notepad, which
        // spell-checks the word meanwhile, is also waited on until its process goes quiet: Word stays busy and would always take the
        // full SettleMaxMs, so other targets get the plain SettleMs gap.
        long settleAt = -1; int settleMin; IntPtr settleWindow; bool settleQuiet;
        internal int PacedOutputs, HeldKeys, OutputFailures, SettleTimeouts;
        internal bool OutputIdle { get { lock (Sync) return !pacerRunning; } }

        internal InputController(FocusMonitor focus, Detector detector)
        {
            this.focus = focus; Composition = new Composition(detector);
            keyboardProc = Keyboard; mouseProc = Mouse;
            focusProc = delegate { AbortPacing(); PreservePending(); focus.Invalidate("foreground-event"); };
            hookThread = new Thread(HookLoop) { IsBackground = true, Name = "Hanautomata keyboard hook" };
            hookThread.Start();
            ready.WaitOne();
            if (startupError != null) { Dispose(); throw new Win32Exception(startupCode, startupError); }
        }
        void HookLoop()
        {
            hookThreadId = Native.GetCurrentThreadId();
            IntPtr module = Native.GetModuleHandle(null);
            keyboardHook = Native.SetWindowsHookEx(13, keyboardProc, module, 0);
            int code = Marshal.GetLastWin32Error();
            mouseHook = Native.SetWindowsHookEx(14, mouseProc, module, 0);
            if (mouseHook == IntPtr.Zero) code = Marshal.GetLastWin32Error();
            foregroundHook = Native.SetWinEventHook(3, 3, IntPtr.Zero, focusProc, 0, 0, 0);
            if (keyboardHook == IntPtr.Zero || mouseHook == IntPtr.Zero) { startupCode = code; startupError = "키보드 후크를 설치하지 못했습니다."; }
            ready.Set();
            if (startupError != null) { ReleaseHooks(); return; }
            try
            {
                sentinel = new SentinelWindow(this);
                var devices = new[] { new Native.RawInputDevice { UsagePage = 1, Usage = 6, Flags = 0x100, Target = sentinel.Handle } };
                SentinelActive = Native.RegisterRawInputDevices(devices, 1, (uint)Marshal.SizeOf(typeof(Native.RawInputDevice)));
            }
            catch { SentinelActive = false; }
            Native.Message message;
            while (Native.GetMessage(out message, IntPtr.Zero, 0, 0) > 0)
            {
                if (message.Window == IntPtr.Zero && message.Id == ReconnectMessage) { ReinstallKeyboardHook(); continue; }
                Native.TranslateMessage(ref message); Native.DispatchMessage(ref message);
            }
            if (sentinel != null) { sentinel.Release(); sentinel = null; SentinelActive = false; }
            ReleaseHooks();
        }
        sealed class SentinelWindow : NativeWindow
        {
            readonly InputController owner; readonly IntPtr buffer = Marshal.AllocHGlobal(256);
            internal SentinelWindow(InputController owner)
            { this.owner = owner; CreateHandle(new CreateParams { Parent = new IntPtr(-3), Caption = "Hanautomata sentinel" }); }
            protected override void WndProc(ref Message m)
            {
                if (m.Msg == 0xff) owner.RawInput(m.LParam, buffer);
                base.WndProc(ref m);
            }
            internal void Release() { DestroyHandle(); Marshal.FreeHGlobal(buffer); }
        }
        void RawInput(IntPtr handle, IntPtr buffer)
        {
            uint size = 256;
            if (Native.GetRawInputData(handle, 0x10000003, buffer, ref size, (uint)Native.RawHeaderSize) == uint.MaxValue) return;
            if (Marshal.ReadInt32(buffer, 0) != 1) return; // RIM_TYPEKEYBOARD only
            IntPtr device = Marshal.ReadIntPtr(buffer, 8);
            int messageId = Marshal.ReadInt32(buffer, Native.RawHeaderSize + 8);
            if (device == IntPtr.Zero || (messageId != 0x100 && messageId != 0x104)) return; // synthesized or key-up
            lock (Sync)
            {
                RawHardwareKeys++;
                long now = clock.ElapsedMilliseconds;
                bool seen = lastHardwareTick >= 0 && now - lastHardwareTick <= 250;
                sentinelMisses = seen ? 0 : sentinelMisses + 1;
                if (sentinelMisses < 2) return;
                sentinelMisses = 0; SentinelAlarms++; HookHealthy = false;
                if (now >= nextReconnect) { nextReconnect = now + 5000; LastError = "키 입력 연결이 끊겨 다시 연결합니다."; RequestReconnect(); }
            }
        }
        // Shift+F2 with nothing composing: flip the selected text between Hangul and QWERTY letters in place.
        void RequestSelectionFlip(FocusSnapshot target)
        {
            if (flipPending || target == null) return;
            flipPending = true; lastCommit = null;
            ThreadPool.QueueUserWorkItem(delegate { FlipSelection(target); });
        }
        void FlipSelection(FocusSnapshot target)
        {
            try
            {
                var now = focus.Snapshot;
                if (!focus.IsSafe(now) || now.Window != target.Window || now.NativeFocus != target.NativeFocus)
                { LastError = "선택 영역 변환: 입력 칸이 바뀌어 취소했습니다."; return; }
                string source, error;
                string selected = SelectionReader.Read(out source, out error);
                if (string.IsNullOrEmpty(selected)) { LastError = error.Length > 0 ? error : "선택한 글자가 없습니다. 글자를 선택한 뒤 Shift+F2를 누르세요."; return; }
                if (selected.Length > SelectionReader.MaxLength) { LastError = "선택 영역이 너무 깁니다 (4,096자 이내)."; return; }
                string flipped = Hangul.Flip(selected);
                if (flipped == selected) { LastError = "선택 영역에 바꿀 글자가 없습니다."; return; }
                List<string> parts = Parts(flipped); OutputStep last = null;
                lock (Sync)
                    if (!pacerRunning) { pacerRunning = true; last = QueueOutput(null, parts, null, target); Composition.Clear(); owner = null; lastCommit = null; }
                if (last == null) { LastError = "선택 영역 변환: 앞의 입력을 보내는 중이라 취소했습니다. 다시 누르세요."; return; }
                // Sent on this worker thread like paced output; keys typed meanwhile are held and replayed after it.
                Drain();
                if (last.Failed) return;
                // Composition was cleared before pacing: keys replayed after the flip may already be composing.
                lock (Sync) { SelectionFlips++; LastError = ""; }
                focus.Invalidate("selection-flip");
            }
            catch { LastError = "선택 영역 변환 중 오류가 났습니다."; }
            finally { lock (Sync) flipPending = false; }
        }
        void ReleaseHooks()
        {
            if (keyboardHook != IntPtr.Zero) Native.UnhookWindowsHookEx(keyboardHook);
            if (mouseHook != IntPtr.Zero) Native.UnhookWindowsHookEx(mouseHook);
            if (foregroundHook != IntPtr.Zero) Native.UnhookWinEvent(foregroundHook);
            keyboardHook = mouseHook = foregroundHook = IntPtr.Zero;
        }
        // Runs on the hook thread only: SetWindowsHookEx binds the hook to the installing thread's message loop.
        void ReinstallKeyboardHook()
        {
            if (keyboardHook != IntPtr.Zero) Native.UnhookWindowsHookEx(keyboardHook);
            keyboardHook = Native.SetWindowsHookEx(13, keyboardProc, Native.GetModuleHandle(null), 0);
            lock (Sync)
            {
                HookRestarts++; nextProbe = clock.ElapsedMilliseconds;
                if (keyboardHook == IntPtr.Zero) LastError = "키 입력 연결 실패 · 잠시 후 다시 연결합니다.";
            }
        }
        // Hook thread, under Sync (as Commit). Sends the text now when nothing is queued and the target has settled; otherwise
        // queues it for the pacer. False when nothing could be sent.
        bool Emit(string erase, string text, List<Native.Input> suffix, FocusSnapshot target)
        {
            List<string> parts = Parts(text);
            if (!pacerRunning && erase == null && parts.Count == 1 && (suffix == null || !EndsAtBoundary(parts[0])) && !Unsettled())
            {
                var batch = Native.Text(parts[0]);
                if (suffix != null) batch.AddRange(suffix);
                if (!Native.Inject(batch)) return false;
                Sent(parts[0], suffix != null, target);
                return true;
            }
            QueueOutput(erase, parts, suffix, target);
            if (!pacerRunning)
            {
                pacerRunning = true;
                new Thread(Drain) { IsBackground = true, Name = "Hanautomata paced output" }.Start();
            }
            return true;
        }
        // Under Sync. Returns the last step queued; a failure marks it and every step queued behind the one that failed.
        OutputStep QueueOutput(string erase, List<string> parts, List<Native.Input> suffix, FocusSnapshot target)
        {
            if (erase != null || parts.Count > 1) PacedOutputs++;
            // Backspaces go out apart from the text, split after each erased boundary: Notepad garbled Unicode keys that followed
            // backspaces in the same batch and once lost two of five backspaces that followed an erased space.
            if (erase != null)
                foreach (int count in OutputSegments.EraseSteps(erase))
                {
                    var keys = new List<Native.Input>();
                    for (int i = 0; i < count; i++) { keys.Add(Native.Key(0x08, false, Native.OutputTag, false)); keys.Add(Native.Key(0x08, true, Native.OutputTag, false)); }
                    outputs.Enqueue(new OutputStep { Prefix = keys, Target = target });
                }
            OutputStep last = null;
            for (int i = 0; i < parts.Count; i++)
                outputs.Enqueue(last = new OutputStep { Text = parts[i], Suffix = i == parts.Count - 1 ? suffix : null, Target = target });
            // A trailing key (Enter, a modifier) after a boundary is text after a boundary too: Notepad once dropped "!" + Enter's Enter.
            if (suffix != null && EndsAtBoundary(last.Text)) { last.Suffix = null; outputs.Enqueue(last = new OutputStep { Suffix = suffix, Target = target }); }
            return last;
        }
        static bool EndsAtBoundary(string text) { return text.Length > 0 && OutputSegments.Boundary(text[text.Length - 1]); }
        // More parts than PaceSegmentLimit (a long selection) go out as one batch, as before 0.6.0.
        static List<string> Parts(string text)
        {
            List<string> parts = OutputSegments.Split(text);
            return parts.Count > PaceSegmentLimit ? new List<string> { text } : parts;
        }
        // Under Sync, after a batch reached the target. Text ending at a word boundary, a trailing key or an erase starts a settle.
        void Sent(string text, bool key, FocusSnapshot target, bool erase = false)
        {
            if (!key && text.Length > 0 && !EndsAtBoundary(text)) { settleAt = -1; return; }
            settleAt = clock.ElapsedMilliseconds; settleWindow = target != null ? target.Window : Native.GetForegroundWindow();
            string process = (target ?? focus.Snapshot).ProcessName;
            settleQuiet = string.Equals(process, "Notepad", StringComparison.OrdinalIgnoreCase);
            // Notepad re-checks longer after Latin words and after backspaces: one of ten erases lost a backspace 60 ms later, none at 100-250 ms.
            settleMin = settleQuiet && (erase || OutputSegments.HasLatinLetter(text)) ? CheckSettleMs : SettleMs;
        }
        bool Unsettled() { return settleAt >= 0 && clock.ElapsedMilliseconds - settleAt < (settleQuiet ? SettleMaxMs : settleMin); }
        // Pacer thread (or the selection flip worker that reserved it). SendInput is never called here while Sync is held: the hook
        // thread waits on Sync for every key, and Windows removes a low-level hook whose callback overruns LowLevelHooksTimeout.
        void Drain()
        {
            while (true)
            {
                OutputStep step = null; Native.Input key;
                lock (Sync)
                {
                    if (outputs.Count > 0) { step = outputs.Peek(); key = default(Native.Input); }
                    else if (held.Count > 0) key = held.Dequeue();
                    else { pacerRunning = false; return; }
                }
                Settle();
                if (step != null) Send(step); else Replay(key);
            }
        }
        void Send(OutputStep step)
        {
            var batch = new List<Native.Input>();
            if (step.Prefix != null) batch.AddRange(step.Prefix);
            batch.AddRange(Native.Text(step.Text));
            if (step.Suffix != null) batch.AddRange(step.Suffix);
            bool moved;
            lock (Sync)
            {
                // Never finish the output in another field or at a caret the writer moved with the mouse.
                var now = focus.Snapshot;
                moved = disposed || step.Aborted || !focus.IsSafe(now) || now.Window != step.Target.Window || now.NativeFocus != step.Target.NativeFocus;
                outputs.Dequeue();
            }
            bool failed = !moved && !Native.Inject(batch);
            lock (Sync)
            {
                if (!moved && !failed) { Sent(step.Text, step.Prefix != null || step.Suffix != null, step.Target, step.Prefix != null); return; }
                // The failed step and everything queued behind it go to Recovery; keys typed meanwhile are still replayed.
                step.Failed = true; var rest = new System.Text.StringBuilder(step.Text);
                while (outputs.Count > 0) { OutputStep next = outputs.Dequeue(); next.Failed = true; rest.Append(next.Text); }
                Recovery = rest.ToString(); lastCommit = null; OutputFailures++;
                LastError = moved ? "입력 칸이 바뀌어 나머지 글자를 보내지 않았습니다. 트레이 메뉴에서 복사할 수 있습니다." :
                    "입력 전송을 마치지 못했습니다. 남은 글자는 트레이 메뉴에서 복사할 수 있습니다.";
            }
        }
        // One held key event; the hook acknowledges it under Sync once processed, so output it makes is queued before the next key.
        void Replay(Native.Input key)
        {
            long ticket;
            lock (Sync) ticket = ++replaySent;
            if (!Native.Inject(new List<Native.Input> { key }))
            { lock (Sync) { replayAcked = Math.Max(replayAcked, ticket); LastError = "보관한 키 입력을 다시 보내지 못했습니다."; } return; }
            lock (Sync)
            {
                long deadline = clock.ElapsedMilliseconds + ReplayAckMs;
                while (replayAcked < ticket)
                {
                    long left = deadline - clock.ElapsedMilliseconds;
                    if (left <= 0) { replayAcked = ticket; break; } // hook gone: the key reached the application unprocessed
                    Monitor.Wait(Sync, (int)left);
                }
            }
        }
        // Pacer thread, Sync not held. After a word boundary, waits at least settleMin and, in Notepad, then until its process has
        // gone quiet (it spell-checks the word meanwhile), at most SettleMaxMs after the boundary.
        void Settle()
        {
            long at; int min; IntPtr window; bool quiet;
            lock (Sync) { at = settleAt; min = settleMin; window = settleWindow; quiet = settleQuiet; }
            if (at < 0) return;
            long elapsed = clock.ElapsedMilliseconds - at;
            if (elapsed < min) Thread.Sleep((int)(min - elapsed));
            if (quiet && elapsed < SettleMaxMs)
            {
                uint pid; Native.GetWindowThreadProcessId(window, out pid);
                IntPtr process = pid == 0 ? IntPtr.Zero : Native.OpenProcess(0x1000, false, pid);
                try
                {
                    ulong last, now;
                    if (process == IntPtr.Zero || !Native.QueryProcessCycleTime(process, out last))
                    {
                        long rest = Math.Min(at + min + SettleFallbackMs, at + SettleMaxMs) - clock.ElapsedMilliseconds;
                        if (rest > 0) Thread.Sleep((int)rest);
                    }
                    else
                        while (true)
                        {
                            if (clock.ElapsedMilliseconds - at >= SettleMaxMs) { lock (Sync) SettleTimeouts++; break; }
                            long started = clock.ElapsedMilliseconds;
                            Thread.Sleep(QuietWindowMs);
                            long span = Math.Max(1, clock.ElapsedMilliseconds - started);
                            if (!Native.QueryProcessCycleTime(process, out now) || now - last < QuietCyclesPerMs * (ulong)span) break;
                            last = now;
                        }
                }
                finally { if (process != IntPtr.Zero) Native.CloseHandle(process); }
            }
            lock (Sync) if (settleAt == at) settleAt = -1;
        }
        // Called under Sync for every key event before it is processed. Our own output, replays and lower-integrity injections pass.
        bool Holding(Native.KeyboardData data)
        { return pacerRunning && data.Extra != Native.ReplayTag && !Native.BypassKeyboard(data); }
        // A click or a foreground change cancels the output queued so far; keys typed meanwhile are still replayed.
        void AbortPacing() { lock (Sync) foreach (OutputStep step in outputs) step.Aborted = true; }
        internal void PreservePending()
        {
            lock (Sync)
            {
                if (!Composition.IsEmpty) Recovery = Composition.Current.Text;
                Composition.Clear(); owner = null; lastCommit = null;
            }
        }
        internal void ValidatePending()
        {
            lock (Sync) if (owner != null && !SameOwner()) PreservePending();
        }
        internal void CheckHook()
        {
            if (disposed) return;
            var snapshot = focus.Snapshot;
            lock (Sync)
            {
                // No probes in passwords, unknown/elevated targets, excluded apps, or outside a test fixture.
                if (!focus.IsSafe(snapshot) || Excluded.Contains(snapshot.ProcessName) ||
                    (TestWindow != IntPtr.Zero && Native.GetForegroundWindow() != TestWindow))
                { probePending = false; return; }
                long now = clock.ElapsedMilliseconds;
                if (probePending && now - probeSent >= 400)
                {
                    probePending = false; HookHealthy = false;
                    // Some Windows input stacks stop delivering low-level callbacks as soon as Raw Input is
                    // registered. Only degrade when no callback has ever arrived, including our tagged probe.
                    // Keep the established probe/reconnect watchdog; never leave all input uncaptured silently.
                    if (SentinelActive && HookCallbacks == 0)
                    {
                        var devices = new[] { new Native.RawInputDevice { UsagePage = 1, Usage = 6, Flags = 1, Target = IntPtr.Zero } };
                        if (Native.RegisterRawInputDevices(devices, 1, (uint)Marshal.SizeOf(typeof(Native.RawInputDevice))))
                        {
                            SentinelActive = false; SentinelFallback = true;
                            LastError = "Raw Input 감시 호환 문제로 기본 연결 검사를 사용합니다.";
                        }
                    }
                    if (now >= nextReconnect) { nextReconnect = now + 5000; RequestReconnect(); }
                    nextProbe = now;
                }
                if (probePending || now < nextProbe) return;
                probePending = true; probeSent = now; nextProbe = now + 2000;
            }
            if (!Native.Inject(new List<Native.Input> { Native.Key(Native.ProbeKey, true, Native.ProbeTag, false) }))
                lock (Sync) { probePending = false; HookHealthy = false; }
        }
        internal void Reconnect()
        {
            if (disposed) return;
            lock (Sync) RequestReconnect();
        }
        void RequestReconnect()
        {
            PreservePending(); swallowed.Clear(); HookHealthy = false; probePending = false;
            Native.PostThreadMessage(hookThreadId, ReconnectMessage, IntPtr.Zero, IntPtr.Zero);
        }
        bool SameOwner()
        {
            var now = focus.Snapshot;
            return owner != null && focus.IsSafe(now) && owner.Window == now.Window &&
                owner.NativeFocus == now.NativeFocus && owner.Identity == now.Identity;
        }
        IntPtr Mouse(int code, IntPtr message, IntPtr data)
        {
            if (code >= 0 && (message.ToInt32() == 0x201 || message.ToInt32() == 0x204 || message.ToInt32() == 0x207 || message.ToInt32() == 0x20b || message.ToInt32() == 0x20a))
            { if (message.ToInt32() != 0x20a) AbortPacing(); PreservePending(); focus.Invalidate("mouse"); }
            return Native.CallNextHookEx(mouseHook, code, message, data);
        }
        IntPtr Pass(int code, IntPtr message, IntPtr data)
        { BypassedKeys++; return Native.CallNextHookEx(keyboardHook, code, message, data); }
        IntPtr Eat(int key) { swallowed.Add(key); CapturedKeys++; return new IntPtr(1); }
        // The text a commit of this decision types: Korean dependent-noun spacing applied unless the writer turned it off or undid it.
        internal string Spaced(string text)
        { lock (Sync) return AutoSpacing && !respacingDeclined.Contains(text) ? Spacing.Correct(text) : text; }
        bool Commit(int trailingKey, bool extended, bool keyDownOnly, string literalSuffix = null)
        {
            if (!SameOwner()) { PreservePending(); return false; }
            Decision current = Composition.Current;
            string text = current.Text, typed = Spaced(text);
            bool respaced = typed != text;
            var suffix = new List<Native.Input>();
            if (trailingKey > 0 && trailingKey != 0x20)
            {
                suffix.Add(Native.Key((ushort)trailingKey, false, Native.OutputTag, extended));
                if (!keyDownOnly) suffix.Add(Native.Key((ushort)trailingKey, true, Native.OutputTag, extended));
            }
            // The physical modifier's down event has not reached the target yet when called from its hook.
            if (!Emit(null, typed + (literalSuffix ?? "") + (trailingKey == 0x20 ? " " : ""), suffix, owner))
            {
                Recovery = text; LastError = "입력 전송을 완료하지 못해 일시정지했습니다. 보관된 조합을 확인하세요.";
                Enabled = false; Composition.Clear(); owner = null; lastCommit = null; return false;
            }
            string raw = Composition.Raw, keys = Composition.Keys, context = Composition.Context;
            string trailing = trailingKey == 0x20 ? " " : (literalSuffix ?? "");
            bool swappable = (current.Text != current.Alternative || respaced) && (trailingKey == 0x20 || trailingKey == 0) &&
                (trailing.Length == 0 || !char.IsSurrogate(trailing[0]));
            // Context and learning keep the decided word; spacing is presentation of that word, not a new choice.
            Composition.AcceptCommit(trailingKey == 0x20);
            Commits++;
            if (respaced) Respacings++;
            if (current.IsKorean && current.Confidence != ConfidenceLevel.Explicit) AutoConversions++;
            lastCommit = swappable ? new CommittedWord { Raw = raw, Keys = keys, Text = typed, Alternative = current.Alternative, Korean = current.IsKorean,
                Unspaced = respaced ? text : null, Trailing = trailing, Context = context, Owner = owner, Time = clock.ElapsedMilliseconds } : null;
            if (Composition.Context.Length == 0) owner = null;
            return true;
        }
        // F2 with no live composition: delete the word just committed (plus its trailing space) and insert the other
        // candidate. Only in the same target, within a short window, and only after Space or a literal echo.
        // After automatic spacing the first F2 restores the word as typed ("할 수 있다" → "할수있다"); the next F2 swaps language as usual.
        bool SwapLastCommit()
        {
            CommittedWord word = lastCommit;
            if (word == null) return false;
            var now = focus.Snapshot;
            if (clock.ElapsedMilliseconds - word.Time > SwapWindowMs || word.Owner == null || !focus.IsSafe(now) ||
                word.Owner.Window != now.Window || word.Owner.NativeFocus != now.NativeFocus || word.Owner.Identity != now.Identity)
            { lastCommit = null; return false; }
            if (!Emit(word.Text + word.Trailing, (word.Unspaced ?? word.Alternative) + word.Trailing, null, now)) { lastCommit = null; LastError = "직전 단어를 바꾸지 못했습니다."; return false; }
            if (word.Unspaced != null)
            {
                // Undo only the spacing: the language decision stands, so nothing is learned and the context is unchanged.
                if (respacingDeclined.Count < 256) respacingDeclined.Add(word.Unspaced);
                word.Text = word.Unspaced; word.Unspaced = null; word.Time = clock.ElapsedMilliseconds; RespacingReverts++;
                if (word.Text == word.Alternative) lastCommit = null;
                return true;
            }
            string previous = word.Text; SwapsAfterSpace++;
            word.Text = word.Alternative; word.Alternative = previous; word.Korean = !word.Korean; word.Time = clock.ElapsedMilliseconds;
            Composition.RememberExplicit(word.Raw, word.Keys, word.Korean, word.Context);
            Composition.ResetContext(word.Text, word.Trailing == " ");
            return true;
        }
        IntPtr Keyboard(int code, IntPtr message, IntPtr data)
        {
            HookCallbacks++;
            if (code < 0) return Native.CallNextHookEx(keyboardHook, code, message, data);
            var keyData = (Native.KeyboardData)Marshal.PtrToStructure(data, typeof(Native.KeyboardData));
            bool down = message.ToInt32() == 0x100 || message.ToInt32() == 0x104;
            lock (Sync)
            {
                // Stamp every hardware key-down before any filtering so the Raw Input sentinel can tell a live hook from a removed one.
                if (down && (keyData.Flags & 0x10) == 0) lastHardwareTick = clock.ElapsedMilliseconds;
                IntPtr result = Process(code, message, data, keyData);
                if (keyData.Extra == Native.ReplayTag)
                {
                    // A replayed key that went on to the application settles like our own output before the next step.
                    if (down && result == IntPtr.Zero) Sent("", true, null);
                    replayAcked++; Monitor.PulseAll(Sync);
                }
                return result;
            }
        }
        IntPtr Process(int code, IntPtr message, IntPtr data, Native.KeyboardData keyData)
        {
            int key = (int)keyData.Key, releaseKey = key;
            bool packet = key == 0xe7;
            char packetText = (char)keyData.Scan;
            if (packet && packetText == ' ') key = 0x20;
            if (TestWindow != IntPtr.Zero && Native.GetForegroundWindow() != TestWindow)
                return Native.CallNextHookEx(keyboardHook, code, message, data);
            bool down = message.ToInt32() == 0x100 || message.ToInt32() == 0x104;
            if (keyData.Extra == Native.ProbeTag && key == Native.ProbeKey && !down && (keyData.Flags & 0x10) != 0)
            { probePending = false; HookHealthy = true; return new IntPtr(1); }
            if (Holding(keyData)) { held.Enqueue(Native.Replay(keyData, !down)); HeldKeys++; return new IntPtr(1); }
            InputDiagnostics diagnostics = Diagnostics;
            if (down && keyData.Extra != Native.OutputTag && diagnostics != null) diagnostics.Key(keyData, focus, Enabled);
            if (Native.BypassKeyboard(keyData))
            {
                if (down && keyData.Extra != Native.OutputTag)
                { FilteredKeys++; LastBypass = "lower-integrity"; PreservePending(); }
                return Native.CallNextHookEx(keyboardHook, code, message, data);
            }
            HookHealthy = true;
            if (down) { if ((keyData.Flags & 0x10) != 0) InjectedKeys++; else HardwareKeys++; }
            if (!down)
            {
                if (swallowed.Remove(releaseKey)) return new IntPtr(1);
                return Pass(code, message, data);
            }
            try
            {
                bool ctrl = Native.IsDown(0x11), alt = Native.IsDown(0x12), win = Native.IsDown(0x5b) || Native.IsDown(0x5c);
                if (key == 0x7b && !ctrl && !alt && !win) // F12 is the single emergency pause key.
                { PreservePending(); Enabled = !Enabled; focus.Invalidate(); return Eat(releaseKey); }
                var snapshot = focus.Snapshot;
                if (!Enabled || !focus.IsSafe(snapshot) || Excluded.Contains(snapshot.ProcessName))
                { LastBypass = !Enabled ? "paused" : Excluded.Contains(snapshot.ProcessName) ? "excluded-app" : "focus-guard"; PreservePending(); return Pass(code, message, data); }
                if (owner != null && !SameOwner()) PreservePending();

                bool modifier = key == 0x11 || key == 0x12 || key == 0xa2 || key == 0xa3 || key == 0xa4 || key == 0xa5 || key == 0x5b || key == 0x5c;
                if (modifier)
                {
                    if (!Composition.IsEmpty)
                    {
                        bool success = Commit(key, (keyData.Flags & 1) != 0, true);
                        focus.Invalidate();
                        if (success) return new IntPtr(1); // Real key-up must pass for the replayed modifier.
                    }
                    lastCommit = null; focus.Invalidate(); return Pass(code, message, data);
                }
                if (key == 0x76 && ctrl && Native.IsDown(0x10) && !alt && !win && Composition.IsEmpty && ProofreadRequested != null)
                { if (!swallowed.Contains(releaseKey)) ProofreadRequested(); return Eat(releaseKey); }
                if (ctrl || alt || win) { LastBypass = "shortcut-modifier"; PreservePending(); focus.Invalidate(); return Pass(code, message, data); }
                if (key == 0x10 || key == 0xa0 || key == 0xa1) return Pass(code, message, data);
                if (key == 0x71)
                {
                    bool repeat = swallowed.Contains(releaseKey);
                    if (Composition.IsEmpty && Native.IsDown(0x10))
                    { if (!repeat) RequestSelectionFlip(snapshot); return Eat(releaseKey); }
                    if (!Composition.IsEmpty) { if (!repeat) Composition.Toggle(); return Eat(releaseKey); }
                    if (repeat) return Eat(releaseKey);
                    if (lastCommit != null && SwapLastCommit()) return Eat(releaseKey);
                }
                if (key == 0x1b && !Composition.IsEmpty) { Composition.Clear(); owner = null; return Eat(releaseKey); }
                if (key == 0x08 && !Composition.IsEmpty) { Composition.Backspace(); return Eat(releaseKey); }
                if (packet && (packetText < ' ' || packetText > '~'))
                {
                    // Echo completed characters through the same output queue as preceding ASCII commits.
                    // Passing them directly would let a batch's Hangul/emoji overtake our queued output.
                    if (!Composition.IsEmpty)
                    { if (Commit(0, false, false, packetText.ToString())) return Eat(releaseKey); }
                    else if (Emit(null, packetText.ToString(), null, snapshot))
                    { Composition.Clear(); owner = null; lastCommit = null; return Eat(releaseKey); }
                    else { Enabled = false; LastError = "문자 입력 전송에 실패했습니다."; }
                    return Pass(code, message, data);
                }
                if (key == 0x20 && Composition.IsEmpty)
                {
                    if (!Emit(null, " ", null, snapshot)) { Enabled = false; LastError = "공백 입력 전송에 실패했습니다."; }
                    lastCommit = null; return Eat(releaseKey);
                }
                char raw, hangul;
                bool printable;
                if (packet) { raw = hangul = packetText; printable = packetText > ' ' && packetText <= '~'; }
                else printable = Native.TryAscii(key, Native.IsDown(0x10), (Native.GetKeyState(0x14) & 1) != 0, out raw, out hangul);
                if (printable)
                {
                    if (Composition.IsEmpty) owner = snapshot;
                    if (raw == ';' && SemicolonShortcut)
                    { if (!swallowed.Contains(releaseKey)) Composition.Semicolon(); }
                    else Composition.Append(raw, hangul);
                    if (Composition.Length >= 128) Commit(0, false, false);
                    return Eat(releaseKey);
                }
                if (!Composition.IsEmpty)
                {
                    // Space, Enter and navigation are replayed after the Unicode word in one ordered batch.
                    bool success = Commit(key, (keyData.Flags & 1) != 0, false);
                    if (key != 0x20) focus.Invalidate();
                    if (success) return Eat(releaseKey);
                }
                if (key != 0x20) { PreservePending(); focus.Invalidate(); }
                return Pass(code, message, data);
            }
            catch
            {
                PreservePending(); Enabled = false; LastError = "입력 처리 오류로 일시정지했습니다.";
                return Pass(code, message, data);
            }
        }
        public void Dispose()
        {
            if (disposed) return;
            disposed = true;
            if (hookThreadId != 0) Native.PostThreadMessage(hookThreadId, QuitMessage, IntPtr.Zero, IntPtr.Zero);
            if (hookThread != null && hookThread.IsAlive && Thread.CurrentThread != hookThread) hookThread.Join(2000);
        }
    }
}
