using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading;
using System.Web.Script.Serialization;
namespace HanFlow
{
    // Explicit diagnostic mode only: bounded aggregate counters and field metadata, never typed text.
    // The per-process guard histogram counts physical (non-injected) key-downs by the reason they were or were
    // not captured, which is the first question when conversion fails in a specific application.
    internal sealed class InputDiagnostics
    {
        readonly string path;
        readonly object sync = new object();
        readonly List<object> samples = new List<object>();
        readonly Queue<object> events = new Queue<object>();
        readonly Dictionary<string, int> guards = new Dictionary<string, int>(StringComparer.Ordinal);
        long next;
        int writing;
        internal InputDiagnostics(string path) { this.path = Path.GetFullPath(path); }
        static string KeyKind(Native.KeyboardData key)
        {
            uint c=key.Key==0xe7?key.Scan:key.Key;
            if(c==32)return "space";
            if((c>=65&&c<=90)||(key.Key==0xe7&&c>=97&&c<=122))return "letter";
            if(c>=48&&c<=57)return "digit";
            if(key.Key==0xe7)return c>127?"non-ascii-text":"ascii-symbol";
            if(c>=0x70&&c<=0x87)return "function";
            if(c==0x10||c==0x11||c==0x12||c>=0xa0&&c<=0xa5||c==0x5b||c==0x5c)return "modifier";
            return "control-or-symbol";
        }
        internal void Key(Native.KeyboardData key, FocusMonitor monitor, bool enabled)
        {
            var s = monitor.Snapshot;
            uint pid; Native.GetWindowThreadProcessId(Native.GetForegroundWindow(), out pid);
            string guard = monitor.GuardReason(s);
            string source = key.Key==0xe7?"unicode":(key.Flags&2)!=0?"lower-integrity":(key.Flags&16)!=0?"injected":"hardware";
            lock (sync)
            {
                events.Enqueue(new { timestamp=DateTimeOffset.Now.ToString("o"), pid, enabled,
                    kind=KeyKind(key), source,
                    guard, process=s.ProcessName, s.Known, s.Writable, s.Password, s.Elevated, s.Focused, s.Pending, fresh=monitor.IsFresh(s),
                    ageMs=s.Ticks==0?-1:(DateTime.UtcNow.Ticks-s.Ticks)/TimeSpan.TicksPerMillisecond,
                    monitor.LastInvalidation, monitor.QueryPhase,
                    ctrl=Native.IsDown(0x11), alt=Native.IsDown(0x12), win=Native.IsDown(0x5b)||Native.IsDown(0x5c) });
                while(events.Count>512) events.Dequeue();
                if (source == "hardware" && KeyKind(key) == "letter")
                {
                    string bucket = s.ProcessName + "|" + (enabled ? guard : "paused");
                    int count; guards.TryGetValue(bucket, out count); guards[bucket] = count + 1;
                }
            }
        }
        internal void Tick(InputController input, FocusMonitor monitor)
        {
            if (DateTime.UtcNow.Ticks < next) return;
            next = DateTime.UtcNow.AddSeconds(1).Ticks;
            var s = monitor.Snapshot;
            string json;
            lock (sync)
            {
                samples.Add(new { timestamp = DateTimeOffset.Now.ToString("o"), input.Enabled, input.HookHealthy, input.HookRestarts,
                    input.Commits, input.CapturedKeys, input.BypassedKeys, input.HardwareKeys, input.InjectedKeys, input.FilteredKeys,
                    input.SentinelActive, input.SentinelFallback, input.HookCallbacks, input.RawHardwareKeys, input.SentinelAlarms, input.AutoConversions, input.SwapsAfterSpace, input.SelectionFlips,
                    input.LastBypass, input.LastError, process=s.ProcessName, s.Reason, s.Diagnostic, s.Known, s.Writable, s.Password, s.Elevated, s.Focused, s.Pending,
                    safe=monitor.IsSafe(s), fresh=monitor.IsFresh(s), ageMs=(DateTime.UtcNow.Ticks-s.Ticks)/TimeSpan.TicksPerMillisecond,
                    ctrl=Native.IsDown(0x11), alt=Native.IsDown(0x12), win=Native.IsDown(0x5b)||Native.IsDown(0x5c) });
                if (samples.Count > 180) samples.RemoveAt(0);
                if (Interlocked.CompareExchange(ref writing, 1, 0) != 0) return;
                var histogram = guards.OrderByDescending(pair => pair.Value).Select(pair => new { process = pair.Key.Split('|')[0], guard = pair.Key.Split('|')[1], count = pair.Value }).ToArray();
                json = new JavaScriptSerializer().Serialize(new { samples=samples.ToArray(), hardwareLetterGuards=histogram, events=events.ToArray() });
            }
            ThreadPool.QueueUserWorkItem(delegate {
                try { Directory.CreateDirectory(Path.GetDirectoryName(path)); File.WriteAllText(path,json); }
                catch { }
                finally { Interlocked.Exchange(ref writing,0); }
            });
        }
    }
}
