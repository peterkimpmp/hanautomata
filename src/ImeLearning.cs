using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Text;
using System.Threading.Tasks;
using System.Web.Script.Serialization;
using System.Windows.Forms;
using Microsoft.Win32;

namespace Hanautomata
{
    internal sealed class LearningVersion
    {
        public LearningVersion() { }
        public string store_id { get; set; }
        public ulong generation { get; set; }
        public ulong revision { get; set; }
    }
    internal sealed class LearningWord
    {
        public LearningWord() { }
        public string raw { get; set; }
        public string keys { get; set; }
        public bool korean { get; set; }
        public long sequence { get; set; }
        public string context { get; set; }
        public double korean_weight { get; set; }
        public double english_weight { get; set; }
    }
    internal sealed class LearningSnapshot
    {
        public LearningSnapshot() { }
        public LearningVersion version { get; set; }
        public LearningWord[] entries { get; set; }
    }
    internal sealed class LearningReply
    {
        public LearningReply() { }
        public int protocol { get; set; }
        public bool ok { get; set; }
        public string error { get; set; }
        public LearningSnapshot snapshot { get; set; }
        static bool Key(char c) { return c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z'; }
        static bool Hex(string text, int length) { return text != null && text.Length == length && text.All(c => c >= '0' && c <= '9' || c >= 'a' && c <= 'f'); }
        internal static LearningReply Parse(string json)
        {
            var parser = new JavaScriptSerializer { MaxJsonLength = 2097152, RecursionLimit = 16 };
            var fields = parser.Deserialize<Dictionary<string, object>>(json);
            object protocol, success;
            if (fields == null || !fields.TryGetValue("protocol", out protocol) || !(protocol is int) || (int)protocol != 1
                || !fields.TryGetValue("ok", out success) || !(success is bool)) throw new InvalidDataException();
            var reply = new LearningReply { protocol = 1, ok = (bool)success };
            if (!reply.ok)
            {
                object error; reply.error = fields.TryGetValue("error", out error) ? error as string : null;
                if (string.IsNullOrEmpty(reply.error) || reply.error.Length > 64 || !reply.error.All(c => c >= 'a' && c <= 'z' || c == '_')) throw new InvalidDataException();
                return reply;
            }
            object payload, stamp;
            var snapshotFields = fields.TryGetValue("snapshot", out payload) ? payload as Dictionary<string, object> : null;
            var version = snapshotFields != null && snapshotFields.TryGetValue("version", out stamp) ? parser.ConvertToType<LearningVersion>(stamp) : null;
            if (version == null || !Hex(version.store_id, 32) || version.store_id.All(c => c == '0') || version.generation == 0
                || version.revision < version.generation) throw new InvalidDataException();
            reply.snapshot = new LearningSnapshot { version = version };
            // A valid version still permits an explicitly confirmed Reset when one entry cannot be displayed.
            // Never show a partial list or allow an individual deletion from a rejected list.
            try
            {
                object entries;
                var words = snapshotFields.TryGetValue("entries", out entries) ? parser.ConvertToType<LearningWord[]>(entries) : null;
                ValidateEntries(words); reply.snapshot.entries = words;
            }
            catch (Exception e)
            {
                if (!(e is InvalidDataException || e is ArgumentException || e is InvalidOperationException || e is FormatException || e is OverflowException || e is InvalidCastException)) throw;
            }
            return reply;
        }
        static void ValidateEntries(LearningWord[] entries)
        {
            if (entries == null || entries.Length > 1024) throw new InvalidDataException();
            long last = 0; var identities = new HashSet<string>(StringComparer.Ordinal);
            foreach (var word in entries)
            {
                if (word == null || string.IsNullOrEmpty(word.raw) || word.keys == null || word.raw.Length > 64 || word.raw.Length != word.keys.Length
                    || !word.raw.All(Key) || !word.keys.All(Key) || word.sequence <= last || word.context == null
                    || word.context != "" && !Hex(word.context, 64) || !identities.Add(word.raw + "\t" + word.keys + "\t" + word.context)
                    || double.IsNaN(word.korean_weight) || double.IsNaN(word.english_weight)
                    || word.korean_weight < 0 || word.english_weight < 0 || word.korean_weight > 16 || word.english_weight > 16
                    || word.korean_weight + word.english_weight == 0) throw new InvalidDataException();
                last = word.sequence;
            }
        }
    }
    internal interface ILearningClient { Task<LearningReply> SendAsync(object request); }
    internal sealed class LearningClient : ILearningClient
    {
        readonly string executable;
        readonly int timeoutMs;
        internal LearningClient(string executable, int timeoutMs = 10000) { this.executable = Path.GetFullPath(executable); this.timeoutMs = timeoutMs; }
        internal static ILearningClient ForSettings(string settingsPath)
        {
            // Alternate settings/preview windows must never fall through to the real user's learning database.
            if (!string.Equals(Path.GetFullPath(settingsPath), Path.GetFullPath(ImeSettingsStore.DefaultPath), StringComparison.OrdinalIgnoreCase)) return null;
            var directory = AppDomain.CurrentDomain.BaseDirectory.TrimEnd(Path.DirectorySeparatorChar);
            if (string.Equals(Path.GetFileName(directory), "ui", StringComparison.OrdinalIgnoreCase))
            {
                var architecture = Path.Combine(Directory.GetParent(directory).FullName, Environment.Is64BitOperatingSystem ? "x64" : "x86");
                var sibling = Path.Combine(architecture, "hanautomata-ime-setup.exe");
                if (File.Exists(sibling) && File.Exists(Path.Combine(architecture, "hanautomata_tsf.dll"))) return new LearningClient(sibling);
                return null; // An incomplete revision must not silently fall back to a different installed revision.
            }
            // The tray entry point lives outside a TSF revision. Resolve only machine registration, not HKCU
            // overrides, PATH or the working directory; its native helper still enforces the desktop token.
            try
            {
                using (var machine = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, Environment.Is64BitOperatingSystem ? RegistryView.Registry64 : RegistryView.Registry32))
                using (var key = machine.OpenSubKey(@"Software\Classes\CLSID\{AC481E74-0295-436A-B0B8-AFA81FE9CBF1}\InprocServer32"))
                {
                    var helper = RegisteredHelper(key == null ? null : key.GetValue(null, null, RegistryValueOptions.DoNotExpandEnvironmentNames) as string);
                    return helper == null ? null : new LearningClient(helper);
                }
            }
            catch (Exception e)
            {
                if (!(e is IOException || e is UnauthorizedAccessException || e is System.Security.SecurityException || e is ArgumentException || e is NotSupportedException)) throw;
                return null;
            }
        }
        internal static string RegisteredHelper(string library)
        {
            if (string.IsNullOrEmpty(library) || !Path.IsPathRooted(library)
                || !string.Equals(Path.GetFullPath(library), library, StringComparison.OrdinalIgnoreCase)
                || !string.Equals(Path.GetFileName(library), "hanautomata_tsf.dll", StringComparison.OrdinalIgnoreCase)) return null;
            var root = Path.GetPathRoot(library);
            if (root.Length != 3 || root[1] != ':') return null;
            return Path.Combine(Path.GetDirectoryName(library), "hanautomata-ime-setup.exe");
        }
        internal static string Encode(object request)
        {
            var json = new JavaScriptSerializer { MaxJsonLength = 4096, RecursionLimit = 16 }.Serialize(request);
            var ascii = new StringBuilder();
            foreach (char c in json) { if (c > 127) ascii.Append("\\u" + ((int)c).ToString("x4")); else ascii.Append(c); }
            if (ascii.Length > 4096) throw new InvalidDataException(); return ascii.ToString();
        }
        static async Task<string> ReadBounded(StreamReader reader, int maximum)
        {
            var text = new StringBuilder(); var buffer = new char[4096]; int count;
            while ((count = await reader.ReadAsync(buffer, 0, buffer.Length).ConfigureAwait(false)) != 0)
            {
                if (text.Length > maximum - count) throw new InvalidDataException(); text.Append(buffer, 0, count);
            }
            return text.ToString();
        }
        static async void Observe(Task task) { try { await task.ConfigureAwait(false); } catch (Exception) { } }
        public Task<LearningReply> SendAsync(object request)
        {
            string input = Encode(request);
            return Task.Run(() => {
                if ((File.GetAttributes(executable) & FileAttributes.ReparsePoint) != 0) throw new IOException();
                using (var process = new Process { StartInfo = new ProcessStartInfo {
                    FileName = executable, Arguments = "--learning-manage", WorkingDirectory = Path.GetDirectoryName(executable),
                    UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden,
                    RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true,
                    StandardOutputEncoding = Encoding.UTF8, StandardErrorEncoding = Encoding.UTF8 } })
                {
                    process.Start();
                    var output = ReadBounded(process.StandardOutput, 2097152); var errors = ReadBounded(process.StandardError, 4096);
                    var write = Task.Run(() => {
                        // ASCII JSON escapes preserve Unicode across .NET Framework's OEM stdin encoding.
                        if (!process.StandardInput.Encoding.GetBytes(input).SequenceEqual(Encoding.ASCII.GetBytes(input))) throw new InvalidDataException();
                        process.StandardInput.Write(input); process.StandardInput.Close();
                    });
                    Observe(output); Observe(errors); Observe(write);
                    try
                    {
                        if (!process.WaitForExit(timeoutMs) || !Task.WaitAll(new Task[] { output, errors, write }, 2000)) throw new IOException();
                        var reply = LearningReply.Parse(output.Result);
                        if (process.ExitCode != (reply.ok ? 0 : 2)) throw new InvalidDataException();
                        return reply;
                    }
                    finally
                    {
                        if (!process.HasExited) { try { process.Kill(); process.WaitForExit(2000); } catch (InvalidOperationException) { } }
                    }
                }
            });
        }
    }

    internal sealed class LearningPane : UserControl
    {
        readonly ILearningClient client;
        readonly Func<string, bool> confirm;
        readonly Label notice = new Label { AutoSize = true, Dock = DockStyle.Fill };
        readonly Label status = new Label { AutoSize = true, Dock = DockStyle.Fill };
        readonly DataGridView list = new DataGridView { ReadOnly = true, AllowUserToAddRows = false, AllowUserToDeleteRows = false,
            MultiSelect = false, RowHeadersVisible = false, SelectionMode = DataGridViewSelectionMode.FullRowSelect,
            AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill, Dock = DockStyle.Fill, AccessibleName = "기억한 선택 목록" };
        readonly Button reload = new Button { Text = "새로고침", AutoSize = true };
        readonly Button forget = new Button { Text = "선택 지우기", AutoSize = true };
        readonly Button clear = new Button { Text = "전체 초기화", AutoSize = true };
        LearningSnapshot snapshot;
        internal bool Busy { get; private set; }
        internal LearningPane(ILearningClient client, Func<string, bool> confirm = null)
        {
            this.client = client; this.confirm = confirm ?? (message => MessageBox.Show(this, message, "기억한 선택", MessageBoxButtons.YesNo, MessageBoxIcon.Question, MessageBoxDefaultButton.Button2) == DialogResult.Yes);
            AccessibleName = "기억한 선택 관리";
            AutoScaleMode = AutoScaleMode.None; Height = 300; MinimumSize = new Size(240, 260);
            var layout = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 4 };
            layout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            layout.RowStyles.Add(new RowStyle(SizeType.AutoSize)); layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            layout.RowStyles.Add(new RowStyle(SizeType.AutoSize)); layout.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            notice.Text = "영문은 눌렀던 키입니다. 선택을 지우면 그 단어의 문맥별 기억도 함께 지웁니다.";
            list.Columns.Add(new DataGridViewTextBoxColumn { HeaderText = "입력한 키", FillWeight = 45 });
            list.Columns.Add(new DataGridViewTextBoxColumn { HeaderText = "최근 선택", FillWeight = 30 });
            list.Columns.Add(new DataGridViewTextBoxColumn { HeaderText = "범위", FillWeight = 25 });
            list.ColumnHeadersHeightSizeMode = DataGridViewColumnHeadersHeightSizeMode.AutoSize;
            var buttons = new FlowLayoutPanel { AutoSize = true, Dock = DockStyle.Fill, WrapContents = true };
            buttons.Controls.AddRange(new Control[] { reload, forget, clear });
            layout.Controls.Add(notice, 0, 0); layout.Controls.Add(list, 0, 1); layout.Controls.Add(buttons, 0, 2); layout.Controls.Add(status, 0, 3);
            Controls.Add(layout); reload.Click += async delegate { await RefreshAsync(); };
            forget.Click += async delegate { await ForgetAsync(); }; clear.Click += async delegate { await ResetAsync(); };
            list.SelectionChanged += delegate { EnableActions(); };
            FontChanged += delegate { list.RowTemplate.Height = Font.Height + 12; };
            status.Text = client == null ? "이 실행 환경에서는 학습 저장소를 연결하지 않습니다. 설치된 입력기의 설정 창에서 확인해 주세요." : "새로고침을 누르면 저장된 선택을 읽습니다.";
            EnableActions();
        }
        void EnableActions()
        {
            reload.Enabled = client != null && !Busy; clear.Enabled = snapshot != null && !Busy;
            forget.Enabled = snapshot != null && !Busy && list.SelectedRows.Count == 1;
            list.Enabled = !Busy;
        }
        internal Task RefreshAsync() { return SendAsync(new { command = "snapshot" }, false); }
        internal Task ForgetAsync()
        {
            if (Busy || snapshot == null || list.SelectedRows.Count != 1) return Task.FromResult(0);
            var word = (LearningWord)list.SelectedRows[0].Tag;
            if (!confirm("‘" + word.raw + "’의 기억과 문맥별 기억을 지울까요?")) return Task.FromResult(0);
            return SendAsync(new { command = "forget", version = snapshot.version, raw = word.raw, keys = word.keys }, true);
        }
        internal Task ResetAsync()
        {
            if (Busy || snapshot == null || !confirm("현재 입력기 저장소의 기억을 모두 지울까요?\n기존 트레이 앱의 학습 파일과 백업은 유지됩니다.")) return Task.FromResult(0);
            return SendAsync(new { command = "reset", version = snapshot.version }, true);
        }
        async Task SendAsync(object request, bool mutation)
        {
            if (Busy || client == null) return;
            Busy = true; EnableActions(); status.Text = "학습 저장소를 확인하고 있습니다…";
            try
            {
                var reply = await client.SendAsync(request);
                if (IsDisposed) return;
                if (!reply.ok)
                {
                    snapshot = null;
                    switch (reply.error)
                    {
                        case "stale_generation": case "external_change": case "wrong_store":
                            status.Text = "다른 앱에서 목록이 바뀌었습니다. 새로고침 후 다시 선택해 주세요."; break;
                        case "missing_state": status.Text = "저장된 학습 파일을 찾지 못했습니다. 입력기에서 학습한 뒤 다시 확인해 주세요."; break;
                        case "busy": status.Text = "다른 앱이 저장 중입니다. 잠시 후 새로고침해 주세요."; break;
                        case "unauthorized": status.Text = "일반 사용자 데스크톱에서 설정 창을 열어 주세요."; break;
                        default: status.Text = "저장 결과를 확인하지 못했습니다. 새로고침으로 확인해 주세요."; break;
                    }
                    return;
                }
                snapshot = reply.snapshot; list.Rows.Clear();
                foreach (var word in snapshot.entries ?? new LearningWord[0])
                {
                    int index = list.Rows.Add(word.raw, word.korean ? "한글" : "영문", word.context == "" ? "공통" : "문맥별");
                    list.Rows[index].Tag = word;
                }
                status.Text = snapshot.entries == null ? "목록을 완전히 읽지 못했습니다. 새로고침하거나 현재 저장소의 기억을 전체 초기화할 수 있습니다."
                    : mutation ? "저장소를 변경했습니다. 앱에서 입력기를 다시 선택한 뒤 결과를 확인해 주세요." : "저장된 선택 " + snapshot.entries.Length + "개를 읽었습니다.";
            }
            catch (Exception) { if (!IsDisposed) { snapshot = null; status.Text = "저장 결과를 확인하지 못했습니다. 새로고침으로 확인해 주세요."; } }
            finally { Busy = false; if (!IsDisposed) EnableActions(); }
        }
    }
}
