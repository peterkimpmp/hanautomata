using System;
using System.IO;
using System.Security.Principal;
using System.Xml;
using Hanautomata;

class StartupTests
{
    sealed class FakeTasks : IStartupTasks
    {
        internal string Xml;
        internal bool Reject;
        public string Read(string name, out DateTime date, out int result) { date = DateTime.MinValue; result = 0; return Xml; }
        public void Register(string name, string xml, string sid) { if (Reject) throw new UnauthorizedAccessException(); Xml = xml; }
        public void Delete(string name) { Xml = null; }
    }
    static int count;
    static void Check(bool value, string message) { if (!value) throw new Exception(message); count++; }
    static string Change(string xml, string xpath, string value)
    {
        var doc = new XmlDocument(); doc.LoadXml(xml);
        var ns = new XmlNamespaceManager(doc.NameTable); ns.AddNamespace("t", "http://schemas.microsoft.com/windows/2004/02/mit/task");
        doc.SelectSingleNode("/t:Task/" + xpath, ns).InnerText = value; return doc.OuterXml;
    }
    static int Main(string[] args)
    {
        try
        {
            string exe = Path.GetFullPath(args[0]); string sid = WindowsIdentity.GetCurrent().User.Value;
            var tasks = new FakeTasks(); var registration = new StartupRegistration(tasks, exe, sid);
            Check(!registration.Read().Enabled && !registration.Read().Registered, "unregistered must be off");
            Check(registration.Enable().Enabled, "enable must verify its registration");
            string original = tasks.Xml;
            Check(tasks.Xml.Contains("PT15S") && tasks.Xml.Contains("PT1M"), "login delay and retry must be present");
            foreach (string[] change in new[] {
                new[] { "t:Settings/t:Enabled", "false" },
                new[] { "t:Triggers/t:LogonTrigger/t:Enabled", "false" },
                new[] { "t:Actions/t:Exec/t:Command", @"C:\missing\Hanautomata.exe" },
                new[] { "t:Actions/t:Exec/t:Arguments", "" },
                new[] { "t:Actions/t:Exec/t:WorkingDirectory", @"C:\wrong" },
                new[] { "t:Principals/t:Principal/t:UserId", "S-1-5-18" },
                new[] { "t:Principals/t:Principal/t:LogonType", "S4U" },
                new[] { "t:Principals/t:Principal/t:RunLevel", "HighestAvailable" },
                new[] { "t:Triggers/t:LogonTrigger/t:UserId", "S-1-5-18" },
                new[] { "t:Triggers/t:LogonTrigger/t:Delay", "PT0S" },
                new[] { "t:Settings/t:DisallowStartIfOnBatteries", "true" },
                new[] { "t:Settings/t:StopIfGoingOnBatteries", "true" },
                new[] { "t:Settings/t:ExecutionTimeLimit", "PT72H" },
                new[] { "t:Settings/t:RunOnlyIfNetworkAvailable", "true" },
                new[] { "t:Settings/t:RunOnlyIfIdle", "true" },
                new[] { "t:Settings/t:RestartOnFailure/t:Count", "0" },
                new[] { "t:Settings/t:RestartOnFailure/t:Interval", "PT10M" },
                new[] { "t:Settings/t:MultipleInstancesPolicy", "Parallel" },
                new[] { "t:Settings/t:StartWhenAvailable", "false" },
                new[] { "t:Settings/t:AllowStartOnDemand", "false" }
            }) {
                tasks.Xml = Change(original, change[0], change[1]);
                Check(!registration.Read().Enabled, "misleading checkmark for " + change[0]);
            }
            tasks.Xml = original.Replace("</Actions>", "<Exec><Command>other.exe</Command></Exec></Actions>");
            Check(!registration.Read().Enabled, "unexpected extra action must not be healthy");
            tasks.Xml = original;
            Check(!registration.Disable().Registered, "disable must remove task");
            tasks.Reject = true; bool denied = false;
            try { registration.Enable(); } catch (UnauthorizedAccessException) { denied = true; }
            Check(denied && !registration.Read().Enabled, "access denial must not look like success");
            string special = Path.Combine(Path.GetTempPath(), "한글 & spaced", "Hanautomata.exe");
            var escaped = new StartupRegistration(tasks, special, sid); tasks.Xml = escaped.BuildXml();
            Check(escaped.Read().Enabled, "XML must round-trip Korean, spaces and ampersands");

            // Exercise the actual Windows API under a unique disposable name. Never modify the user's app task.
            string testName = "Hanautomata-StartupTest-" + Guid.NewGuid().ToString("N");
            using (var live = new WindowsStartupTasks())
            {
                var actual = new StartupRegistration(live, exe, sid, testName);
                try {
                    Check(!actual.Read().Registered, "unique task absent before test");
                    Check(actual.Enable().Enabled, "live task registration and XML normalization");
                    live.Register(testName, Change(actual.BuildXml(), "t:Settings/t:Enabled", "false"), sid);
                    Check(!actual.Read().Enabled, "live disabled task must report off");
                    Check(actual.Enable().Enabled, "explicit re-enable repairs disabled task");
                    Check(!actual.Disable().Registered, "live disable removes task");
                } catch {
                    DateTime observedDate; int observedResult;
                    string observed = live.Read(testName, out observedDate, out observedResult);
                    if (observed != null) File.WriteAllText(Path.Combine(Path.GetDirectoryName(exe), "startup-task-observed.xml"), observed);
                    throw;
                } finally { live.Delete(testName); }
            }
            Console.WriteLine("PASS: " + count + " startup assertions; 0 failures"); return 0;
        }
        catch (Exception ex) { Console.Error.WriteLine(ex); return 1; }
    }
}
