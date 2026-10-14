using System;
using System.IO;
using System.Linq;
using System.Diagnostics;
using System.Collections.Generic;
using System.Security.AccessControl;
using System.Security.Principal;
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
    static ImeSettingsSnapshot Prepare(string path)
    {
        return ImeSettingsStore.PrepareUser(path);
    }
    static bool HasPackageRead(string path)
    {
        var rules = File.GetAccessControl(path, AccessControlSections.Access).GetAccessRules(true, true, typeof(SecurityIdentifier)).Cast<FileSystemAccessRule>();
        var app = rules.Where(r => r.IdentityReference.Value == "S-1-15-2-1" && r.AccessControlType == AccessControlType.Allow).ToArray();
        FileSystemRights rights = app.Aggregate((FileSystemRights)0, (all, rule) => all | rule.FileSystemRights);
        return (rights & (FileSystemRights.Read | FileSystemRights.Synchronize)) == (FileSystemRights.Read | FileSystemRights.Synchronize)
            && (rights & (FileSystemRights.Write | FileSystemRights.Delete | FileSystemRights.ChangePermissions | FileSystemRights.TakeOwnership)) == 0;
    }
    static int Main(string[] args)
    {
        if (args.Length == 4 && args[0] == "--shared-resources") return SharedResources(args[1], args[2], args[3]);
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
            Test("user preparation creates defaults once and preserves existing bytes and history", delegate {
                string path = Path.Combine(dir, "prepare.json");
                var first = Prepare(path);
                Check(first.Warning == null && first.Value.InputMode == "Auto" && HasPackageRead(path), "first activation did not prepare readable defaults");
                byte[] custom = System.Text.Encoding.UTF8.GetBytes("{ \"InputMode\":\"English\", \"ExcludedApps\":[\"private.exe\"] }\n");
                File.WriteAllBytes(path, custom); File.WriteAllText(path + ".previous", "private recovery bytes");
                var prepared = Prepare(path);
                Check(prepared.Value.InputMode == "English" && prepared.Value.ExcludedApps.Single() == "private.exe", "existing options reset");
                Check(File.ReadAllBytes(path).SequenceEqual(custom) && File.ReadAllText(path + ".previous") == "private recovery bytes", "preparation rewrote configuration or backup");
                Check(prepared.BackupPath == null && HasPackageRead(path), "preparation rotated history or omitted read access");
            });
            Test("user preparation refuses corrupt oversized and missing-after-save settings", delegate {
                // Check the entry point outside Reject so a missing implementation cannot pass the rejection cases.
                Prepare(Path.Combine(dir, "prepare-entry.json"));
                string path = Path.Combine(dir, "prepare-invalid.json");
                foreach (byte[] bytes in new[] { System.Text.Encoding.UTF8.GetBytes("{broken"), new byte[65537] })
                {
                    File.WriteAllBytes(path, bytes); Reject(delegate { Prepare(path); });
                    Check(File.ReadAllBytes(path).SequenceEqual(bytes), "preparation replaced invalid settings");
                }
                File.Delete(path); File.WriteAllText(path + ".lock", "history");
                Reject(delegate { Prepare(path); });
                Check(!File.Exists(path) && File.ReadAllText(path + ".lock") == "history", "preparation erased a repair state");
            });
            Test("new settings and atomic replacement keep package read without write", delegate {
                string path = Path.Combine(dir, "shared.json");
                var first = ImeSettingsStore.Save(path, new ImeSettings(), "missing");
                Check(HasPackageRead(path), "new settings are not read-only for packages");
                var second = ImeSettingsStore.Save(path, new ImeSettings { InputMode = "Korean" }, first.Fingerprint);
                Check(HasPackageRead(path) && second.Warning == null, "replacement lost package access");
            });
            Test("repair publishes package-readable settings without opening the corrupt backup", delegate {
                string path = Path.Combine(dir, "shared-repair.json");
                File.WriteAllText(path, "{private broken contents");
                string originalAcl = File.GetAccessControl(path).GetSecurityDescriptorSddlForm(AccessControlSections.Access);
                var target = ImeSettingsStore.Load(path);
                ImeSettingsStore.Save(path, new ImeSettings(), target.Fingerprint);
                Check(HasPackageRead(path), "repair did not publish readable settings");
                Check(File.GetAccessControl(path + ".previous").GetSecurityDescriptorSddlForm(AccessControlSections.Access) == originalAcl, "repair broadened backup permissions");
            });
            Test("post-publication access failure returns saved fingerprint and permits retry", delegate {
                string path = Path.Combine(dir, "shared-warning.json"); int calls = 0;
                var saved = ImeSettingsStore.Save(path, new ImeSettings { InputMode = "Korean" }, "missing", false, delegate(string file) {
                    calls++; if (calls == 2) throw new UnauthorizedAccessException("injected publication denial");
                    ImeSharedAccess.GrantSettingsRead(file);
                });
                Check(calls == 2 && saved.Warning != null && saved.Value.InputMode == "Korean", "saved-but-unpublished status lost");
                Check(ImeSettingsStore.Load(path).Fingerprint == saved.Fingerprint, "reported an obsolete fingerprint after save");
                var retried = ImeSettingsStore.Save(path, saved.Value, saved.Fingerprint);
                Check(retried.Warning == null && HasPackageRead(path), "access retry failed");
            });
            Test("access preparation failure leaves the previous bytes untouched", delegate {
                string path = Path.Combine(dir, "shared-preparation.json");
                var first = ImeSettingsStore.Save(path, new ImeSettings(), "missing"); byte[] before = File.ReadAllBytes(path);
                Reject(delegate { ImeSettingsStore.Save(path, new ImeSettings { InputMode = "Korean" }, first.Fingerprint, false,
                    delegate(string file) { throw new UnauthorizedAccessException("injected staging denial"); }); });
                Check(File.ReadAllBytes(path).SequenceEqual(before), "failed preparation committed changes");
            });
            Test("access warning retry preserves normal and restore backups", delegate {
                foreach (bool restoring in new[] { false, true })
                {
                    string path = Path.Combine(dir, "retry-backup-" + restoring + ".json");
                    var first = ImeSettingsStore.Save(path, new ImeSettings { InputMode = "English" }, "missing");
                    byte[] original = File.ReadAllBytes(path);
                    File.WriteAllBytes(path + ".previous", new ImeSettings { InputMode = "Auto" }.Serialize());
                    int calls = 0;
                    var saved = ImeSettingsStore.Save(path, new ImeSettings { InputMode = "Korean" }, first.Fingerprint, restoring,
                        delegate(string file) { if (++calls == 2) throw new UnauthorizedAccessException("injected publication denial"); ImeSharedAccess.GrantSettingsRead(file); });
                    Check(saved.Warning != null && File.ReadAllBytes(saved.BackupPath).SequenceEqual(original), "setup did not preserve original");
                    byte[] previous = File.ReadAllBytes(path + ".previous"), live = File.ReadAllBytes(path);
                    int retryCalls = 0;
                    var retried = ImeSettingsStore.Save(path, saved.Value, saved.Fingerprint, false,
                        delegate(string file) { retryCalls++; Check(file == path, "retry created a replacement file"); ImeSharedAccess.GrantSettingsRead(file); });
                    Check(retried.Warning == null && retryCalls == 1, "retry did not repair access only");
                    Check(File.ReadAllBytes(path + ".previous").SequenceEqual(previous), "retry rotated the recovery source");
                    Check(File.ReadAllBytes(saved.BackupPath).SequenceEqual(original) && File.ReadAllBytes(path).SequenceEqual(live), "retry changed saved or backup bytes");
                    Reject(delegate { ImeSettingsStore.Save(path, saved.Value, first.Fingerprint); });
                }
            });
            Test("failed first save cleans only its new marker and stage under the lock", delegate {
                string path = Path.Combine(dir, "first-failed.json"); bool competingRejected = false;
                Reject(delegate { ImeSettingsStore.Save(path, new ImeSettings(), "missing", false, delegate(string file) {
                    try { using (var competing = new FileStream(path + ".lock", FileMode.Open, FileAccess.ReadWrite, FileShare.ReadWrite | FileShare.Delete)) { } }
                    catch (IOException) { competingRejected = true; }
                    throw new UnauthorizedAccessException("injected first preparation denial");
                }); });
                Check(competingRejected, "first writer did not exclude a competing writer");
                var loaded = ImeSettingsStore.Load(path);
                Check(loaded.Error == null && loaded.Fingerprint == "missing" && loaded.Value != null, "failed first save became a repair state");
                Check(Directory.GetFiles(dir, "first-failed.json*").Length == 0, "failed first save left owned artifacts");
                var saved = ImeSettingsStore.Save(path, new ImeSettings { InputMode = "Korean" }, "missing");
                File.Delete(path);
                Check(ImeSettingsStore.Load(path).Error != null, "successful first save lost its history marker");
            });
            Test("failed first repair preserves an existing history marker", delegate {
                string path = Path.Combine(dir, "first-repair-failed.json");
                File.WriteAllText(path + ".lock", "existing history");
                Reject(delegate { ImeSettingsStore.Save(path, new ImeSettings(), "missing", false,
                    delegate(string file) { throw new UnauthorizedAccessException("injected repair preparation denial"); }); });
                Check(File.ReadAllText(path + ".lock") == "existing history" && ImeSettingsStore.Load(path).Error != null, "failed repair erased history");
                Check(Directory.GetFiles(dir, "first-repair-failed.json*.tmp").Length == 0, "failed repair left its new stage");
            });
            Test("a busy stage cleanup cannot leave a false first-save history marker", delegate {
                string path = Path.Combine(dir, "busy-first-stage.json"); FileStream held = null;
                try
                {
                    Reject(delegate { ImeSettingsStore.Save(path, new ImeSettings(), "missing", false, delegate(string file) {
                        held = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
                        throw new UnauthorizedAccessException("injected staging denial with temporary scanner handle");
                    }); });
                    Check(held != null && ImeSettingsStore.Load(path).Error == null, "stage cleanup failure left a false history marker");
                }
                finally { if (held != null) held.Dispose(); }
            });
            Test("tray marker gives packages synchronize without modify ownership", delegate {
                bool created;
                using (var mutex = ImeSharedAccess.CreateTrayMutex("Local\\Hanautomata.QualityProbe.settings-" + Guid.NewGuid().ToString("N"), out created))
                {
                    try {
                        Check(created, "unique test mutex already existed");
                        var rules = mutex.GetAccessControl().GetAccessRules(true, true, typeof(SecurityIdentifier)).Cast<MutexAccessRule>();
                        var rights = rules.Where(r => r.IdentityReference.Value == "S-1-15-2-1" && r.AccessControlType == AccessControlType.Allow)
                            .Aggregate((MutexRights)0, (all, rule) => all | rule.MutexRights);
                        Check(rights == MutexRights.Synchronize, "packages do not have exactly synchronize access");
                    }
                    finally { if (created) mutex.ReleaseMutex(); }
                }
            });
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
            Test("repairing a corrupt live file preserves the last good backup", delegate {
                string path = Path.Combine(dir, "repair-with-history.json");
                var first = ImeSettingsStore.Save(path, new ImeSettings { InputMode = "Korean", ExcludedApps = new[] { "code.exe" } }, "missing");
                byte[] good = File.ReadAllBytes(path);
                ImeSettingsStore.Save(path, new ImeSettings { InputMode = "English" }, first.Fingerprint);
                byte[] broken = ImeSettings.Utf8.GetBytes("{broken live bytes"); File.WriteAllBytes(path, broken);
                var target = ImeSettingsStore.Load(path);
                var repaired = ImeSettingsStore.Save(path, new ImeSettings(), target.Fingerprint);
                Check(File.ReadAllBytes(path + ".previous").SequenceEqual(good), "repair destroyed the last good backup");
                Check(repaired.BackupPath != null && repaired.BackupPath != path + ".previous" && File.ReadAllBytes(repaired.BackupPath).SequenceEqual(broken), "repair lost the corrupt original");
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
                Check(Directory.GetFiles(dir, "no-delete-sharing.json*.tmp").Length == 0, "temporary file not cleaned");
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
            Test("restore prior options and exclusions from the preserved backup", delegate {
                string path = Path.Combine(dir, "restore-previous.json");
                ImeSettingsStore.Save(path, new ImeSettings { InputMode = "Korean", ExcludedApps = new[] { "code.exe" } }, "missing");
                File.Move(path, path + ".previous");
                byte[] before = File.ReadAllBytes(path + ".previous");
                var target = ImeSettingsStore.Load(path);
                var recovered = ImeSettingsStore.ReadRecovery(path + ".previous");
                Check(!File.Exists(path), "preview must not restore before Save");
                var restored = ImeSettingsStore.Save(path, recovered, target.Fingerprint, true);
                Check(restored.Value.InputMode == "Korean" && restored.Value.ExcludedApps[0] == "code.exe", "restore lost options");
                Check(File.ReadAllBytes(path + ".previous").SequenceEqual(before), "backup source was changed");
            });
            Test("restore staged settings without deleting the selected recovery file", delegate {
                string path = Path.Combine(dir, "restore-stage.json"), staged = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
                byte[] bytes = new ImeSettings { InputMode = "English", RightAlt = false }.Serialize();
                File.WriteAllBytes(staged, bytes); File.WriteAllText(path + ".lock", "");
                var target = ImeSettingsStore.Load(path);
                var recovered = ImeSettingsStore.ReadRecovery(staged);
                var restored = ImeSettingsStore.Save(path, recovered, target.Fingerprint, true);
                Check(restored.Value.InputMode == "English" && !restored.Value.RightAlt, "staged options lost");
                Check(File.ReadAllBytes(staged).SequenceEqual(bytes), "staged source lost");
            });
            Test("recovery preview cannot overwrite a newer live configuration", delegate {
                string path = Path.Combine(dir, "restore-race.json"), backup = path + ".previous";
                var target = ImeSettingsStore.Save(path, new ImeSettings(), "missing");
                File.WriteAllBytes(backup, new ImeSettings { InputMode = "Korean" }.Serialize());
                var recovered = ImeSettingsStore.ReadRecovery(backup);
                ImeSettingsStore.Save(path, new ImeSettings { InputMode = "English" }, target.Fingerprint);
                Reject(delegate { ImeSettingsStore.Save(path, recovered, target.Fingerprint, true); });
                Check(ImeSettingsStore.Load(path).Value.InputMode == "English", "restore clobbered newer settings");
                // The original .previous may already have been rotated by the other writer. Keep the preview value
                // and retry only after the user explicitly accepts a fresh snapshot; the later live value is backed up.
                byte[] newer = File.ReadAllBytes(path);
                var confirmed = ImeSettingsStore.Load(path);
                var retried = ImeSettingsStore.Save(path, recovered, confirmed.Fingerprint, true);
                Check(retried.Value.InputMode == "Korean", "retained preview could not be restored after explicit retry");
                Check(retried.BackupPath != null && File.ReadAllBytes(retried.BackupPath).SequenceEqual(newer), "retry lost the other writer's configuration");
            });
            Test("restoring over a live file preserves the chosen backup and the live version", delegate {
                string path = Path.Combine(dir, "restore-existing.json"), backup = path + ".previous";
                var target = ImeSettingsStore.Save(path, new ImeSettings { InputMode = "English" }, "missing");
                byte[] live = File.ReadAllBytes(path), source = new ImeSettings { InputMode = "Korean" }.Serialize();
                File.WriteAllBytes(backup, source);
                var restored = ImeSettingsStore.Save(path, ImeSettingsStore.ReadRecovery(backup), target.Fingerprint, true);
                Check(ImeSettingsStore.Load(path).Value.InputMode == "Korean", "recovery not applied");
                Check(File.ReadAllBytes(backup).SequenceEqual(source), "chosen backup overwritten");
                string[] preserved = Directory.GetFiles(dir, "restore-existing.json.before-restore.*.json");
                Check(preserved.Length == 1 && File.ReadAllBytes(preserved[0]).SequenceEqual(live), "pre-restore live settings lost");
                Check(restored.BackupPath == preserved[0], "backup location not returned for the user");
            });
            Test("corrupt and oversized recovery files do not change live settings", delegate {
                string path = Path.Combine(dir, "restore-invalid.json"), candidate = path + ".previous";
                ImeSettingsStore.Save(path, new ImeSettings(), "missing"); byte[] before = File.ReadAllBytes(path);
                File.WriteAllText(candidate, "{broken"); Reject(delegate { ImeSettingsStore.ReadRecovery(candidate); });
                File.WriteAllBytes(candidate, new byte[65537]); Reject(delegate { ImeSettingsStore.ReadRecovery(candidate); });
                Reject(delegate { ImeSettingsStore.ReadRecovery(path + ".absent"); });
                Check(File.ReadAllBytes(path).SequenceEqual(before), "invalid recovery modified live file");
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
    // Invoked only by the native probe with its create_new synthetic file and unique mutex. Never launches a hook/UI.
    static int SharedResources(string path, string mutexName, string ready)
    {
        var cleanup = new List<string>(); bool created = false; int result = 0;
        try
        {
            string name = Path.GetFileName(path);
            string directory = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Hanautomata");
            if (!name.StartsWith("quality-probe-", StringComparison.Ordinal) || !name.EndsWith(".json", StringComparison.Ordinal)
                || name.Contains("..") || name.Any(c => !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '-' || c == '.'))
                || !Path.GetFullPath(path).Equals(Path.Combine(directory, name), StringComparison.OrdinalIgnoreCase)
                || !mutexName.StartsWith("Local\\Hanautomata.QualityProbe.", StringComparison.Ordinal)) throw new IOException("unexpected synthetic resource path");
            if (!File.Exists(path) || File.Exists(path + ".lock") || File.Exists(path + ".previous") || File.Exists(ready))
                throw new IOException("unexpected existing probe artifacts");
            using (var mutex = ImeSharedAccess.CreateTrayMutex(mutexName, out created))
            {
                if (!created) throw new IOException("probe mutex was not new");
                try
                {
                    var before = ImeSettingsStore.Load(path);
                    if (before.Value == null || before.Value.InputMode != "Korean" || !before.Value.ExcludedApps.SequenceEqual(new[] { "probe-only.exe" }))
                        throw new IOException("unexpected baseline settings");
                    cleanup.Add(path + ".lock"); cleanup.Add(path + ".previous");
                    var first = ImeSettingsStore.Save(path, before.Value, before.Fingerprint);
                    if (first.Warning != null || !HasPackageRead(path)) throw new IOException(first.Warning ?? "save access check failed");
                    var second = ImeSettingsStore.Save(path, new ImeSettings { InputMode = "English" }, first.Fingerprint);
                    if (second.Warning != null || !HasPackageRead(path)) throw new IOException(second.Warning ?? "replacement access check failed");
                    var restored = ImeSettingsStore.Save(path, ImeSettingsStore.ReadRecovery(path + ".previous"), second.Fingerprint, true);
                    if (restored.BackupPath != null) cleanup.Add(restored.BackupPath);
                    if (restored.Warning != null || !HasPackageRead(path)) throw new IOException(restored.Warning ?? "restore access check failed");
                    using (var signal = new FileStream(ready, FileMode.CreateNew, FileAccess.Write, FileShare.Read))
                    { byte[] bytes = System.Text.Encoding.ASCII.GetBytes("ready"); signal.Write(bytes, 0, bytes.Length); signal.Flush(true); }
                    Console.In.ReadLine(); // EOF when the owning native parent closes stdin also releases the mutex.
                }
                finally { if (created) mutex.ReleaseMutex(); }
            }
        }
        catch (Exception e) { Console.Error.WriteLine(e.Message); result = 2; }
        finally
        {
            foreach (string artifact in cleanup)
                try { if (File.Exists(artifact)) File.Delete(artifact); }
                catch (Exception e) { Console.Error.WriteLine("owned probe cleanup failed: " + e.Message); result = 2; }
        }
        return result;
    }
}
