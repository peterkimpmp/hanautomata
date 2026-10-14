using System;
using System.IO;
using System.Linq;
using System.Diagnostics;
using System.Collections.Generic;
using Hanautomata;

internal static class ImeSettingsTests
{
    static int passed, failed;
    static void Check(bool ok, string message) { if (!ok) throw new Exception(message); }
    static void Test(string name, Action run)
    {
        try { run(); passed++; Console.WriteLine("PASS " + name); }
        catch (Exception e) { failed++; Console.WriteLine("FAIL " + name + ": " + e.Message); }
    }
    static void Reject(Action run) { bool rejected = false; try { run(); } catch (Exception) { rejected = true; } Check(rejected, "invalid input accepted"); }
    static int Main(string[] args)
    {
        if (args.Length == 4 && args[0] == "--writer")
        {
            try { ImeSettingsStore.Save(args[1], new ImeSettings { InputMode = args[3] }, args[2]); return 0; }
            catch (IOException) { return 3; }
        }
        string fixture = args[0], output = args[1];
        string dir = Path.Combine(Path.GetTempPath(), "hanautomata-ime-settings-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(dir);
        try
        {
            Test("partial defaults and missing file are read only", delegate {
                Check(ImeSettings.Parse(ImeSettings.Utf8.GetBytes("{}")).ShiftSpace, "default lost");
                var path = Path.Combine(dir, "missing.json");
                Check(ImeSettingsStore.Load(path).Fingerprint == "missing" && !File.Exists(path), "missing file created");
            });
            Test("fixture options and UTF8 survive serialization", delegate {
                var v = ImeSettings.Parse(File.ReadAllBytes(fixture));
                Check(v.InputMode == "Korean" && v.Keyboard == "Sebeolsik390" && !v.ShiftSpace && v.LearningPaused, "options discarded");
                Check(v.ExcludedApps[1] == "한글 도구.exe", "Unicode file name lost");
                var bytes = v.Serialize();
                Check(!bytes.Take(3).SequenceEqual(new byte[] { 239, 187, 191 }), "unexpected BOM");
                Check(ImeSettings.Parse(bytes).ExcludedApps.SequenceEqual(v.ExcludedApps), "roundtrip failed");
                File.WriteAllBytes(output, bytes);
            });
            Test("invalid JSON, duplicate fields, types and unknown fields rejected", delegate {
                foreach (string text in new[] { "{", "null", "[]", "{} {}", "{'InputMode':'English'}", "{InputMode:\"English\"}", "{\"ShiftSpace\":\"false\"}", "{\"Format\":1,\"Format\":2}", "{\"RightAltt\":false}", "{\"ExcludedApps\":null}", "{\"ExcludedApps\":[null]}" })
                    try { Reject(delegate { ImeSettings.Parse(ImeSettings.Utf8.GetBytes(text)); }); } catch (Exception e) { throw new Exception(text + ": " + e.Message); }
            });
            Test("limits, future format and executable paths rejected", delegate {
                Reject(delegate { ImeSettings.Parse(new byte[65537]); });
                Reject(delegate { ImeSettings.Parse(new byte[] { 255 }); });
                foreach (var v in new[] { new ImeSettings { Format = 2 }, new ImeSettings { InputMode = "wrong" }, new ImeSettings { Keyboard = "wrong" }, new ImeSettings { ExcludedApps = new[] { "C:\\code.exe" } }, new ImeSettings { ExcludedApps = new[] { "*.exe" } }, new ImeSettings { ExcludedApps = new string[129] } }) Reject(v.Validate);
            });
            Test("atomic save retains previous version and detects stale editor", delegate {
                string path = Path.Combine(dir, "roundtrip.json");
                var first = ImeSettingsStore.Save(path, new ImeSettings(), "missing");
                var bytes = File.ReadAllBytes(path);
                var second = ImeSettingsStore.Save(path, new ImeSettings { InputMode = "English" }, first.Fingerprint);
                Check(File.ReadAllBytes(path + ".previous").SequenceEqual(bytes), "backup missing");
                Reject(delegate { ImeSettingsStore.Save(path, new ImeSettings { InputMode = "Korean" }, first.Fingerprint); });
                Check(ImeSettingsStore.Load(path).Value.InputMode == "English", "stale editor overwrote current settings");
                Check(second.Fingerprint != first.Fingerprint, "revision unchanged");
            });
            Test("corrupt settings survive load and explicit repair keeps backup", delegate {
                string path = Path.Combine(dir, "corrupt.json");
                byte[] broken = ImeSettings.Utf8.GetBytes("{broken"); File.WriteAllBytes(path, broken);
                var loaded = ImeSettingsStore.Load(path);
                Check(loaded.Error != null && loaded.Value == null, "corruption was hidden");
                Check(File.ReadAllBytes(path).SequenceEqual(broken), "load modified the file");
                ImeSettingsStore.Save(path, new ImeSettings(), loaded.Fingerprint);
                Check(File.ReadAllBytes(path + ".previous").SequenceEqual(broken), "corrupt original lost");
            });
            Test("an unavailable lock does not replace settings", delegate {
                string path = Path.Combine(dir, "locked.json");
                var first = ImeSettingsStore.Save(path, new ImeSettings(), "missing");
                using (var hold = new FileStream(path + ".lock", FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None))
                    Reject(delegate { ImeSettingsStore.Save(path, new ImeSettings { InputMode = "Korean" }, first.Fingerprint); });
                Check(ImeSettingsStore.Load(path).Value.InputMode == "Auto", "locked save changed the file");
            });
            Test("failed atomic replacement leaves original intact and cleans its temp", delegate {
                string path = Path.Combine(dir, "no-delete-sharing.json");
                var first = ImeSettingsStore.Save(path, new ImeSettings(), "missing");
                byte[] bytes = File.ReadAllBytes(path);
                using (var hold = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite))
                    Reject(delegate { ImeSettingsStore.Save(path, new ImeSettings { InputMode = "English" }, first.Fingerprint); });
                Check(File.ReadAllBytes(path).SequenceEqual(bytes), "failed replacement damaged original");
                Check(Directory.GetFiles(dir, "*.tmp").Length == 0, "temporary file not cleaned");
            });
            Test("malformed settings never crash loading or create defaults", delegate {
                string path = Path.Combine(dir, "invalid-types.json");
                foreach (string json in new[] { "{\"__type\":\"unknown\"}", "{\"InputMode\":null}", "\uFEFF\uFEFF{}", "{\"Format\":1e0}", "{\"InputMode\":\"\\ud800\"}" })
                {
                    File.WriteAllBytes(path, ImeSettings.Utf8.GetBytes(json));
                    var loaded = ImeSettingsStore.Load(path);
                    Check(loaded.Error != null && loaded.Value == null, "invalid configuration accepted: " + json);
                }
            });
            Test("two editor processes cannot silently overwrite each other", delegate {
                string path = Path.Combine(dir, "race.json");
                var first = ImeSettingsStore.Save(path, new ImeSettings(), "missing");
                var jobs = new List<Process>();
                foreach (string mode in new[] { "Korean", "English" })
                    jobs.Add(Process.Start(new ProcessStartInfo { FileName = Process.GetCurrentProcess().MainModule.FileName, Arguments = "--writer \"" + path + "\" " + first.Fingerprint + " " + mode, UseShellExecute = false, CreateNoWindow = true }));
                foreach (var job in jobs) { if (!job.WaitForExit(10000)) throw new Exception("writer timeout"); }
                Check(jobs.Count(p => p.ExitCode == 0) == 1 && jobs.Count(p => p.ExitCode == 3) == 1, "expected one success and one conflict");
                foreach (var job in jobs) job.Dispose();
                Check(ImeSettingsStore.Load(path).Error == null, "invalid result after race");
            });
            Test("missing live file after replacement failure is a repair state", delegate {
                string path = Path.Combine(dir, "interrupted-replace.json");
                ImeSettingsStore.Save(path, new ImeSettings { ExcludedApps = new[] { "code.exe" } }, "missing");
                File.Move(path, path + ".previous");
                var loaded = ImeSettingsStore.Load(path);
                Check(loaded.Error != null && loaded.Value == null && loaded.Fingerprint == "missing", "interrupted replace was hidden as first run");
                ImeSettingsStore.Save(path, new ImeSettings(), loaded.Fingerprint);
                Check(ImeSettings.Parse(File.ReadAllBytes(path + ".previous")).ExcludedApps[0] == "code.exe", "repair erased recovered original");
            });
        }
        finally
        {
            // Only exact files this run created; never recursively delete a computed directory.
            foreach (string file in Directory.GetFiles(dir)) File.Delete(file);
            Directory.Delete(dir);
        }
        Console.WriteLine("Settings tests: " + passed + " passed, " + failed + " failed");
        return failed == 0 ? 0 : 1;
    }
}
