using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Threading;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace Hanautomata
{
    // All generated keys stay inside this fixture's HWND. Untagged cases exercise production filtering.
    internal static class IntegrationTests
    {
        static readonly List<object> results = new List<object>();
        static TrayApplication app;
        static Form host;
        static TextBox plain, password, readOnly, second;
        static RichTextBox rich;
        static int failures;
        static UIntPtr inputTag = Native.TestTag;
        sealed class CorpusData { public List<CorpusItem> rows { get; set; } }
        sealed class CorpusItem
        {
            public int token { get; set; }
            public string original { get; set; }
            public string raw { get; set; }
            public string expected { get; set; }
            public string actual { get; set; }
            public bool asciiPhysical { get; set; }
        }
        static readonly HashSet<int> heldTestKeys = new HashSet<int>();
        static void Ui(Action action) { host.Invoke(action); }
        static T Ui<T>(Func<T> action) { return (T)host.Invoke(action); }
        static void Assert(string name, bool passed, object details)
        { results.Add(new { name = name, passed = passed, details = details }); if (!passed) failures++; }
        static void Focus(Control control)
        {
            Ui(delegate
            {
                app.Input.PreservePending(); Native.ShowWindow(host.Handle, 9);
                uint pid; uint fgThread = Native.GetWindowThreadProcessId(Native.GetForegroundWindow(), out pid);
                uint ownThread = Native.GetCurrentThreadId();
                bool attached = fgThread != ownThread && Native.AttachThreadInput(ownThread, fgThread, true);
                try { Native.BringWindowToTop(host.Handle); Native.SetForegroundWindow(host.Handle); host.Activate(); control.Focus(); }
                finally { if (attached) Native.AttachThreadInput(ownThread, fgThread, false); }
            });
            for (int i = 0; i < 100; i++)
            {
                Thread.Sleep(35);
                bool nativeMatch = Native.GetForegroundWindow() == host.Handle && Native.GetFocus(host.Handle).Focus == Ui(delegate { return control.Handle; });
                var snap = app.Focus.Snapshot;
                bool needsWritable = control != password && control != readOnly;
                if (nativeMatch && snap.NativeFocus == Ui(delegate { return control.Handle; }) && snap.Ticks != 0 && (!needsWritable || app.Focus.IsFresh(snap))) return;
            }
            throw new Exception("Fixture focus not ready: " + app.Focus.Snapshot.Reason + " " + app.Focus.Snapshot.Diagnostic + " foreground=" + (Native.GetForegroundWindow() == host.Handle));
        }
        static void Key(int key, bool up)
        {
            if (Native.GetForegroundWindow() != host.Handle)
            {
                IntPtr fg = Native.GetForegroundWindow(); uint pid; Native.GetWindowThreadProcessId(fg, out pid);
                string formType = Ui(delegate { return string.Join(",", Application.OpenForms.Cast<Form>().Where(f => f.Handle == fg).Select(f => f.GetType().Name)); });
                throw new Exception("Fixture lost foreground; input stopped. process=" + System.Diagnostics.Process.GetProcessById((int)pid).ProcessName + " form=" + formType + " captured=" + app.Input.CapturedKeys);
            }
            if (!Native.Inject(new List<Native.Input> { Native.Key((ushort)key, up, inputTag, key == 0x25 || key == 0x27) }))
                throw new InvalidOperationException("Test SendInput failed.");
            if (up) heldTestKeys.Remove(key); else heldTestKeys.Add(key);
            Thread.Sleep(7);
        }
        static void Tap(int key) { Key(key, false); Key(key, true); }
        static string WaitForText(TextBoxBase target, string expected, int timeoutMs)
        {
            string actual = "";
            for (int waited = 0; waited < timeoutMs; waited += 50)
            {
                actual = Ui(delegate { return target.Text; });
                if (actual == expected) return actual;
                Thread.Sleep(50);
            }
            return actual;
        }
        static void Type(string text)
        {
            foreach (char c in text)
            {
                if (c == ' ') { Tap(0x20); continue; }
                int vk = ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')) ? char.ToUpperInvariant(c) : 0;
                bool shift = char.IsUpper(c);
                if (vk == 0)
                {
                    for (int candidate = 0x30; candidate <= 0xde && vk == 0; candidate++)
                        for (int s = 0; s < 2; s++)
                        {
                            char literal, hangul;
                            if (Native.TryAscii(candidate, s == 1, false, out literal, out hangul) && literal == c)
                            { vk = candidate; shift = s == 1; break; }
                        }
                }
                if (vk == 0) throw new Exception("Unsupported test char " + c);
                if (shift) Key(0xa0, false);
                Tap(vk);
                if (shift) Key(0xa0, true);
            }
        }
        static void TextCase(string name, TextBoxBase target, string keys, string expected)
        {
            Ui(delegate { app.Words.Clear(); target.Clear(); }); Focus(target); Type(keys); Thread.Sleep(240);
            string actual = Ui(delegate { return target.Text; });
            Assert(name, actual == expected, new { expected, actual, focus = app.Focus.Snapshot.Reason, diagnostic = app.Focus.Snapshot.Diagnostic,
                targetFocused = Ui(delegate { return target.Focused; }), captured = Ui(delegate { return app.Input.CapturedKeys; }), commits = Ui(delegate { return app.Input.Commits; }),
                fgMatch = Native.GetForegroundWindow() == host.Handle, nativeMatch = Native.GetFocus(host.Handle).Focus == Ui(delegate { return target.Handle; }),
                ctrl = Native.IsDown(0x11), alt = Native.IsDown(0x12), win = Native.IsDown(0x5b), pending = Ui(delegate { return app.Input.Composition.Raw; }) });
        }
        static void UnicodeCase(string name, TextBoxBase target, string text, string expected)
        {
            Ui(delegate { app.Words.Clear(); target.Clear(); }); Focus(target);
            if (Native.GetForegroundWindow() != host.Handle) throw new Exception("Unicode fixture lost foreground");
            var packets = Native.Unicode(text);
            for (int i = 0; i < packets.Count; i++)
            { var item = packets[i]; item.Value.Keyboard.Extra = UIntPtr.Zero; packets[i] = item; }
            Native.Inject(packets); Thread.Sleep(250);
            string actual = Ui(delegate { return target.Text; });
            Assert(name, actual == expected, new { expected, actual });
        }
        static bool SetIme(bool on)
        {
            return Ui(delegate
            {
                plain.Focus(); plain.ImeMode = on ? ImeMode.On : ImeMode.Off;
                IntPtr context = Native.ImmGetContext(plain.Handle);
                try { return context != IntPtr.Zero && Native.ImmSetOpenStatus(context, on) && Native.ImmGetOpenStatus(context) == on; }
                finally { if (context != IntPtr.Zero) Native.ImmReleaseContext(plain.Handle, context); }
            });
        }
        static void Execute(string outputPath)
        {
            IntPtr originalLayout = IntPtr.Zero;
            bool capsWasOn = Ui(delegate { return (Native.GetKeyState(0x14) & 1) != 0; });
            try
            {
                originalLayout = Ui(delegate { return Native.GetKeyboardLayout(0); });
                Ui(delegate { Native.LoadKeyboardLayout("00000412", 1); });
                Focus(plain);
                if (capsWasOn) { Tap(0x14); Thread.Sleep(200); Focus(plain); }
                bool off = SetIme(false); Thread.Sleep(400);
                Assert("IME off state verified", off, new { open = false });
                // Allow the startup watchdog to recover a Raw Input / low-level-hook compatibility failure.
                for (int attempt = 0; attempt < 40 && !app.Input.HookHealthy; attempt++) Thread.Sleep(50);
                int hookKeysBefore = app.Input.InjectedKeys;
                Tap(Native.ProbeKey);
                Assert("Keyboard hook receives input after Raw Input registration", app.Input.InjectedKeys > hookKeysBefore,
                    new { before = hookKeysBefore, after = app.Input.InjectedKeys, app.Input.HookHealthy, app.Input.HookRestarts,
                        app.Input.FilteredKeys, app.Input.LastBypass, app.Input.LastError, app.Input.SentinelActive,
                        app.Input.HookCallbacks, app.Input.SentinelFallback });
                if (app.Input.InjectedKeys == hookKeysBefore) throw new Exception("Keyboard hook did not receive the startup test key; input suite stopped.");
                try
                {
                    inputTag = UIntPtr.Zero;
                    TextCase("Production untagged virtual keys convert Korean", plain, "dkssudgktpdy ", "안녕하세요 ");
                    TextCase("Production user screenshot lowercase sentence", plain, "gksrmffh dleofh skdml ahems rjtdmf ", "한글로 이대로 나의 모든 것을 ");
                    Focus(plain); Tap(0x7b); Thread.Sleep(200);
                    Assert("Production untagged F12 pauses", Ui(delegate { return !app.Input.Enabled; }), null);
                    Tap(0x7b); Thread.Sleep(300);
                    Assert("Production untagged F12 resumes", Ui(delegate { return app.Input.Enabled; }), null);
                    Ui(delegate { app.Input.Enabled = true; });
                }
                finally { inputTag = Native.TestTag; }
                TextCase("English IME: Korean greeting", plain, "dkssudgktpdy ", "안녕하세요 ");
                TextCase("Public word list converts a deferred Korean word", plain, "dirks ", "야간 ");
                TextCase("Public word list keeps a deferred English word", plain, "torch ", "torch ");
                Ui(delegate { app.ApplySensitivity("conservative"); });
                TextCase("Conservative sensitivity keeps a boundary word literal", plain, "tp ", "tp ");
                Ui(delegate { app.ApplySensitivity("aggressive"); });
                TextCase("Aggressive sensitivity converts a boundary word", plain, "wl ", "지 ");
                Ui(delegate { app.ApplySensitivity("balanced"); });
                TextCase("Balanced sensitivity restores the boundary word", plain, "wl ", "wl ");
                Focus(plain); Tap(0x14); Thread.Sleep(200);
                Assert("Caps Lock on state verified", Ui(delegate { return (Native.GetKeyState(0x14) & 1) != 0; }), new { caps = true });
                TextCase("Caps Lock: user malfunction report", plain, "ehdwkrdksgka ehdwkrgkehfhr gownj ", "동작안함 동작하도록 해줘 ");
                TextCase("Caps Lock: Korean greeting and English acronyms", plain, "dkssudgktpdy api nasa go ", "안녕하세요 API NASA GO ");
                Focus(plain); Tap(0x14); Thread.Sleep(200);
                Assert("Caps Lock off state restored for suite", Ui(delegate { return (Native.GetKeyState(0x14) & 1) == 0; }), new { caps = false });
                TextCase("English IME: short Korean and particles", plain, "wjsfir tndbf rotjs dml fmf dndb ", "전략 수율 개선 의 를 우유 ");
                TextCase("English IME: mixed words", plain, "AIdml flarefmf thwochange ", "AI의 flare를 소재change ");
                TextCase("English IME: punctuation and numbers", plain, "4wja rjawmd/wjrdyd wkrdjqwltltj(tkdidtj) ", "4점 검증/적용 작업지시서(사양서) ");
                bool on = SetIme(true); Thread.Sleep(400);
                Assert("IME on state verified", on, new { open = true });
                TextCase("Korean IME: same physical keys", plain, "dkssudgktpdy ", "안녕하세요 ");
                TextCase("Korean IME: English retained", plain, "hello world ", "hello world ");
                TextCase("Mixed Korean English", plain, "dhsmf meeting ghldml ", "오늘 meeting 회의 ");
                TextCase("Compound vowels and shift", plain, "Enlek ", "뛰다 ");
                TextCase("Punctuation", plain, "dkssudgktpdy! ", "안녕하세요! ");
                TextCase("Email retained", plain, "peter@example.com ", "peter@example.com ");
                TextCase("Korean chat suffix", plain, "dkssudzz rkatkgkqslekbb! ", "안녕ㅋㅋ 감사합니다ㅠㅠ! ");
                TextCase("English identifiers and Korean shift", plain, "doWork DoWork qkRnjdy ", "doWork DoWork 바꿔요 ");
                TextCase("Rich edit target", rich, "qksrkqtmqslek ", "반갑습니다 ");
                Ui(delegate { plain.Clear(); }); Focus(plain); Type("dkssud"); Thread.Sleep(180);
                Assert("Live preedit is visible without stealing focus", Ui(delegate { return app.Preview.IsShown && app.Preview.ContentsVisible && Native.GetForegroundWindow() == host.Handle && plain.Text.Length == 0; }), new { contentsVisible = Ui(delegate { return app.Preview.ContentsVisible; }) });
                Ui(delegate { using (var bitmap = new Bitmap(app.Preview.Width, app.Preview.Height)) { app.Preview.DrawToBitmap(bitmap, new Rectangle(Point.Empty, bitmap.Size)); bitmap.Save(Path.Combine(Path.GetDirectoryName(outputPath), "preedit.png")); } });
                Tap(0x1b);
                Ui(delegate { plain.Clear(); }); Focus(plain); Type("dkssud"); Tap(0x08); Type("d "); Thread.Sleep(150);
                Assert("Backspace edits one composition key", Ui(delegate { return plain.Text; }) == "안녕 ", new { actual = Ui(delegate { return plain.Text; }) });
                Ui(delegate { plain.Clear(); }); Focus(plain); Type("dkssud"); Tap(0x71); Type(" "); Thread.Sleep(150);
                Assert("F2 selects literal candidate", Ui(delegate { return plain.Text; }) == "dkssud ", null);
                Ui(delegate { plain.Clear(); }); Focus(plain); Type("wbs"); Tap(0x71); Type(" "); Thread.Sleep(150);
                Assert("F2 selects Korean for ambiguous wbs", Ui(delegate { return plain.Text; }) == "쥰 ", null);
                Ui(delegate { plain.Clear(); }); Focus(plain); Type("vy"); Tap(0x71); Type(" "); Thread.Sleep(150); // vy/표 is literal by default (one syllable, llr 0.62); F2 picks the Korean reading
                Assert("F2 selects unknown short Korean", Ui(delegate { return plain.Text; }) == "표 ", null);
                Ui(delegate { app.Words.Clear(); plain.Clear(); }); Focus(plain); Type("dkssud"); Tap(0x71);
                for (int i = 0; i < 6; i++) Tap(0x08);
                Type("dkssud "); Thread.Sleep(150);
                Assert("Empty preedit resets F2 selection for next word", Ui(delegate { return plain.Text; }) == "안녕 ", new { actual = Ui(delegate { return plain.Text; }) });
                Ui(delegate { plain.Clear(); }); Focus(plain); Type("dkssud"); Tap(0x1b); Type(" "); Thread.Sleep(150);
                Assert("Escape discards only pending composition", Ui(delegate { return plain.Text; }) == " ", null);
                Ui(delegate { plain.Clear(); }); Focus(plain); Type("hello"); Key(0xa2, false); Tap(0x41); Key(0xa2, true); Thread.Sleep(200);
                Assert("Ctrl+A commits before shortcut and preserves selection", Ui(delegate { return plain.Text == "hello" && plain.SelectionLength == 5; }), new { actual = Ui(delegate { return plain.Text; }), selection = Ui(delegate { return plain.SelectionLength; }) });
                Ui(delegate { plain.Clear(); second.Clear(); }); Focus(plain); Type("dkssud");
                Ui(delegate { second.Focus(); }); Thread.Sleep(450);
                Assert("Focus change never commits into next field", Ui(delegate { return plain.Text.Length == 0 && second.Text.Length == 0 && app.Input.Composition.IsEmpty && app.Input.Recovery == "안녕"; }), new { recoveryRetained = Ui(delegate { return app.Input.Recovery == "안녕"; }) });
                Ui(delegate { Native.LoadKeyboardLayout("00000409", 1); });
                Focus(password);
                Assert("Password is excluded", !app.Focus.IsSafe(app.Focus.Snapshot), new { reason = app.Focus.Snapshot.Reason });
                int capturedBefore = Ui(delegate { return app.Input.CapturedKeys; }); Type("safe "); Thread.Sleep(150);
                Assert("Password keystrokes bypass interception", Ui(delegate { return app.Input.CapturedKeys; }) == capturedBefore, new { targetLength = Ui(delegate { return password.TextLength; }) });
                Focus(readOnly); capturedBefore = Ui(delegate { return app.Input.CapturedKeys; }); Type("test "); Thread.Sleep(100);
                Assert("Read-only target bypasses interception", Ui(delegate { return app.Input.CapturedKeys; }) == capturedBefore && Ui(delegate { return readOnly.Text; }) == "READ ONLY", null);
                Ui(delegate { plain.Clear(); }); Focus(plain);
                capturedBefore = Ui(delegate { return app.Input.CapturedKeys; });
                Ui(delegate { plain.ReadOnly = true; }); Type("safe "); Thread.Sleep(100);
                Assert("Native read-only style is checked before cached UIA expires", Ui(delegate { return app.Input.CapturedKeys; }) == capturedBefore, null);
                Ui(delegate { plain.ReadOnly = false; });
                Ui(delegate { plain.Clear(); }); Focus(plain); Tap(0x7b); Thread.Sleep(350); Type("plain "); Thread.Sleep(150);
                Assert("F12 pause passes normal typing", Ui(delegate { return !app.Input.Enabled && plain.Text == "plain "; }), null);
                Tap(0x7b); Thread.Sleep(400);
                Assert("F12 resumes", Ui(delegate { return app.Input.Enabled; }), null);
                Ui(delegate { plain.Clear(); }); Focus(plain);
                var injected = Native.Unicode("literal "); Native.Inject(injected); Thread.Sleep(200);
                Assert("Injected output does not recurse", Ui(delegate { return plain.Text == "literal " && app.Input.Composition.IsEmpty; }), null);
                Ui(delegate { plain.Clear(); });
                var literal = Native.Unicode("dkssudgktpdy ");
                for (int i = 0; i < literal.Count; i++)
                { var item = literal[i]; item.Value.Keyboard.Extra = UIntPtr.Zero; literal[i] = item; }
                Native.Inject(literal); Thread.Sleep(200);
                Assert("External Unicode ASCII converts Korean", Ui(delegate { return plain.Text == "안녕하세요 " && app.Input.Composition.IsEmpty; }), Ui(delegate { return plain.Text; }));
                UnicodeCase("Unicode user report converts two words", plain, "gksrmfrhk zjavbxj ", "한글과 컴퓨터 ");
                UnicodeCase("Unicode English and Korean keep ordered commits", plain, "hello world gksrmf ", "hello world 한글 ");
                UnicodeCase("Unicode completed Hangul accents and emoji stay intact", plain, "dkssud 한글 Café 🙂 ", "안녕 한글 Café 🙂 ");
                UnicodeCase("Unicode non ASCII flushes pending prefix in order", plain, "gksrmf🙂 ", "한글🙂 ");
                UnicodeCase("Unicode ASCII is literal in password", password, "gksrmf ", "gksrmf ");
                Assert("Lower integrity injected keys bypass capture", Native.BypassKeyboard(new Native.KeyboardData { Key = 0x41, Flags = 0x12 }), null);
                TextCase("Repeated words in order", plain, "dkssudgktpdy hello world ghldml ", "안녕하세요 hello world 회의 ");
                TextCase("Scorer: short Korean outside hand lists converts", plain, "rmfjs gksms rjt ", "그런 하는 것 ");
                TextCase("Scorer: continuity converts a near-tie word after Korean", plain, "sksms dh ", "나는 오 ");
                TextCase("Scorer: unknown short English stays literal", plain, "dori sudoku plain ", "dori sudoku plain ");
                // Before v0.2.0 any invalidation blanked the target snapshot, so keys typed during the refresh leaked unconverted.
                Ui(delegate { plain.Clear(); }); Focus(plain); Ui(delegate { app.Focus.Invalidate("test-pending-target"); }); Type("dkssud "); Thread.Sleep(240);
                Assert("Pending target refresh does not leak keys before conversion", Ui(delegate { return plain.Text; }) == "안녕 ", new { actual = Ui(delegate { return plain.Text; }), guard = app.Focus.GuardReason(app.Focus.Snapshot) });
                Ui(delegate { app.Words.Clear(); plain.Clear(); }); Focus(plain); Type("wbs "); Thread.Sleep(120); Tap(0x71); Thread.Sleep(240);
                Assert("F2 after Space swaps the previous word", Ui(delegate { return plain.Text; }) == "쥰 ", new { actual = Ui(delegate { return plain.Text; }) });
                Tap(0x71); Thread.Sleep(240);
                Assert("F2 after Space swaps back", Ui(delegate { return plain.Text; }) == "wbs ", new { actual = Ui(delegate { return plain.Text; }) });
                Assert("F2 after Space learns the explicit choice", app.Words.Count == 1, new { count = app.Words.Count });
                Type("wbs "); Thread.Sleep(200);
                Assert("Learned swap applies to the next occurrence", Ui(delegate { return plain.Text; }) == "wbs wbs ", new { actual = Ui(delegate { return plain.Text; }) });
                Ui(delegate { app.Words.Clear(); plain.Clear(); }); Focus(plain); Type("dkssud "); Tap(0x25); Thread.Sleep(120); Tap(0x71); Thread.Sleep(240);
                Assert("F2 after navigation does not rewrite the previous word", Ui(delegate { return plain.Text; }) == "안녕 ", new { actual = Ui(delegate { return plain.Text; }) });
                Assert("Raw Input sentinel is active or its compatibility fallback has a healthy hook",
                    app.Input.SentinelActive || (app.Input.SentinelFallback && app.Input.HookHealthy && app.Input.HookCallbacks > 0),
                    new { active = app.Input.SentinelActive, fallback = app.Input.SentinelFallback, healthy = app.Input.HookHealthy,
                        callbacks = app.Input.HookCallbacks, rawKeys = app.Input.RawHardwareKeys, alarms = app.Input.SentinelAlarms });
                Ui(delegate { app.Words.Clear(); plain.Clear(); plain.Text = "dkssud hello"; }); Focus(plain); Ui(delegate { plain.SelectAll(); });
                Key(0xa0, false); Tap(0x71); Key(0xa0, true);
                string flipped = WaitForText(plain, "안녕 ㅗ디ㅣㅐ", 3000);
                Assert("Shift+F2 flips the selected text in place", flipped == "안녕 ㅗ디ㅣㅐ", new { actual = flipped, error = app.Input.LastError, flips = app.Input.SelectionFlips });
                Ui(delegate { plain.SelectAll(); }); Key(0xa0, false); Tap(0x71); Key(0xa0, true);
                flipped = WaitForText(plain, "dkssud hello", 3000);
                Assert("Shift+F2 flips the selection back", flipped == "dkssud hello", new { actual = flipped, error = app.Input.LastError, flips = app.Input.SelectionFlips });
                Ui(delegate { plain.Clear(); }); Focus(plain); Key(0xa0, false); Tap(0x71); Key(0xa0, true); Thread.Sleep(900);
                Assert("Shift+F2 without a selection changes nothing", Ui(delegate { return plain.Text; }) == "", new { actual = Ui(delegate { return plain.Text; }), error = app.Input.LastError });
                ExecuteLearning();
                ExecuteIntelligence();
                Focus(plain);
                int reconnectsBefore = Ui(delegate { return app.Input.HookRestarts; });
                Ui(delegate { plain.Clear(); }); Type("dkssud");
                Ui(delegate
                {
                    var field = typeof(InputController).GetField("keyboardHook", System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.NonPublic);
                    Native.UnhookWindowsHookEx((IntPtr)field.GetValue(app.Input));
                });
                Thread.Sleep(4500);
                Assert("Hook recovery never inserts probe text", Ui(delegate { return plain.Text.Length == 0 && app.Input.Composition.IsEmpty; }), null);
                Assert("Hook recovery preserves unfinished word for manual recovery", Ui(delegate { return app.Input.Recovery == "안녕"; }), null);
                Assert("Removed hook is detected and reinstalled", Ui(delegate { return app.Input.HookRestarts > reconnectsBefore && app.Input.HookHealthy; }), new { restarts = Ui(delegate { return app.Input.HookRestarts; }) });
                TextCase("Keyboard hook reconnects after forced removal", plain, "dkssudgktpdy ", "안녕하세요 ");
                Ui(delegate
                {
                    app.Dashboard.Show(); app.Dashboard.Activate(); app.Dashboard.Practice.Text = "안녕하세요 hello world\r\n오늘 meeting 회의";
                    app.Dashboard.State.Text = failures == 0 ? "Windows 입력 경로 검증 완료" : "입력 경로 검증 중";
                    using (var bitmap = new Bitmap(app.Dashboard.Width, app.Dashboard.Height))
                    { app.Dashboard.DrawToBitmap(bitmap, new Rectangle(Point.Empty, bitmap.Size)); bitmap.Save(Path.ChangeExtension(outputPath, ".png")); }
                });
            }
            catch (Exception ex) { Assert("Test harness exception", false, ex.ToString()); }
            finally
            {
                Native.Inject(heldTestKeys.Select(key => Native.Key((ushort)key, true, Native.OutputTag, key == 0x25 || key == 0x27)).ToList());
                try
                {
                    if (Ui(delegate { return (Native.GetKeyState(0x14) & 1) != 0; }) != capsWasOn)
                    { Focus(plain); Tap(0x14); Thread.Sleep(150); }
                }
                catch { }
                try { Ui(delegate { if (originalLayout != IntPtr.Zero) Native.ActivateKeyboardLayout(originalLayout, 0); }); } catch { }
                var report = new { timestamp = DateTimeOffset.Now.ToString("o"), scope = "WinForms TextBox/RichTextBox; production keyboard filter with tagged and untagged SendInput, forced hook removal; not physical hardware or all Windows apps", integrityLevel = Native.IntegrityLevel((uint)System.Diagnostics.Process.GetCurrentProcess().Id), passed = failures == 0, failures, tests = results };
                File.WriteAllText(outputPath, new JavaScriptSerializer().Serialize(report));
                Ui(delegate { host.Close(); app.ExitThread(); });
            }
        }
        internal static int Run(string outputPath)
        { return RunFixture(outputPath, null); }
        static void LearningCase(string name, string expected, Action input, bool reset)
        {
            if (reset) Ui(delegate { app.Words.Clear(); plain.Clear(); });
            Focus(plain); input(); Thread.Sleep(240);
            string actual = Ui(delegate { return plain.Text; });
            Assert(name, actual == expected, new { expected, actual, remembered = app.Words.Count });
        }
        static void ExecuteLearning()
        {
            Ui(delegate { Native.LoadKeyboardLayout("00000412", 1); }); Focus(plain);
            Assert("Learning: English input mode verified", SetIme(false), null); Thread.Sleep(350);
            LearningCase("Semicolon selects Korean", "그 ", delegate { Type("rm; "); }, true);
            LearningCase("Confirmed semicolon choice applies next time", "그 그 ", delegate { Type("rm "); }, false);
            LearningCase("F2 choice applies next time", "그 그 ", delegate { Type("rm"); Tap(0x71); Type(" rm "); }, true);
            LearningCase("Double semicolon inserts punctuation", "hello; ", delegate { Type("hello;; "); }, true);
            Assert("Escaped punctuation is not learned", app.Words.Count == 0, null);
            LearningCase("Semicolon without a word is literal", "; ", delegate { Type("; "); }, true);
            LearningCase("Shift semicolon remains colon", "hello: ", delegate { Type("hello: "); }, true);
            LearningCase("Semicolon Backspace cancels choice", "rm ", delegate { Type("rm;"); Tap(0x08); Type(" "); }, true);
            Assert("Undone shortcut is not learned", app.Words.Count == 0, null);
            LearningCase("Escape cancels unconfirmed learning", "rm ", delegate { Type("rm;"); Tap(0x1b); Type("rm "); }, true);
            Assert("Cancelled choice is not learned", app.Words.Count == 0, null);
            LearningCase("Confirmed opposite choice replaces learning", "쥰 wbs wbs ", delegate { Type("wbs; wbs"); Tap(0x71); Type(" wbs "); }, true);
            Ui(delegate { app.Words.UndoLast(); });
            LearningCase("Undo restores former learned choice", "쥰 wbs wbs 쥰 ", delegate { Type("wbs "); }, false);
            Ui(delegate { app.Words.Enabled = false; });
            // rm/그 can now convert from the statistical continuity prior even with personal learning off.
            // A protected English word isolates this test from that independent model behavior.
            LearningCase("Learning off keeps manual conversion only", "쥰 wbs ", delegate { Type("wbs; wbs "); }, true);
            Assert("Learning off creates no entry", app.Words.Count == 0, null);
            Ui(delegate { app.Words.Enabled = true; app.Input.SemicolonShortcut = false; });
            LearningCase("Semicolon shortcut can be disabled", "hello; ", delegate { Type("hello; "); }, true);
            Ui(delegate { app.Input.SemicolonShortcut = true; });
            LearningCase("Held semicolon does not trigger literal escape", "그 ", delegate { Type("rm"); Key(0xba, false); Key(0xba, false); Key(0xba, true); Type(" "); }, true);
            Ui(delegate { app.Words.Clear(); plain.Clear(); second.Clear(); }); Focus(plain); Type("rm;"); Focus(second);
            Assert("Focus loss never trains an unconfirmed choice", app.Words.Count == 0 && Ui(delegate { return plain.TextLength == 0 && second.TextLength == 0; }), null);
        }
        internal static int RunCorpus(string inputPath, string outputPath)
        { return RunFixture(outputPath, delegate { ExecuteCorpus(inputPath, outputPath); }); }
        static void ExecuteIntelligence()
        {
            TextCase("Intelligence: dictionary lattice mixed words", plain,
                "APIrotjs AIrjawmd AIrotjsAPIrjawmd ghldmlmeetingwnsql ", "API개선 AI검증 AI개선API검증 회의meeting준비 ");
            TextCase("Round2: Korean verb forms with English", plain,
                "APIrotjsgkqslek reportrjawmdgkrh ", "API개선합니다 report검증하고 ");
            TextCase("Round2: noun particles across language boundary", plain,
                "ghldmldptjreport APIrotjsdmf ", "회의에서report API개선을 ");
            TextCase("Round2: eight alternating lexical spans", rich,
                "AIgortlaAPIrotjsSDKrjawmdHTMLrufrhk ", "AI핵심API개선SDK검증HTML결과 ");
            TextCase("Round2: short core term and English preservation", plain,
                "gortla hello world go DoWork ", "핵심 hello world go DoWork ");
            LearningCase("Round2: explicit literal choice overrides morphology", "APIrotjsgkqslek ", delegate { Type("APIrotjsgkqslek; "); }, true);
            LearningCase("Round2: confirmed literal choice survives next word", "APIrotjsgkqslek APIrotjsgkqslek ", delegate { Type("APIrotjsgkqslek "); }, false);
            LearningCase("Context: Korean choice after 나는", "나는 해 ", delegate { Type("sksms go; "); }, true);
            LearningCase("Context: English choice after please", "나는 해 please go ", delegate { Type("please go; "); }, false);
            LearningCase("Context: Korean choice survives opposite English learning", "나는 해 please go 나는 해 ", delegate { Type("sksms go "); }, false);
            LearningCase("Context: English choice survives Korean learning", "나는 해 please go 나는 해 please go ", delegate { Type("please go "); }, false);
            int learned = app.Words.Count;
            LearningCase("Context: unseen context keeps ambiguous English", "나는 해 please go 나는 해 please go world go ", delegate { Type("world go "); }, false);
            Assert("Context: automatic confirmations add no training entries", learned == app.Words.Count, null);
            Ui(delegate { plain.Clear(); }); Focus(plain); Type("sksms "); Thread.Sleep(160);
            Assert("Context: committed word stays available after Space", Ui(delegate { return app.Input.Composition.Context == WordPreferences.ContextKey("나는"); }), null);
            Tap(0x0d); Thread.Sleep(350);
            Assert("Context: Enter resets preceding word", Ui(delegate { return app.Input.Composition.Context.Length == 0; }), null);
            Focus(plain); Type("sksms "); Thread.Sleep(160); Tap(0x08); Thread.Sleep(160);
            Assert("Context: editing committed text clears preceding word", Ui(delegate { return app.Input.Composition.Context.Length == 0; }), null);
            Focus(plain); Type("sksms "); Thread.Sleep(160);
            Ui(delegate { second.Focus(); }); Thread.Sleep(450);
            Assert("Context: real field change resets empty composition history", Ui(delegate { return app.Input.Composition.Context.Length == 0; }), null);
            Ui(delegate { app.Words.Clear(); });
        }
        static void ExecuteCorpus(string inputPath, string outputPath)
        {
            IntPtr originalLayout = IntPtr.Zero; int typed = 0, pathMatches = 0, literalMatches = 0, intendedMatches = 0, skipped = 0;
            bool capsWasOn = false, englishImeVerified = false; string error = null;
            var mismatches = new List<object>();
            try
            {
                var corpus = new JavaScriptSerializer { MaxJsonLength = 10000000 }.Deserialize<CorpusData>(File.ReadAllText(inputPath));
                // Historical corpus uses literal semicolons. Shortcut behavior has separate native cases.
                Ui(delegate { app.Input.SemicolonShortcut = false; });
                originalLayout = Ui(delegate { return Native.GetKeyboardLayout(0); });
                Ui(delegate { Native.LoadKeyboardLayout("00000412", 1); });
                Focus(plain); englishImeVerified = SetIme(false);
                if (!englishImeVerified) throw new Exception("English IME state could not be verified.");
                Thread.Sleep(400);
                capsWasOn = (Native.GetKeyState(0x14) & 1) != 0;
                if (capsWasOn) { Tap(0x14); Thread.Sleep(350); }
                Ui(delegate { plain.Clear(); }); Focus(plain);
                int previousLength = 0;
                foreach (CorpusItem item in corpus.rows)
                {
                    if (!item.asciiPhysical) { skipped++; continue; }
                    Type(item.raw + " ");
                    // SendInput queues WM_CHAR messages; Invoke alone is not an input-queue barrier.
                    // Wait for the space delimiter, without comparing to the predicted output.
                    string value = null;
                    for (int wait = 0; wait < 100; wait++)
                    {
                        Thread.Sleep(20);
                        value = Ui(delegate { return plain.Text; });
                        if (value.Length > previousLength && value.EndsWith(" ", StringComparison.Ordinal) &&
                            Ui(delegate { return app.Input.Composition.IsEmpty; })) break;
                        if (wait == 99) throw new Exception("Corpus commit delimiter timed out at token " + item.token);
                    }
                    string added = value.Substring(previousLength); previousLength = value.Length; typed++;
                    if (added == item.actual + " ") pathMatches++;
                    else mismatches.Add(new { item.token, expectedEngine = item.actual, actual = added });
                    if (added == item.original + " ") literalMatches++;
                    if (added == item.expected + " ") intendedMatches++;
                }
                File.WriteAllText(Path.ChangeExtension(outputPath, ".actual.txt"), Ui(delegate { return plain.Text; }));
            }
            catch (Exception ex) { error = ex.ToString(); failures++; }
            finally
            {
                foreach (int key in heldTestKeys.ToArray()) Native.Inject(new List<Native.Input> { Native.Key((ushort)key, true, Native.OutputTag, false) });
                heldTestKeys.Clear();
                if (capsWasOn && Native.GetForegroundWindow() == host.Handle && (Native.GetKeyState(0x14) & 1) == 0) Tap(0x14);
                if (originalLayout != IntPtr.Zero) Ui(delegate { Native.ActivateKeyboardLayout(originalLayout, 0); });
                if (pathMatches != typed) failures++;
                var report = new { timestamp = DateTimeOffset.Now.ToString("o"), englishImeVerified,
                    typed, pathMatches, literalMatches, intendedMatches, skipped, error, mismatches,
                    passed = error == null && typed > 0 && pathMatches == typed,
                    scope = "Actual hook and SendInput in WinForms; tokens separated by spaces; semicolon shortcut disabled for literal fixture punctuation. Cafe accented token excluded. Path parity is distinct from intent accuracy." };
                File.WriteAllText(outputPath, new JavaScriptSerializer { MaxJsonLength = 10000000 }.Serialize(report));
                Ui(delegate { host.Close(); app.ExitThread(); });
            }
        }
        static int RunFixture(string outputPath, Action corpusDriver)
        {
            outputPath = Path.GetFullPath(outputPath);
            Directory.CreateDirectory(Path.GetDirectoryName(outputPath));
            app = new TrayApplication(true, true, null);
            host = new Form { Text = "Hanautomata input integration fixture", Width = 660, Height = 440, StartPosition = FormStartPosition.CenterScreen, TopMost = true };
            plain = new TextBox { Name = "plain", AccessibleName = "Plain input", Multiline = true, Location = new Point(16, 16), Size = new Size(610, 62) };
            rich = new RichTextBox { Name = "rich", AccessibleName = "Rich input", Location = new Point(16, 88), Size = new Size(610, 62) };
            password = new TextBox { Name = "password", AccessibleName = "Password input", UseSystemPasswordChar = true, Location = new Point(16, 164), Width = 610 };
            readOnly = new TextBox { Name = "readOnly", AccessibleName = "Read only input", ReadOnly = true, Text = "READ ONLY", Location = new Point(16, 206), Width = 610 };
            second = new TextBox { Name = "second", AccessibleName = "Second input", Location = new Point(16, 248), Width = 610 };
            host.Controls.AddRange(new Control[] { plain, rich, password, readOnly, second });
            host.Shown += delegate
            {
                app.Input.TestWindow = host.Handle;
                // Shown fires before initial activation has settled. Start input only after the UI pump is running.
                var driver = new Thread(delegate() { Thread.Sleep(500); if (corpusDriver == null) Execute(outputPath); else corpusDriver(); }) { IsBackground = true };
                driver.Start();
            };
            host.Show(); Application.Run(app); return failures == 0 ? 0 : 1;
        }
    }
}
