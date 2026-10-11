using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Web.Script.Serialization;
using System.Xml;
using Microsoft.Win32;

namespace Hanautomata
{
    internal sealed class StartupStatus
    {
        public bool Enabled { get; set; }
        public bool Registered { get; set; }
        public bool Healthy { get; set; }
        public bool LegacyEnabled { get; set; }
        public string TaskName { get; set; }
        public string Detail { get; set; }
        public string LastRun { get; set; }
        public int LastResult { get; set; }
        public string LogPath { get; set; }
    }

    internal interface IStartupTasks
    {
        string Read(string name, out DateTime lastRun, out int lastResult);
        void Register(string name, string xml, string sid);
        void Delete(string name);
    }

    // Task Scheduler uses the signed-in user's interactive token; no password or elevation.
    internal sealed class WindowsStartupTasks : IStartupTasks, IDisposable
    {
        dynamic service, folder;
        internal WindowsStartupTasks()
        {
            try
            {
                service = Activator.CreateInstance(Type.GetTypeFromProgID("Schedule.Service", true));
                service.Connect(); folder = service.GetFolder("\\");
            }
            catch { Dispose(); throw; }
        }
        static bool Missing(COMException ex) { return ex.ErrorCode == unchecked((int)0x80070002); }
        static void Release(object value) { if (value != null && Marshal.IsComObject(value)) Marshal.FinalReleaseComObject(value); }
        public string Read(string name, out DateTime lastRun, out int lastResult)
        {
            dynamic task = null; lastRun = DateTime.MinValue; lastResult = 0;
            try
            {
                task = folder.GetTask(name); lastRun = task.LastRunTime; lastResult = task.LastTaskResult;
                return task.Xml;
            }
            catch (COMException ex) { if (Missing(ex)) return null; throw; }
            catch (FileNotFoundException) { return null; }
            finally { Release(task); }
        }
        public void Register(string name, string xml, string sid)
        {
            object task = folder.RegisterTask(name, xml, 6, sid, null, 3, null);
            Release(task);
        }
        public void Delete(string name)
        {
            try { folder.DeleteTask(name, 0); }
            catch (COMException ex) { if (!Missing(ex)) throw; }
            catch (FileNotFoundException) { }
        }
        public void Dispose() { Release(folder); Release(service); folder = null; service = null; }
    }

    internal sealed class StartupRegistration
    {
        internal const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
        const string Namespace = "http://schemas.microsoft.com/windows/2004/02/mit/task";
        readonly IStartupTasks tasks;
        readonly string executable, sid;
        internal string TaskName { get; private set; }
        internal StartupRegistration(IStartupTasks tasks, string executable, string sid, string taskName = null)
        { this.tasks = tasks; this.executable = Path.GetFullPath(executable); this.sid = sid; TaskName = taskName ?? ("Hanautomata-" + sid); }

        internal static bool LegacyEnabled(string name)
        {
            using (RegistryKey run = Registry.CurrentUser.OpenSubKey(RunKey))
            {
                if (run == null || string.IsNullOrWhiteSpace(run.GetValue(name) as string)) return false;
            }
            foreach (string view in new[] { "Run", "Run32" })
            using (RegistryKey approval = Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\" + view))
            {
                byte[] value = approval == null ? null : approval.GetValue(name) as byte[];
                // Windows represents disabled entries by an odd state; do not re-enable them during migration.
                if (value != null && value.Length > 0 && (value[0] & 1) != 0) return false;
            }
            return true;
        }
        internal static void RemoveLegacy()
        {
            using (RegistryKey run = Registry.CurrentUser.OpenSubKey(RunKey, true))
                if (run != null) { run.DeleteValue("Hanautomata", false); run.DeleteValue("HanFlow", false); }
        }
        static XmlNamespaceManager Namespaces(XmlDocument doc)
        { var ns = new XmlNamespaceManager(doc.NameTable); ns.AddNamespace("t", Namespace); return ns; }
        static XmlDocument Parse(string xml)
        {
            var doc = new XmlDocument { XmlResolver = null };
            using (var reader = XmlReader.Create(new StringReader(xml), new XmlReaderSettings { DtdProcessing = DtdProcessing.Prohibit, XmlResolver = null })) doc.Load(reader);
            return doc;
        }
        static string Value(XmlDocument doc, string path, string defaultValue = "")
        { XmlNode node = doc.SelectSingleNode("/t:Task/" + path, Namespaces(doc)); return node == null ? defaultValue : node.InnerText; }
        bool SameUser(string value)
        {
            if (value == sid) return true;
            if (string.IsNullOrEmpty(value)) return false;
            try { return ((SecurityIdentifier)new NTAccount(value).Translate(typeof(SecurityIdentifier))).Value == sid; }
            catch (IdentityNotMappedException) { return false; }
            catch (System.Security.SecurityException) { return false; }
        }
        internal string BuildXml()
        {
            // XML values are escaped by XmlWriter, including Korean paths, spaces and ampersands.
            var output = new System.Text.StringBuilder();
            using (var writer = XmlWriter.Create(output, new XmlWriterSettings { OmitXmlDeclaration = true }))
            {
                writer.WriteStartElement("Task", Namespace); writer.WriteAttributeString("version", "1.2");
                writer.WriteStartElement("RegistrationInfo"); writer.WriteElementString("Description", "Hanautomata: start 15 seconds after this user signs in."); writer.WriteEndElement();
                writer.WriteStartElement("Triggers"); writer.WriteStartElement("LogonTrigger");
                writer.WriteElementString("Enabled", "true"); writer.WriteElementString("UserId", sid); writer.WriteElementString("Delay", "PT15S"); writer.WriteEndElement(); writer.WriteEndElement();
                writer.WriteStartElement("Principals"); writer.WriteStartElement("Principal"); writer.WriteAttributeString("id", "CurrentUser");
                writer.WriteElementString("UserId", sid); writer.WriteElementString("LogonType", "InteractiveToken"); writer.WriteElementString("RunLevel", "LeastPrivilege"); writer.WriteEndElement(); writer.WriteEndElement();
                writer.WriteStartElement("Settings");
                writer.WriteElementString("MultipleInstancesPolicy", "IgnoreNew");
                writer.WriteElementString("DisallowStartIfOnBatteries", "false"); writer.WriteElementString("StopIfGoingOnBatteries", "false");
                writer.WriteElementString("AllowHardTerminate", "false"); writer.WriteElementString("StartWhenAvailable", "true");
                writer.WriteElementString("RunOnlyIfNetworkAvailable", "false");
                writer.WriteStartElement("IdleSettings"); writer.WriteElementString("StopOnIdleEnd", "false"); writer.WriteElementString("RestartOnIdle", "false"); writer.WriteEndElement();
                writer.WriteElementString("AllowStartOnDemand", "true"); writer.WriteElementString("Enabled", "true"); writer.WriteElementString("Hidden", "false");
                writer.WriteElementString("RunOnlyIfIdle", "false"); writer.WriteElementString("WakeToRun", "false"); writer.WriteElementString("ExecutionTimeLimit", "PT0S");
                writer.WriteStartElement("RestartOnFailure"); writer.WriteElementString("Interval", "PT1M"); writer.WriteElementString("Count", "3"); writer.WriteEndElement();
                writer.WriteEndElement();
                writer.WriteStartElement("Actions"); writer.WriteAttributeString("Context", "CurrentUser"); writer.WriteStartElement("Exec");
                writer.WriteElementString("Command", executable); writer.WriteElementString("Arguments", "--background --autostart");
                writer.WriteElementString("WorkingDirectory", Path.GetDirectoryName(executable)); writer.WriteEndElement(); writer.WriteEndElement(); writer.WriteEndElement();
            }
            return output.ToString();
        }
        internal StartupStatus Read()
        {
            DateTime lastRun; int lastResult;
            string xml = tasks.Read(TaskName, out lastRun, out lastResult);
            var status = new StartupStatus { TaskName = TaskName, Registered = xml != null, Healthy = true,
                LastRun = lastRun.Year > 2000 ? lastRun.ToString("o") : null, LastResult = lastResult, LogPath = StartupLog.FilePath };
            if (xml == null) { status.Detail = "꺼짐"; return status; }
            XmlDocument doc = Parse(xml);
            bool enabled = Value(doc, "t:Settings/t:Enabled") != "false" && Value(doc, "t:Triggers/t:LogonTrigger/t:Enabled") != "false";
            bool matches = doc.SelectNodes("/t:Task/t:Actions/*", Namespaces(doc)).Count == 1 &&
                doc.SelectNodes("/t:Task/t:Triggers/*", Namespaces(doc)).Count == 1 &&
                string.Equals(Value(doc, "t:Actions/t:Exec/t:Command"), executable, StringComparison.OrdinalIgnoreCase) &&
                Value(doc, "t:Actions/t:Exec/t:Arguments") == "--background --autostart" &&
                string.Equals(Value(doc, "t:Actions/t:Exec/t:WorkingDirectory"), Path.GetDirectoryName(executable), StringComparison.OrdinalIgnoreCase) &&
                SameUser(Value(doc, "t:Principals/t:Principal/t:UserId")) && Value(doc, "t:Principals/t:Principal/t:LogonType") == "InteractiveToken" &&
                Value(doc, "t:Principals/t:Principal/t:RunLevel", "LeastPrivilege") == "LeastPrivilege" && SameUser(Value(doc, "t:Triggers/t:LogonTrigger/t:UserId")) &&
                Value(doc, "t:Triggers/t:LogonTrigger/t:Delay") == "PT15S" &&
                Value(doc, "t:Settings/t:DisallowStartIfOnBatteries") == "false" && Value(doc, "t:Settings/t:StopIfGoingOnBatteries") == "false" &&
                Value(doc, "t:Settings/t:ExecutionTimeLimit") == "PT0S" && Value(doc, "t:Settings/t:MultipleInstancesPolicy") == "IgnoreNew" &&
                Value(doc, "t:Settings/t:RunOnlyIfNetworkAvailable", "false") == "false" && Value(doc, "t:Settings/t:RunOnlyIfIdle", "false") == "false" &&
                Value(doc, "t:Settings/t:StartWhenAvailable") == "true" && Value(doc, "t:Settings/t:AllowStartOnDemand", "true") == "true" &&
                Value(doc, "t:Settings/t:RestartOnFailure/t:Count") == "3" && Value(doc, "t:Settings/t:RestartOnFailure/t:Interval") == "PT1M";
            status.Healthy = matches;
            status.Enabled = enabled && matches;
            status.Detail = !matches ? "등록 복구 필요" : enabled ? "로그인 15초 뒤 실행 · 실패 시 1분 간격으로 최대 3회 재시도" : "Windows 작업 스케줄러에서 꺼짐";
            return status;
        }
        internal StartupStatus Enable()
        {
            if (!File.Exists(executable)) throw new FileNotFoundException("자동 시작 실행 파일을 찾을 수 없습니다.", executable);
            tasks.Register(TaskName, BuildXml(), sid);
            StartupStatus status = Read();
            if (!status.Enabled) throw new IOException("자동 시작 등록을 확인하지 못했습니다.");
            return status;
        }
        internal StartupStatus Disable() { tasks.Delete(TaskName); return Read(); }
        internal static StartupStatus Current(string executable, string operation)
        {
            string sid = WindowsIdentity.GetCurrent().User.Value;
            using (var tasks = new WindowsStartupTasks())
            {
                var registration = new StartupRegistration(tasks, executable, sid);
                StartupStatus status = registration.Read();
                bool legacy = LegacyEnabled("Hanautomata") || LegacyEnabled("HanFlow");
                if (operation == "enable" || (operation == "migrate" && !status.Registered && legacy))
                { status = registration.Enable(); RemoveLegacy(); }
                else if (operation == "disable") { status = registration.Disable(); RemoveLegacy(); }
                else if (operation != "status" && operation != "migrate") throw new ArgumentException("startup operation must be enable, disable, status or migrate");
                if (operation == "migrate" && status.Registered && status.Healthy) RemoveLegacy();
                status.LegacyEnabled = LegacyEnabled("Hanautomata") || LegacyEnabled("HanFlow");
                if (status.LegacyEnabled) status.Detail += " · 이전 Run 등록 남음";
                return status;
            }
        }
        internal static int Command(string executable, string operation, string reportPath)
        {
            try
            {
                StartupStatus result = Current(executable, operation);
                File.WriteAllText(reportPath, new JavaScriptSerializer().Serialize(result)); return 0;
            }
            catch (Exception ex)
            {
                if (operation != "status") StartupLog.Write("registration-failed", operation, ex);
                try { File.WriteAllText(reportPath, new JavaScriptSerializer().Serialize(new { Error = ex.Message })); } catch { }
                return 1;
            }
        }
    }

    internal static class StartupLog
    {
        internal static string FilePath { get { return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Hanautomata", "startup.log"); } }
        internal static void Write(string state, string mode, Exception error)
        {
            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(FilePath));
                if (File.Exists(FilePath) && new FileInfo(FilePath).Length >= 65536) { File.Copy(FilePath, FilePath + ".previous", true); File.WriteAllText(FilePath, ""); }
                File.AppendAllText(FilePath, DateTimeOffset.Now.ToString("o") + " pid=" + System.Diagnostics.Process.GetCurrentProcess().Id + " " + state + " mode=" + mode +
                    (error == null ? "" : " error=" + error.GetType().Name + " hresult=" + error.HResult.ToString("X8")) + Environment.NewLine);
            }
            catch (IOException) { } catch (UnauthorizedAccessException) { } catch (System.Security.SecurityException) { }
        }
    }
}
