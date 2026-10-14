using System;
using System.IO;
using System.Text;
using System.Linq;
using System.Collections.Generic;
using System.Globalization;
using System.Security.Cryptography;
using System.Threading;
using System.Web.Script.Serialization;
using System.Runtime.Serialization.Json;
using System.Xml;
using System.Xml.Linq;

namespace Hanautomata
{
    // Separate from the hook application's settings.json. This type has no input or startup side effects.
    internal sealed class ImeSettings
    {
        public int Format { get; set; }
        public string InputMode { get; set; }
        public string Keyboard { get; set; }
        public string SwitchKind { get; set; }
        public string Sensitivity { get; set; }
        public bool SemicolonShortcut { get; set; }
        public bool LearnChoices { get; set; }
        public bool LearningPaused { get; set; }
        public bool DeleteSyllable { get; set; }
        public bool MsClusters { get; set; }
        public bool Strict { get; set; }
        public bool ShiftSpace { get; set; }
        public bool CapsLock { get; set; }
        public bool HangulKey { get; set; }
        public bool RightAlt { get; set; }
        public bool PauseKey { get; set; }
        public bool CorrectionKeys { get; set; }
        public string[] ExcludedApps { get; set; }
        internal const int MaxBytes = 65536;
        internal static readonly UTF8Encoding Utf8 = new UTF8Encoding(false, true);
        internal ImeSettings()
        {
            Format = 1; InputMode = "Auto"; Keyboard = "Dubeolsik"; SwitchKind = "SingleShot"; Sensitivity = "balanced";
            SemicolonShortcut = LearnChoices = ShiftSpace = CapsLock = HangulKey = RightAlt = PauseKey = CorrectionKeys = true;
            ExcludedApps = new string[0];
        }
        internal static ImeSettings Parse(byte[] bytes)
        {
            if (bytes == null || bytes.Length > MaxBytes) throw new FormatException("설정 파일은 64 KiB 이하여야 합니다.");
            string json = Utf8.GetString(bytes);
            if (json.StartsWith("\uFEFF", StringComparison.Ordinal)) json = json.Substring(1);
            // The XML-mapped reader stops after the first root. Check full-input consumption separately.
            new JavaScriptSerializer { MaxJsonLength = MaxBytes, RecursionLimit = 8 }.DeserializeObject(json);
            byte[] clean = Utf8.GetBytes(json);
            var quotas = new XmlDictionaryReaderQuotas { MaxDepth = 8, MaxStringContentLength = MaxBytes, MaxArrayLength = MaxBytes, MaxBytesPerRead = MaxBytes, MaxNameTableCharCount = MaxBytes };
            XElement root;
            // Unlike Deserialize<T>, this preserves duplicate fields and never coerces strings into booleans.
            using (var reader = JsonReaderWriterFactory.CreateJsonReader(clean, quotas)) root = XElement.Load(reader);
            if ((string)root.Attribute("type") != "object" || root.Attributes().Any(a => a.Name != "type")) throw new FormatException("설정은 JSON 객체여야 합니다.");
            var value = new ImeSettings(); var names = new HashSet<string>(StringComparer.Ordinal);
            foreach (var item in root.Elements())
            {
                string name = item.Name.LocalName;
                var property = typeof(ImeSettings).GetProperty(name);
                if (item.Name.NamespaceName != "" || property == null || !names.Add(name) || item.Attributes().Any(a => a.Name != "type")) throw new FormatException("알 수 없거나 중복된 설정 항목입니다.");
                string type = (string)item.Attribute("type"); object parsed;
                if (property.PropertyType == typeof(string[]) && type == "array")
                {
                    if (item.Elements().Count() > 128 || item.Elements().Any(e => e.Name != "item" || (string)e.Attribute("type") != "string" || e.HasElements || e.Attributes().Any(a => a.Name != "type"))) throw new FormatException("앱 제외 목록이 올바르지 않습니다.");
                    parsed = item.Elements().Select(e => e.Value).ToArray();
                }
                else if (item.HasElements) throw new FormatException("설정 항목의 자료형이 올바르지 않습니다.");
                else if (property.PropertyType == typeof(string) && type == "string") parsed = item.Value;
                else if (property.PropertyType == typeof(bool) && type == "boolean" && (item.Value == "true" || item.Value == "false")) parsed = item.Value == "true";
                else if (property.PropertyType == typeof(int) && type == "number")
                {
                    int n; if (!int.TryParse(item.Value, NumberStyles.None, CultureInfo.InvariantCulture, out n)) throw new FormatException("설정 버전은 정수여야 합니다."); parsed = n;
                }
                else throw new FormatException("설정 항목의 자료형이 올바르지 않습니다.");
                property.SetValue(value, parsed, null);
            }
            value.Validate(); return value;
        }
        internal void Validate()
        {
            if (Format != 1) throw new FormatException("지원하지 않는 설정 버전입니다.");
            if (!new[] { "Auto", "Korean", "English" }.Contains(InputMode)) throw new FormatException("입력 모드를 확인해 주세요.");
            if (!new[] { "Dubeolsik", "Sebeolsik390", "SebeolsikFinal" }.Contains(Keyboard)) throw new FormatException("자판을 확인해 주세요.");
            if (!new[] { "SingleShot", "Cycle", "Toggle" }.Contains(SwitchKind)) throw new FormatException("전환 방식을 확인해 주세요.");
            if (!new[] { "conservative", "balanced", "aggressive" }.Contains(Sensitivity)) throw new FormatException("감도를 확인해 주세요.");
            if (ExcludedApps == null || ExcludedApps.Length > 128) throw new FormatException("제외 앱은 128개까지 지정할 수 있습니다.");
            foreach (string app in ExcludedApps)
            {
                try
                {
                    if (app == null || app.Length <= 4 || Utf8.GetByteCount(app) > 1020 || app.Count(c => !char.IsLowSurrogate(c)) > 255 || app.Trim() != app || !app.EndsWith(".exe", StringComparison.OrdinalIgnoreCase) || app.Any(c => char.IsControl(c) || "\\/:*?\"<>|".IndexOf(c) >= 0))
                        throw new FormatException("제외 앱에는 경로 없이 실행 파일 이름만 적어 주세요. 예: code.exe");
                    app.Normalize(System.Text.NormalizationForm.FormC);
                }
                catch (ArgumentException) { throw new FormatException("제외 앱 이름의 유니코드 표기를 확인해 주세요."); }
            }
        }
        internal byte[] Serialize()
        {
            Validate(); byte[] bytes = Utf8.GetBytes(new JavaScriptSerializer().Serialize(this));
            if (bytes.Length > MaxBytes) throw new FormatException("설정 파일은 64 KiB 이하여야 합니다.");
            return bytes;
        }
    }

    internal sealed class ImeSettingsSnapshot
    {
        internal ImeSettings Value;
        internal string Fingerprint;
        internal string Error;
        internal string BackupPath;
    }

    internal sealed class ImeSettingsConflictException : IOException
    {
        internal ImeSettingsConflictException() : base("다른 창에서 설정을 변경했습니다. 다시 읽기 후 변경해 주세요.") { }
    }

    internal static class ImeSettingsStore
    {
        internal static string DefaultPath { get { return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Hanautomata", "tsf-settings.json"); } }
        static byte[] Read(string path)
        {
            try
            {
                using (var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete))
                {
                    if (stream.Length > ImeSettings.MaxBytes) throw new FormatException("설정 파일이 64 KiB를 초과합니다.");
                    using (var output = new MemoryStream())
                    {
                        var buffer = new byte[4096]; int count;
                        while ((count = stream.Read(buffer, 0, buffer.Length)) != 0)
                        {
                            if (output.Length + count > ImeSettings.MaxBytes) throw new FormatException("설정 파일이 64 KiB를 초과합니다.");
                            output.Write(buffer, 0, count);
                        }
                        return output.ToArray();
                    }
                }
            }
            catch (FileNotFoundException) { return null; }
            catch (DirectoryNotFoundException) { return null; }
        }
        static string Fingerprint(byte[] bytes)
        {
            if (bytes == null) return "missing";
            using (var sha = SHA256.Create()) return BitConverter.ToString(sha.ComputeHash(bytes)).Replace("-", "");
        }
        internal static ImeSettingsSnapshot Load(string path)
        {
            var snapshot = new ImeSettingsSnapshot();
            try
            {
                byte[] bytes = Read(path); snapshot.Fingerprint = Fingerprint(bytes);
                if (bytes == null && (MarkerExists(path + ".lock") || MarkerExists(path + ".previous")))
                    throw new FormatException("설정 파일이 사라졌습니다. 보관 파일을 선택해 복원하거나 기본값을 저장해 주세요. 복구 전에는 입력기가 키를 통과시킵니다.");
                snapshot.Value = bytes == null ? new ImeSettings() : ImeSettings.Parse(bytes);
            }
            catch (Exception e)
            {
                if (!(e is IOException || e is UnauthorizedAccessException || e is FormatException || e is XmlException || e is ArgumentException || e is InvalidOperationException || e is System.Runtime.Serialization.SerializationException)) throw;
                if (e is IOException || e is UnauthorizedAccessException) snapshot.Fingerprint = null;
                snapshot.Error = "설정 파일을 읽지 못했습니다. 파일은 변경하지 않았습니다. " + e.Message;
            }
            return snapshot;
        }
        static bool MarkerExists(string path)
        {
            try { File.GetAttributes(path); return true; }
            catch (FileNotFoundException) { return false; }
            catch (DirectoryNotFoundException) { return false; }
        }
        internal static ImeSettings ReadRecovery(string path)
        {
            byte[] bytes = Read(path);
            if (bytes == null) throw new FileNotFoundException("복원할 설정 파일을 찾지 못했습니다.", path);
            return ImeSettings.Parse(bytes);
        }
        static FileStream Lock(string path)
        {
            for (int attempt = 0; ; attempt++)
            {
                try { return new FileStream(path, FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.None); }
                catch (IOException) { if (attempt >= 10) throw; Thread.Sleep(25); }
            }
        }
        internal static ImeSettingsSnapshot Save(string path, ImeSettings value, string expected, bool restoring = false)
        {
            if (expected == null) throw new IOException("현재 파일을 확인할 수 없어 저장하지 않았습니다.");
            byte[] bytes = value.Serialize(); path = Path.GetFullPath(path);
            Directory.CreateDirectory(Path.GetDirectoryName(path));
            using (var guard = Lock(path + ".lock"))
            {
                byte[] current = Read(path);
                if (Fingerprint(current) != expected) throw new ImeSettingsConflictException();
                string temp = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
                string backup = current == null ? null : restoring ? path + ".before-restore." + Guid.NewGuid().ToString("N") + ".json" : path + ".previous";
                try
                {
                    using (var stream = new FileStream(temp, FileMode.CreateNew, FileAccess.Write, FileShare.None)) { stream.Write(bytes, 0, bytes.Length); stream.Flush(true); }
                    if (current == null) File.Move(temp, path);
                    // Recovery must not overwrite a selected .previous file. Keep the pre-restore live file separately.
                    else File.Replace(temp, path, backup);
                }
                catch (IOException e)
                {
                    if (backup != null && !File.Exists(path) && File.Exists(backup))
                        throw new IOException("현재 설정 파일을 확인할 수 없습니다. 보관된 원본: " + backup + "\n파일에서 복원으로 선택해 주세요.", e);
                    throw;
                }
                // ReplaceFile can leave the original only at the backup path on error 1177. Keep staged bytes
                // if the live file disappeared; the .lock marker makes readers fail closed until explicit repair.
                finally { if (File.Exists(path) && File.Exists(temp)) File.Delete(temp); }
                return new ImeSettingsSnapshot { Value = ImeSettings.Parse(bytes), Fingerprint = Fingerprint(bytes), BackupPath = backup };
            }
        }
    }
}
