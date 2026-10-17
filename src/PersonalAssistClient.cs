using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;

namespace Hanautomata
{
    internal sealed class AssistItem
    {
        public string id { get; set; }
        public string text { get; set; }
        public uint copies { get; set; }
        public ulong last_copied_day { get; set; }
        public override string ToString() { return text; }
    }
    internal sealed class AssistReply
    {
        public int schema_version { get; set; }
        public string request_id { get; set; }
        public bool ok { get; set; }
        public bool enabled { get; set; }
        public AssistItem[] items { get; set; }
        public string error { get; set; }
        public string undo_id { get; set; }
    }

    // A dedicated child process; never called from a keyboard callback. Serial requests with a bounded timeout.
    internal sealed class PersonalAssistClient : IDisposable
    {
        readonly string executable, store;
        readonly object serial = new object(), lifetime = new object();
        Process process; bool disposed; int sequence, activeSequence;
        internal PersonalAssistClient(string executable, string store) { this.executable=executable; this.store=store; }
        internal void Send(string command, Dictionary<string,object> fields, Action<AssistReply> completed)
        {
            ThreadPool.QueueUserWorkItem(delegate {
                AssistReply reply;
                lock (serial) {
                    int number=Interlocked.Increment(ref sequence);
                    string id = number.ToString(System.Globalization.CultureInfo.InvariantCulture);
                    try {
                        Process child;
                        lock (lifetime) {
                            if (disposed) return;
                            if (process==null || process.HasExited) {
                                if (process!=null) process.Dispose();
                                var start = new ProcessStartInfo(executable, "--store \""+store+"\"") {
                                    UseShellExecute=false, CreateNoWindow=true, RedirectStandardInput=true,
                                    RedirectStandardOutput=true, RedirectStandardError=true,
                                    StandardOutputEncoding=new UTF8Encoding(false,true), StandardErrorEncoding=new UTF8Encoding(false,true) };
                                process=Process.Start(start); process.BeginErrorReadLine();
                            }
                            child=process;
                        }
                        var request=new Dictionary<string,object>(fields);
                        request["schema_version"]=1; request["request_id"]=id; request["command"]=command;
                        var json=new JavaScriptSerializer { MaxJsonLength=1048576 };
                        var bytes=Encoding.UTF8.GetBytes(json.Serialize(request)+"\n");
                        if (bytes.Length>16384) throw new InvalidDataException();
                        Interlocked.Exchange(ref activeSequence,number);
                        using (var timeout=new Timer(delegate { if(Interlocked.CompareExchange(ref activeSequence,0,number)==number) Stop(child); },null,3000,Timeout.Infinite)) {
                            try {
                                child.StandardInput.BaseStream.Write(bytes,0,bytes.Length); child.StandardInput.BaseStream.Flush();
                                reply=json.Deserialize<AssistReply>(ReadFrame(child.StandardOutput));
                            } finally { Interlocked.CompareExchange(ref activeSequence,0,number); }
                        }
                        if (reply==null || reply.schema_version!=1 || (reply.request_id!=id && !(reply.request_id=="" && !reply.ok)))
                            throw new InvalidDataException();
                        if (reply.items==null || reply.items.Length>512) throw new InvalidDataException();
                        foreach (var item in reply.items)
                            if (item==null || item.id==null || item.id.Length!=64 || string.IsNullOrEmpty(item.text) || item.text.Length>512)
                                throw new InvalidDataException();
                    } catch {
                        lock(lifetime) { Stop(process); if(process!=null) { process.Dispose(); process=null; } }
                        reply=new AssistReply { ok=false, error="connection_failed", items=new AssistItem[0] };
                    }
                }
                lock(lifetime) { if(disposed) return; }
                completed(reply);
            });
        }
        internal static string ReadFrame(TextReader input)
        {
            var frame=new StringBuilder(); int c;
            while((c=input.Read())!=-1) {
                if(c=='\n') { string value=frame.ToString(); if(Encoding.UTF8.GetByteCount(value)>1048576) throw new InvalidDataException(); return value; }
                if(frame.Length>=1048576) throw new InvalidDataException(); frame.Append((char)c);
            }
            throw new EndOfStreamException();
        }
        static void Stop(Process child) { if(child!=null) try { if(!child.HasExited) child.Kill(); } catch {} }
        public void Dispose()
        {
            // Killing our own worker releases the OS file lock even if an outstanding read is stalled.
            lock(lifetime) { disposed=true; Stop(process); }
            ThreadPool.QueueUserWorkItem(delegate { lock(serial) { lock(lifetime) { if(process!=null) { process.Dispose(); process=null; } } } });
        }
        internal static string Message(string error)
        {
            switch(error) {
                case "invalid_text": return "줄바꿈·제어문자 없이 2~256자로 등록해 주세요.";
                case "full": return "512개까지 저장할 수 있습니다. 사용하지 않는 표현을 삭제해 주세요.";
                case "disabled": return "추천 사용을 켜면 순위를 기억합니다.";
                case "stale_undo": return "되돌릴 수 있는 최근 순위 이력이 없습니다.";
                case "not_found": return "표현이 삭제되었습니다. 다시 불러와 주세요.";
                case "store_unavailable": return "저장소를 열거나 저장하지 못했습니다. 다른 내 표현 창을 닫고 재시도해 주세요. 손상 파일은 덮어쓰지 않습니다.";
                default: return "추천 서비스에 연결하지 못했습니다. 서비스가 포함된 빌드에서 다시 시도해 주세요.";
            }
        }
    }
}
