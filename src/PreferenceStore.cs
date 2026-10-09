using System;
using System.IO;
using System.Text;
using System.Threading;
using System.Runtime.InteropServices;
using System.Web.Script.Serialization;

namespace HanFlow
{
    // Disk access occurs at startup, on a worker, or during shutdown -- never inside the hook.
    internal sealed class PreferenceStore
    {
        public sealed class Document
        {
            public int Format { get; set; }
            public LearnedWord[] Entries { get; set; }
        }
        readonly string path;
        readonly WordPreferences words;
        readonly object io = new object();
        volatile int savedVersion;
        int queued;
        internal volatile string LastError = "";
        internal volatile string LastFailureCode = ""; // Phase/type/code only; never paths or learned text.
        internal PreferenceStore(string path, WordPreferences words)
        {
            this.path = Path.GetFullPath(path); this.words = words;
            try
            {
                if (File.Exists(this.path))
                {
                    if (new FileInfo(this.path).Length > 524288) throw new InvalidDataException();
                    var document = Serializer().Deserialize<Document>(File.ReadAllText(this.path, Encoding.UTF8));
                    if (document == null || (document.Format != 1 && document.Format != 2)) throw new InvalidDataException();
                    words.Restore(document.Entries);
                }
            }
            catch { LastError = "개인 학습 파일을 읽지 못했습니다. 기본 판별로 동작합니다."; }
            savedVersion = words.Version;
        }
        static JavaScriptSerializer Serializer() { return new JavaScriptSerializer { MaxJsonLength = 524288 }; }
        internal void ScheduleSave()
        {
            if (words.Version == savedVersion || Interlocked.CompareExchange(ref queued, 1, 0) != 0) return;
            ThreadPool.QueueUserWorkItem(delegate
            {
                bool saved = false;
                try
                {
                    do { saved = FlushNow(); } while (saved && words.Version != savedVersion);
                }
                finally
                {
                    Interlocked.Exchange(ref queued, 0);
                    // Close the race where a new choice arrived while the worker was queued.
                    // Persistent I/O errors wait for the next normal timer tick, never spin.
                    if (saved && words.Version != savedVersion) ScheduleSave();
                }
            });
        }
        static void ReplaceWithRetry(string temporary, string destination)
        {
            for (int attempt = 0; ; attempt++)
            {
                try
                {
                    if (File.Exists(destination)) File.Replace(temporary, destination, null);
                    else File.Move(temporary, destination);
                    return;
                }
                catch (IOException ex)
                {
                    int code = Marshal.GetHRForException(ex) & 0xffff;
                    // ReplaceFileW 1175 retains both original names, so retry is safe.
                    // Do not treat 1176/1177 (different file-state contracts) as equivalent.
                    if ((code != 32 && code != 33 && code != 1175) || attempt >= 5) throw;
                    Thread.Sleep(15 << attempt); // Up to 465ms, only on worker/shutdown paths.
                }
            }
        }
        internal bool FlushNow()
        {
            lock (io)
            {
                int revision; var entries = words.Export(out revision);
                if (revision == savedVersion) return true;
                string temporary = path + ".tmp";
                string phase = "prepare";
                try
                {
                    Directory.CreateDirectory(Path.GetDirectoryName(path));
                    phase = "write-temporary";
                    File.WriteAllText(temporary, Serializer().Serialize(new Document { Format = 2, Entries = entries }), new UTF8Encoding(false));
                    phase = "replace";
                    ReplaceWithRetry(temporary, path);
                    savedVersion = revision; LastError = ""; LastFailureCode = ""; return true;
                }
                catch (Exception ex)
                {
                    LastFailureCode = phase + ":" + ex.GetType().Name + ":" + (Marshal.GetHRForException(ex) & 0xffff);
                    LastError = "개인 학습을 저장하지 못했습니다. 이번 실행의 선택은 메모리에 유지됩니다."; return false;
                }
                finally { try { if (File.Exists(temporary)) File.Delete(temporary); } catch { } }
            }
        }
    }
}
