using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Threading;
using System.Web.Script.Serialization;
using System.Windows.Forms;
using Microsoft.Win32;

namespace Hanautomata
{
    internal sealed class Settings
    {
        public string[] ExcludedApps { get; set; }
        public bool? LearnChoices { get; set; }
        public bool? SemicolonShortcut { get; set; }
        public string Sensitivity { get; set; }
        internal static readonly string DirectoryPath = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Hanautomata");
        internal static string FilePath { get { return Path.Combine(DirectoryPath, "settings.json"); } }
        // Pre-rename data folder (this app was called HanFlow until 2026-10-09). Copied once on first start, never deleted.
        static readonly string LegacyDirectoryPath = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "HanFlow");
        internal static void MigrateLegacyData()
        {
            try
            {
                if (Directory.Exists(DirectoryPath) || !Directory.Exists(LegacyDirectoryPath)) return;
                Directory.CreateDirectory(DirectoryPath);
                foreach (string file in Directory.GetFiles(LegacyDirectoryPath))
                {
                    try { File.Copy(file, Path.Combine(DirectoryPath, Path.GetFileName(file)), false); }
                    catch (IOException) { }
                    catch (UnauthorizedAccessException) { }
                }
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
        }
        internal static Settings Load()
        {
            try { return new JavaScriptSerializer().Deserialize<Settings>(File.ReadAllText(FilePath)) ?? new Settings(); }
            catch { return new Settings(); }
        }
        internal void Save()
        {
            Directory.CreateDirectory(DirectoryPath);
            string temp = FilePath + ".tmp";
            File.WriteAllText(temp, new JavaScriptSerializer().Serialize(this));
            if (File.Exists(FilePath)) File.Replace(temp, FilePath, null);
            else File.Move(temp, FilePath);
        }
    }
    internal sealed class PreeditWindow : Form
    {
        readonly Label candidate, hint;
        string last = "";
        internal bool IsShown;
        internal bool ContentsVisible { get { return candidate.IsHandleCreated && hint.IsHandleCreated && Native.IsWindowVisible(candidate.Handle) && Native.IsWindowVisible(hint.Handle); } }
        internal void Prepare()
        {
            IntPtr window = Handle;
            Native.ShowWindow(candidate.Handle, 8);
            Native.ShowWindow(hint.Handle, 8);
            Native.ShowWindow(window, 0);
        }
        internal PreeditWindow()
        {
            FormBorderStyle = FormBorderStyle.None; ShowInTaskbar = false;
            AutoScaleMode = AutoScaleMode.None;
            BackColor = Color.FromArgb(21, 32, 53); Size = new Size(420, 94);
            candidate = new Label { Location = new Point(16, 10), Size = new Size(388, 36), ForeColor = Color.White, Font = new Font("Malgun Gothic", 23, FontStyle.Bold, GraphicsUnit.Pixel), AutoEllipsis = true };
            hint = new Label { Location = new Point(17, 54), Size = new Size(386, 28), ForeColor = Color.FromArgb(165, 187, 218), Font = new Font("Malgun Gothic", 12, FontStyle.Regular, GraphicsUnit.Pixel), AutoEllipsis = true };
            Controls.Add(candidate); Controls.Add(hint);
        }
        protected override bool ShowWithoutActivation { get { return true; } }
        protected override CreateParams CreateParams
        {
            get { var p = base.CreateParams; p.ExStyle |= 0x08000000 | 0x80 | 0x20; return p; }
        }
        internal void UpdateCandidate(Decision decision, Point position, bool semicolon)
        {
            string identity = decision.Text + "|" + decision.Alternative + "|" + decision.Reason + "|" + semicolon;
            if (last != identity)
            {
                last = identity; candidate.Text = decision.Text;
                string evidence = decision.Confidence == ConfidenceLevel.Personal ? (decision.Reason.StartsWith("앞 단어") ? "문맥 학습 · " : "개인 학습 · ") :
                    decision.NeedsConfirmation ? "확인 필요 · " : decision.Confidence == ConfidenceLevel.Pattern ? "음절 추정 · " : "";
                hint.Text = evidence + (semicolon ? "; / F2 → " : "F2 → ") + decision.Alternative + "   Space 확정";
            }
            Rectangle screen = Screen.FromPoint(position).WorkingArea;
            Point location = new Point(Math.Max(screen.Left, Math.Min(position.X, screen.Right - Width)), Math.Max(screen.Top, Math.Min(position.Y, screen.Bottom - Height)));
            Native.SetWindowPos(Handle, new IntPtr(-1), location.X, location.Y, Width, Height, 0x10 | 0x40);
            IsShown = true;
        }
        internal void HidePreview() { if (IsShown) Native.ShowWindow(Handle, 0); IsShown = false; }
    }
    internal sealed class Dashboard : Form
    {
        internal readonly TextBox Practice;
        internal readonly Label State, Detail, Recovery, Instructions;
        internal readonly Button Toggle, Reconnect;
        internal bool ClosingPermanently;
        internal Dashboard()
        {
            Text = "Hanautomata v0.4.0 베타 — 한영 모드 없는 입력";
            AutoScaleMode = AutoScaleMode.None;
            ClientSize = new Size(760, 570); MinimumSize = Size;
            StartPosition = FormStartPosition.CenterScreen; MaximizeBox = false;
            BackColor = Color.FromArgb(246, 248, 252); Font = new Font("Malgun Gothic", 14, FontStyle.Regular, GraphicsUnit.Pixel);
            AddLabel("Hanautomata", 28, 20, 480, 45, 25, true, Color.FromArgb(21, 32, 53));
            AddLabel("한영키 대신, 쓰고 싶은 말을 입력하세요.", 31, 72, 685, 28, 12, false, Color.FromArgb(65, 80, 101));
            State = AddLabel("입력 칸 확인 중", 32, 116, 500, 27, 12, true, Color.FromArgb(0, 112, 92));
            Detail = AddLabel("", 32, 146, 680, 24, 9, false, Color.DimGray);
            AddLabel("여기에서 연습해 보세요", 32, 194, 540, 28, 11, true, Color.FromArgb(21, 32, 53));
            AddLabel("dkssudgktpdy → 안녕하세요     hello world → hello world", 32, 225, 690, 25, 10, false, Color.FromArgb(78, 91, 111));
            Practice = new TextBox { Name = "HanautomataPractice", AccessibleName = "Hanautomata 입력 연습", Multiline = true, AcceptsReturn = true, AcceptsTab = false,
                Location = new Point(32, 264), Size = new Size(694, 122), Font = new Font("Malgun Gothic", 20, FontStyle.Regular, GraphicsUnit.Pixel), BorderStyle = BorderStyle.FixedSingle, ScrollBars = ScrollBars.Vertical };
            Controls.Add(Practice);
            Instructions = AddLabel("", 32, 399, 700, 23, 10, false, Color.FromArgb(55, 70, 94));
            AddLabel("직접 선택하면 앞 단어와 함께 기억합니다. ‘확인 필요’일 때 ; 또는 F2로 선택하세요.", 32, 430, 700, 23, 9, false, Color.DimGray);
            Recovery = AddLabel("", 32, 458, 700, 23, 9, false, Color.FromArgb(138, 81, 18));
            Toggle = new Button { Text = "일시정지  F12", Location = new Point(32, 506), Size = new Size(175, 36), FlatStyle = FlatStyle.Flat, BackColor = Color.White };
            Controls.Add(Toggle);
            Reconnect = new Button { Text = "입력 연결 복구", Location = new Point(220, 506), Size = new Size(175, 36), FlatStyle = FlatStyle.Flat, BackColor = Color.White };
            Controls.Add(Reconnect);
            var hide = new Button { Text = "트레이로 보내기", Location = new Point(551, 506), Size = new Size(175, 36), FlatStyle = FlatStyle.Flat, BackColor = Color.FromArgb(29, 77, 216), ForeColor = Color.White };
            hide.Click += delegate { Hide(); }; Controls.Add(hide);
            FormClosing += delegate(object sender, FormClosingEventArgs e) { if (!ClosingPermanently) { e.Cancel = true; Hide(); } };
        }
        Label AddLabel(string text, int x, int y, int w, int h, float size, bool bold, Color color)
        {
            var label = new Label { Text = text, Location = new Point(x, y), Size = new Size(w, h), Font = new Font("Malgun Gothic", size * 1.33f, bold ? FontStyle.Bold : FontStyle.Regular, GraphicsUnit.Pixel), ForeColor = color, AutoEllipsis = true };
            Controls.Add(label); return label;
        }
    }
    internal sealed class TrayApplication : ApplicationContext
    {
        internal readonly FocusMonitor Focus;
        internal readonly InputController Input;
        internal readonly Dashboard Dashboard;
        internal readonly WordPreferences Words;
        internal readonly Detector Engine;
        string currentSensitivity = "balanced";
        readonly PreferenceStore personalStore;
        readonly Settings settings;
        readonly PreeditWindow preview;
        internal PreeditWindow Preview { get { return preview; } }
        readonly NotifyIcon tray;
        readonly System.Windows.Forms.Timer timer;
        readonly ToolStripMenuItem enabled, startup, excluded, recovery, learn, semicolon, undoLearning, clearLearning, sensitivity;
        readonly Icon icon;
        readonly EventWaitHandle stopEvent;
        readonly bool testing;
        internal InputDiagnostics Diagnostics;
        string lastOtherProcess = "";
        internal TrayApplication(bool background, bool testing, EventWaitHandle stopEvent)
        {
            this.testing = testing; this.stopEvent = stopEvent;
            Dashboard = new Dashboard(); preview = new PreeditWindow();
            preview.Prepare(); // Create every HWND before keyboard processing, never during a composing key.
            settings = testing ? new Settings() : Settings.Load();
            Words = new WordPreferences { Enabled = settings.LearnChoices ?? true };
            personalStore = testing ? null : new PreferenceStore(Path.Combine(Settings.DirectoryPath, "learned-words.json"), Words);
            Focus = new FocusMonitor(); Engine = new Detector(Words); Input = new InputController(Focus, Engine);
            ApplySensitivity(settings.Sensitivity);
            Input.SemicolonShortcut = settings.SemicolonShortcut ?? true;
            if (!testing) foreach (string name in settings.ExcludedApps ?? new string[0]) Input.Excluded.Add(name);
            icon = MakeIcon(); Dashboard.Icon = icon;
            var menu = new ContextMenuStrip { Font = new Font("Malgun Gothic", 10) };
            enabled = new ToolStripMenuItem("자동 판별 켜짐", null, delegate { Toggle(); });
            menu.Items.Add(enabled);
            menu.Items.Add("입력 연습 및 상태", null, delegate { ShowDashboard(); });
            excluded = new ToolStripMenuItem("현재 앱에서 사용 안 함", null, delegate { ExcludeCurrent(); }); menu.Items.Add(excluded);
            recovery = new ToolStripMenuItem("미확정 조합 복사", null, delegate { if (Input.Recovery.Length > 0) Clipboard.SetText(Input.Recovery); }); menu.Items.Add(recovery);
            menu.Items.Add(new ToolStripSeparator());
            semicolon = new ToolStripMenuItem("단어 뒤 ; 로 후보 전환", null, delegate
            { Input.PreservePending(); Input.SemicolonShortcut = !Input.SemicolonShortcut; settings.SemicolonShortcut = Input.SemicolonShortcut; SaveSettings(); });
            menu.Items.Add(semicolon);
            learn = new ToolStripMenuItem("개인 선택 기억 사용", null, delegate
            { Input.PreservePending(); Words.Enabled = !Words.Enabled; settings.LearnChoices = Words.Enabled; SaveSettings(); });
            menu.Items.Add(learn);
            sensitivity = new ToolStripMenuItem("판별 민감도");
            foreach (string[] option in new[] { new[] { "conservative", "보수 · 확실할 때만 한글" }, new[] { "balanced", "표준" }, new[] { "aggressive", "적극 · 짧은 단어도 한글" } })
            {
                string key = option[0];
                var item = new ToolStripMenuItem(option[1], null, delegate { Input.PreservePending(); ApplySensitivity(key); settings.Sensitivity = currentSensitivity; SaveSettings(); }) { Tag = key };
                sensitivity.DropDownItems.Add(item);
            }
            menu.Items.Add(sensitivity);
            undoLearning = new ToolStripMenuItem("최근 학습 되돌리기", null, delegate { Input.PreservePending(); Words.UndoLast(); }); menu.Items.Add(undoLearning);
            clearLearning = new ToolStripMenuItem("개인 학습 초기화", null, delegate { Input.PreservePending(); Words.Clear(); }); menu.Items.Add(clearLearning);
            menu.Items.Add(new ToolStripSeparator());
            startup = new ToolStripMenuItem("Windows 시작 시 실행", null, delegate { SetStartup(!StartupEnabled()); }); menu.Items.Add(startup);
            menu.Items.Add("종료", null, delegate { ExitThread(); });
            tray = new NotifyIcon { Text = "Hanautomata · 자동 한영 입력", Icon = icon, ContextMenuStrip = menu, Visible = true };
            tray.DoubleClick += delegate { ShowDashboard(); };
            menu.Opening += delegate { enabled.Checked = Input.Enabled; startup.Checked = StartupEnabled(); recovery.Enabled = Input.Recovery.Length > 0;
                semicolon.Checked = Input.SemicolonShortcut; learn.Checked = Words.Enabled;
                foreach (ToolStripMenuItem option in sensitivity.DropDownItems) option.Checked = (string)option.Tag == currentSensitivity;
                undoLearning.Enabled = Words.CanUndo; clearLearning.Enabled = Words.Count > 0; clearLearning.Text = "개인 학습 초기화 (" + Words.Count + "개)";
                excluded.Enabled = lastOtherProcess.Length > 0; excluded.Text = (Input.Excluded.Contains(lastOtherProcess) ? "다시 사용: " : "사용 안 함: ") + lastOtherProcess; };
            Dashboard.Toggle.Click += delegate { Toggle(); };
            Dashboard.Reconnect.Click += delegate { Input.Reconnect(); };
            timer = new System.Windows.Forms.Timer { Interval = 60 }; timer.Tick += Tick; timer.Start();
            if (!background) ShowDashboard();
        }
        void ShowDashboard()
        {
            Dashboard.Show(); Native.ShowWindow(Dashboard.Handle, 9);
            Dashboard.Activate(); Dashboard.Practice.Focus();
        }
        void Toggle() { lock (Input.Sync) { Input.PreservePending(); Input.Enabled = !Input.Enabled; } Focus.Invalidate(); }
        internal void ApplySensitivity(string preset)
        {
            double korean, english;
            currentSensitivity = LanguageScorer.PresetThresholds(preset, out korean, out english);
            Engine.Scorer.Configure(korean, english);
        }
        internal string Sensitivity { get { return currentSensitivity; } }
        void Tick(object sender, EventArgs args)
        {
            if (stopEvent != null && stopEvent.WaitOne(0)) { ExitThread(); return; }
            Input.CheckHook(); Input.ValidatePending(); var snapshot = Focus.Snapshot;
            if (Diagnostics != null) Diagnostics.Tick(Input, Focus);
            if (personalStore != null) personalStore.ScheduleSave();
            if (!string.IsNullOrEmpty(snapshot.ProcessName) && snapshot.ProcessName != "Hanautomata") lastOtherProcess = snapshot.ProcessName;
            bool active = Input.Enabled && Focus.IsSafe(snapshot) && !Input.Excluded.Contains(snapshot.ProcessName);
            // Keys are captured while a refresh is pending, but the candidate window only shows verified fields.
            bool fresh = active && Focus.IsFresh(snapshot);
            Dashboard.State.Text = !Input.Enabled ? "일시정지 · 기본 입력기를 사용합니다" : active ?
                (Input.HookHealthy ? "자동 판별 중 · " + snapshot.ProcessName : "키 입력 연결 확인 중 · 자동 복구 대기") : snapshot.Reason;
            Dashboard.Detail.Text = "입력 " + (Input.HardwareKeys + Input.InjectedKeys) + "키 · 확정 " + Input.Commits + "회 · 자동 한글 " + Input.AutoConversions + " · 되돌림 " + Input.SwapsAfterSpace +
                " · 선택 변환 " + Input.SelectionFlips + " · 개인 학습 " + Words.Count + "개 · 연결 복구 " + Input.HookRestarts + "회" + (Input.SentinelActive ? " · 감시 켜짐" : Input.SentinelFallback ? " · 감시 호환모드" : " · 감시 없음") +
                " · 민감도 " + (currentSensitivity == "conservative" ? "보수" : currentSensitivity == "aggressive" ? "적극" : "표준") + (Words.Enabled ? " (학습 켜짐)" : " (학습 꺼짐)");
            Dashboard.Instructions.Text = Input.SemicolonShortcut ? "Space 확정 · ; / F2 후보 · 확정 직후 F2 = 직전 단어 바꾸기 · Shift+F2 = 선택 영역 한↔영 · ;; 문자 ; · Esc 취소 · F12 일시정지" : "Space 확정 · F2 후보 바꾸기 · 확정 직후 F2 = 직전 단어 바꾸기 · Shift+F2 = 선택 영역 한↔영 · Esc 조합 취소 · F12 일시정지";
            Dashboard.Toggle.Text = Input.Enabled ? "일시정지  F12" : "다시 시작  F12";
            Dashboard.Recovery.Text = Input.LastError.Length > 0 ? Input.LastError : Input.Recovery.Length > 0 ? "이동 전 조합이 보관되었습니다. 트레이 메뉴에서 복사할 수 있습니다." : "";
            if (personalStore != null && personalStore.LastError.Length > 0) Dashboard.Recovery.Text = personalStore.LastError;
            tray.Text = Input.Enabled ? "Hanautomata · 자동 판별" : "Hanautomata · 일시정지";
            Decision candidate = null;
            lock (Input.Sync) { if (!Input.Composition.IsEmpty) candidate = Input.Composition.Current; }
            if (fresh && candidate != null) preview.UpdateCandidate(candidate, snapshot.Position, Input.SemicolonShortcut);
            else preview.HidePreview();
        }
        void ExcludeCurrent()
        {
            if (lastOtherProcess.Length == 0) return;
            if (!Input.Excluded.Add(lastOtherProcess)) Input.Excluded.Remove(lastOtherProcess);
            Input.PreservePending();
            SaveSettings();
        }
        void SaveSettings()
        {
            if (testing) return;
            settings.ExcludedApps = Input.Excluded.OrderBy(x => x).ToArray(); settings.Save();
        }
        static Icon MakeIcon()
        {
            using (var bitmap = new Bitmap(32, 32)) using (var graphics = Graphics.FromImage(bitmap))
            using (var brush = new SolidBrush(Color.FromArgb(29, 77, 216))) using (var font = new Font("Malgun Gothic", 17, FontStyle.Bold, GraphicsUnit.Pixel))
            {
                graphics.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
                graphics.FillEllipse(brush, 0, 0, 31, 31);
                graphics.DrawString("한", font, Brushes.White, 5, 5);
                IntPtr native = bitmap.GetHicon();
                try { using (var original = Icon.FromHandle(native)) return (Icon)original.Clone(); }
                finally { Native.DestroyIcon(native); }
            }
        }
        // Carries the autostart entry over from the previous name so two instances never hook the keyboard together.
        internal static void MigrateLegacyStartup()
        {
            try
            {
                using (RegistryKey key = Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Run", true))
                {
                    if (key == null || key.GetValue("HanFlow") == null) return;
                    key.DeleteValue("HanFlow", false);
                    key.SetValue("Hanautomata", "\"" + Application.ExecutablePath + "\" --background");
                }
            }
            catch (System.Security.SecurityException) { }
            catch (UnauthorizedAccessException) { }
            catch (IOException) { }
        }
        static bool StartupEnabled()
        {
            using (RegistryKey key = Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Run"))
                return key != null && string.Equals(key.GetValue("Hanautomata") as string, "\"" + Application.ExecutablePath + "\" --background", StringComparison.OrdinalIgnoreCase);
        }
        static void SetStartup(bool enabled)
        {
            using (RegistryKey key = Registry.CurrentUser.CreateSubKey(@"Software\Microsoft\Windows\CurrentVersion\Run"))
            {
                if (enabled) key.SetValue("Hanautomata", "\"" + Application.ExecutablePath + "\" --background");
                else key.DeleteValue("Hanautomata", false);
            }
        }
        protected override void ExitThreadCore()
        {
            timer.Stop(); Input.Dispose(); Focus.Dispose(); tray.Visible = false;
            if (personalStore != null) personalStore.FlushNow();
            preview.Dispose(); Dashboard.ClosingPermanently = true; Dashboard.Close(); Dashboard.Dispose();
            tray.Dispose(); timer.Dispose(); icon.Dispose(); base.ExitThreadCore();
        }
    }
    internal static class Program
    {
        [STAThread]
        static int Main(string[] args)
        {
            Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
            // Shorter, less frequent blocking collections keep the hook thread inside LowLevelHooksTimeout.
            System.Runtime.GCSettings.LatencyMode = System.Runtime.GCLatencyMode.SustainedLowLatency;
            string sid = System.Security.Principal.WindowsIdentity.GetCurrent().User.Value;
            string stopName = "Local\\Hanautomata.Stop." + sid;
            if (args.Contains("--exit"))
            {
                try { using (var signal = EventWaitHandle.OpenExisting(stopName)) signal.Set(); } catch (WaitHandleCannotBeOpenedException) { }
                return 0;
            }
            if (args.Length == 2 && args[0] == "--integration-test") return IntegrationTests.Run(args[1]);
            if (args.Length == 3 && args[0] == "--corpus-integration") return IntegrationTests.RunCorpus(args[1], args[2]);
            bool created;
            using (var mutex = new Mutex(true, "Local\\Hanautomata.Instance." + sid, out created))
            {
                if (!created) { MessageBox.Show("Hanautomata가 이미 실행 중입니다. 작업 표시줄의 한 아이콘을 확인하세요.", "Hanautomata"); return 0; }
                using (var stop = new EventWaitHandle(false, EventResetMode.ManualReset, stopName))
                {
                    try
                    {
                        Settings.MigrateLegacyData(); TrayApplication.MigrateLegacyStartup();
                        var app = new TrayApplication(args.Contains("--background"), false, stop);
                        int diagnostic = Array.IndexOf(args, "--diagnose-input");
                        if (diagnostic >= 0 && diagnostic + 1 < args.Length) app.Input.Diagnostics = app.Diagnostics = new InputDiagnostics(args[diagnostic + 1]);
                        Application.Run(app);
                    }
                    catch (Exception ex) { MessageBox.Show("Hanautomata를 시작하지 못했습니다.\n" + ex.Message, "Hanautomata", MessageBoxButtons.OK, MessageBoxIcon.Error); return 1; }
                }
                mutex.ReleaseMutex();
            }
            return 0;
        }
    }
}
