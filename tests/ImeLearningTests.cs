using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Text;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Web.Script.Serialization;
using System.Windows.Forms;
using Hanautomata;

// Hidden in-process controls and owned helper fixtures only. Never opens the user's learning store.
internal static class ImeLearningTests
{
    static int assertions;
    static void Check(bool condition, string name) { assertions++; if (!condition) throw new Exception(name); }
    static void Rejected(Action action, string name)
    {
        bool rejected = false; try { action(); } catch (Exception) { rejected = true; }
        Check(rejected, name);
    }
    static LearningReply Reply(ulong revision = 7, bool empty = false)
    {
        return new LearningReply { protocol = 1, ok = true, snapshot = new LearningSnapshot {
            version = new LearningVersion { store_id = "1234567890abcdef1234567890abcdef", generation = 3, revision = revision },
            entries = empty ? new LearningWord[0] : new[] { new LearningWord { raw = "hello", keys = "hello", korean = false,
                sequence = 1, context = "", korean_weight = 0, english_weight = 1 } } } };
    }
    static string Json(object value) { return new JavaScriptSerializer { MaxJsonLength = 2097152 }.Serialize(value); }
    static IEnumerable<Control> Controls(Control root)
    {
        foreach (Control c in root.Controls) { yield return c; foreach (var nested in Controls(c)) yield return nested; }
    }
    static void Finish(Task task)
    {
        var watch = Stopwatch.StartNew();
        while (!task.IsCompleted && watch.ElapsedMilliseconds < 10000) { Application.DoEvents(); Thread.Sleep(1); }
        Check(task.IsCompleted, "async operation terminates"); task.GetAwaiter().GetResult();
    }
    sealed class Client : ILearningClient
    {
        internal readonly List<string> Requests = new List<string>();
        internal TaskCompletionSource<LearningReply> Pending;
        internal LearningReply Next = Reply();
        public Task<LearningReply> SendAsync(object request)
        {
            Requests.Add(LearningClient.Encode(request)); return Pending == null ? Task.FromResult(Next) : Pending.Task;
        }
    }
    static int Helper()
    {
        string mode = Environment.GetEnvironmentVariable("HANAUTOMATA_LEARNING_TEST_MODE");
        if (mode == "hang")
        {
            File.WriteAllText(Environment.GetEnvironmentVariable("HANAUTOMATA_LEARNING_TEST_PID"), Process.GetCurrentProcess().Id.ToString());
            Thread.Sleep(30000); return 0;
        }
        string input;
        using (var stream = Console.OpenStandardInput())
        using (var bytes = new MemoryStream())
        {
            stream.CopyTo(bytes); byte[] raw = bytes.ToArray();
            // Unlike Console.In, raw bytes do not hide an encoding preamble from this transport test.
            if (raw.Length == 0 || raw.Length > 4096 || raw[0] != (byte)'{') return 3;
            input = new UTF8Encoding(false, true).GetString(raw);
        }
        if (mode == "flood") { Console.Out.Write(new string('x', 2097153)); return 0; }
        if (mode == "stderr") { Console.Error.Write(new string('x', 4097)); Console.Write(Json(Reply())); return 0; }
        if (mode == "bad") { Console.Write("not-json"); return 0; }
        if (mode == "exit") { Console.Write(Json(Reply())); return 2; }
        if (mode == "unicode")
        {
            var request = new JavaScriptSerializer().Deserialize<Dictionary<string, object>>(input);
            if ((string)request["raw"] != "한글") return 3;
        }
        Console.Out.Write(Json(Reply(ulong.MaxValue))); return 0;
    }
    [STAThread]
    static int Main(string[] args)
    {
        if (args.Length == 1 && args[0] == "--learning-manage") return Helper();
        try
        {
            Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
            var reply = LearningReply.Parse(Json(Reply(ulong.MaxValue)));
            Check(reply.snapshot.version.revision == ulong.MaxValue, "64-bit version remains exact");
            var wire = new JavaScriptSerializer().Deserialize<Dictionary<string, object>>(Json(Reply()));
            ((Dictionary<string, object>)wire["snapshot"])["fences"] = new[] { new { generation = 2, word = new string('a', 64) } };
            Check(LearningReply.Parse(Json(wire)).snapshot.entries.Length == Reply().snapshot.entries.Length,
                "additional native deletion history does not invalidate the management list");
            Check(LearningClient.Encode(new { version = reply.snapshot.version }).Contains(ulong.MaxValue.ToString()), "version round trip remains exact");
            Check(LearningClient.Encode(new { raw = "한글" }).All(c => c < 128), "Unicode request is ASCII escaped");
            Rejected(() => LearningClient.Encode(new { raw = new string('한', 4096) }), "escaped request is bounded");
            Rejected(() => LearningReply.Parse("{}"), "unknown reply is rejected");
            var bad = Reply(); bad.snapshot.version.generation = 0;
            Rejected(() => LearningReply.Parse(Json(bad)), "zero generation rejected");
            bad = Reply(); bad.snapshot.entries[0].context = "raw private text";
            Check(LearningReply.Parse(Json(bad)).snapshot.entries == null, "context text rejected while version retained");
            bad = Reply(); bad.snapshot.entries[0].raw = "한글"; bad.snapshot.entries[0].keys = "한글";
            Check(LearningReply.Parse(Json(bad)).snapshot.entries == null, "raw key alphabet matches native ASCII contract");
            bad = Reply(); bad.snapshot.entries = new[] { bad.snapshot.entries[0], bad.snapshot.entries[0] };
            Check(LearningReply.Parse(Json(bad)).snapshot.entries == null, "duplicate entries rejected");
            bad = Reply(); bad.snapshot.entries = Enumerable.Repeat(bad.snapshot.entries[0], 1025).ToArray();
            Check(LearningReply.Parse(Json(bad)).snapshot.entries == null, "oversized list rejected");
            string alternate = Path.Combine(Path.GetTempPath(), "ime-learning-preview-" + Guid.NewGuid().ToString("N"), "settings.json");
            Check(LearningClient.ForSettings(alternate) == null && !Directory.Exists(Path.GetDirectoryName(alternate)), "preview never connects or creates user store");
            Check(LearningClient.RegisteredHelper(@"C:\Program Files\한오토마타\revision\x64\hanautomata_tsf.dll") == @"C:\Program Files\한오토마타\revision\x64\hanautomata-ime-setup.exe", "tray locates the registered revision with spaces and Hangul");
            foreach (string path in new[] { null, "hanautomata_tsf.dll", @"C:hanautomata_tsf.dll", @"C:\revision\other.dll", @"\\server\share\hanautomata_tsf.dll" })
                Check(LearningClient.RegisteredHelper(path) == null, "untrusted helper source rejected");

            var client = new Client(); bool confirm = false; int confirmations = 0;
            using (var host = new Form())
            using (var pane = new LearningPane(client, text => { confirmations++; return confirm; }))
            {
                host.Controls.Add(pane); foreach (var control in Controls(host).ToArray()) { IntPtr handle = control.Handle; }
                Finish(pane.RefreshAsync());
                var list = Controls(pane).OfType<DataGridView>().Single();
                Check(list.Rows.Count == 1 && (string)list.Rows[0].Cells[0].Value == "hello", "saved choice is shown");
                list.Rows[0].Selected = true; list.CurrentCell = list.Rows[0].Cells[0];
                Finish(pane.ForgetAsync()); Check(confirmations == 1 && client.Requests.Count == 1, "cancel does not submit delete");
                confirm = true; client.Next = new LearningReply { protocol = 1, ok = false, error = "external_change" };
                Finish(pane.ForgetAsync());
                var sent = new JavaScriptSerializer().Deserialize<Dictionary<string, object>>(client.Requests.Last());
                Check((string)sent["command"] == "forget" && (string)sent["raw"] == "hello", "selected identity submitted");
                Check(client.Requests.Last().Contains("\"revision\":7"), "delete uses the displayed version");
                int previous = client.Requests.Count; Finish(pane.ResetAsync());
                Check(client.Requests.Count == previous, "stale result prevents retry without refresh");
                Check(Controls(pane).OfType<Button>().Where(b => b.Text != "새로고침").All(b => !b.Enabled), "stale mutations disabled");
                client.Pending = new TaskCompletionSource<LearningReply>(); var pending = pane.RefreshAsync();
                Check(pane.Busy, "pending read reports busy"); previous = client.Requests.Count;
                Finish(pane.RefreshAsync()); Finish(pane.ResetAsync());
                Check(client.Requests.Count == previous, "busy read does not overlap requests");
                client.Pending.SetResult(Reply(9)); Finish(pending); client.Pending = null;
                client.Next = Reply(10, true); Finish(pane.ResetAsync());
                Check(client.Requests.Last().Contains("\"revision\":9") && list.Rows.Count == 0 && !pane.Busy, "reset confirms refreshed version and renders empty list");
                client.Next = LearningReply.Parse(Json(bad)); Finish(pane.RefreshAsync());
                Check(list.Rows.Count == 0 && Controls(pane).OfType<Button>().Single(b => b.Text == "전체 초기화").Enabled
                    && !Controls(pane).OfType<Button>().Single(b => b.Text == "선택 지우기").Enabled, "invalid list preserves only explicit reset and refresh");
                client.Next = Reply(11, true); Finish(pane.ResetAsync());
                Check(client.Requests.Last().Contains("\"revision\":7"), "reset after invalid list uses its valid version");
                foreach (float fontSize in new[] { 9F, 13.5F, 18F })
                {
                    using (var font = new Font("Malgun Gothic", fontSize))
                    {
                        pane.Font = font; pane.Height = Math.Max(300, font.Height * 20);
                        foreach (int width in new[] { 280, 500, 1040 })
                        {
                            pane.Width = width;
                            foreach (var control in Controls(pane).Reverse()) control.PerformLayout();
                            pane.PerformLayout();
                            foreach (var label in Controls(pane).OfType<Label>())
                            {
                                int needed = label.GetPreferredSize(new Size(label.Width, 0)).Height;
                                Check(label.Height >= needed && label.Bottom <= label.Parent.ClientSize.Height,
                                    "learning text fits at width=" + width + " font=" + fontSize + " actual=" + label.Height + " needed=" + needed);
                            }
                            Check(list.Height >= font.Height * 2 && list.Bottom <= list.Parent.ClientSize.Height, "learning list stays usable");
                            foreach (var button in Controls(pane).OfType<Button>())
                                Check(button.Right <= button.Parent.ClientSize.Width && button.Bottom <= button.Parent.ClientSize.Height, "learning buttons wrap inside viewport");
                        }
                    }
                }
            }
            string self = typeof(ImeLearningTests).Assembly.Location;
            string modeBefore = Environment.GetEnvironmentVariable("HANAUTOMATA_LEARNING_TEST_MODE");
            string pidBefore = Environment.GetEnvironmentVariable("HANAUTOMATA_LEARNING_TEST_PID");
            string pidFile = Path.Combine(Path.GetTempPath(), "ime-learning-owned-" + Guid.NewGuid().ToString("N") + ".pid");
            try
            {
                Environment.SetEnvironmentVariable("HANAUTOMATA_LEARNING_TEST_MODE", "unicode");
                var helper = new LearningClient(self, 1500);
                Check(helper.SendAsync(new { command = "snapshot", raw = "한글" }).GetAwaiter().GetResult().snapshot.version.revision == ulong.MaxValue, "actual private pipes preserve Unicode and version");
                foreach (string mode in new[] { "bad", "exit", "stderr", "flood" })
                {
                    Environment.SetEnvironmentVariable("HANAUTOMATA_LEARNING_TEST_MODE", mode);
                    Rejected(() => helper.SendAsync(new { command = "snapshot" }).GetAwaiter().GetResult(), "reject helper " + mode);
                }
                Environment.SetEnvironmentVariable("HANAUTOMATA_LEARNING_TEST_MODE", "hang");
                Environment.SetEnvironmentVariable("HANAUTOMATA_LEARNING_TEST_PID", pidFile);
                var watch = Stopwatch.StartNew();
                Rejected(() => helper.SendAsync(new { command = "snapshot" }).GetAwaiter().GetResult(), "hung child times out");
                Check(watch.ElapsedMilliseconds < 7000 && File.Exists(pidFile), "timeout remains bounded");
                int owned = int.Parse(File.ReadAllText(pidFile)); bool gone;
                try { using (var process = Process.GetProcessById(owned)) gone = process.HasExited; } catch (ArgumentException) { gone = true; }
                Check(gone, "owned hung helper is reaped");
            }
            finally
            {
                Environment.SetEnvironmentVariable("HANAUTOMATA_LEARNING_TEST_MODE", modeBefore);
                Environment.SetEnvironmentVariable("HANAUTOMATA_LEARNING_TEST_PID", pidBefore);
                if (File.Exists(pidFile)) File.Delete(pidFile);
            }
            if (args.Length >= 1)
            {
                var native = new LearningClient(args[0]);
                var rejected = native.SendAsync(new { command = "snapshot", path = "not-authorized" }).GetAwaiter().GetResult();
                Check(!rejected.ok && rejected.error == "invalid_request", "real native protocol rejects caller path before database access");
            }
            if (args.Length == 2)
            {
                var snapshots = File.ReadAllLines(args[1]).Select(LearningReply.Parse).ToArray();
                Check(snapshots.Length == 4 && snapshots.All(value => value.ok && value.snapshot.entries != null), "actual native snapshots parse completely");
                Check(snapshots[0].snapshot.entries.Length == 4 && snapshots[1].snapshot.entries.Length == 2 && snapshots[2].snapshot.entries.Length == 0, "native context deletion and reset remain readable");
                Check(snapshots[0].snapshot.entries.Any(word => word.context.Length == 64) && snapshots[0].snapshot.entries.Any(word => word.raw.Length == 64), "native context hash and maximum word remain readable");
                Check(snapshots[0].snapshot.entries.Any(word => word.korean_weight == 16 && word.english_weight == 0), "native boundary weights remain exact");
                Check(snapshots[3].snapshot.entries.Length == 1024 && snapshots[3].snapshot.entries.All(word => word.raw.StartsWith("Ab", StringComparison.Ordinal)), "native full-capacity mixed-case list remains readable");
            }
            Console.WriteLine("PASS: " + assertions + " learning management checks; synthetic state only"); return 0;
        }
        catch (Exception error) { Console.Error.WriteLine(error); return 1; }
    }
}
